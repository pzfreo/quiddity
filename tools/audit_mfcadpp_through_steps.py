#!/usr/bin/env python3
"""Audit unrecalled MFCAD++ through-step component proxies by geometric anatomy.

This is repository evidence tooling, not a recogniser.  Dataset labels select the faces to
describe, but never alter production candidates or predicates.  "Component" consistently means
the non-native shared-edge same-class proxy documented by Epic 0005.
"""

from __future__ import annotations

import argparse
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

from quiddity._adjacency import FaceGraph, FaceNode, axis_aligned_axis  # noqa: E402
from quiddity._candidates import FamilyId  # noqa: E402
from quiddity._dispositions import Outcome  # noqa: E402
from quiddity._geometry import COORD_FLOOR  # noqa: E402
from quiddity._volume_probe import prism_is_empty  # noqa: E402
from quiddity.result import _take_inventory  # noqa: E402
from quiddity.through_steps import (  # noqa: E402
    SPAN_EPS,
    _common_terminal,
    _four_principal_runs,
    _Region,
    _regions,
    _relation,
    _section_and_spans,
    _shared_run_is_complete,
)
from tools.derive_mfcadpp_components import _components  # noqa: E402
from tools.effectiveness_report import load_mfcadpp_truth  # noqa: E402

_AXES = "xyz"
_PUBLISHED_VERSION = (
    "MFCAD++ published test split; DOI 10.17034/d1fec5a0-8c10-4630-b02e-b92dc81df823"
)
_GATE_NAMES = (
    "no_orthogonal_rectangular_pair",
    "not_full_run",
    "not_concave",
    "not_on_envelope",
    "incomplete_seam",
    "missing_terminal",
    "third_cospanning_wall",
    "material_in_prism",
    "recognisable",
)


@dataclass(frozen=True, slots=True)
class ComponentAnatomy:
    """Traversal-independent descriptor for one labelled component proxy."""

    face_count: int
    surface_counts: tuple[tuple[str, int], ...]
    principal_plane_axes: tuple[tuple[str, int], ...]
    nonprincipal_planar_faces: int
    rectangular_outer_faces: int
    faces_with_inner_wires: int
    faces_with_curved_edges: int
    internal_arc_counts: tuple[tuple[str, int], ...]
    boundary_arc_counts: tuple[tuple[str, int], ...]
    inferred_run_axis: str | None
    full_run_faces: int | None
    terminal_count: int | None
    exact_empty_prism: bool | None
    first_failed_gate: str

    def key(self) -> str:
        """Stable orientation-neutral motif key containing no corpus identity.

        Exact axis names and the number of external adjacencies remain useful anatomy, but are
        presentation- and surrounding-part-dependent, so neither fragments the motif grouping.
        """

        motif = {
            "face_count": self.face_count,
            "surface_counts": self.surface_counts,
            "principal_plane_multiplicities": sorted(
                count for _axis, count in self.principal_plane_axes
            ),
            "nonprincipal_planar_faces": self.nonprincipal_planar_faces,
            "rectangular_outer_faces": self.rectangular_outer_faces,
            "faces_with_inner_wires": self.faces_with_inner_wires,
            "faces_with_curved_edges": self.faces_with_curved_edges,
            "internal_arc_counts": self.internal_arc_counts,
            "has_inferred_run_axis": self.inferred_run_axis is not None,
            "full_run_faces": self.full_run_faces,
            "terminal_count": self.terminal_count,
            "exact_empty_prism": self.exact_empty_prism,
            "first_failed_gate": self.first_failed_gate,
        }
        return json.dumps(motif, sort_keys=True, separators=(",", ":"))


@dataclass(frozen=True, slots=True)
class _PairProbe:
    stage: int
    run: int
    full_run_faces: int
    terminal_count: int | None
    exact_empty_prism: bool | None


def _commit() -> str:
    return subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, check=True, capture_output=True, text=True
    ).stdout.strip()


def _selection_hash(ids: list[str]) -> str:
    return hashlib.sha256(("\n".join(ids) + "\n").encode()).hexdigest()


def _counts(values: list[str]) -> tuple[tuple[str, int], ...]:
    return tuple(sorted(Counter(values).items()))


def _arc_name(graph: FaceGraph, left: FaceNode, right: FaceNode) -> str | None:
    """Return one closed arc value for descriptive evidence, preserving absence."""

    kind = graph.arc(left, right)
    return None if kind is None else str(kind)


def _solid_bounds(solid: Any) -> tuple[tuple[float, float], ...]:
    bounds = solid.bounding_box()
    return (
        (bounds.min.X, bounds.max.X),
        (bounds.min.Y, bounds.max.Y),
        (bounds.min.Z, bounds.max.Z),
    )


def _probe_pair(
    graph: FaceGraph,
    solid: Any,
    left: Any,
    right: Any,
    regions: list[Any],
    planes: dict[FaceNode, tuple[int, float] | None],
) -> _PairProbe | None:
    if left.normal_axis == right.normal_axis:
        return None
    run = 3 - left.normal_axis - right.normal_axis
    bounds = _solid_bounds(solid)
    low, high = left.bounds[run]
    full_run_faces = sum(
        abs(region.bounds[run][0] - bounds[run][0]) <= SPAN_EPS
        and abs(region.bounds[run][1] - bounds[run][1]) <= SPAN_EPS
        for region in (left, right)
    )
    if (
        abs(low - right.bounds[run][0]) > SPAN_EPS
        or abs(high - right.bounds[run][1]) > SPAN_EPS
        or full_run_faces != 2
    ):
        return _PairProbe(1, run, full_run_faces, None, None)
    if _relation(graph, left, right) != "concave":
        return _PairProbe(2, run, full_run_faces, None, None)
    measured = _section_and_spans(left, right, run, bounds)
    if measured is None:
        return _PairProbe(3, run, full_run_faces, None, None)
    _section, spans = measured
    if not _shared_run_is_complete(graph, left, right, run, low, high):
        return _PairProbe(4, run, full_run_faces, None, None)
    terminals = sum(
        _common_terminal(graph, left, right, run, station, spans, planes) for station in (low, high)
    )
    if terminals != 2:
        return _PairProbe(5, run, full_run_faces, terminals, None)
    if any(
        candidate not in (left, right)
        and abs(candidate.bounds[run][0] - low) <= SPAN_EPS
        and abs(candidate.bounds[run][1] - high) <= SPAN_EPS
        and (
            _relation(graph, left, candidate) == "concave"
            or _relation(graph, right, candidate) == "concave"
        )
        for candidate in regions
    ):
        return _PairProbe(6, run, full_run_faces, terminals, None)
    empty = prism_is_empty(spans, solid, inset=COORD_FLOOR)
    return _PairProbe(8 if empty else 7, run, full_run_faces, terminals, empty)


def describe_component(graph: FaceGraph, nodes: tuple[FaceNode, ...]) -> ComponentAnatomy:
    """Describe one same-class shared-edge component without retaining corpus identity."""

    ordered = tuple(sorted(nodes, key=lambda node: node.index))
    solid_ref = graph.common_valid_solid(ordered)
    if solid_ref is None:
        raise ValueError("component faces do not belong to exactly one valid solid")
    solid = graph.solid_shape(solid_ref)
    planes = {node: axis_aligned_axis(graph.face(node).wrapped) for node in graph.nodes}
    component = set(ordered)
    surface_counts = _counts([graph.face(node).geom_type.name for node in ordered])
    principal = _counts([_AXES[plane[0]] for node in ordered if (plane := planes[node])])
    nonprincipal = sum(graph.is_planar(node) and planes[node] is None for node in ordered)
    rectangular_outer = sum(
        plane is not None and _four_principal_runs(graph.face(node).outer_wire(), plane[0])
        for node in ordered
        if (plane := planes[node]) is not None
    )
    faces_with_inner_wires = sum(len(graph.face(node).wires()) > 1 for node in ordered)
    faces_with_curved_edges = sum(
        any(edge.geom_type.name != "LINE" for edge in graph.edges(node)) for node in ordered
    )
    internal = _counts(
        [
            kind
            for at, left in enumerate(ordered)
            for right in ordered[at + 1 :]
            if (kind := _arc_name(graph, left, right)) is not None
        ]
    )
    boundary = _counts(
        [
            kind
            for node in ordered
            for neighbour in graph.neighbours(node)
            if neighbour not in component
            if (kind := _arc_name(graph, node, neighbour)) is not None
        ]
    )
    regions = _regions(graph, {graph.require_node(face) for face in solid.faces()}, planes)
    touching = [region for region in regions if component.intersection(region.nodes)]
    probes = [
        probe
        for at, left in enumerate(touching)
        for right in touching[at + 1 :]
        if (probe := _probe_pair(graph, solid, left, right, regions, planes)) is not None
    ]
    best = max(probes, key=lambda probe: (probe.stage, -probe.run), default=None)
    component_plane_axes = {plane[0] for node in ordered if (plane := planes[node]) is not None}
    inferred_run = 3 - sum(component_plane_axes) if len(component_plane_axes) == 2 else None
    if best is not None:
        inferred_run = best.run
    diagnostic = best
    if best is None and len(ordered) == 2 and len(component_plane_axes) == 2:
        raw_regions = []
        for node in ordered:
            plane = planes[node]
            assert plane is not None
            raw_regions.append(_Region((node,), plane[0], plane[1], graph.bounds(node)))
        diagnostic = _probe_pair(
            graph,
            solid,
            raw_regions[0],
            raw_regions[1],
            regions,
            planes,
        )
    bounds = _solid_bounds(solid)
    full_run_faces = (
        None
        if inferred_run is None
        else sum(
            abs(graph.bounds(node)[inferred_run][0] - bounds[inferred_run][0]) <= SPAN_EPS
            and abs(graph.bounds(node)[inferred_run][1] - bounds[inferred_run][1]) <= SPAN_EPS
            for node in ordered
        )
    )
    terminal_count = None
    if inferred_run is not None:
        terminal_nodes = {
            neighbour
            for node in ordered
            for neighbour in graph.neighbours(node)
            if neighbour not in component
            and (plane := planes[neighbour]) is not None
            and plane[0] == inferred_run
            and any(abs(plane[1] - station) <= SPAN_EPS for station in bounds[inferred_run])
        }
        terminal_count = len(terminal_nodes)
    failure = _GATE_NAMES[0 if best is None else best.stage]
    if best is None and inferred_run is not None:
        failure = "nonrectangular_regions"
    return ComponentAnatomy(
        face_count=len(ordered),
        surface_counts=surface_counts,
        principal_plane_axes=principal,
        nonprincipal_planar_faces=nonprincipal,
        internal_arc_counts=internal,
        boundary_arc_counts=boundary,
        inferred_run_axis=None if inferred_run is None else _AXES[inferred_run],
        rectangular_outer_faces=rectangular_outer,
        faces_with_inner_wires=faces_with_inner_wires,
        faces_with_curved_edges=faces_with_curved_edges,
        full_run_faces=full_run_faces if diagnostic is None else diagnostic.full_run_faces,
        terminal_count=terminal_count if diagnostic is None else diagnostic.terminal_count,
        exact_empty_prism=None if diagnostic is None else diagnostic.exact_empty_prism,
        first_failed_gate=failure,
    )


def _rank_clusters(items: list[dict[str, Any]]) -> list[dict[str, Any]]:
    grouped: dict[str, dict[str, Any]] = {}
    for item in sorted(items, key=lambda row: (row["model_id"], row["face_indices"])):
        key = item["anatomy_key"]
        group = grouped.setdefault(
            key,
            {
                "anatomy": item["anatomy"],
                "components": 0,
                "faces": 0,
                "sample_components": [],
            },
        )
        group["components"] += 1
        group["faces"] += item["face_count"]
        if len(group["sample_components"]) < 3:
            group["sample_components"].append(
                {"model_id": item["model_id"], "face_indices": item["face_indices"]}
            )
    return sorted(
        grouped.values(),
        key=lambda row: (-row["components"], -row["faces"], json.dumps(row["anatomy"])),
    )


def _is_two_wall_boundary_interruption(anatomy: dict[str, Any]) -> bool:
    """Whether only non-pristine wall boundaries block the complete two-wall proof."""

    return bool(
        anatomy["first_failed_gate"] == "nonrectangular_regions"
        and anatomy["face_count"] == 2
        and anatomy["surface_counts"] == (("PLANE", 2),)
        and sorted(count for _axis, count in anatomy["principal_plane_axes"]) == [1, 1]
        and anatomy["nonprincipal_planar_faces"] == 0
        and anatomy["internal_arc_counts"] == (("concave", 1),)
        and anatomy["full_run_faces"] == 2
        and anatomy["terminal_count"] == 2
        and anatomy["exact_empty_prism"] is True
    )


def _rank_broad_motifs(items: list[dict[str, Any]]) -> list[dict[str, Any]]:
    interrupted = [item for item in items if _is_two_wall_boundary_interruption(item["anatomy"])]
    subtypes = Counter(
        (
            item["anatomy"]["rectangular_outer_faces"],
            item["anatomy"]["faces_with_inner_wires"],
            item["anatomy"]["faces_with_curved_edges"],
        )
        for item in interrupted
    )
    return [
        {
            "motif": "two_wall_boundary_interruption",
            "definition": (
                "exactly two orthogonal principal planar faces with one concave join, both "
                "spanning the inferred run, two terminal planes, and an exactly empty envelope "
                "prism; one or both wall boundaries are not pristine four-run rectangles"
            ),
            "components": len(interrupted),
            "faces": sum(item["face_count"] for item in interrupted),
            "component_share_of_unrecalled": (len(interrupted) / len(items) if items else None),
            "subtypes": [
                {
                    "rectangular_outer_faces": key[0],
                    "faces_with_inner_wires": key[1],
                    "faces_with_curved_edges": key[2],
                    "components": count,
                }
                for key, count in sorted(subtypes.items(), key=lambda row: (-row[1], row[0]))
            ],
        }
    ]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("root", type=Path)
    parser.add_argument("--class-id", type=int, default=8)
    parser.add_argument("--limit", type=int, default=500)
    parser.add_argument("--dataset-version", default=_PUBLISHED_VERSION)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    from quiddity import import_step_geometry as import_step

    paths = sorted(args.root.glob("*.st*p"))[: args.limit]
    if not paths:
        parser.error("the selected workload contains no STEP files")
    items: list[dict[str, Any]] = []
    recalled_components = recalled_faces = labelled_faces = 0
    derived_components = 0
    for path in paths:
        truth = load_mfcadpp_truth(path)
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
        claims = tuple(
            product.evidence.defining_of(disposition.candidate)
            for disposition in product.reconciliation.for_family(FamilyId.THROUGH_STEPS)
            if disposition.outcome is Outcome.ACCEPTED
        )
        labelled_faces += len(labelled)
        derived_components += len(components)
        for component in components:
            claimed_nodes = set().union(*(component & claim for claim in claims))
            recalled = bool(claimed_nodes)
            if recalled:
                if claimed_nodes != set(component):
                    raise RuntimeError(f"{path.stem}: partially recalled component proxy")
                recalled_components += 1
                recalled_faces += len(claimed_nodes)
                continue
            anatomy = describe_component(graph, tuple(component))
            items.append(
                {
                    "model_id": path.stem,
                    "face_indices": sorted(node.index for node in component),
                    "face_count": len(component),
                    "anatomy_key": anatomy.key(),
                    "anatomy": asdict(anatomy),
                }
            )
    unrecalled_faces = sum(item["face_count"] for item in items)
    report = {
        "format": "b123d-recognisers-mfcadpp-through-step-miss-audit",
        "format_version": 1,
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
        },
        "reconciliation": {
            "labelled_faces": labelled_faces,
            "recalled_faces": recalled_faces,
            "unrecalled_faces": unrecalled_faces,
            "derived_components": derived_components,
            "recalled_components": recalled_components,
            "unrecalled_components": len(items),
        },
        "ranked_broad_motifs": _rank_broad_motifs(items),
        "clusters": _rank_clusters(items),
        "unrecalled_components": items,
    }
    if recalled_faces + unrecalled_faces != labelled_faces:
        raise RuntimeError("face reconciliation failed")
    if recalled_components + len(items) != derived_components:
        raise RuntimeError("component reconciliation failed")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    summary = {key: value for key, value in report.items() if key != "unrecalled_components"}
    print(json.dumps(summary, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
