#!/usr/bin/env python3
"""Classify the first production rejection gate for two-ended MFCAD++ enclosures."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import subprocess
import sys
import time
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any, cast

ROOT = Path(__file__).parents[1]
sys.path.insert(0, str(ROOT))

from quiddity._adjacency import FaceGraph, FaceNode  # noqa: E402
from quiddity._section_passages import (  # noqa: E402
    _COORD_FLOOR,
    _INTERVAL_TOL,
    _BodyAdapter,
    _dot,
    _enclosure_proposals,
    _line_section,
    _mouth_regions,
    _parallel,
    _point,
    _same_section,
    _termination_plane,
    _void_and_open,
    _void_and_planar_open,
    _wall_run,
    section_ring_proposals,
)
from quiddity._sections import LocalFrame  # noqa: E402
from tools.audit_mfcadpp_cavity_enclosures import _two_ended_regions  # noqa: E402
from tools.derive_mfcadpp_components import _components  # noqa: E402
from tools.effectiveness_report import load_mfcadpp_truth  # noqa: E402

TARGET_CLASS = 4
PASSAGE_CLASSES = frozenset({2, 3, 4})
GATES = (
    "planar_mouth_seed",
    "opposed_openings_or_solid",
    "straight_polygonal_sections",
    "mouth_congruence",
    "axial_interval",
    "material_or_open_ends",
    "duplicate_or_existing_cycle",
    "accepted_fallback",
)
GATE_METRICS = (
    "regions",
    "candidate_faces",
    "class_4_face_occurrences",
    "unique_class_4_faces",
    "class_4_pure_regions",
    "class_4_components_touched",
    "class_4_components_fully_reached",
)

Mouth = tuple[FaceNode, Any, frozenset[FaceNode]]


def _classify_region(
    graph: FaceGraph,
    region: frozenset[FaceNode],
    mouths: tuple[Mouth, ...] | None,
    fallback_by_region: dict[frozenset[FaceNode], Any],
    existing_cycle_regions: frozenset[frozenset[FaceNode]],
    final_fallback_regions: frozenset[frozenset[FaceNode]],
) -> str:
    """Mirror the production fallback and return its first failed gate."""

    if region in existing_cycle_regions:
        return "duplicate_or_existing_cycle"
    if mouths is None or len(mouths) != 2:
        return "planar_mouth_seed"
    (
        (first_opening, first_wire, _first_seed),
        (
            second_opening,
            second_wire,
            _second_seed,
        ),
    ) = mouths
    first_normal = graph.normal(first_opening)
    second_normal = graph.normal(second_opening)
    solid = graph.common_valid_solid(region | {first_opening, second_opening})
    if first_normal is None or second_normal is None or solid is None:
        return "opposed_openings_or_solid"
    if not _parallel(first_normal, second_normal):
        run = _wall_run(graph, region)
        if run is None:
            return "opposed_openings_or_solid"
        base = LocalFrame.canonical(run, (0.0, 0.0, 0.0))
        first = _line_section(first_wire, base)
        second = _line_section(second_wire, base)
        if first is None or second is None:
            return "straight_polygonal_sections"
        if not _same_section(first[0], second[0]) or math.dist(first[1], second[1]) > _INTERVAL_TOL:
            return "mouth_congruence"
        section, centre = first
        frame = LocalFrame.canonical(base.run, centre)
        planes = (
            _termination_plane(first_normal, first_wire, frame),
            _termination_plane(second_normal, second_wire, frame),
        )
        if any(plane is None for plane in planes):
            return "axial_interval"
        low, high = sorted(
            cast(tuple[tuple[float, tuple[float, float]], ...], planes),
            key=lambda item: item[0],
        )
        if high[0] - low[0] <= _COORD_FLOOR:
            return "axial_interval"
        if not _void_and_planar_open(graph.solid_shape(solid), frame, low, high, section):
            return "material_or_open_ends"
        fallback = fallback_by_region.get(region)
        if fallback is None:
            raise RuntimeError("production fallback disagrees with rejection census")
        return (
            "accepted_fallback"
            if region in final_fallback_regions
            else "duplicate_or_existing_cycle"
        )
    if _dot(first_normal, second_normal) > 0.0:
        return "opposed_openings_or_solid"
    base = LocalFrame.canonical(first_normal, (0.0, 0.0, 0.0))
    first = _line_section(first_wire, base)
    second = _line_section(second_wire, base)
    if first is None or second is None:
        return "straight_polygonal_sections"
    if not _same_section(first[0], second[0]):
        return "mouth_congruence"
    section, centre = first
    frame = LocalFrame.canonical(base.run, centre)
    interval = tuple(
        sorted(
            (
                _dot(_point(first_wire.vertices()[0]), frame.run),
                _dot(_point(second_wire.vertices()[0]), frame.run),
            )
        )
    )
    run_interval = cast(tuple[float, float], interval)
    if run_interval[1] - run_interval[0] <= _COORD_FLOOR:
        return "axial_interval"
    if not _void_and_open(graph.solid_shape(solid), frame, run_interval, section):
        return "material_or_open_ends"
    fallback = fallback_by_region.get(region)
    if fallback is None:
        raise RuntimeError("production fallback disagrees with rejection census")
    return (
        "accepted_fallback" if region in final_fallback_regions else "duplicate_or_existing_cycle"
    )


def _dominant_passage_class(labels: tuple[int, ...], nodes: tuple[FaceNode, ...]) -> str:
    matches = Counter(labels[node.index] for node in nodes if labels[node.index] in PASSAGE_CLASSES)
    if not matches:
        return "unmapped"
    best = max(matches.values())
    winners = sorted(class_id for class_id, count in matches.items() if count == best)
    return str(winners[0]) if len(winners) == 1 else "ambiguous"


def _ratio(numerator: int, denominator: int) -> dict[str, int | float | None]:
    return {
        "numerator": numerator,
        "denominator": denominator,
        "value": numerator / denominator if denominator else None,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("--limit", type=int, default=2500)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.limit <= 0:
        parser.error("--limit must be positive")

    from quiddity import import_step_geometry as import_step

    paths = sorted(args.root.glob("*.st*p"), key=lambda path: path.name)[: args.limit]
    if not paths:
        parser.error("the selected workload contains no STEP files")

    started = time.perf_counter()
    totals: dict[str, Counter[str]] = {gate: Counter() for gate in GATES}
    rows = []
    sources = []
    selected_class_faces = 0
    production_class_records: Counter[str] = Counter()
    for path in paths:
        truth = load_mfcadpp_truth(path)
        sources.append((truth.model_id, truth.source_sha256))
        part = import_step(path)
        faces = tuple(part.faces())
        if len(faces) != len(truth.semantic):
            raise RuntimeError(f"{path.stem}: imported face count does not match labels")
        graph = FaceGraph(part)
        raw = _two_ended_regions(graph)
        production_mouths = dict(_mouth_regions(graph))
        fallback = _enclosure_proposals(graph, _BodyAdapter())
        fallback_by_region = {proposal.constituent: proposal for proposal in fallback}
        final = section_ring_proposals(part, graph)
        existing_cycle_regions = frozenset(
            frozenset(proposal.nodes) for proposal in final if not proposal.constituent
        )
        final_fallback_regions = frozenset(
            proposal.constituent for proposal in final if proposal.constituent
        )
        for proposal in final:
            production_class_records[_dominant_passage_class(truth.semantic, proposal.nodes)] += 1

        class_nodes = {
            graph.require_node(faces[index])
            for index, label in enumerate(truth.semantic)
            if label == TARGET_CLASS
        }
        selected_class_faces += len(class_nodes)
        components = _components(graph, class_nodes) if class_nodes else ()
        reached_by_gate: dict[str, set[FaceNode]] = defaultdict(set)
        model_candidates = []
        for region, openings in raw:
            matched_mouths = production_mouths.get(region)
            gate = _classify_region(
                graph,
                region,
                matched_mouths,
                fallback_by_region,
                existing_cycle_regions,
                final_fallback_regions,
            )
            labels = Counter(truth.semantic[node.index] for node in region)
            class_faces = labels[TARGET_CLASS]
            totals[gate]["regions"] += 1
            totals[gate]["candidate_faces"] += len(region)
            totals[gate]["class_4_face_occurrences"] += class_faces
            totals[gate]["class_4_pure_regions"] += class_faces == len(region)
            reached_by_gate[gate].update(region & class_nodes)
            model_candidates.append(
                {
                    "gate": gate,
                    "face_indices": sorted(node.index for node in region),
                    "mouth_face_indices": [node.index for node in openings],
                    "labels": dict(sorted(labels.items())),
                    "production_mouth_edge_types": (
                        [
                            [edge.geom_type.name for edge in mouth[1].edges()]
                            for mouth in matched_mouths
                        ]
                        if matched_mouths is not None
                        else None
                    ),
                }
            )
        for gate in GATES:
            reached = reached_by_gate[gate]
            totals[gate]["unique_class_4_faces"] += len(reached)
            totals[gate]["class_4_components_touched"] += sum(
                bool(component & reached) for component in components
            )
            totals[gate]["class_4_components_fully_reached"] += sum(
                component <= reached for component in components
            )
        if model_candidates:
            rows.append({"model_id": truth.model_id, "candidates": model_candidates})

    report = {
        "format": "b123d-recognisers-mfcadpp-passage-rejection-census",
        "format_version": 1,
        "implementation_commit": subprocess.run(
            ["git", "rev-parse", "HEAD"],
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip(),
        "labels_used_in_candidate_construction": False,
        "target_class": TARGET_CLASS,
        "gates": list(GATES),
        "selection": {
            "limit": args.limit,
            "selected_ids_sha256": hashlib.sha256(
                ("\n".join(path.stem for path in paths) + "\n").encode()
            ).hexdigest(),
            "selected_sources_sha256": hashlib.sha256(
                "".join(f"{model_id}:{digest}\n" for model_id, digest in sources).encode()
            ).hexdigest(),
        },
        "summary": {
            "models": len(paths),
            "class_4_faces": selected_class_faces,
            "production_section_proposals_by_dominant_passage_class": dict(
                sorted(production_class_records.items())
            ),
            "first_rejection_gates": {
                gate: {metric: totals[gate][metric] for metric in GATE_METRICS} for gate in GATES
            },
            "accepted_fallback_class_4_face_reach": _ratio(
                totals["accepted_fallback"]["unique_class_4_faces"],
                selected_class_faces,
            ),
        },
        "runtime_seconds": time.perf_counter() - started,
        "models": rows,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({key: value for key, value in report.items() if key != "models"}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
