"""Reproduce the defining-wall diagnosis for the E2 PrismaticPocket frame losses.

The tool compares accepted raw and normalized candidates by validated face identity. For every
raw occurrence that disappears, it reports whether its complete wall ring has a common local
principal run axis and, when it does, whether the walls retain the equal run span required by
the production ring walker.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import sys
from pathlib import Path
from typing import Any, cast

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from quiddity import import_step_geometry as import_step  # noqa: E402
from quiddity._adjacency import FaceGraph  # noqa: E402
from quiddity._candidates import FamilyId  # noqa: E402
from quiddity._geometry import AXIS_ZERO_COS  # noqa: E402
from quiddity._rings import SPAN_EPS  # noqa: E402
from quiddity.frames import (  # noqa: E402
    PartFrame,
    RefusedPartFrame,
    _normalize_part,
    infer_part_frame,
)
from quiddity.prismatic_pockets import (  # noqa: E402
    PrismaticPocket,
    recognise_prismatic_pockets,
)
from quiddity.result import _take_inventory  # noqa: E402

MODEL_IDS = ("10649", "10653", "10761", "11203")
CORRESPONDENCE_TOLERANCE = 1e-7


def _rounded(values) -> list[float]:
    return [round(float(value), 12) for value in values]


def _dot(left, right) -> float:
    return sum(float(a) * float(b) for a, b in zip(left, right, strict=True))


def _local_direction(frame: PartFrame, direction) -> tuple[float, float, float]:
    return cast(
        tuple[float, float, float],
        tuple(_dot(direction, axis) for axis in (frame.x, frame.y, frame.z)),
    )


def _line_deviation_from_world_axis(direction) -> float:
    component = max(abs(float(value)) for value in direction)
    return math.degrees(math.acos(min(1.0, component)))


def _candidate_faces(product, candidate) -> tuple[int, ...]:
    return tuple(sorted(node.index for node in product.evidence.defining_of(candidate)))


def _validate_face_correspondence(
    raw_graph: FaceGraph,
    local_graph: FaceGraph,
    face_indices: tuple[int, ...],
    frame: PartFrame,
) -> list[dict[str, Any]]:
    checked = []
    for index in face_indices:
        raw_face = raw_graph.face(raw_graph.nodes[index])
        local_face = local_graph.face(local_graph.nodes[index])
        raw_center = tuple(raw_face.center())
        local_center = tuple(local_face.center())
        expected_center = frame.to_local(raw_center)
        raw_normal = tuple(raw_face.normal_at())
        local_normal = tuple(local_face.normal_at())
        expected_normal = _local_direction(frame, raw_normal)
        center_error = math.dist(local_center, expected_center)
        normal_error = math.dist(local_normal, expected_normal)
        preserved = (
            raw_face.geom_type == local_face.geom_type
            and len(raw_face.edges()) == len(local_face.edges())
            and abs(raw_face.area - local_face.area) <= CORRESPONDENCE_TOLERANCE
            and center_error <= CORRESPONDENCE_TOLERANCE
            and normal_error <= CORRESPONDENCE_TOLERANCE
        )
        if not preserved:
            raise RuntimeError(f"normalization did not preserve face {index}")
        checked.append(
            {
                "face_index": index,
                "raw_normal": _rounded(raw_normal),
                "local_normal": _rounded(local_normal),
                "center_error": round(center_error, 15),
                "normal_error": round(normal_error, 15),
            }
        )
    return checked


def _ring_boundary(graph: FaceGraph, face_indices: tuple[int, ...]) -> dict[str, Any]:
    nodes = tuple(graph.nodes[index] for index in face_indices)
    axis_reads = []
    passing_axes = []
    for axis in (0, 1, 2):
        normals = [graph.normal(node) for node in nodes]
        normal_components = [
            None if normal is None else round(abs(normal[axis]), 12) for normal in normals
        ]
        all_walls_parallel = all(
            component is not None and component <= AXIS_ZERO_COS for component in normal_components
        )
        spans = [graph.bounds(node)[axis] for node in nodes]
        low_spread = max(low for low, _high in spans) - min(low for low, _high in spans)
        high_spread = max(high for _low, high in spans) - min(high for _low, high in spans)
        equal_span = low_spread <= SPAN_EPS and high_spread <= SPAN_EPS
        if all_walls_parallel and equal_span:
            passing_axes.append("xyz"[axis])
        axis_reads.append(
            {
                "axis": "xyz"[axis],
                "absolute_normal_components": normal_components,
                "all_walls_parallel": all_walls_parallel,
                "low_span_spread": round(low_spread, 12),
                "high_span_spread": round(high_spread, 12),
                "equal_span": equal_span,
            }
        )
    if passing_axes:
        disposition = "passes-principal-wall-and-span-gates"
    elif any(read["all_walls_parallel"] for read in axis_reads):
        disposition = "unequal-principal-spans"
    else:
        disposition = "no-common-principal-run-axis"
    return {
        "axis_zero_cos": AXIS_ZERO_COS,
        "span_epsilon": SPAN_EPS,
        "axes": axis_reads,
        "passing_axes": passing_axes,
        "first_boundary": disposition,
    }


def audit(dataset_root: Path) -> dict[str, Any]:
    models = []
    absent_total = 0
    for model_id in MODEL_IDS:
        path = dataset_root / f"{model_id}.step"
        raw = import_step(path)
        frame = infer_part_frame(raw)
        if isinstance(frame, RefusedPartFrame):
            raise RuntimeError(f"{path.name}: frame refused: {frame.reason.value}")
        local = _normalize_part(raw, frame)
        raw_product = _take_inventory(raw)
        local_product = _take_inventory(local)
        raw_candidates = raw_product.accepted.candidate_set(FamilyId.PRISMATIC_POCKETS).candidates
        local_candidates = local_product.accepted.candidate_set(
            FamilyId.PRISMATIC_POCKETS
        ).candidates
        local_face_sets = {_candidate_faces(local_product, item) for item in local_candidates}
        raw_graph = raw_product.context.graph
        local_graph = local_product.context.graph
        absent = []
        for candidate in raw_candidates:
            face_indices = _candidate_faces(raw_product, candidate)
            if face_indices in local_face_sets:
                continue
            raw_axis = cast(PrismaticPocket, candidate.record).axis
            world_run = tuple(1.0 if index == "xyz".index(raw_axis) else 0.0 for index in range(3))
            local_run = _local_direction(frame, world_run)
            absent.append(
                {
                    "raw_record": repr(candidate.record),
                    "raw_run_axis": raw_axis,
                    "local_run_direction": _rounded(local_run),
                    "local_run_deviation_from_principal_degrees": round(
                        _line_deviation_from_world_axis(local_run), 6
                    ),
                    "defining_face_indices": list(face_indices),
                    "correspondence": _validate_face_correspondence(
                        raw_graph, local_graph, face_indices, frame
                    ),
                    "local_ring_boundary": _ring_boundary(local_graph, face_indices),
                }
            )
        absent_total += len(absent)
        models.append(
            {
                "file": path.name,
                "step_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                "frame": {
                    "origin": _rounded(frame.origin),
                    "x": _rounded(frame.x),
                    "y": _rounded(frame.y),
                    "z": _rounded(frame.z),
                    "gauge": frame.gauge.value,
                    "largest_basis_line_deviation_from_world_degrees": round(
                        max(
                            _line_deviation_from_world_axis(axis)
                            for axis in (frame.x, frame.y, frame.z)
                        ),
                        6,
                    ),
                },
                "raw_direct_prismatic_pockets": len(recognise_prismatic_pockets(raw)),
                "raw_aggregate_prismatic_pockets": len(raw_candidates),
                "local_direct_prismatic_pockets": len(recognise_prismatic_pockets(local)),
                "local_aggregate_prismatic_pockets": len(local_candidates),
                "absent_occurrences": absent,
            }
        )
    if absent_total != 6:
        raise RuntimeError(f"expected six absent occurrences, observed {absent_total}")
    return {
        "schema": 1,
        "dataset": "MFCAD++ published test split (development evidence)",
        "selection": "four model IDs carrying all six first-500 PrismaticPocket losses",
        "face_identity": "zero-based build123d face order, validated across normalization",
        "absent_occurrences": absent_total,
        "models": models,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dataset_root", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    rendered = json.dumps(audit(args.dataset_root), indent=2, sort_keys=True)
    if args.output is None:
        print(rendered)
    else:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(rendered + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
