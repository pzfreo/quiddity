#!/usr/bin/env python3
"""Prototype a coordinate-free closed circular-end Pocket proof on MFCAD++.

Candidate construction is geometry-only and runs before labels are consulted.  Labels are read
only to measure the resulting defining/constituent sets.  At the time of this historical audit,
the detector did not issue a public
record or change aggregate recognition.
"""

from __future__ import annotations

import argparse
import json
import math
import sys
from collections import Counter
from dataclasses import asdict, dataclass
from pathlib import Path

from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.GeomAbs import GeomAbs_Cylinder

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from quiddity._adjacency import (  # noqa: E402
    FaceGraph,
    FaceNode,
    frame_points_outward,
)
from quiddity._geometry import length_tol  # noqa: E402
from quiddity._recess_faces import _dominant_axis  # noqa: E402
from quiddity._recess_obround import _END_RADIUS_FRAC  # noqa: E402
from tools.effectiveness_report import load_mfcadpp_truth  # noqa: E402

_DIRECTION_TOL = 1e-6
_SEMICIRCLE_TOL = 1e-4

Vector3 = tuple[float, float, float]


@dataclass(frozen=True, slots=True)
class PrototypeCandidate:
    floor: int
    cylinders: tuple[int, int]
    sides: tuple[int, int]
    mouth: int
    radius: float
    depth_direction: Vector3
    long_direction: Vector3
    width_direction: Vector3
    run_interval: tuple[float, float]
    oriented: bool

    @property
    def defining(self) -> frozenset[int]:
        return frozenset((*self.cylinders, *self.sides))

    @property
    def constituent(self) -> frozenset[int]:
        return self.defining | {self.floor}


def _dot(left: Vector3, right: Vector3) -> float:
    return sum(a * b for a, b in zip(left, right, strict=True))


def _subtract(left: Vector3, right: Vector3) -> Vector3:
    return tuple(a - b for a, b in zip(left, right, strict=True))  # type: ignore[return-value]


def _scale(vector: Vector3, factor: float) -> Vector3:
    return tuple(value * factor for value in vector)  # type: ignore[return-value]


def _cross(left: Vector3, right: Vector3) -> Vector3:
    return (
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    )


def _unit(vector: Vector3) -> Vector3 | None:
    norm = math.sqrt(_dot(vector, vector))
    if norm <= 1e-12:
        return None
    return tuple(value / norm for value in vector)  # type: ignore[return-value]


def _canonical(vector: Vector3) -> Vector3:
    result = _unit(vector)
    if result is None:
        raise ValueError("direction must be nonzero")
    normalized = result
    pivot = max(range(3), key=lambda axis: (abs(normalized[axis]), axis))
    if normalized[pivot] < 0:
        normalized = _scale(normalized, -1.0)
    return tuple(0.0 if abs(value) < 5e-13 else value for value in normalized)  # type: ignore[return-value]


def _parallel(left: Vector3, right: Vector3) -> bool:
    return abs(abs(_dot(left, right)) - 1.0) <= _DIRECTION_TOL


def _principal(vector: Vector3) -> bool:
    return _dominant_axis(vector) is not None


def _point(value: object) -> Vector3:
    return tuple(float(item) for item in value.Coord())  # type: ignore[attr-defined,return-value]


def _cylinder(graph: FaceGraph, node: FaceNode) -> tuple[float, Vector3, Vector3] | None:
    surface = BRepAdaptor_Surface(graph.face(node).wrapped)
    if surface.GetType() != GeomAbs_Cylinder:
        return None
    cylinder = surface.Cylinder()
    return (
        float(cylinder.Radius()),
        _canonical(_point(cylinder.Axis().Direction())),
        _point(cylinder.Axis().Location()),
    )


def _edge_sweep(
    graph: FaceGraph, floor: FaceNode, cylinder: FaceNode, radius: float
) -> float | None:
    occurrences = graph.shared_occurrences(floor, cylinder)
    if not occurrences or any(item.edge.geom_type.name != "CIRCLE" for item in occurrences):
        return None
    return sum(float(item.edge.length) for item in occurrences) / radius


def _node_interval(
    graph: FaceGraph, node: FaceNode, direction: Vector3
) -> tuple[float, float] | None:
    values = []
    try:
        for vertex in graph.face(node).vertices():
            position = vertex.center()
            point = (float(position.X), float(position.Y), float(position.Z))
            values.append(_dot(point, direction))
    except (AttributeError, RuntimeError, TypeError, ValueError):
        return None
    return (min(values), max(values)) if values else None


def _one_candidate(graph: FaceGraph, floor: FaceNode) -> PrototypeCandidate | None:
    floor_normal = graph.normal(floor)
    if floor_normal is None:
        return None
    depth = _canonical(floor_normal)
    concave = tuple(node for node in graph.neighbours(floor) if graph.arc(floor, node) == "concave")
    cylinders = tuple(node for node in concave if graph.surface(node) == GeomAbs_Cylinder)
    sides = tuple(node for node in concave if graph.is_planar(node))
    if len(cylinders) != 2 or len(sides) != 2 or len(concave) != 4:
        return None
    if any(frame_points_outward(graph.face(node)) is not False for node in cylinders):
        return None
    cylinder_data = tuple(_cylinder(graph, node) for node in cylinders)
    if any(item is None for item in cylinder_data):
        return None
    first, second = (item for item in cylinder_data if item is not None)
    radius = first[0]
    if (
        abs(first[0] - second[0]) > length_tol(radius, rel=_END_RADIUS_FRAC)
        or not _parallel(first[1], second[1])
        or not _parallel(first[1], depth)
    ):
        return None
    long = _subtract(second[2], first[2])
    long = _subtract(long, _scale(depth, _dot(long, depth)))
    long_direction = _unit(long)
    if long_direction is None:
        return None
    long_direction = _canonical(long_direction)
    width_direction = _canonical(_cross(depth, long_direction))
    side_normals = tuple(graph.normal(node) for node in sides)
    if any(normal is None for normal in side_normals):
        return None
    normals = tuple(_canonical(normal) for normal in side_normals if normal is not None)
    if (
        not _parallel(normals[0], normals[1])
        or any(not _parallel(normal, width_direction) for normal in normals)
        or any(abs(_dot(normal, depth)) > _DIRECTION_TOL for normal in normals)
    ):
        return None
    if not all(graph.arc(cylinder, side) == "smooth" for cylinder in cylinders for side in sides):
        return None
    sweeps = tuple(_edge_sweep(graph, floor, cylinder, radius) for cylinder in cylinders)
    if any(sweep is None or abs(sweep - math.pi) > _SEMICIRCLE_TOL for sweep in sweeps):
        return None
    intervals = tuple(_node_interval(graph, node, depth) for node in (*cylinders, *sides))
    if any(interval is None for interval in intervals):
        return None
    spans = tuple(interval for interval in intervals if interval is not None)
    low = min(interval[0] for interval in spans)
    high = max(interval[1] for interval in spans)
    tolerance = length_tol(high - low, rel=_END_RADIUS_FRAC)
    if high - low <= tolerance or any(
        abs(interval[0] - low) > tolerance or abs(interval[1] - high) > tolerance
        for interval in spans
    ):
        return None
    floor_interval = _node_interval(graph, floor, depth)
    if floor_interval is None:
        return None
    floor_at = sum(floor_interval) / 2
    if min(abs(floor_at - low), abs(floor_at - high)) > tolerance:
        return None
    mouth_at = high if abs(floor_at - low) <= tolerance else low
    context = set(graph.neighbours(cylinders[0]))
    for node in (*cylinders[1:], *sides):
        context &= set(graph.neighbours(node))
    mouths = []
    for node in context - {floor}:
        normal = graph.normal(node) if graph.is_planar(node) else None
        interval = _node_interval(graph, node, depth)
        if (
            normal is not None
            and _parallel(_canonical(normal), depth)
            and interval is not None
            and abs(sum(interval) / 2 - mouth_at) <= tolerance
            and all(
                graph.arc(node, support) in ("convex", "smooth") for support in (*cylinders, *sides)
            )
        ):
            mouths.append(node)
    if len(mouths) != 1 or graph.common_valid_solid((*cylinders, *sides, floor)) is None:
        return None
    return PrototypeCandidate(
        floor.index,
        tuple(sorted(node.index for node in cylinders)),  # type: ignore[arg-type]
        tuple(sorted(node.index for node in sides)),  # type: ignore[arg-type]
        mouths[0].index,
        round(radius, 6),
        tuple(round(value, 6) for value in depth),  # type: ignore[arg-type]
        tuple(round(value, 6) for value in long_direction),  # type: ignore[arg-type]
        tuple(round(value, 6) for value in width_direction),  # type: ignore[arg-type]
        (round(low, 6), round(high, 6)),
        not (_principal(long_direction) and _principal(width_direction)),
    )


def _candidates(graph: FaceGraph) -> tuple[PrototypeCandidate, ...]:
    found = []
    for node in graph.nodes:
        if graph.is_planar(node) and (candidate := _one_candidate(graph, node)) is not None:
            found.append(candidate)
    return tuple(sorted(set(found), key=lambda item: (item.floor, item.cylinders, item.sides)))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("--limit", type=int, default=2500)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()

    from quiddity import import_step_geometry

    paths = sorted(args.root.glob("*.st*p"))[: args.limit]
    summary: Counter[str] = Counter()
    rows = []
    for path in paths:
        part = import_step_geometry(path)
        candidates = _candidates(FaceGraph(part))
        # Labels are not loaded until every candidate for this model already exists.
        truth = load_mfcadpp_truth(path)
        for candidate in candidates:
            defining_labels = Counter(truth.semantic[index] for index in candidate.defining)
            constituent_labels = Counter(truth.semantic[index] for index in candidate.constituent)
            summary["candidates"] += 1
            summary["oriented" if candidate.oriented else "principal"] += 1
            if set(defining_labels) == {16}:
                summary["class16_defining_pure"] += 1
            if set(constituent_labels) == {16}:
                summary["class16_constituent_pure"] += 1
            rows.append(
                {
                    "model_id": truth.model_id,
                    "candidate": asdict(candidate),
                    "defining_labels": dict(sorted(defining_labels.items())),
                    "constituent_labels": dict(sorted(constituent_labels.items())),
                }
            )
    report = {"summary": dict(sorted(summary.items())), "candidates": rows}
    rendered = json.dumps(report, indent=2, sort_keys=True) + "\n"
    if args.output is None:
        print(rendered, end="")
    else:
        args.output.write_text(rendered, encoding="utf-8")
        print(json.dumps(report["summary"], indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
