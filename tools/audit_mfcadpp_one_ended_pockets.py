#!/usr/bin/env python3
"""Audit one-mouth polygonal cavity regions without changing recognition.

Candidate geometry is built before dataset labels are read. Labels are used only to measure the
reach and confusion of the resulting regions. This tests issue #456's one-ended analogue of the
two-mouth Passage proof; it is not a recogniser and cannot issue evidence.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import hashlib
import json
import math
import multiprocessing
import os
import subprocess
import sys
import time
from collections import Counter
from collections.abc import Iterable
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any

ROOT = Path(__file__).parents[1]
sys.path.insert(0, str(ROOT))

from quiddity._adjacency import FaceGraph, FaceNode  # noqa: E402
from quiddity._candidates import FamilyId  # noqa: E402
from quiddity._dispositions import Outcome  # noqa: E402
from quiddity._geometry import SMOOTH_ARC_GAP  # noqa: E402
from quiddity._section_passages import (  # noqa: E402
    _COORD_FLOOR,
    _END_PROBE,
    _MATERIAL_VOL_FRAC,
    _end_slab,
    _line_section,
    _material_fraction,
    _parallel,
    _probe_prism,
)
from quiddity._sections import LocalFrame, PlanarSection  # noqa: E402
from quiddity.result import _take_inventory  # noqa: E402
from tools.audit_mfcadpp_cavity_enclosures import (  # noqa: E402
    _candidate_regions,
    _convex_mouth,
    _wire_seed,
)
from tools.derive_mfcadpp_components import _components  # noqa: E402
from tools.effectiveness_report import load_mfcadpp_truth  # noqa: E402
from tools.run_effectiveness_baseline import _KNOWN_MFCADPP_2500_INVALID  # noqa: E402

_PUBLISHED_VERSION = (
    "MFCAD++ published test split; DOI 10.17034/d1fec5a0-8c10-4630-b02e-b92dc81df823"
)
_KNOWN_INVALID_REASON = "Hole cylindrical evidence does not prove one valid solid"
_TARGET_SIDES = {13: 3, 14: 4, 15: 6}


@dataclass(frozen=True, slots=True)
class OneEndedRegion:
    region: frozenset[FaceNode]
    opening: FaceNode
    floor: frozenset[FaceNode]
    section: PlanarSection
    run: tuple[float, float, float]
    mouth_at: float
    floor_at: float


@dataclass(frozen=True, slots=True)
class RegionProbe:
    first_failed_gate: str
    mouths: int
    section_sides: int | None = None
    raw_section_sides: int | None = None
    floor_planes: int | None = None
    floor_faces: int = 0


def _commit() -> str:
    return subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, check=True, capture_output=True, text=True
    ).stdout.strip()


def _selection_hash(values: list[str]) -> str:
    return hashlib.sha256(("\n".join(values) + "\n").encode()).hexdigest()


def _point(value: object) -> tuple[float, float, float]:
    return (float(value.X), float(value.Y), float(value.Z))  # type: ignore[attr-defined]


def _dot(left: tuple[float, ...], right: tuple[float, ...]) -> float:
    return sum(a * b for a, b in zip(left, right, strict=True))


def _mouth_wires(
    graph: FaceGraph, region: frozenset[FaceNode], owners: frozenset[FaceNode]
) -> tuple[tuple[FaceNode, Any], ...]:
    found = []
    for owner in sorted(owners, key=lambda node: node.index):
        if not _convex_mouth(graph, owner, region) or not graph.is_planar(owner):
            continue
        for wire in graph.face(owner).inner_wires():
            seed = _wire_seed(graph, owner, wire)
            if seed and seed <= region and all(graph.arc(owner, node) == "convex" for node in seed):
                found.append((owner, wire))
    return tuple(found)


def _plane_at(graph: FaceGraph, node: FaceNode, run: tuple[float, float, float]) -> float | None:
    normal = graph.normal(node)
    if normal is None or not _parallel(normal, run):
        return None
    try:
        vertices = graph.face(node).vertices()
        if not vertices:
            return None
        values = tuple(_dot(_point(vertex), run) for vertex in vertices)
    except (AttributeError, RuntimeError, TypeError, ValueError):
        return None
    if max(values) - min(values) > _COORD_FLOOR:
        return None
    return sum(values) / len(values)


def _floor_clusters(
    graph: FaceGraph,
    region: frozenset[FaceNode],
    run: tuple[float, float, float],
    mouth_at: float,
) -> tuple[tuple[float, frozenset[FaceNode]], ...]:
    planes: list[tuple[float, FaceNode]] = []
    for node in region:
        at = _plane_at(graph, node, run)
        if at is not None and abs(at - mouth_at) > _COORD_FLOOR:
            planes.append((at, node))
    clusters: list[tuple[float, set[FaceNode]]] = []
    for at, node in sorted(planes, key=lambda item: (item[0], item[1].index)):
        if clusters and abs(at - clusters[-1][0]) <= _COORD_FLOOR:
            clusters[-1][1].add(node)
        else:
            clusters.append((at, {node}))
    return tuple((at, frozenset(nodes)) for at, nodes in clusters)


def _void_and_closed(
    solid: Any,
    frame: LocalFrame,
    mouth_at: float,
    floor_at: float,
    section: PlanarSection,
) -> bool:
    """Prove void inside, exterior at the mouth, and material behind the floor."""

    low, high = sorted((mouth_at, floor_at))
    interval = (low, high)
    try:
        if _material_fraction(solid, _probe_prism(frame, interval, section)) > _MATERIAL_VOL_FRAC:
            return False
        scale = max(1.0, interval[1] - interval[0])
        radius = max(math.hypot(*vertex.point) for vertex in section.boundary)
        thickness = max(_END_PROBE, scale * 1e-4, radius * 1e-4)
        mouth_sign = -1.0 if mouth_at == interval[0] else 1.0
        floor_sign = -mouth_sign
        mouth_slab = _end_slab(frame, mouth_at, mouth_sign, thickness, section)
        floor_slab = _end_slab(frame, floor_at, floor_sign, thickness, section)
        return (
            _material_fraction(solid, mouth_slab) <= _MATERIAL_VOL_FRAC
            and _material_fraction(solid, floor_slab) >= 1.0 - _MATERIAL_VOL_FRAC
        )
    except (RuntimeError, TypeError, ValueError, ZeroDivisionError):
        return False


def _without_collinear_subdivisions(section: PlanarSection) -> PlanarSection | None:
    """Collapse only cyclic, co-directed straight runs in an already-linear boundary.

    A STEP exporter may split one geometric side at a harmless vertex.  That changes the raw
    edge count but not the polygon.  Reversals and genuine corners remain; degenerate results
    fail closed.  The tolerance is the shared dimensionless smooth-direction tolerance used by
    the existing subdivided-terminal query, rather than a corpus-derived distance.
    """

    points = tuple(vertex.point for vertex in section.boundary)
    if len(points) < 3:
        return None

    def direction(
        left: tuple[float, float], right: tuple[float, float]
    ) -> tuple[float, float] | None:
        delta = (right[0] - left[0], right[1] - left[1])
        length = math.hypot(*delta)
        return None if length == 0.0 else (delta[0] / length, delta[1] / length)

    retained = []
    for previous, current, following in zip(
        points[-1:] + points[:-1], points, points[1:] + points[:1], strict=True
    ):
        incoming = direction(previous, current)
        outgoing = direction(current, following)
        if incoming is None or outgoing is None:
            return None
        if 1.0 - _dot(incoming, outgoing) > SMOOTH_ARC_GAP:
            retained.append(current)
    if len(retained) < 3:
        return None
    try:
        return PlanarSection(tuple(type(section.boundary[0])(point) for point in retained))
    except ValueError:
        return None


def _probe_region(
    graph: FaceGraph,
    region: frozenset[FaceNode],
    owners: frozenset[FaceNode],
    expected_sides: int,
) -> tuple[RegionProbe, OneEndedRegion | None]:
    mouths = _mouth_wires(graph, region, owners)
    if len(mouths) != 1:
        return RegionProbe("not_one_mouth", len(mouths)), None
    opening, wire = mouths[0]
    normal = graph.normal(opening)
    if normal is None:
        return RegionProbe("opening_run", 1), None
    base = LocalFrame.canonical(normal, (0.0, 0.0, 0.0))
    section_read = _line_section(wire, base)
    if section_read is None:
        return RegionProbe("straight_polygonal_mouth", 1), None
    raw_section, centre = section_read
    section = _without_collinear_subdivisions(raw_section)
    if section is None:
        return RegionProbe(
            "degenerate_polygonal_mouth", 1, raw_section_sides=len(raw_section.boundary)
        ), None
    if len(section.boundary) != expected_sides:
        return RegionProbe(
            "unexpected_side_count", 1, len(section.boundary), len(raw_section.boundary)
        ), None
    frame = LocalFrame.canonical(base.run, centre)
    mouth_at = _plane_at(graph, opening, frame.run)
    if mouth_at is None:
        return RegionProbe("opening_run", 1, expected_sides, len(raw_section.boundary)), None
    floors = _floor_clusters(graph, region, frame.run, mouth_at)
    if len(floors) != 1:
        return RegionProbe(
            "not_one_floor_plane",
            1,
            expected_sides,
            len(raw_section.boundary),
            len(floors),
        ), None
    floor_at, floor = floors[0]
    if abs(floor_at - mouth_at) <= _COORD_FLOOR:
        return RegionProbe(
            "zero_depth", 1, expected_sides, len(raw_section.boundary), 1, len(floor)
        ), None
    solid = graph.common_valid_solid(region | owners | floor)
    if solid is None:
        return RegionProbe(
            "not_one_valid_solid",
            1,
            expected_sides,
            len(raw_section.boundary),
            1,
            len(floor),
        ), None
    if not _void_and_closed(graph.solid_shape(solid), frame, mouth_at, floor_at, section):
        return RegionProbe(
            "not_bounded_prismatic_void",
            1,
            expected_sides,
            len(raw_section.boundary),
            1,
            len(floor),
        ), None
    return (
        RegionProbe("candidate", 1, expected_sides, len(raw_section.boundary), 1, len(floor)),
        OneEndedRegion(region, opening, floor, section, frame.run, mouth_at, floor_at),
    )


def _one_ended_regions(
    graph: FaceGraph, expected_sides: int = 6
) -> tuple[tuple[RegionProbe, OneEndedRegion | None], ...]:
    return tuple(
        _probe_region(graph, region, owners, expected_sides)
        for region, owners in _candidate_regions(graph)
    )


def _accepted_constituent(product: Any) -> frozenset[FaceNode]:
    nodes: set[FaceNode] = set()
    for family in FamilyId:
        if family is FamilyId.LEGACY:
            continue
        for disposition in product.reconciliation.for_family(family):
            if disposition.outcome is Outcome.ACCEPTED:
                nodes.update(product.evidence.constituent_of(disposition.candidate))
    return frozenset(nodes)


def _audit_model(
    path: Path, class_id: int
) -> tuple[str, list[dict[str, Any]], Counter[str], dict[str, str] | None]:
    """Evaluate one immutable model; the parent retains ordering and report authority."""

    from quiddity import import_step_geometry

    part = import_step_geometry(path)
    try:
        product = _take_inventory(part)
    except (RuntimeError, ValueError) as error:
        truth = load_mfcadpp_truth(path)
        if truth.model_id not in _KNOWN_MFCADPP_2500_INVALID or str(error) != _KNOWN_INVALID_REASON:
            raise
        return (
            f"{truth.model_id}:{truth.source_sha256}",
            [],
            Counter(),
            {
                "model_id": truth.model_id,
                "source_sha256": truth.source_sha256,
                "reason": str(error),
            },
        )
    graph = product.context.graph
    accepted_constituent = _accepted_constituent(product)
    candidate_regions = _candidate_regions(graph)
    expected_sides = _TARGET_SIDES[class_id]
    probes = tuple(
        (region, owners, *_probe_region(graph, region, owners, expected_sides))
        for region, owners in candidate_regions
    )

    # Labels score the complete, fully probed geometric roster; they never author a candidate.
    truth = load_mfcadpp_truth(path)
    faces = tuple(part.faces())
    if len(faces) != len(truth.semantic):
        raise RuntimeError(f"{truth.model_id}: imported face count does not match labels")
    labelled = {
        graph.require_node(faces[index])
        for index, label in enumerate(truth.semantic)
        if label == class_id
    }
    components = _components(graph, labelled) if labelled else ()
    rows: list[dict[str, Any]] = []
    gates: Counter[str] = Counter()
    for region, owners, probe, candidate in probes:
        gates[probe.first_failed_gate] += 1
        labels = Counter(truth.semantic[node.index] for node in region)
        overlaps = [len(region & component) for component in components]
        target_nodes = frozenset(node for node in region if truth.semantic[node.index] == class_id)
        rows.append(
            {
                "model_id": truth.model_id,
                "source_sha256": truth.source_sha256,
                "probe": asdict(probe),
                "region_faces": sorted(node.index for node in region),
                "opening_faces": sorted(node.index for node in owners),
                "accepted_as_candidate": candidate is not None,
                "candidate_opening_face": candidate.opening.index if candidate else None,
                "candidate_floor_faces": (
                    sorted(node.index for node in candidate.floor) if candidate else []
                ),
                "labels": dict(sorted(labels.items())),
                "target_faces": labels[class_id],
                "target_face_indices": sorted(node.index for node in target_nodes),
                "target_faces_already_covered": len(target_nodes & accepted_constituent),
                "target_face_indices_already_covered": sorted(
                    node.index for node in target_nodes & accepted_constituent
                ),
                "target_faces_new_if_accepted": (
                    len(target_nodes - accepted_constituent) if candidate else 0
                ),
                "target_components": sum(overlap > 0 for overlap in overlaps),
                "largest_component_overlap": max(overlaps, default=0),
                "pure_target": bool(labels) and set(labels) == {class_id},
                "depth": abs(candidate.floor_at - candidate.mouth_at) if candidate else None,
            }
        )
    return f"{truth.model_id}:{truth.source_sha256}", rows, gates, None


def _audit_model_star(
    args: tuple[Path, int],
) -> tuple[str, list[dict[str, Any]], Counter[str], dict[str, str] | None]:
    return _audit_model(*args)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("--class-id", type=int, default=15)
    parser.add_argument("--limit", type=int, default=500)
    parser.add_argument(
        "--allow-invalid",
        action="store_true",
        help="retain the seven documented invalid rows in the complete 2,500-model selection",
    )
    parser.add_argument(
        "--workers",
        type=int,
        default=min(4, os.cpu_count() or 1),
        help="independent model workers; lexical report order is retained",
    )
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    if args.class_id not in _TARGET_SIDES:
        parser.error("--class-id must identify a polygonal Pocket class: 13, 14, or 15")

    paths = sorted(args.root.glob("*.st*p"))[: args.limit]
    if not paths:
        parser.error("the selected workload contains no STEP files")
    started = time.perf_counter()
    rows: list[dict[str, Any]] = []
    gates: Counter[str] = Counter()
    sources: list[str] = []
    selected_ids = [path.stem for path in paths]
    full_known_selection = (
        len(selected_ids) == 2500 and set(selected_ids) >= _KNOWN_MFCADPP_2500_INVALID
    )
    if full_known_selection and not args.allow_invalid:
        parser.error(
            "the known MFCAD++-2,500 selection contains seven invalid models; "
            "supply the documented --allow-invalid policy before recognition"
        )
    invalid: list[dict[str, str]] = []
    if args.workers < 1:
        parser.error("--workers must be positive")
    work = ((path, args.class_id) for path in paths)
    results: Iterable[tuple[str, list[dict[str, Any]], Counter[str], dict[str, str] | None]]
    if args.workers == 1:
        results = map(lambda item: _audit_model(*item), work)
    else:
        executor = concurrent.futures.ProcessPoolExecutor(
            max_workers=args.workers, mp_context=multiprocessing.get_context("spawn")
        )
        results = executor.map(_audit_model_star, work)
    try:
        for source, model_rows, model_gates, model_invalid in results:
            sources.append(source)
            rows.extend(model_rows)
            gates.update(model_gates)
            if model_invalid is not None:
                invalid.append(model_invalid)
    finally:
        if args.workers != 1:
            executor.shutdown()
    if invalid and not args.allow_invalid:
        parser.error("audit encountered invalid models without --allow-invalid")
    if (
        full_known_selection
        and {item["model_id"] for item in invalid} != _KNOWN_MFCADPP_2500_INVALID
    ):
        parser.error("the full-corpus invalid-model set differs from the documented policy")
    evidence_rows = [row for row in rows if row["target_faces"] > 0 or row["accepted_as_candidate"]]
    report = {
        "format": "b123d-recognisers-mfcadpp-one-ended-pocket-audit",
        "format_version": 1,
        "implementation_commit": _commit(),
        "dataset_version": _PUBLISHED_VERSION,
        "class_id": args.class_id,
        "selection": {
            "limit": args.limit,
            "selected_models": len(paths),
            "evaluated_models": len(paths) - len(invalid),
            "allow_invalid": args.allow_invalid,
            "selected_ids_sha256": _selection_hash(selected_ids),
            "selected_sources_sha256": _selection_hash(sources),
        },
        "invalid_models": invalid,
        "invalid_policy": {
            "expected_ids": sorted(_KNOWN_MFCADPP_2500_INVALID),
            "expected_reason": _KNOWN_INVALID_REASON,
        },
        "regions": len(rows),
        "reported_rows": len(evidence_rows),
        "row_policy": "target-touching or accepted candidate regions",
        "candidate_regions": sum(row["accepted_as_candidate"] for row in rows),
        "target_touching_regions": sum(row["target_faces"] > 0 for row in rows),
        "candidate_target_touching_regions": sum(
            row["accepted_as_candidate"] and row["target_faces"] > 0 for row in rows
        ),
        "candidate_pure_target_regions": sum(
            row["accepted_as_candidate"] and row["pure_target"] for row in rows
        ),
        "target_faces_reached": len(
            {
                (row["model_id"], face)
                for row in rows
                if row["accepted_as_candidate"]
                for face in row["target_face_indices"]
            }
        ),
        "new_target_faces_if_accepted": len(
            {
                (row["model_id"], face)
                for row in rows
                if row["accepted_as_candidate"]
                for face in row["target_face_indices"]
                if face not in row["target_face_indices_already_covered"]
            }
        ),
        "first_failed_gates": dict(sorted(gates.items())),
        "target_first_failed_gates": dict(
            sorted(
                Counter(
                    row["probe"]["first_failed_gate"] for row in rows if row["target_faces"] > 0
                ).items()
            )
        ),
        "runtime_seconds": time.perf_counter() - started,
        "rows": evidence_rows,
    }
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({key: value for key, value in report.items() if key != "rows"}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
