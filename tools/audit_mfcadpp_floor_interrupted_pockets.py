#!/usr/bin/env python3
"""Audit polygonal pockets whose complete wall cycle is interrupted at the floor.

Candidates are constructed from STEP geometry before dataset labels are read.  This is an
audit, not a recogniser: labels measure reach and confusion but cannot author a candidate.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import hashlib
import json
import multiprocessing
import os
import subprocess
import sys
import time
from collections import Counter, defaultdict
from collections.abc import Iterable
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any

ROOT = Path(__file__).parents[1]
sys.path.insert(0, str(ROOT))

from quiddity._adjacency import FaceGraph, FaceNode  # noqa: E402
from quiddity._geometry import AXIS_ZERO_COS  # noqa: E402
from quiddity._rings import SPAN_EPS, _cross_section  # noqa: E402
from quiddity.prismatic_pockets import (  # noqa: E402
    _END_PROBE,
    _MATERIAL_VOL_FRAC,
    _axis_for_opening,
    _inner_region,
    _material_fraction,
    _plane_at,
    _section_prism,
    _section_slab,
    _wire_seed,
)
from quiddity.result import _take_inventory  # noqa: E402
from tools.audit_mfcadpp_one_ended_pockets import _accepted_constituent  # noqa: E402
from tools.effectiveness_report import load_mfcadpp_truth  # noqa: E402
from tools.run_effectiveness_baseline import _KNOWN_MFCADPP_2500_INVALID  # noqa: E402

_PUBLISHED_VERSION = (
    "MFCAD++ published test split; DOI 10.17034/d1fec5a0-8c10-4630-b02e-b92dc81df823"
)
_KNOWN_INVALID_REASON = "Hole cylindrical evidence does not prove one valid solid"
_TARGET_CLASSES = frozenset({13, 14, 15})


@dataclass(frozen=True, slots=True)
class FloorInterruptedRegion:
    region: frozenset[FaceNode]
    opening: FaceNode
    floor: frozenset[FaceNode]
    walls: tuple[FaceNode, ...]
    interruptions: frozenset[FaceNode]
    axis: int


@dataclass(frozen=True, slots=True)
class RegionProbe:
    first_failed_gate: str
    wall_count: int = 0
    shortened_walls: int = 0
    interruption_faces: int = 0


def _commit() -> str:
    return subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()


def _selection_hash(values: list[str]) -> str:
    return hashlib.sha256(("\n".join(values) + "\n").encode()).hexdigest()


def _raw_regions(
    graph: FaceGraph,
) -> dict[frozenset[FaceNode], list[tuple[FaceNode, frozenset[FaceNode], int]]]:
    raw: dict[frozenset[FaceNode], list[tuple[FaceNode, frozenset[FaceNode], int]]] = defaultdict(
        list
    )
    for opening in graph.nodes:
        axis = _axis_for_opening(graph, opening) if graph.is_planar(opening) else None
        if axis is None:
            continue
        for wire in graph.face(opening).inner_wires():
            seed = _wire_seed(graph, opening, wire)
            arcs = []
            for node in seed:
                kind = graph.arc(opening, node)
                arcs.append(kind)
            if seed and all(kind in ("convex", "smooth") for kind in arcs) and "convex" in arcs:
                raw[_inner_region(graph, opening, seed)].append((opening, seed, axis))
    return raw


def _floor_groups(
    graph: FaceGraph,
    region: frozenset[FaceNode],
    axis: int,
    mouth_at: float,
) -> tuple[tuple[float, frozenset[FaceNode]], ...]:
    planes = sorted(
        (
            (at, node)
            for node in region
            if (at := _plane_at(graph, node, axis)) is not None and abs(at - mouth_at) > SPAN_EPS
        ),
        key=lambda item: (item[0], item[1].index),
    )
    groups: list[tuple[float, set[FaceNode]]] = []
    for at, node in planes:
        if groups and abs(at - groups[-1][0]) <= SPAN_EPS:
            groups[-1][1].add(node)
        else:
            groups.append((at, {node}))
    return tuple((at, frozenset(nodes)) for at, nodes in groups)


def _probe_region(
    graph: FaceGraph,
    region: frozenset[FaceNode],
    mouths: list[tuple[FaceNode, frozenset[FaceNode], int]],
) -> tuple[RegionProbe, FloorInterruptedRegion | None]:
    if len(mouths) != 1:
        return RegionProbe("not_one_mouth"), None
    opening, _seed, axis = mouths[0]
    mouth_at = _plane_at(graph, opening, axis)
    if mouth_at is None:
        return RegionProbe("opening_plane"), None
    floors = _floor_groups(graph, region, axis, mouth_at)
    if len(floors) != 1:
        return RegionProbe("not_one_floor"), None
    floor_at, floor = floors[0]
    walls = tuple(
        sorted(
            (
                node
                for node in region
                if graph.is_planar(node)
                and (normal := graph.normal(node)) is not None
                and abs(normal[axis]) <= AXIS_ZERO_COS
            ),
            key=lambda node: node.index,
        )
    )
    wall_set = set(walls)
    interruptions = region - wall_set - floor
    if len(walls) < 3 or any(len(set(graph.neighbours(node)) & wall_set) != 2 for node in walls):
        return RegionProbe("not_complete_wall_cycle", len(walls), 0, len(interruptions)), None
    if not interruptions or any(not graph.is_planar(node) for node in interruptions):
        return RegionProbe("not_planar_floor_interruption", len(walls), 0, len(interruptions)), None

    direction = 1 if floor_at > mouth_at else -1
    far_by_wall: dict[FaceNode, float] = {}
    for wall in walls:
        low, high = graph.bounds(wall)[axis]
        if abs(low - mouth_at) <= SPAN_EPS:
            far_by_wall[wall] = high
        elif abs(high - mouth_at) <= SPAN_EPS:
            far_by_wall[wall] = low
        else:
            return RegionProbe("wall_not_mouth_rooted", len(walls), 0, len(interruptions)), None
    shortened = frozenset(
        wall for wall, far in far_by_wall.items() if abs(far - floor_at) > SPAN_EPS
    )
    if not shortened or any(
        direction * (floor_at - far_by_wall[wall]) <= SPAN_EPS for wall in shortened
    ):
        return RegionProbe(
            "not_shortened_before_floor", len(walls), len(shortened), len(interruptions)
        ), None

    # Each interruption must be physical closure between the unique floor and at least one
    # shortened wall.  Conversely every shortened wall must terminate on such an interruption.
    if any(
        not any(graph.arc(node, floor_node) == "concave" for floor_node in floor)
        or not any(graph.arc(node, wall) == "concave" for wall in shortened)
        for node in interruptions
    ) or any(
        not any(graph.arc(wall, node) == "concave" for node in interruptions) for wall in shortened
    ):
        return RegionProbe(
            "interruption_does_not_bridge_floor", len(walls), len(shortened), len(interruptions)
        ), None

    section = _cross_section(graph, walls, wall_set, axis)
    solid = graph.common_valid_solid(region | {opening})
    clear_at = min(far_by_wall.values()) if direction > 0 else max(far_by_wall.values())
    low, high = sorted((mouth_at, clear_at))
    if section is None or solid is None or high - low <= SPAN_EPS:
        return RegionProbe(
            "not_clear_nominal_section", len(walls), len(shortened), len(interruptions)
        ), None
    thickness = max(_END_PROBE, abs(floor_at - mouth_at) * 1e-4)
    try:
        body = graph.solid_shape(solid)
        interior_fraction = _material_fraction(body, _section_prism(section, axis, low, high))
        backed = _material_fraction(
            body,
            _section_slab(section, axis, floor_at, direction, thickness),
        )
    except (RuntimeError, TypeError, ValueError, ZeroDivisionError):
        return RegionProbe(
            "floor_probe_failed", len(walls), len(shortened), len(interruptions)
        ), None
    if interior_fraction > _MATERIAL_VOL_FRAC:
        return RegionProbe(
            "not_clear_nominal_section", len(walls), len(shortened), len(interruptions)
        ), None
    if backed < 1.0 - _MATERIAL_VOL_FRAC:
        return RegionProbe(
            "floor_not_material_backed", len(walls), len(shortened), len(interruptions)
        ), None
    return (
        RegionProbe("candidate", len(walls), len(shortened), len(interruptions)),
        FloorInterruptedRegion(region, opening, floor, walls, interruptions, axis),
    )


def _audit_model(
    path: Path,
) -> tuple[str, list[dict[str, Any]], Counter[str], dict[str, str] | None]:
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
    accepted = _accepted_constituent(product)
    raw = _raw_regions(graph)
    intersecting = {
        region for region in raw if any(region != other and region & other for other in raw)
    }
    probes = [
        (region, *_probe_region(graph, region, mouths))
        for region, mouths in raw.items()
        if region not in intersecting
    ]

    # Labels evaluate an already complete geometric roster; they never author candidates.
    truth = load_mfcadpp_truth(path)
    if len(part.faces()) != len(truth.semantic):
        raise RuntimeError(f"{truth.model_id}: imported face count does not match labels")
    rows = []
    gates: Counter[str] = Counter()
    for region, probe, candidate in probes:
        gates[probe.first_failed_gate] += 1
        labels = Counter(truth.semantic[node.index] for node in region)
        target = frozenset(node for node in region if truth.semantic[node.index] in _TARGET_CLASSES)
        if candidate is not None:
            rows.append(
                {
                    "model_id": truth.model_id,
                    "source_sha256": truth.source_sha256,
                    "probe": asdict(probe),
                    "region_faces": sorted(node.index for node in region),
                    "labels": dict(sorted(labels.items())),
                    "accepted_as_candidate": candidate is not None,
                    "target_faces": len(target),
                    "target_face_indices": sorted(node.index for node in target),
                    "target_faces_new_if_accepted": len(target - accepted) if candidate else 0,
                    "pure_target_plus_interruption": bool(target)
                    and all(
                        label in _TARGET_CLASSES
                        or node in (candidate.interruptions if candidate else ())
                        for node, label in ((node, truth.semantic[node.index]) for node in region)
                    ),
                }
            )
    return f"{truth.model_id}:{truth.source_sha256}", rows, gates, None


def _audit_model_star(path: Path):
    return _audit_model(path)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("--limit", type=int, default=500)
    parser.add_argument("--allow-invalid", action="store_true")
    parser.add_argument("--workers", type=int, default=min(4, os.cpu_count() or 1))
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    paths = sorted(args.root.glob("*.st*p"))[: args.limit]
    if not paths or args.workers < 1:
        parser.error("select a non-empty workload and at least one worker")
    selected_ids = [path.stem for path in paths]
    full = len(paths) == 2500 and set(selected_ids) >= _KNOWN_MFCADPP_2500_INVALID
    if full and not args.allow_invalid:
        parser.error("the full selection needs the documented --allow-invalid policy")
    started = time.perf_counter()
    rows: list[dict[str, Any]] = []
    gates: Counter[str] = Counter()
    sources, invalid = [], []
    work: Iterable[Any]
    if args.workers == 1:
        work = map(_audit_model, paths)
    else:
        executor = concurrent.futures.ProcessPoolExecutor(
            max_workers=args.workers, mp_context=multiprocessing.get_context("spawn")
        )
        work = executor.map(_audit_model_star, paths)
    try:
        for source, model_rows, model_gates, bad in work:
            sources.append(source)
            rows.extend(model_rows)
            gates.update(model_gates)
            if bad is not None:
                invalid.append(bad)
    finally:
        if args.workers != 1:
            executor.shutdown()
    if invalid and not args.allow_invalid:
        parser.error("audit encountered invalid models without --allow-invalid")
    if full and {item["model_id"] for item in invalid} != _KNOWN_MFCADPP_2500_INVALID:
        parser.error("the full-corpus invalid-model set differs from the documented policy")
    candidates = [row for row in rows if row["accepted_as_candidate"]]
    report = {
        "format": "b123d-recognisers-mfcadpp-floor-interrupted-pocket-audit",
        "format_version": 1,
        "implementation_commit": _commit(),
        "dataset_version": _PUBLISHED_VERSION,
        "labels_read_after_candidate_construction": True,
        "selection": {
            "limit": args.limit,
            "selected_models": len(paths),
            "evaluated_models": len(paths) - len(invalid),
            "selected_ids_sha256": _selection_hash(selected_ids),
            "selected_sources_sha256": _selection_hash(sources),
            "workers": args.workers,
        },
        "invalid_models": invalid,
        "invalid_policy": {
            "expected_ids": sorted(_KNOWN_MFCADPP_2500_INVALID),
            "expected_reason": _KNOWN_INVALID_REASON,
        },
        "gates": dict(sorted(gates.items())),
        "candidate_regions": len(candidates),
        "candidate_pure_target_plus_interruption_regions": sum(
            row["pure_target_plus_interruption"] for row in candidates
        ),
        "target_faces_reached": len(
            {(row["model_id"], face) for row in candidates for face in row["target_face_indices"]}
        ),
        "new_target_faces_if_accepted": sum(
            row["target_faces_new_if_accepted"] for row in candidates
        ),
        "row_policy": "accepted geometric candidates only",
        "rows": rows,
        "runtime_seconds": time.perf_counter() - started,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({key: value for key, value in report.items() if key != "rows"}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
