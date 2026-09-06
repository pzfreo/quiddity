#!/usr/bin/env python3
"""Audit unrecalled MFCAD++ paired-ramp component proxies by explicit geometry gates.

Dataset labels select faces to describe. They never participate in recognition or weaken the
production predicate. A component is the documented non-native shared-edge same-class proxy.
The legacy boundary-bypass fields remain in format version 2 but are empty: production has no
four-edge ramp gate since #397, so the ordinary probe must continue through subdivided boundaries.
"""

from __future__ import annotations

import argparse
import contextlib
import hashlib
import json
import subprocess
import sys
from collections import Counter
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any

ROOT = Path(__file__).parents[1]
sys.path.insert(0, str(ROOT))

from build123d import GeomType  # noqa: E402

from quiddity._adjacency import FaceGraph, FaceNode  # noqa: E402
from quiddity._bevel import BevelReject, classify_bevel  # noqa: E402
from quiddity._candidates import FamilyId  # noqa: E402
from quiddity._dispositions import Outcome  # noqa: E402
from quiddity._geometry import (  # noqa: E402
    AXIS_ALIGNED_COS,
    SMOOTH_ARC_GAP,
    length_tol,
    part_scale,
)
from quiddity.paired_ramp_steps import _RUN_DIRECTION_COS  # noqa: E402
from quiddity.result import _take_inventory  # noqa: E402
from tools.derive_mfcadpp_components import _components  # noqa: E402
from tools.effectiveness_report import load_mfcadpp_truth  # noqa: E402

_PUBLISHED_VERSION = (
    "MFCAD++ published test split; DOI 10.17034/d1fec5a0-8c10-4630-b02e-b92dc81df823"
)
_GATE_ORDER = (
    "no_adjacent_bevel_pair",
    "different_run_axis",
    "invalid_cross_section_signs",
    "asymmetric_ramps",
    "missing_single_linear_ridge",
    "fragmented_ramp_boundary",
    "ridge_not_run_aligned",
    "ridge_not_concave",
    "not_two_common_axis_terminals",
    "cross_solid_or_invalid_solid",
    "top_opening_thickness_direction",
    "not_one_exterior_one_internal_terminal",
    "exterior_not_convex",
    "internal_not_concave",
    "incomplete_shared_run",
    "recognisable",
)


@dataclass(frozen=True, slots=True)
class PairProbe:
    """Production-gate result for one adjacent labelled bevel pair."""

    stage: int
    first_failed_gate: str
    run_axis: str | None
    mirror_delta: float | None
    shared_edge_count: int | None
    ramp_edge_counts: tuple[int, int] | None
    common_axis_terminal_count: int | None
    internal_terminal_edges: int | None
    exterior_terminal_edges: int | None
    full_shared_run: bool | None
    internal_terminal_index: int | None
    exterior_terminal_index: int | None


@dataclass(frozen=True, slots=True)
class ComponentAnatomy:
    """Traversal-independent descriptor for one class-9 component proxy."""

    face_count: int
    surface_counts: tuple[tuple[str, int], ...]
    planar_faces: int
    bevel_faces: int
    face_edge_counts: tuple[int, ...]
    faces_with_inner_wires: int
    faces_with_curved_edges: int
    internal_arc_counts: tuple[tuple[str, int], ...]
    accepted_other_family_claims: tuple[tuple[str, int], ...]
    accepted_other_family_records: tuple[tuple[str, int], ...]
    pair_gate_counts: tuple[tuple[str, int], ...]
    best_pair: PairProbe

    def key(self) -> str:
        value = asdict(self)
        value["best_pair"].pop("run_axis")
        value["best_pair"].pop("internal_terminal_index")
        value["best_pair"].pop("exterior_terminal_index")
        return json.dumps(value, sort_keys=True, separators=(",", ":"))


def _commit() -> str:
    return subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, check=True, capture_output=True, text=True
    ).stdout.strip()


def _selection_hash(ids: list[str]) -> str:
    return hashlib.sha256(("\n".join(ids) + "\n").encode()).hexdigest()


def _source_selection_hash(sources: list[tuple[str, str]]) -> str:
    """Pin selected IDs to their exact STEP bytes (which embed MFCAD++ face labels)."""

    manifest = "".join(f"{model_id}:{source_sha256}\n" for model_id, source_sha256 in sources)
    return hashlib.sha256(manifest.encode()).hexdigest()


def _counts(values: list[str]) -> tuple[tuple[str, int], ...]:
    return tuple(sorted(Counter(values).items()))


def _terminal_coordinate(graph: FaceGraph, node: FaceNode, axis: int) -> float:
    low, high = graph.bounds(node)[axis]
    return 0.5 * (low + high)


def _is_concave(graph: FaceGraph, left: FaceNode, right: FaceNode) -> bool:
    return graph.arc(left, right) == "concave"


def _is_convex(graph: FaceGraph, left: FaceNode, right: FaceNode) -> bool:
    return graph.arc(left, right) == "convex"


def _arc_name(graph: FaceGraph, left: FaceNode, right: FaceNode) -> str | None:
    kind = graph.arc(left, right)
    return kind


def _failed(
    stage: int,
    *,
    axis: int | None = None,
    mirror_delta: float | None = None,
    shared_edges: int | None = None,
    ramp_edges: tuple[int, int] | None = None,
    terminals: int | None = None,
    internal_edges: int | None = None,
    exterior_edges: int | None = None,
    full_run: bool | None = None,
    internal_index: int | None = None,
    exterior_index: int | None = None,
) -> PairProbe:
    return PairProbe(
        stage,
        _GATE_ORDER[stage],
        None if axis is None else "xyz"[axis],
        mirror_delta,
        shared_edges,
        ramp_edges,
        terminals,
        internal_edges,
        exterior_edges,
        full_run,
        internal_index,
        exterior_index,
    )


def _probe_pair(
    graph: FaceGraph,
    left: FaceNode,
    right: FaceNode,
    left_read: tuple[Any, ...],
    right_read: tuple[Any, ...],
    *,
    bypass_ramp_boundary: bool = False,
) -> PairProbe:
    axis, left_normal, left_span, _left_hi, _left_lo = left_read
    right_axis, right_normal, right_span, _right_hi, _right_lo = right_read
    if right_axis != axis:
        return _failed(1)
    cross = tuple(index for index in (0, 1, 2) if index != axis)
    opposed = tuple(index for index in cross if left_normal[index] * right_normal[index] < 0)
    same = tuple(index for index in cross if left_normal[index] * right_normal[index] > 0)
    if len(opposed) != 1 or len(same) != 1:
        return _failed(2, axis=axis)
    mirror_delta = max(abs(abs(left_normal[index]) - abs(right_normal[index])) for index in cross)
    if mirror_delta > SMOOTH_ARC_GAP:
        return _failed(3, axis=axis, mirror_delta=mirror_delta)
    shared = graph.shared_edges(left, right)
    if len(shared) != 1 or shared[0].geom_type != GeomType.LINE:
        return _failed(4, axis=axis, mirror_delta=mirror_delta, shared_edges=len(shared))
    ramp_edges = (len(graph.edges(left)), len(graph.edges(right)))
    # Production deliberately has no ramp edge-count gate: independent straight, curved and
    # inner-wire interruptions are B-Rep presentation details when the shared ridge, terminal,
    # material side and complete run remain proved. ``bypass_ramp_boundary`` is retained only so
    # format-v2 callers fail compatibly while the legacy projection fields drain to zero.
    try:
        tangent = shared[0].tangent_at()
    except Exception:  # pragma: no cover - defensive imported-kernel boundary
        return _failed(6, axis=axis, ramp_edges=ramp_edges, shared_edges=1)
    if abs((tangent.X, tangent.Y, tangent.Z)[axis]) < _RUN_DIRECTION_COS:
        return _failed(6, axis=axis, ramp_edges=ramp_edges, shared_edges=1)
    if not _is_concave(graph, left, right):
        return _failed(7, axis=axis, ramp_edges=ramp_edges, shared_edges=1)
    common = set(graph.neighbours(left)).intersection(graph.neighbours(right))
    terminals = sorted(
        (
            node
            for node in common
            if graph.is_planar(node)
            and (normal := graph.normal(node)) is not None
            and abs(normal[axis]) >= AXIS_ALIGNED_COS
        ),
        key=lambda node: node.index,
    )
    if len(terminals) != 2:
        return _failed(
            8,
            axis=axis,
            ramp_edges=ramp_edges,
            shared_edges=1,
            terminals=len(terminals),
        )
    solid = graph.common_valid_solid((left, right, *terminals))
    if solid is None:
        return _failed(9, axis=axis, ramp_edges=ramp_edges, shared_edges=1, terminals=2)
    bounds = graph.solid_shape(solid).bounding_box()
    solid_axis = (
        (bounds.min.X, bounds.max.X),
        (bounds.min.Y, bounds.max.Y),
        (bounds.min.Z, bounds.max.Z),
    )[axis]
    tolerance = length_tol(part_scale(bounds), rel=1e-9, floor=1e-6)
    extents = (
        bounds.max.X - bounds.min.X,
        bounds.max.Y - bounds.min.Y,
        bounds.max.Z - bounds.min.Z,
    )
    if extents[axis] < min(extents[index] for index in cross) - tolerance:
        return _failed(10, axis=axis, ramp_edges=ramp_edges, shared_edges=1, terminals=2)
    exterior = [
        node
        for node in terminals
        if min(
            abs(_terminal_coordinate(graph, node, axis) - solid_axis[0]),
            abs(_terminal_coordinate(graph, node, axis) - solid_axis[1]),
        )
        <= tolerance
    ]
    internal = [node for node in terminals if node not in exterior]
    if len(exterior) != 1 or len(internal) != 1:
        return _failed(11, axis=axis, ramp_edges=ramp_edges, shared_edges=1, terminals=2)
    exterior_edges = len(graph.edges(exterior[0]))
    internal_edges = len(graph.edges(internal[0]))
    if not _is_convex(graph, left, exterior[0]) or not _is_convex(graph, right, exterior[0]):
        return _failed(
            12,
            axis=axis,
            ramp_edges=ramp_edges,
            shared_edges=1,
            terminals=2,
            internal_edges=internal_edges,
            exterior_edges=exterior_edges,
        )
    if not _is_concave(graph, left, internal[0]) or not _is_concave(graph, right, internal[0]):
        return _failed(
            13,
            axis=axis,
            ramp_edges=ramp_edges,
            shared_edges=1,
            terminals=2,
            internal_edges=internal_edges,
            exterior_edges=exterior_edges,
        )
    edge_box = shared[0].bounding_box()
    edge_bounds = (
        (edge_box.min.X, edge_box.max.X),
        (edge_box.min.Y, edge_box.max.Y),
        (edge_box.min.Z, edge_box.max.Z),
    )
    run_tolerance = length_tol(
        max(left_span[axis][1] - left_span[axis][0], right_span[axis][1] - right_span[axis][0]),
        rel=1e-9,
        floor=1e-6,
    )
    shared_run = edge_bounds[axis]
    full_run = not any(
        abs(face_run[end] - shared_run[end]) > run_tolerance
        for face_run in (left_span[axis], right_span[axis])
        for end in (0, 1)
    )
    if not full_run:
        return _failed(
            14,
            axis=axis,
            ramp_edges=ramp_edges,
            shared_edges=1,
            terminals=2,
            internal_edges=internal_edges,
            exterior_edges=exterior_edges,
            full_run=False,
            internal_index=internal[0].index,
            exterior_index=exterior[0].index,
        )
    return _failed(
        15,
        axis=axis,
        mirror_delta=mirror_delta,
        ramp_edges=ramp_edges,
        shared_edges=1,
        terminals=2,
        internal_edges=internal_edges,
        exterior_edges=exterior_edges,
        full_run=True,
        internal_index=internal[0].index,
        exterior_index=exterior[0].index,
    )


def _ramp_boundary_bypass_pairs(
    graph: FaceGraph,
    nodes: tuple[FaceNode, ...],
    other_claims: dict[FaceNode, set[str]] | None = None,
    accepted_claims: tuple[tuple[str, frozenset[FaceNode]], ...] = (),
) -> tuple[dict[str, Any], ...]:
    """Return no legacy bypass projections now that production has no four-edge ramp gate."""

    ordered = tuple(sorted(set(nodes), key=lambda node: node.index))
    bevels: dict[FaceNode, tuple[Any, ...]] = {}
    for node in ordered:
        with contextlib.suppress(BevelReject):
            bevels[node] = classify_bevel(graph.face(node))
    rows: list[dict[str, Any]] = []
    for at, left in enumerate(ordered):
        for right in ordered[at + 1 :]:
            if left not in bevels or right not in bevels or right not in graph.neighbours(left):
                continue
            ordinary = _probe_pair(graph, left, right, bevels[left], bevels[right])
            if ordinary.first_failed_gate != "fragmented_ramp_boundary":
                continue
            bypass = _probe_pair(
                graph,
                left,
                right,
                bevels[left],
                bevels[right],
                bypass_ramp_boundary=True,
            )
            defining: list[int] = []
            if bypass.first_failed_gate == "recognisable":
                assert bypass.internal_terminal_index is not None
                defining = [left.index, right.index, bypass.internal_terminal_index]
            nodes_by_index = {node.index: node for node in graph.nodes}
            projected_nodes = {nodes_by_index[index] for index in defining}
            rows.append(
                {
                    "left_index": left.index,
                    "right_index": right.index,
                    "projected_defining_indices": sorted(defining),
                    "projected_defining_claims": _counts(
                        [
                            family
                            for node in projected_nodes
                            for family in sorted((other_claims or {}).get(node, set()))
                        ]
                    ),
                    "projected_record_overlaps": _counts(
                        [
                            family
                            for family, claim in accepted_claims
                            if family != FamilyId.PAIRED_RAMP_STEPS.value.replace("_", "-")
                            and bool(projected_nodes.intersection(claim))
                        ]
                    ),
                    "result": asdict(bypass),
                }
            )
    return tuple(rows)


def _describe_component(
    graph: FaceGraph,
    nodes: tuple[FaceNode, ...],
    other_claims: dict[FaceNode, set[str]],
    accepted_claims: tuple[tuple[str, frozenset[FaceNode]], ...] = (),
) -> ComponentAnatomy:
    ordered = tuple(sorted(nodes, key=lambda node: node.index))
    bevels: dict[FaceNode, tuple[Any, ...]] = {}
    for node in ordered:
        with contextlib.suppress(BevelReject):
            bevels[node] = classify_bevel(graph.face(node))
    probed_pairs = [
        (left, right, _probe_pair(graph, left, right, bevels[left], bevels[right]))
        for at, left in enumerate(ordered)
        for right in ordered[at + 1 :]
        if left in bevels and right in bevels and right in graph.neighbours(left)
    ]
    best = max(
        (probe for _left, _right, probe in probed_pairs),
        key=lambda probe: (probe.stage, probe.run_axis or ""),
        default=_failed(0),
    )
    component = set(ordered)
    return ComponentAnatomy(
        face_count=len(ordered),
        surface_counts=_counts([graph.face(node).geom_type.name for node in ordered]),
        planar_faces=sum(graph.is_planar(node) for node in ordered),
        bevel_faces=len(bevels),
        face_edge_counts=tuple(sorted(len(graph.edges(node)) for node in ordered)),
        faces_with_inner_wires=sum(len(graph.face(node).wires()) > 1 for node in ordered),
        faces_with_curved_edges=sum(
            any(edge.geom_type != GeomType.LINE for edge in graph.edges(node)) for node in ordered
        ),
        internal_arc_counts=_counts(
            [
                arc
                for at, left in enumerate(ordered)
                for right in ordered[at + 1 :]
                if (arc := _arc_name(graph, left, right)) is not None
            ]
        ),
        accepted_other_family_claims=_counts(
            [family for node in component for family in sorted(other_claims.get(node, set()))]
        ),
        accepted_other_family_records=_counts(
            [
                family
                for family, claim in accepted_claims
                if family != FamilyId.PAIRED_RAMP_STEPS.value.replace("_", "-")
                and bool(component.intersection(claim))
            ]
        ),
        pair_gate_counts=_counts(
            [probe.first_failed_gate for _left, _right, probe in probed_pairs]
        ),
        best_pair=best,
    )


def _rank(items: list[dict[str, Any]]) -> list[dict[str, Any]]:
    grouped: dict[str, dict[str, Any]] = {}
    for item in sorted(items, key=lambda row: (row["model_id"], row["face_indices"])):
        group = grouped.setdefault(
            item["anatomy_key"],
            {"anatomy": item["anatomy"], "components": 0, "faces": 0, "samples": []},
        )
        group["components"] += 1
        group["faces"] += item["face_count"]
        if len(group["samples"]) < 3:
            group["samples"].append(
                {"model_id": item["model_id"], "face_indices": item["face_indices"]}
            )
    return sorted(
        grouped.values(),
        key=lambda row: (-row["components"], -row["faces"], json.dumps(row["anatomy"])),
    )


def _reconciliation(rows: list[dict[str, Any]], labelled_faces: int) -> dict[str, int]:
    """Reconcile all labelled faces without hiding residuals in touched components."""

    matched = sum(len(row["matched_face_indices"]) for row in rows)
    unmatched = sum(len(row["unmatched_face_indices"]) for row in rows)
    if matched + unmatched != labelled_faces:
        raise RuntimeError("face reconciliation failed")
    recalled = sum(bool(row["matched_face_indices"]) for row in rows)
    partial = sum(
        bool(row["matched_face_indices"]) and bool(row["unmatched_face_indices"]) for row in rows
    )
    return {
        "labelled_faces": labelled_faces,
        "matched_defining_faces": matched,
        "unmatched_labelled_faces": unmatched,
        "derived_components": len(rows),
        "recalled_components": recalled,
        "unrecalled_components": len(rows) - recalled,
        "partially_recalled_components": partial,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("root", type=Path)
    parser.add_argument("--class-id", type=int, default=9)
    parser.add_argument("--limit", type=int, default=500)
    parser.add_argument("--dataset-version", default=_PUBLISHED_VERSION)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    from quiddity import import_step_geometry as import_step

    paths = sorted(args.root.glob("*.st*p"))[: args.limit]
    if not paths:
        parser.error("the selected workload contains no STEP files")
    selected = [(path, load_mfcadpp_truth(path)) for path in paths]
    items: list[dict[str, Any]] = []
    labelled_faces = 0
    for path, truth in selected:
        labelled = {index for index, value in enumerate(truth.semantic) if value == args.class_id}
        if not labelled:
            continue
        part = import_step(path)
        faces = tuple(part.faces())
        if len(faces) != len(truth.semantic):
            raise RuntimeError(f"{path.stem}: imported face count does not match labels")
        product = _take_inventory(part)
        graph = product.context.graph
        components = _components(graph, {graph.require_node(faces[index]) for index in labelled})
        family_claims: dict[FaceNode, set[str]] = {}
        accepted_claims: list[tuple[str, frozenset[FaceNode]]] = []
        paired_claims: list[frozenset[FaceNode]] = []
        for disposition in product.reconciliation.dispositions:
            if disposition.outcome is not Outcome.ACCEPTED:
                continue
            claim = product.evidence.defining_of(disposition.candidate)
            family = disposition.candidate.family.value.replace("_", "-")
            accepted_claims.append((family, claim))
            for node in claim:
                family_claims.setdefault(node, set()).add(family)
            if disposition.candidate.family is FamilyId.PAIRED_RAMP_STEPS:
                paired_claims.append(claim)
        labelled_faces += len(labelled)
        for component in components:
            matched = (
                set(component).intersection(set().union(*paired_claims)) if paired_claims else set()
            )
            anatomy = _describe_component(
                graph,
                tuple(component),
                family_claims,
                tuple(accepted_claims),
            )
            bypass_pairs = _ramp_boundary_bypass_pairs(
                graph,
                tuple(component),
                family_claims,
                tuple(accepted_claims),
            )
            items.append(
                {
                    "model_id": path.stem,
                    "source_sha256": truth.source_sha256,
                    "face_indices": sorted(node.index for node in component),
                    "matched_face_indices": sorted(node.index for node in matched),
                    "unmatched_face_indices": sorted(
                        node.index for node in set(component).difference(matched)
                    ),
                    "face_count": len(component),
                    "anatomy_key": anatomy.key(),
                    "anatomy": asdict(anatomy),
                    "ramp_boundary_bypass_pairs": bypass_pairs,
                }
            )
    unrecalled = [item for item in items if not item["matched_face_indices"]]
    partial = [
        item for item in items if item["matched_face_indices"] and item["unmatched_face_indices"]
    ]
    reconciliation = _reconciliation(items, labelled_faces)
    boundary_components = [item for item in items if item["ramp_boundary_bypass_pairs"]]
    projected_pairs = [
        (item, pair)
        for item in boundary_components
        for pair in item["ramp_boundary_bypass_pairs"]
        if pair["result"]["first_failed_gate"] == "recognisable"
    ]
    projected_faces = {
        (item["model_id"], index)
        for item, pair in projected_pairs
        for index in pair["projected_defining_indices"]
    }
    report = {
        "format": "b123d-recognisers-mfcadpp-paired-ramp-miss-audit",
        "format_version": 2,
        "implementation_commit": _commit(),
        "dataset": {
            "name": "MFCAD++",
            "version": args.dataset_version,
            "partition": "test",
            "root": str(args.root.resolve()),
        },
        "class_id": args.class_id,
        "component_derivation": (
            "connected components of same-class original faces under shared-edge adjacency"
        ),
        "native_instance_labels": False,
        "selection": {
            "limit": args.limit,
            "selected_ids_sha256": _selection_hash([path.stem for path in paths]),
            "selected_sources_sha256": _source_selection_hash(
                [(truth.model_id, truth.source_sha256) for _path, truth in selected]
            ),
        },
        "reconciliation": reconciliation,
        "gate_counts": dict(
            sorted(
                Counter(
                    item["anatomy"]["best_pair"]["first_failed_gate"] for item in unrecalled
                ).items()
            )
        ),
        "partial_component_gate_counts": dict(
            sorted(
                Counter(
                    item["anatomy"]["best_pair"]["first_failed_gate"] for item in partial
                ).items()
            )
        ),
        "ramp_boundary_bypass": {
            "affected_components": len(boundary_components),
            "wholly_unrecalled_components": sum(
                not item["matched_face_indices"] for item in boundary_components
            ),
            "partially_recalled_components": sum(
                bool(item["matched_face_indices"]) and bool(item["unmatched_face_indices"])
                for item in boundary_components
            ),
            "candidate_pairs": sum(
                len(item["ramp_boundary_bypass_pairs"]) for item in boundary_components
            ),
            "projected_recognisable_pairs": len(projected_pairs),
            "projected_components": len(
                {(item["model_id"], tuple(item["face_indices"])) for item, _pair in projected_pairs}
            ),
            "projected_distinct_defining_faces": len(projected_faces),
        },
        "ramp_boundary_bypass_gate_counts": dict(
            sorted(
                Counter(
                    pair["result"]["first_failed_gate"]
                    for item in items
                    for pair in item["ramp_boundary_bypass_pairs"]
                ).items()
            )
        ),
        "unrecalled_clusters": _rank(unrecalled),
        "partial_clusters": _rank(partial),
        "components": items,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    summary = {key: value for key, value in report.items() if key != "components"}
    print(json.dumps(summary, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
