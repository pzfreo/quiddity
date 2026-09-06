# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Private, traversal-neutral geometry facts for one graph-authorized solid.

The values in this module are correspondence evidence, never persistent identity.  They contain
no graph handles or kernel objects and deliberately preserve equal multiplicity.
"""

from __future__ import annotations

import math
from dataclasses import dataclass
from itertools import permutations, product
from typing import Any, TypeAlias, cast

from build123d import Edge, GeomType
from OCP.BRep import BRep_Tool
from OCP.BRepAdaptor import BRepAdaptor_Curve, BRepAdaptor_Curve2d, BRepAdaptor_Surface
from OCP.BRepCheck import BRepCheck_Analyzer
from OCP.BRepGProp import BRepGProp
from OCP.gp import gp_Pnt, gp_Pnt2d, gp_Vec, gp_Vec2d
from OCP.GProp import GProp_GProps
from OCP.Standard import Standard_Failure
from OCP.TopAbs import (
    TopAbs_FORWARD,
    TopAbs_Orientation,
    TopAbs_REVERSED,
    TopAbs_SOLID,
)
from OCP.TopExp import TopExp
from OCP.TopoDS import TopoDS

DESCRIPTOR_REL = 1e-9
DESCRIPTOR_FLOOR = 1e-7
DIRECTION_TOL = 1e-10
ANGLE_TOL = 1e-10
CANONICAL_SERIALIZATION_BUDGET = 100_000

QScalar: TypeAlias = float
QPoint: TypeAlias = tuple[QScalar, QScalar, QScalar]


class UnsupportedBodyGeometry(ValueError):
    """An authorized solid cannot be represented by the bounded analytic grammar."""


@dataclass(frozen=True, slots=True)
class BodyPlacement:
    centre_of_mass: tuple[float, float, float]
    frame_status: str = "unframed"


@dataclass(frozen=True, slots=True)
class BodyIntrinsic:
    volume: QScalar
    surface_area: QScalar
    principal_moments: tuple[QScalar, QScalar, QScalar]


@dataclass(frozen=True, slots=True)
class DescriptorQuantization:
    """Raw-mass-derived quantization authority retained for correspondence."""

    characteristic_scale: float
    metric_quantum: float
    area_quantum: float
    volume_quantum: float
    moment_quantum: float


@dataclass(frozen=True, order=True, slots=True)
class EdgeGeometry:
    kind: str
    start: QPoint
    end: QPoint
    length: QScalar
    centre: QPoint | None = None
    axis: QPoint | None = None
    radius: QScalar | None = None
    sweep: QScalar | None = None
    full: bool = False


@dataclass(frozen=True, order=True, slots=True)
class WireGeometry:
    role: str
    semantic_winding: int
    edges: tuple[tuple[EdgeGeometry, int], ...]


@dataclass(frozen=True, order=True, slots=True)
class FaceGeometry:
    kind: str
    parameters: tuple[QScalar, ...]
    area: QScalar
    centroid: QPoint
    material_side: int
    wires: tuple[WireGeometry, ...]


@dataclass(frozen=True, order=True, slots=True)
class MatchingCurve:
    """One graph-global analytic curve label for schema-3 matching."""

    kind: str
    vertices: tuple[int, int] | None
    length: QScalar
    centre: QPoint | None
    axis: QPoint | None
    radius: QScalar | None
    sweep: QScalar | None
    full: bool


@dataclass(frozen=True, order=True, slots=True)
class MatchingWireVertex:
    """One body-global vertex in one canonical face parameter gauge."""

    vertex: int | None
    parameter: tuple[QScalar, QScalar]


@dataclass(frozen=True, order=True, slots=True)
class MatchingHalfEdge:
    """One material-oriented use of a graph-global curve."""

    curve: int
    direction: int
    start: MatchingWireVertex | None
    end: MatchingWireVertex | None


@dataclass(frozen=True, order=True, slots=True)
class MatchingWire:
    """One material-oriented wire with a canonical cyclic start."""

    role: str
    theta_winding: int
    cycle: tuple[MatchingHalfEdge, ...]


@dataclass(frozen=True, order=True, slots=True)
class MatchingFace:
    """One canonical analytic face and its stable matching wires."""

    kind: str
    parameters: tuple[QScalar, ...]
    area: QScalar
    centroid: QPoint
    material_side: int
    wires: tuple[MatchingWire, ...]


@dataclass(frozen=True, slots=True)
class MatchingBoundaryGraph:
    """Complete token-erased schema-3 boundary topology for one body."""

    vertices: tuple[QPoint, ...]
    curves: tuple[MatchingCurve, ...]
    faces: tuple[MatchingFace, ...]
    incidence: tuple[tuple[int, tuple[tuple[int, int, int], ...]], ...]
    face_count: int
    wire_count: int
    edge_occurrence_count: int
    symmetric: bool


@dataclass(slots=True)
class _MatchingConstructionBudget:
    attempts: int = 0

    def charge(self) -> None:
        self.attempts += 1
        if self.attempts > CANONICAL_SERIALIZATION_BUDGET:
            raise UnsupportedBodyGeometry("matching boundary construction budget exceeded")


@dataclass(frozen=True, slots=True)
class _CylinderPcurveOccurrence:
    """One unordered, topology-owned pcurve occurrence on one cylindrical wire."""

    curve: int
    start_vertex: int | None
    end_vertex: int | None
    start_parameter: tuple[float, float]
    end_parameter: tuple[float, float]


def validate_matching_boundary_graph(
    value: MatchingBoundaryGraph,
    quantization: DescriptorQuantization,
) -> None:
    """Fail closed on a copied or mutated schema-three boundary value."""

    validate_descriptor_quantization(quantization)
    quantum = quantization.metric_quantum

    if (
        type(value) is not MatchingBoundaryGraph
        or type(value.vertices) is not tuple
        or type(value.curves) is not tuple
        or type(value.faces) is not tuple
        or type(value.incidence) is not tuple
        or type(value.face_count) is not int
        or type(value.wire_count) is not int
        or type(value.edge_occurrence_count) is not int
        or type(value.symmetric) is not bool
    ):
        raise UnsupportedBodyGeometry("matching boundary schema is malformed")

    def valid_point(point: object) -> bool:
        return (
            type(point) is tuple
            and len(point) == 3
            and all(type(item) is float and math.isfinite(item) for item in point)
        )

    if any(
        type(vertex) is not tuple
        or len(vertex) != 3
        or any(type(item) is not float or not math.isfinite(item) for item in vertex)
        for vertex in value.vertices
    ) or any(type(curve) is not MatchingCurve for curve in value.curves):
        raise UnsupportedBodyGeometry("matching boundary vertex or curve schema is malformed")
    for curve in value.curves:
        if (
            curve.kind not in {"LINE", "CIRCLE"}
            or type(curve.length) is not float
            or not math.isfinite(curve.length)
            or curve.length <= 0.0
            or type(curve.full) is not bool
        ):
            raise UnsupportedBodyGeometry("matching curve value is malformed")
        if curve.kind == "LINE":
            if (
                curve.full
                or type(curve.vertices) is not tuple
                or len(curve.vertices) != 2
                or curve.vertices[0] == curve.vertices[1]
                or any(
                    type(item) is not int or item < 0 or item >= len(value.vertices)
                    for item in curve.vertices
                )
                or any(
                    item is not None
                    for item in (curve.centre, curve.axis, curve.radius, curve.sweep)
                )
            ):
                raise UnsupportedBodyGeometry("matching line value is malformed")
            line_left, line_right = curve.vertices
            if abs(
                math.dist(value.vertices[line_left], value.vertices[line_right]) - curve.length
            ) > (6.0 * quantum):
                raise UnsupportedBodyGeometry("matching line no longer reconstructs its vertices")
        elif (
            not valid_point(curve.centre)
            or not valid_point(curve.axis)
            or type(curve.radius) is not float
            or not math.isfinite(curve.radius)
            or curve.radius <= 0.0
            or type(curve.sweep) is not float
            or not math.isfinite(curve.sweep)
            or (curve.full and (curve.vertices is not None or curve.sweep != 2.0 * math.pi))
            or (
                not curve.full
                and (
                    type(curve.vertices) is not tuple
                    or len(curve.vertices) != 2
                    or curve.vertices[0] == curve.vertices[1]
                    or any(
                        type(item) is not int or item < 0 or item >= len(value.vertices)
                        for item in curve.vertices
                    )
                )
            )
        ):
            raise UnsupportedBodyGeometry("matching circle value is malformed")
        else:
            assert curve.axis is not None
            assert curve.centre is not None
            assert curve.radius is not None
            assert curve.sweep is not None
            if _qaxis(curve.axis) != curve.axis or curve.radius <= quantum:
                raise UnsupportedBodyGeometry("matching circle gauge changed")
            if abs(curve.length - curve.radius * abs(curve.sweep)) > 8.0 * quantum:
                raise UnsupportedBodyGeometry("matching circle no longer reconstructs its length")
            if not curve.full:
                for circle_vertex_index in cast(tuple[int, int], curve.vertices):
                    delta = tuple(
                        point - origin
                        for point, origin in zip(
                            value.vertices[circle_vertex_index], curve.centre, strict=True
                        )
                    )
                    radial = math.sqrt(sum(item * item for item in delta))
                    axial = abs(
                        sum(item * axis for item, axis in zip(delta, curve.axis, strict=True))
                    )
                    if abs(radial - curve.radius) > 6.0 * quantum or axial > 6.0 * quantum:
                        raise UnsupportedBodyGeometry(
                            "matching circle no longer reconstructs its vertices"
                        )
    occurrences = []
    if len(value.faces) != value.face_count:
        raise UnsupportedBodyGeometry("matching boundary face count changed")
    for face_index, face in enumerate(value.faces):
        if (
            type(face) is not MatchingFace
            or face.kind not in {"PLANE", "CYLINDER"}
            or type(face.parameters) is not tuple
            or len(face.parameters) != (4 if face.kind == "PLANE" else 7)
            or any(type(item) is not float or not math.isfinite(item) for item in face.parameters)
            or type(face.area) is not float
            or not math.isfinite(face.area)
            or face.area <= 0.0
            or not valid_point(face.centroid)
            or type(face.material_side) is not int
            or face.material_side not in {-1, 1}
            or type(face.wires) is not tuple
            or sum(wire.role == "outer" for wire in face.wires) != 1
        ):
            raise UnsupportedBodyGeometry("matching boundary face schema is malformed")
        axis = cast(QPoint, face.parameters[:3])
        if _qaxis(axis) != axis:
            raise UnsupportedBodyGeometry("matching face analytic gauge changed")
        if face.kind == "CYLINDER" and face.parameters[6] <= quantum:
            raise UnsupportedBodyGeometry("matching cylinder radius is degenerate")
        for wire_index, wire in enumerate(face.wires):
            if (
                type(wire) is not MatchingWire
                or wire.role not in {"outer", "inner"}
                or type(wire.theta_winding) is not int
                or (face.kind == "PLANE" and wire.theta_winding != 0)
                or type(wire.cycle) is not tuple
                or not wire.cycle
            ):
                raise UnsupportedBodyGeometry("matching boundary wire schema is malformed")
            if wire.cycle != min(
                wire.cycle[index:] + wire.cycle[:index] for index in range(len(wire.cycle))
            ):
                raise UnsupportedBodyGeometry("matching wire canonical start changed")
            for occurrence_index, half_edge in enumerate(wire.cycle):
                if (
                    type(half_edge) is not MatchingHalfEdge
                    or type(half_edge.curve) is not int
                    or half_edge.curve < 0
                    or half_edge.curve >= len(value.curves)
                    or half_edge.direction not in {-1, 1}
                    or (half_edge.start is None) != (half_edge.end is None)
                    or (
                        half_edge.start is not None
                        and (
                            type(half_edge.start) is not MatchingWireVertex
                            or type(half_edge.end) is not MatchingWireVertex
                        )
                    )
                ):
                    raise UnsupportedBodyGeometry("matching boundary half-edge is malformed")
                curve = value.curves[half_edge.curve]
                if curve.full != (half_edge.start is None):
                    raise UnsupportedBodyGeometry("matching full-circle endpoint schema changed")
                if half_edge.start is not None and half_edge.end is not None:
                    if any(
                        vertex.vertex is None
                        or type(vertex.vertex) is not int
                        or vertex.vertex < 0
                        or vertex.vertex >= len(value.vertices)
                        or type(vertex.parameter) is not tuple
                        or len(vertex.parameter) != 2
                        or any(
                            type(item) is not float or not math.isfinite(item)
                            for item in vertex.parameter
                        )
                        for vertex in (half_edge.start, half_edge.end)
                    ):
                        raise UnsupportedBodyGeometry("matching half-edge parameter schema changed")
                    expected_vertices = (
                        cast(tuple[int, int], curve.vertices)
                        if half_edge.direction == 1
                        else tuple(reversed(cast(tuple[int, int], curve.vertices)))
                    )
                    if (half_edge.start.vertex, half_edge.end.vertex) != expected_vertices:
                        raise UnsupportedBodyGeometry(
                            "matching half-edge traversal disagrees with its curve"
                        )
                occurrences.append((half_edge.curve, (face_index, wire_index, occurrence_index)))
            for left_half_edge, right_half_edge in zip(
                wire.cycle, wire.cycle[1:] + wire.cycle[:1], strict=True
            ):
                if left_half_edge.end is None or right_half_edge.start is None:
                    continue
                if left_half_edge.end.vertex != right_half_edge.start.vertex:
                    raise UnsupportedBodyGeometry("matching wire topology no longer joins")
                if face.kind == "PLANE":
                    if (
                        math.dist(
                            left_half_edge.end.parameter,
                            right_half_edge.start.parameter,
                        )
                        > 4.0 * quantum
                    ):
                        raise UnsupportedBodyGeometry(
                            "matching planar pcurve cycle no longer joins"
                        )
                else:
                    theta_residual = (
                        left_half_edge.end.parameter[0] - right_half_edge.start.parameter[0]
                    )
                    turns = round(theta_residual / (2.0 * math.pi))
                    if (
                        abs(theta_residual - turns * 2.0 * math.pi) > 4.0 * ANGLE_TOL
                        or abs(left_half_edge.end.parameter[1] - right_half_edge.start.parameter[1])
                        > 4.0 * quantum
                    ):
                        raise UnsupportedBodyGeometry(
                            "matching cylinder pcurve cycle no longer joins"
                        )
            if face.kind == "PLANE":
                for half_edge in wire.cycle:
                    if half_edge.start is None or half_edge.end is None:
                        continue
                    for wire_vertex in (half_edge.start, half_edge.end):
                        expected_parameter = _plane_parameter(
                            value.vertices[cast(int, wire_vertex.vertex)],
                            cast(FaceGeometry, face),
                            quantum,
                        )
                        if math.dist(expected_parameter, wire_vertex.parameter) > 4.0 * quantum:
                            raise UnsupportedBodyGeometry(
                                "matching planar pcurve parameter changed"
                            )
                signed_area = sum(
                    _half_edge_integral(
                        item,
                        value.curves,
                        cast(FaceGeometry, face),
                        quantum,
                    )
                    for item in wire.cycle
                )
                expected_positive = (wire.role == "outer") == (face.material_side > 0)
                if abs(signed_area) <= quantum**2 or ((signed_area > 0.0) != expected_positive):
                    raise UnsupportedBodyGeometry("matching planar material orientation changed")
            else:
                cylinder_axis = axis
                cylinder_point = cast(QPoint, face.parameters[3:6])
                cylinder_radius = face.parameters[6]
                u_axis, v_axis = _plane_basis(cylinder_axis)
                lifted_area = 0.0
                theta_delta = 0.0
                for half_edge in wire.cycle:
                    if half_edge.start is None or half_edge.end is None:
                        curve = value.curves[half_edge.curve]
                        if not curve.full or curve.centre is None or curve.kind != "CIRCLE":
                            raise UnsupportedBodyGeometry(
                                "matching endpoint-free cylinder curve changed"
                            )
                        cylinder_z = sum(
                            (point - origin) * direction
                            for point, origin, direction in zip(
                                curve.centre,
                                cylinder_point,
                                cylinder_axis,
                                strict=True,
                            )
                        )
                        full_theta_delta = half_edge.direction * 2.0 * math.pi
                        lifted_area -= 0.5 * full_theta_delta * cylinder_z
                        theta_delta += full_theta_delta
                        continue
                    start = half_edge.start.parameter
                    end = half_edge.end.parameter
                    lifted_area += 0.5 * (start[0] * end[1] - end[0] * start[1])
                    theta_delta += end[0] - start[0]
                    for wire_vertex in (half_edge.start, half_edge.end):
                        theta, z = wire_vertex.parameter
                        reconstructed = tuple(
                            cylinder_point[index]
                            + z * cylinder_axis[index]
                            + cylinder_radius
                            * (math.cos(theta) * u_axis[index] + math.sin(theta) * v_axis[index])
                            for index in range(3)
                        )
                        if (
                            math.dist(
                                reconstructed,
                                value.vertices[cast(int, wire_vertex.vertex)],
                            )
                            > 8.0 * quantum
                        ):
                            raise UnsupportedBodyGeometry(
                                "matching cylinder pcurve parameter changed"
                            )
                expected_positive = (wire.role == "outer") == (face.material_side > 0)
                if abs(lifted_area) <= ANGLE_TOL * quantum or (
                    (lifted_area > 0.0) != expected_positive
                ):
                    raise UnsupportedBodyGeometry("matching cylinder material orientation changed")
                winding = int(round(theta_delta / (2.0 * math.pi)))
                if (
                    winding != wire.theta_winding
                    or abs(theta_delta - winding * 2.0 * math.pi) > 4.0 * ANGLE_TOL
                ):
                    raise UnsupportedBodyGeometry("matching cylinder theta winding changed")
    expected = tuple(
        (curve, tuple(item for key, item in occurrences if key == curve))
        for curve in range(len(value.curves))
    )
    if (
        value.wire_count != sum(len(face.wires) for face in value.faces)
        or value.edge_occurrence_count != len(occurrences)
        or value.incidence != expected
        or any(len(items) != 2 for _, items in expected)
    ):
        raise UnsupportedBodyGeometry("matching boundary incidence changed")


@dataclass(frozen=True, slots=True)
class _WireBuild:
    geometry: WireGeometry
    occurrences: tuple[tuple[tuple[object, int], ...], ...]


@dataclass(frozen=True, slots=True)
class _FaceBuild:
    geometry: FaceGeometry
    wires: tuple[_WireBuild, ...]


@dataclass(frozen=True, slots=True)
class BodyBoundaryGeometry:
    faces: tuple[FaceGeometry, ...]
    incidence: tuple[tuple[EdgeGeometry, tuple[tuple[int, int, int, int], ...]], ...]
    face_count: int
    wire_count: int
    edge_occurrence_count: int
    symmetric: bool


@dataclass(frozen=True, slots=True)
class BodyGeometryDescriptor:
    intrinsic: BodyIntrinsic
    boundary: BodyBoundaryGeometry
    placement: BodyPlacement
    quantization: DescriptorQuantization


def validate_descriptor_quantization(value: DescriptorQuantization) -> None:
    """Revalidate one stored quantization contract without kernel state."""

    if type(value) is not DescriptorQuantization or any(
        type(item) is not float
        for item in (
            value.characteristic_scale,
            value.metric_quantum,
            value.area_quantum,
            value.volume_quantum,
            value.moment_quantum,
        )
    ):
        raise UnsupportedBodyGeometry("descriptor quantization has invalid runtime types")
    scale = value.characteristic_scale
    if not math.isfinite(scale) or scale <= 0.0:
        raise UnsupportedBodyGeometry("descriptor characteristic scale is invalid")
    metric = _metric_tolerance(scale)
    expected = (
        metric,
        (scale + metric) ** 2 - scale**2,
        (scale + metric) ** 3 - scale**3,
        (scale + metric) ** 5 - scale**5,
    )
    actual = (
        value.metric_quantum,
        value.area_quantum,
        value.volume_quantum,
        value.moment_quantum,
    )
    if any(not math.isfinite(item) or item <= 0.0 for item in actual) or actual != expected:
        raise UnsupportedBodyGeometry("descriptor quantization authority changed")


@dataclass(frozen=True, slots=True)
class _DescribedBody:
    descriptor: BodyGeometryDescriptor
    faces: tuple[Any, ...]
    face_geometry: tuple[FaceGeometry, ...]
    face_builds: tuple[_FaceBuild, ...]


def _finite(*values: float) -> tuple[float, ...]:
    converted = tuple(float(value) for value in values)
    if not all(math.isfinite(value) for value in converted):
        raise UnsupportedBodyGeometry("body geometry contains a non-finite value")
    return converted


def _metric_tolerance(scale: float) -> float:
    return DESCRIPTOR_REL * scale + DESCRIPTOR_FLOOR


def _snap(value: float, quantum: float) -> float:
    if not math.isfinite(value) or not math.isfinite(quantum) or quantum <= 0.0:
        raise UnsupportedBodyGeometry("body geometry cannot be quantized")
    snapped = float(round(value / quantum) * quantum)
    return 0.0 if snapped == 0.0 else snapped


def _snap_checked(value: float, quantum: float, *, name: str) -> float:
    """Quantize one scalar and prove the frozen reconstruction ceiling."""

    snapped = _snap(value, quantum)
    if abs(snapped - value) > 2.0 * quantum:
        raise UnsupportedBodyGeometry(f"{name} exceeds the reconstruction bound")
    return snapped


def _positive_fact(value: float, quantum: float, *, name: str) -> float:
    if not math.isfinite(value) or value < quantum:
        raise UnsupportedBodyGeometry(f"{name} is degenerate")
    snapped = _snap_checked(value, quantum, name=name)
    if snapped <= 0.0:
        raise UnsupportedBodyGeometry(f"{name} collapses during quantization")
    return snapped


def _point(value, centre: tuple[float, float, float], quantum: float, *, name: str) -> QPoint:
    raw_values = tuple(
        component - origin
        for component, origin in zip(_finite(value.X, value.Y, value.Z), centre, strict=True)
    )
    raw = (raw_values[0], raw_values[1], raw_values[2])
    return _relative_point(raw, quantum, name=name)


def _relative_point(raw: tuple[float, float, float], quantum: float, *, name: str) -> QPoint:
    snapped = tuple(_snap(component, quantum) for component in raw)
    displacement_squared = sum(
        (after - before) ** 2 for before, after in zip(raw, snapped, strict=True)
    )
    if displacement_squared > math.nextafter((2.0 * quantum) ** 2, math.inf):
        raise UnsupportedBodyGeometry(f"{name} exceeds the reconstruction bound")
    return snapped  # type: ignore[return-value]


def _vector(value) -> tuple[float, float, float]:
    raw = _finite(value.X(), value.Y(), value.Z())
    norm = math.sqrt(sum(component * component for component in raw))
    if norm <= DIRECTION_TOL or abs(norm - 1.0) > DIRECTION_TOL:
        raise UnsupportedBodyGeometry("analytic direction is not a supported unit vector")
    return (raw[0] / norm, raw[1] / norm, raw[2] / norm)


def _canonical_axis(raw: tuple[float, float, float]) -> tuple[tuple[float, float, float], int]:
    sign = 1
    for component in raw:
        if abs(component) >= DIRECTION_TOL:
            sign = 1 if component > 0.0 else -1
            break
    canonical = (sign * raw[0], sign * raw[1], sign * raw[2])
    return canonical, sign


def _qaxis(raw: tuple[float, float, float]) -> QPoint:
    canonical, _ = _canonical_axis(raw)
    snapped = tuple(_snap(component, DIRECTION_TOL) for component in canonical)
    norm = math.sqrt(sum(component * component for component in snapped))
    if abs(norm - 1.0) > DIRECTION_TOL:
        raise UnsupportedBodyGeometry("quantized analytic direction is not unit length")
    if sum(
        (after - before) ** 2 for before, after in zip(canonical, snapped, strict=True)
    ) > math.nextafter((2.0 * DIRECTION_TOL) ** 2, math.inf):
        raise UnsupportedBodyGeometry("analytic direction exceeds the reconstruction bound")
    return snapped  # type: ignore[return-value]


def _plane_parameters(
    raw_axis: tuple[float, float, float],
    location: tuple[float, float, float],
    centre: tuple[float, float, float],
    quantum: float,
) -> tuple[float, ...]:
    axis, _ = _canonical_axis(raw_axis)
    offset = sum(
        axis_component * (component - origin)
        for axis_component, component, origin in zip(axis, location, centre, strict=True)
    )
    return (*_qaxis(axis), _snap_checked(offset, quantum, name="plane offset"))


def _reverse_edge(item: tuple[EdgeGeometry, int]) -> tuple[EdgeGeometry, int]:
    edge, direction = item
    return edge, -direction


def _wire_orientation(wire) -> int:
    """Return raw wrapper orientation before presentation canonicalization."""

    return -1 if wire.wrapped.Orientation() == TopAbs_Orientation.TopAbs_REVERSED else 1


def _canonical_cycle(
    items: tuple[tuple[EdgeGeometry, int], ...],
) -> tuple[tuple[EdgeGeometry, int], ...]:
    if not items:
        raise UnsupportedBodyGeometry("body wire has no supported edge occurrences")
    forward = tuple(items[index:] + items[:index] for index in range(len(items)))
    reversed_items = tuple(_reverse_edge(item) for item in reversed(items))
    backward = tuple(
        reversed_items[index:] + reversed_items[:index] for index in range(len(reversed_items))
    )
    return min((*forward, *backward))


def _canonical_cycle_with_tokens(
    items: tuple[tuple[EdgeGeometry, int, object], ...],
    raw_orientation: int,
) -> tuple[
    tuple[tuple[EdgeGeometry, int], ...],
    tuple[tuple[tuple[object, int], ...], ...],
    int,
]:
    """Canonical semantic presentation and every tied physical alignment."""

    if not items:
        raise UnsupportedBodyGeometry("body wire has no supported edge occurrences")
    candidates = []
    for reversed_presentation, source in (
        (False, items),
        (True, tuple((edge, -direction, token) for edge, direction, token in reversed(items))),
    ):
        for index in range(len(source)):
            rotated = source[index:] + source[:index]
            label = tuple((edge, direction) for edge, direction, _token in rotated)
            tokens = tuple((token, direction) for _edge, direction, token in rotated)
            semantic_winding = raw_orientation * (-1 if reversed_presentation else 1)
            candidates.append((semantic_winding, label, tokens))
    label = min(candidate for _semantic, candidate, _tokens in candidates)
    matching = tuple(item for item in candidates if item[1] == label)
    semantics = {semantic for semantic, _candidate, _tokens in matching}
    if len(semantics) != 1:
        raise UnsupportedBodyGeometry("wire semantic winding is ambiguous")
    semantic_winding = semantics.pop()
    alignments = tuple(
        {
            tokens
            for semantic, candidate, tokens in matching
            if semantic == semantic_winding and candidate == label
        }
    )
    return label, alignments, semantic_winding


def _arc_sweep(edge, axis: tuple[float, float, float]) -> float:
    radius = float(edge.radius)
    if radius <= 0.0:
        raise UnsupportedBodyGeometry("circle radius is degenerate")
    magnitude = float(edge.length) / radius
    if magnitude <= ANGLE_TOL or magnitude > 2.0 * math.pi + ANGLE_TOL:
        raise UnsupportedBodyGeometry("circle sweep is outside the supported range")
    if abs(magnitude - 2.0 * math.pi) <= ANGLE_TOL:
        return 2.0 * math.pi
    start = edge.position_at(0.0)
    middle = edge.position_at(0.5)
    centre = edge.arc_center
    sx, sy, sz = start.X - centre.X, start.Y - centre.Y, start.Z - centre.Z
    mx, my, mz = middle.X - centre.X, middle.Y - centre.Y, middle.Z - centre.Z
    cross = (sy * mz - sz * my, sz * mx - sx * mz, sx * my - sy * mx)
    direction = sum(component * normal for component, normal in zip(cross, axis, strict=True))
    return magnitude if direction >= 0.0 else -magnitude


def _edge_geometry(edge, centre: tuple[float, float, float], quantum: float) -> EdgeGeometry:
    kind = getattr(edge.geom_type, "name", str(edge.geom_type))
    start = _point(edge.position_at(0.0), centre, quantum, name="edge endpoint")
    end = _point(edge.position_at(1.0), centre, quantum, name="edge endpoint")
    length = float(edge.length)
    qlength = _positive_fact(length, quantum, name="edge length")
    if edge.geom_type == GeomType.LINE:
        return EdgeGeometry("LINE", min(start, end), max(start, end), qlength)
    if edge.geom_type != GeomType.CIRCLE:
        raise UnsupportedBodyGeometry(f"unsupported body edge curve {kind}")
    curve = BRepAdaptor_Curve(edge.wrapped)
    circle = curve.Circle()
    raw_axis = _vector(circle.Axis().Direction())
    axis, axis_sign = _canonical_axis(raw_axis)
    sweep = axis_sign * _arc_sweep(edge, raw_axis)
    first = (start, end, _snap_checked(sweep, ANGLE_TOL, name="circle sweep"))
    second = (end, start, _snap_checked(-sweep, ANGLE_TOL, name="circle sweep"))
    canonical_start, canonical_end, canonical_sweep = min(first, second)
    return EdgeGeometry(
        "CIRCLE",
        canonical_start,
        canonical_end,
        qlength,
        _point(edge.arc_center, centre, quantum, name="circle centre"),
        _qaxis(axis),
        _positive_fact(float(edge.radius), quantum, name="circle radius"),
        canonical_sweep,
        abs(abs(sweep) - 2.0 * math.pi) <= ANGLE_TOL,
    )


def _face_geometry(face, centre: tuple[float, float, float], scale: float) -> _FaceBuild:
    quantum = _metric_tolerance(scale)
    area_quantum = (scale + quantum) ** 2 - scale**2
    area = float(face.area)
    if not math.isfinite(area) or area <= area_quantum:
        raise UnsupportedBodyGeometry("face area is degenerate")
    adaptor = BRepAdaptor_Surface(face.wrapped)
    parameters: tuple[float, ...]
    if face.geom_type == GeomType.PLANE:
        plane = adaptor.Plane()
        raw_axis = _vector(plane.Axis().Direction())
        location = plane.Location()
        location_values = _finite(location.X(), location.Y(), location.Z())
        location_tuple = (location_values[0], location_values[1], location_values[2])
        axis, _ = _canonical_axis(raw_axis)
        parameters = _plane_parameters(raw_axis, location_tuple, centre, quantum)
        normal = face.normal_at(face.center())
        material_side = (
            1
            if sum(
                component * normal_component
                for component, normal_component in zip(
                    axis, (normal.X, normal.Y, normal.Z), strict=True
                )
            )
            >= 0.0
            else -1
        )
        kind = "PLANE"
    elif face.geom_type == GeomType.CYLINDER:
        cylinder = adaptor.Cylinder()
        raw_axis = _vector(cylinder.Axis().Direction())
        axis, _ = _canonical_axis(raw_axis)
        location = cylinder.Location()
        loc = _finite(location.X(), location.Y(), location.Z())
        delta = tuple(component - origin for component, origin in zip(loc, centre, strict=True))
        along = sum(component * direction for component, direction in zip(delta, axis, strict=True))
        closest_values = tuple(
            component - along * direction for component, direction in zip(delta, axis, strict=True)
        )
        closest = (closest_values[0], closest_values[1], closest_values[2])
        parameters = (
            *_qaxis(axis),
            *_relative_point(closest, quantum, name="cylinder axis point"),
            _positive_fact(float(cylinder.Radius()), quantum, name="cylinder radius"),
        )
        sample = face.center()
        sample_delta = tuple(
            component - origin
            for component, origin in zip((sample.X, sample.Y, sample.Z), loc, strict=True)
        )
        sample_along = sum(
            component * direction for component, direction in zip(sample_delta, axis, strict=True)
        )
        radial = tuple(
            component - sample_along * direction
            for component, direction in zip(sample_delta, axis, strict=True)
        )
        normal = face.normal_at(sample)
        material_side = (
            1
            if sum(
                component * normal_component
                for component, normal_component in zip(
                    radial, (normal.X, normal.Y, normal.Z), strict=True
                )
            )
            >= 0.0
            else -1
        )
        kind = "CYLINDER"
    else:
        raise UnsupportedBodyGeometry(f"unsupported body face surface {face.geom_type}")

    outer = face.outer_wire()
    wire_builds: list[_WireBuild] = []
    for wire in face.wires():
        occurrences = tuple(
            (
                _edge_geometry(edge, centre, quantum),
                -1 if edge.wrapped.Orientation() == TopAbs_Orientation.TopAbs_REVERSED else 1,
                edge,
            )
            for edge in wire.edges()
        )
        role = "outer" if wire == outer else "inner"
        raw_wire_orientation = _wire_orientation(wire)
        raw_orientation = raw_wire_orientation * material_side
        canonical, alignments, semantic_winding = _canonical_cycle_with_tokens(
            occurrences, raw_orientation
        )
        wire_builds.append(_WireBuild(WireGeometry(role, semantic_winding, canonical), alignments))
    wire_builds.sort(key=lambda item: item.geometry)
    face_centre = face.center()
    geometry = FaceGeometry(
        kind,
        tuple(parameters),
        _positive_fact(area, area_quantum, name="face area"),
        _point(face_centre, centre, quantum, name="face centroid"),
        material_side,
        tuple(item.geometry for item in wire_builds),
    )
    return _FaceBuild(geometry, tuple(wire_builds))


def _canonical_topology(
    face_builds: tuple[_FaceBuild, ...],
) -> tuple[
    tuple[FaceGeometry, ...],
    tuple[tuple[EdgeGeometry, tuple[tuple[int, int, int, int], ...]], ...],
    bool,
]:
    """Canonicalize the complete labelled face/wire/edge-occurrence graph."""

    by_label: dict[FaceGeometry, list[int]] = {}
    for index, build in enumerate(face_builds):
        by_label.setdefault(build.geometry, []).append(index)
    classes = tuple(tuple(indices) for _label, indices in sorted(by_label.items()))
    face_choices = 1
    for entries in classes:
        face_choices *= math.factorial(len(entries))
        if face_choices > CANONICAL_SERIALIZATION_BUDGET:
            raise UnsupportedBodyGeometry("body topology canonicalization budget is exhausted")

    wire_variants: dict[
        int,
        tuple[tuple[tuple[int, ...], tuple[tuple[tuple[object, int], ...], ...]], ...],
    ] = {}
    for face_index, build in enumerate(face_builds):
        wire_classes: list[tuple[int, ...]] = []
        by_wire: dict[WireGeometry, list[int]] = {}
        for wire_index, wire in enumerate(build.wires):
            by_wire.setdefault(wire.geometry, []).append(wire_index)
        for _label, wire_entries in sorted(by_wire.items()):
            wire_classes.append(tuple(wire_entries))
        variants = []
        for groups in product(*(permutations(entries) for entries in wire_classes)):
            order = tuple(index for group in groups for index in group)
            for alignments in product(*(build.wires[index].occurrences for index in order)):
                variants.append((order, alignments))
                if len(variants) > CANONICAL_SERIALIZATION_BUDGET:
                    raise UnsupportedBodyGeometry(
                        "body topology canonicalization budget is exhausted"
                    )
        wire_variants[face_index] = tuple(variants)

    complete_choices = face_choices
    for face_index in range(len(face_builds)):
        complete_choices *= len(wire_variants[face_index])
        if complete_choices > CANONICAL_SERIALIZATION_BUDGET:
            raise UnsupportedBodyGeometry("body topology canonicalization budget is exhausted")

    minimum = None
    minimizing = 0
    face_variants = product(*(permutations(entries) for entries in classes))
    generated = 0
    for class_permutations in face_variants:
        ordered_raw = tuple(index for group in class_permutations for index in group)
        for selected_faces in product(*(wire_variants[index] for index in ordered_raw)):
            generated += 1
            if generated > CANONICAL_SERIALIZATION_BUDGET:
                raise UnsupportedBodyGeometry("body topology canonicalization budget is exhausted")
            ordered_faces: list[FaceGeometry] = []
            occurrence_map: dict[object, list[tuple[int, int, int, int]]] = {}
            edge_labels: dict[object, list[EdgeGeometry]] = {}
            for canonical_face, (raw_face, selected_face) in enumerate(
                zip(ordered_raw, selected_faces, strict=True)
            ):
                wire_order, selected_alignments = selected_face
                build = face_builds[raw_face]
                ordered_wire_geometry = tuple(build.wires[index].geometry for index in wire_order)
                ordered_faces.append(
                    FaceGeometry(
                        build.geometry.kind,
                        build.geometry.parameters,
                        build.geometry.area,
                        build.geometry.centroid,
                        build.geometry.material_side,
                        ordered_wire_geometry,
                    )
                )
                for canonical_wire, (raw_wire, selected_alignment) in enumerate(
                    zip(wire_order, selected_alignments, strict=True)
                ):
                    wire = build.wires[raw_wire]
                    for occurrence, ((edge, _), (token, direction)) in enumerate(
                        zip(wire.geometry.edges, selected_alignment, strict=True)
                    ):
                        edge_labels.setdefault(token, []).append(edge)
                        occurrence_map.setdefault(token, []).append(
                            (
                                canonical_face,
                                canonical_wire,
                                occurrence,
                                direction,
                            )
                        )
            incidence_items = []
            for token, occurrences in occurrence_map.items():
                labels = edge_labels[token]
                if len(occurrences) != 2:
                    raise UnsupportedBodyGeometry(
                        "body edge incidence is not a supported closed-shell pair"
                    )
                if any(label != labels[0] for label in labels[1:]):
                    raise UnsupportedBodyGeometry("one body edge has conflicting analytic labels")
                incidence_items.append((labels[0], tuple(sorted(occurrences))))
            incidence = tuple(sorted(incidence_items))
            candidate = (tuple(ordered_faces), incidence)
            if minimum is None or candidate < minimum:
                minimum = candidate
                minimizing = 1
            elif candidate == minimum:
                minimizing += 1
    if minimum is None:
        raise UnsupportedBodyGeometry("body topology has no canonical serialization")
    return minimum[0], minimum[1], minimizing > 1


def describe_solid(solid) -> _DescribedBody:
    """Build one complete supported descriptor or refuse without a partial value."""

    props = GProp_GProps()
    try:
        if (
            solid.wrapped.ShapeType() != TopAbs_SOLID
            or not BRepCheck_Analyzer(solid.wrapped).IsValid()
        ):
            raise UnsupportedBodyGeometry("body is not one valid closed solid")
        BRepGProp.VolumeProperties_s(solid.wrapped, props)
        volume = float(props.Mass())
        centre_point = props.CentreOfMass()
        raw_centre = _finite(centre_point.X(), centre_point.Y(), centre_point.Z())
        centre = (raw_centre[0], raw_centre[1], raw_centre[2])
        surface_props = GProp_GProps()
        BRepGProp.SurfaceProperties_s(solid.wrapped, surface_props)
        surface_area = float(surface_props.Mass())
        moments = tuple(float(value) for value in props.PrincipalProperties().Moments())
    except UnsupportedBodyGeometry:
        raise
    except Standard_Failure as error:
        raise UnsupportedBodyGeometry("kernel mass properties are unavailable") from error
    if (
        not math.isfinite(volume)
        or not math.isfinite(surface_area)
        or volume <= 0.0
        or surface_area <= 0.0
        or not all(math.isfinite(value) and value >= 0.0 for value in moments)
    ):
        raise UnsupportedBodyGeometry("body mass properties are degenerate")
    scale = max(volume ** (1.0 / 3.0), math.sqrt(surface_area))
    quantum = _metric_tolerance(scale)
    area_quantum = (scale + quantum) ** 2 - scale**2
    volume_quantum = (scale + quantum) ** 3 - scale**3
    moment_quantum = (scale + quantum) ** 5 - scale**5
    try:
        raw_faces = tuple(solid.faces())
    except (RuntimeError, Standard_Failure) as error:
        raise UnsupportedBodyGeometry("kernel body boundary is unavailable") from error
    # Python validation/canonicalization failures are programmer errors and must not be
    # relabelled as unsupported kernel geometry. Individual OCCT adaptor failures use the
    # closed Standard_Failure boundary; build123d's solid.faces() RuntimeError is caught above.
    try:
        raw_geometry = tuple(_face_geometry(face, centre, scale) for face in raw_faces)
    except Standard_Failure as error:
        raise UnsupportedBodyGeometry("kernel body boundary is unavailable") from error
    faces, incidence, symmetric = _canonical_topology(raw_geometry)
    wire_count = sum(len(face.wires) for face in faces)
    edge_count = sum(len(wire.edges) for face in faces for wire in face.wires)
    descriptor = BodyGeometryDescriptor(
        BodyIntrinsic(
            _positive_fact(volume, volume_quantum, name="body volume"),
            _positive_fact(surface_area, area_quantum, name="body surface area"),
            tuple(
                sorted(
                    _positive_fact(value, moment_quantum, name="principal moment")
                    for value in moments
                )
            ),  # type: ignore[arg-type]
        ),
        BodyBoundaryGeometry(faces, incidence, len(faces), wire_count, edge_count, symmetric),
        BodyPlacement(centre),
        DescriptorQuantization(
            scale,
            quantum,
            area_quantum,
            volume_quantum,
            moment_quantum,
        ),
    )
    return _DescribedBody(
        descriptor,
        raw_faces,
        tuple(build.geometry for build in raw_geometry),
        raw_geometry,
    )


def _same_shape(left: Any, right: Any) -> bool:
    return bool(left.wrapped.IsSame(right.wrapped))


def _identity_index(items: list[Any], value: Any) -> int:
    for index, item in enumerate(items):
        if _same_shape(item, value):
            return index
    items.append(value)
    return len(items) - 1


def _plane_basis(normal: QPoint) -> tuple[QPoint, QPoint]:
    axes: tuple[QPoint, ...] = (
        (1.0, 0.0, 0.0),
        (0.0, 1.0, 0.0),
        (0.0, 0.0, 1.0),
    )
    reference = min(
        axes,
        key=lambda axis: abs(sum(a * n for a, n in zip(axis, normal, strict=True))),
    )
    projection = tuple(
        axis - sum(a * n for a, n in zip(reference, normal, strict=True)) * component
        for axis, component in zip(reference, normal, strict=True)
    )
    length = math.sqrt(sum(component * component for component in projection))
    if length <= DIRECTION_TOL:
        raise UnsupportedBodyGeometry("matching plane basis is degenerate")
    u = cast(QPoint, tuple(component / length for component in projection))
    v = (
        normal[1] * u[2] - normal[2] * u[1],
        normal[2] * u[0] - normal[0] * u[2],
        normal[0] * u[1] - normal[1] * u[0],
    )
    return u, v


def _plane_parameter(point: QPoint, face: FaceGeometry, quantum: float) -> tuple[float, float]:
    normal = face.parameters[:3]
    if len(normal) != 3:
        raise UnsupportedBodyGeometry("matching plane normal is malformed")
    u, v = _plane_basis(normal)
    origin = tuple(face.parameters[3] * component for component in normal)
    delta = tuple(component - offset for component, offset in zip(point, origin, strict=True))
    return (
        _snap_checked(sum(a * b for a, b in zip(delta, u, strict=True)), quantum, name="pcurve u"),
        _snap_checked(sum(a * b for a, b in zip(delta, v, strict=True)), quantum, name="pcurve v"),
    )


def _half_edge_integral(
    half_edge: MatchingHalfEdge,
    curves: tuple[MatchingCurve, ...],
    face: FaceGeometry,
    quantum: float,
) -> float:
    curve = curves[half_edge.curve]
    normal = face.parameters[:3]
    if curve.kind == "LINE":
        if half_edge.start is None or half_edge.end is None:
            raise UnsupportedBodyGeometry("matching line half-edge has no endpoints")
        left, right = half_edge.start.parameter, half_edge.end.parameter
        return 0.5 * (left[0] * right[1] - right[0] * left[1])
    if (
        curve.kind != "CIRCLE"
        or curve.centre is None
        or curve.axis is None
        or curve.radius is None
        or curve.sweep is None
    ):
        raise UnsupportedBodyGeometry("matching circle half-edge is malformed")
    axis_factor = sum(a * b for a, b in zip(curve.axis, normal, strict=True))
    if abs(abs(axis_factor) - 1.0) > DIRECTION_TOL:
        raise UnsupportedBodyGeometry("matching planar circle axis is not face-normal")
    sweep = half_edge.direction * curve.sweep * (1.0 if axis_factor > 0.0 else -1.0)
    centre = _plane_parameter(curve.centre, face, quantum)
    if curve.full:
        return 0.5 * curve.radius**2 * sweep
    if half_edge.start is None or half_edge.end is None:
        raise UnsupportedBodyGeometry("matching trimmed circle has no endpoints")
    start = half_edge.start.parameter
    end = half_edge.end.parameter
    start_angle = math.atan2(start[1] - centre[1], start[0] - centre[0])
    end_angle = start_angle + sweep
    reconstructed_end = (
        centre[0] + curve.radius * math.cos(end_angle),
        centre[1] + curve.radius * math.sin(end_angle),
    )
    # Centre, radius and endpoint are independently reconstructed values.  Their
    # conservative closed residual is the sum of the four two-quantum contracts.
    if math.dist(reconstructed_end, end) > 8.0 * quantum:
        raise UnsupportedBodyGeometry("matching circle sweep does not reconstruct its endpoint")
    return 0.5 * (
        curve.radius * centre[0] * (math.sin(end_angle) - math.sin(start_angle))
        + curve.radius * centre[1] * (-math.cos(end_angle) + math.cos(start_angle))
        + curve.radius**2 * sweep
    )


def _planar_cycle(
    curve_indices: tuple[int, ...],
    curves: tuple[MatchingCurve, ...],
    role: str,
    face: FaceGeometry,
    quantum: float,
    vertices: tuple[QPoint, ...],
    budget: _MatchingConstructionBudget,
) -> MatchingWire:
    adjacency: dict[int, list[tuple[int, int]]] = {}
    for curve_index in curve_indices:
        curve = curves[curve_index]
        if curve.full:
            if len(curve_indices) != 1 or curve.kind != "CIRCLE":
                raise UnsupportedBodyGeometry("full matching circle is not one complete wire")
            continue
        if curve.kind not in {"LINE", "CIRCLE"} or curve.vertices is None:
            raise UnsupportedBodyGeometry("matching planar curve is malformed")
        left, right = curve.vertices
        adjacency.setdefault(left, []).append((curve_index, right))
        adjacency.setdefault(right, []).append((curve_index, left))
    if len(curve_indices) == 1 and curves[curve_indices[0]].full:
        curve_index = curve_indices[0]
        alternatives = tuple(
            MatchingHalfEdge(curve_index, direction, None, None) for direction in (-1, 1)
        )
        for _alternative in alternatives:
            budget.charge()
        expected_positive = (role == "outer") == (face.material_side > 0)
        full_oriented = tuple(
            item
            for item in alternatives
            if (_half_edge_integral(item, curves, face, quantum) > 0.0) == expected_positive
        )
        if len(full_oriented) != 1:
            raise UnsupportedBodyGeometry("full matching circle orientation is ambiguous")
        return MatchingWire(role, 0, (full_oriented[0],))
    if not adjacency or any(len(entries) != 2 for entries in adjacency.values()):
        raise UnsupportedBodyGeometry("matching wire is not one degree-two cycle")
    normal = cast(QPoint, face.parameters[:3])
    offset = face.parameters[3]
    u_axis, v_axis = _plane_basis(normal)
    plane_origin = tuple(offset * item for item in normal)
    parameters = {}
    for vertex in adjacency:
        delta = tuple(
            value - origin for value, origin in zip(vertices[vertex], plane_origin, strict=True)
        )
        parameters[vertex] = (
            _snap_checked(
                sum(a * b for a, b in zip(delta, u_axis, strict=True)),
                quantum,
                name="pcurve u",
            ),
            _snap_checked(
                sum(a * b for a, b in zip(delta, v_axis, strict=True)),
                quantum,
                name="pcurve v",
            ),
        )

    candidates: list[tuple[MatchingHalfEdge, ...]] = []
    minimum_vertex = min(vertices[index] for index in adjacency)
    starts = tuple(index for index in adjacency if vertices[index] == minimum_vertex)
    for start in starts:
        for first_curve, first_next in adjacency[start]:
            budget.charge()
            cycle: list[MatchingHalfEdge] = []
            current = start
            curve_index = first_curve
            following = first_next
            used: set[int] = set()
            while curve_index not in used:
                budget.charge()
                used.add(curve_index)
                curve = curves[curve_index]
                assert curve.vertices is not None
                direction = 1 if curve.vertices == (current, following) else -1
                cycle.append(
                    MatchingHalfEdge(
                        curve_index,
                        direction,
                        MatchingWireVertex(
                            current,
                            parameters[current],
                        ),
                        MatchingWireVertex(
                            following,
                            parameters[following],
                        ),
                    )
                )
                current = following
                choices = [item for item in adjacency[current] if item[0] not in used]
                if not choices:
                    break
                if len(choices) != 1:
                    raise UnsupportedBodyGeometry("matching wire cycle is ambiguous")
                curve_index, following = choices[0]
            if current == start and len(used) == len(curve_indices):
                candidates.append(tuple(cycle))
    if not candidates:
        raise UnsupportedBodyGeometry("matching wire does not close")
    oriented: list[tuple[MatchingHalfEdge, ...]] = []
    expected_positive = (role == "outer") == (face.material_side > 0)
    for candidate in candidates:
        budget.charge()
        area = sum(_half_edge_integral(item, curves, face, quantum) for item in candidate)
        if abs(area) <= quantum**2:
            raise UnsupportedBodyGeometry("matching wire signed area is degenerate")
        if (area > 0.0) == expected_positive:
            oriented.append(candidate)
    if not oriented:
        raise UnsupportedBodyGeometry("matching wire has no material-oriented cycle")
    return MatchingWire(role, 0, min(oriented))


def _cylinder_parameter(
    adaptor: BRepAdaptor_Surface,
    raw_u: float,
    raw_v: float,
    centre: QPoint,
    quantum: float,
    gauge: tuple[QPoint, QPoint, float, float],
) -> tuple[float, float]:
    axis, axis_point, phase, axis_sign = gauge
    point = adaptor.Value(raw_u, raw_v)
    relative = (
        point.X() - centre[0] - axis_point[0],
        point.Y() - centre[1] - axis_point[1],
        point.Z() - centre[2] - axis_point[2],
    )
    z = sum(value * normal for value, normal in zip(relative, axis, strict=True))
    return (
        _snap_checked(phase + axis_sign * raw_u, ANGLE_TOL, name="cylinder theta"),
        _snap_checked(z, quantum, name="cylinder z"),
    )


def _cylinder_gauge(
    surface: BRepAdaptor_Surface,
    face: FaceGeometry,
    centre: QPoint,
) -> tuple[QPoint, QPoint, float, float]:
    axis = cast(QPoint, face.parameters[:3])
    axis_point = cast(QPoint, face.parameters[3:6])
    u_axis, v_axis = _plane_basis(axis)
    raw_axis = _vector(surface.Cylinder().Axis().Direction())
    axis_sign = 1.0 if sum(a * b for a, b in zip(raw_axis, axis, strict=True)) > 0 else -1.0
    origin = surface.Value(0.0, 0.0)
    origin_relative = (
        origin.X() - centre[0] - axis_point[0],
        origin.Y() - centre[1] - axis_point[1],
        origin.Z() - centre[2] - axis_point[2],
    )
    along = sum(x * n for x, n in zip(origin_relative, axis, strict=True))
    radial = tuple(
        value - along * normal for value, normal in zip(origin_relative, axis, strict=True)
    )
    phase = math.atan2(
        sum(value * basis for value, basis in zip(radial, v_axis, strict=True)),
        sum(value * basis for value, basis in zip(radial, u_axis, strict=True)),
    )
    return axis, axis_point, phase, axis_sign


def _validate_matching_pcurve(
    edge: Any,
    face: Any,
    curve: BRepAdaptor_Curve,
    surface: BRepAdaptor_Surface,
    quantum: float,
    kind: str,
    full: bool,
) -> None:
    """Prove one exact face-edge pcurve occurrence reconstructs its 3-D curve."""

    try:
        pcurve = BRepAdaptor_Curve2d(edge.wrapped, face.wrapped)
        curve_first, curve_last = curve.FirstParameter(), curve.LastParameter()
        pcurve_first, pcurve_last = pcurve.FirstParameter(), pcurve.LastParameter()
    except Standard_Failure as error:
        raise UnsupportedBodyGeometry("matching pcurve is unavailable") from error
    parameters = (curve_first, curve_last, pcurve_first, pcurve_last)
    if not all(math.isfinite(value) for value in parameters):
        raise UnsupportedBodyGeometry("matching pcurve parameter range is non-finite")
    fractions: tuple[float, ...]
    if kind == "LINE":
        fractions = (0.0, 0.5, 1.0)
    elif kind == "CIRCLE":
        fractions = (0.0, 0.25, 0.5, 0.75) + (() if full else (1.0,))
    else:
        raise UnsupportedBodyGeometry("matching pcurve has an unsupported 3-D curve")
    for fraction in fractions:
        curve_parameter = curve_first + fraction * (curve_last - curve_first)
        pcurve_parameter = pcurve_first + fraction * (pcurve_last - pcurve_first)
        try:
            curve_point = gp_Pnt()
            curve_tangent = gp_Vec()
            curve.D1(curve_parameter, curve_point, curve_tangent)
            uv = gp_Pnt2d()
            uv_tangent = gp_Vec2d()
            pcurve.D1(pcurve_parameter, uv, uv_tangent)
            surface_point = gp_Pnt()
            surface_u = gp_Vec()
            surface_v = gp_Vec()
            surface.D1(uv.X(), uv.Y(), surface_point, surface_u, surface_v)
        except Standard_Failure as error:
            raise UnsupportedBodyGeometry("matching pcurve reconstruction failed") from error
        if math.dist(curve_point.Coord(), surface_point.Coord()) > 2.0 * quantum:
            raise UnsupportedBodyGeometry("matching pcurve does not reconstruct its 3-D curve")
        reconstructed = (
            uv_tangent.X() * surface_u.X() + uv_tangent.Y() * surface_v.X(),
            uv_tangent.X() * surface_u.Y() + uv_tangent.Y() * surface_v.Y(),
            uv_tangent.X() * surface_u.Z() + uv_tangent.Y() * surface_v.Z(),
        )
        source = curve_tangent.Coord()
        source_norm = math.sqrt(sum(value * value for value in source))
        reconstructed_norm = math.sqrt(sum(value * value for value in reconstructed))
        if source_norm <= 0.0 or reconstructed_norm <= 0.0:
            raise UnsupportedBodyGeometry("matching pcurve tangent is degenerate")
        cosine = sum(left * right for left, right in zip(source, reconstructed, strict=True)) / (
            source_norm * reconstructed_norm
        )
        if 1.0 - max(-1.0, min(1.0, cosine)) > 2.0 * ANGLE_TOL:
            raise UnsupportedBodyGeometry("matching pcurve tangent exceeds its angular bound")


def _cylinder_pcurve_variants(
    edge: Any,
    face: Any,
    face_adaptor: BRepAdaptor_Surface,
    centre: QPoint,
    quantum: float,
    gauge: tuple[QPoint, QPoint, float, float],
    kind: str,
    full: bool,
    budget: _MatchingConstructionBudget,
) -> tuple[tuple[tuple[float, float], tuple[float, float]], ...]:
    """Return the complete unordered bounded pcurve roster for one edge/face token."""

    variants: set[tuple[tuple[float, float], tuple[float, float]]] = set()
    for orientation in (TopAbs_FORWARD, TopAbs_REVERSED):
        budget.charge()
        oriented = Edge(TopoDS.Edge_s(edge.wrapped.Oriented(orientation)))
        curve = BRepAdaptor_Curve(oriented.wrapped)
        _validate_matching_pcurve(
            oriented,
            face,
            curve,
            face_adaptor,
            quantum,
            kind,
            full,
        )
        try:
            pcurve = BRepAdaptor_Curve2d(oriented.wrapped, face.wrapped)
            first = pcurve.FirstParameter()
            last = pcurve.LastParameter()
            raw_start = pcurve.Value(first)
            raw_end = pcurve.Value(last)
        except Standard_Failure as error:
            raise UnsupportedBodyGeometry(
                "matching cylinder pcurve roster is unavailable"
            ) from error
        budget.charge()
        variants.add(
            (
                _cylinder_parameter(
                    face_adaptor,
                    raw_start.X(),
                    raw_start.Y(),
                    centre,
                    quantum,
                    gauge,
                ),
                _cylinder_parameter(
                    face_adaptor,
                    raw_end.X(),
                    raw_end.Y(),
                    centre,
                    quantum,
                    gauge,
                ),
            )
        )
    if not variants:
        raise UnsupportedBodyGeometry("matching cylinder pcurve roster is empty")
    expected = 2 if BRep_Tool.IsClosed_s(edge.wrapped, face.wrapped) else 1
    if len(variants) != expected:
        raise UnsupportedBodyGeometry("matching cylinder pcurve closure roster is incomplete")
    return tuple(sorted(variants))


def _cylinder_cycle_assignment(
    pcurves: tuple[_CylinderPcurveOccurrence, ...],
    curves: tuple[MatchingCurve, ...],
    role: str,
    face: FaceGeometry,
    quantum: float,
    budget: _MatchingConstructionBudget,
) -> set[MatchingWire]:
    if not pcurves:
        raise UnsupportedBodyGeometry("matching cylinder wire is empty")

    def oriented(
        occurrence: _CylinderPcurveOccurrence, reverse: bool
    ) -> tuple[MatchingHalfEdge, tuple[float, float], tuple[float, float]]:
        curve = curves[occurrence.curve]
        start_index = occurrence.end_vertex if reverse else occurrence.start_vertex
        end_index = occurrence.start_vertex if reverse else occurrence.end_vertex
        start_parameter = occurrence.end_parameter if reverse else occurrence.start_parameter
        end_parameter = occurrence.start_parameter if reverse else occurrence.end_parameter
        if curve.full:
            if start_index is not None or end_index is not None:
                raise UnsupportedBodyGeometry("full cylinder curve retained a seam vertex")
            direction = 1 if end_parameter[0] > start_parameter[0] else -1
            half_edge = MatchingHalfEdge(occurrence.curve, direction, None, None)
        else:
            if curve.vertices is None or start_index is None or end_index is None:
                raise UnsupportedBodyGeometry("trimmed cylinder curve lost a topology vertex")
            if (start_index, end_index) == curve.vertices:
                direction = 1
            elif (end_index, start_index) == curve.vertices:
                direction = -1
            else:
                raise UnsupportedBodyGeometry(
                    "cylinder pcurve vertices disagree with their global curve"
                )
            half_edge = MatchingHalfEdge(
                occurrence.curve,
                direction,
                MatchingWireVertex(start_index, start_parameter),
                MatchingWireVertex(end_index, end_parameter),
            )
        return half_edge, start_parameter, end_parameter

    def lifted_join(
        left: tuple[MatchingHalfEdge, tuple[float, float], tuple[float, float]],
        right: tuple[MatchingHalfEdge, tuple[float, float], tuple[float, float]],
    ) -> tuple[float, float] | None:
        left_edge, _left_start, left_end = left
        right_edge, right_start, _right_end = right
        if (
            left_edge.end is not None
            and right_edge.start is not None
            and left_edge.end.vertex != right_edge.start.vertex
        ):
            return None
        if abs(left_end[1] - right_start[1]) > 4.0 * quantum:
            return None
        turns = round((left_end[0] - right_start[0]) / (2.0 * math.pi))
        shifted = right_start[0] + turns * 2.0 * math.pi
        if abs(left_end[0] - shifted) > 4.0 * ANGLE_TOL:
            return None
        return turns * 2.0 * math.pi, shifted - right_start[0]

    joined_items: set[
        tuple[tuple[MatchingHalfEdge, tuple[float, float], tuple[float, float]], ...]
    ] = set()

    def retain(
        shifted: list[tuple[MatchingHalfEdge, tuple[float, float], tuple[float, float]]],
    ) -> None:
        budget.charge()
        closure = lifted_join(shifted[-1], shifted[0])
        if closure is None:
            return
        # A common lift is presentation only.  Normalize it once for the complete
        # continuous cycle instead of preserving a kernel pcurve seam choice.
        minimum_theta = min(value for _edge, start, end in shifted for value in (start[0], end[0]))
        common = -math.floor(minimum_theta / (2.0 * math.pi)) * 2.0 * math.pi
        normalized = []
        for half_edge, start, end in shifted:
            start_parameter = (
                _snap_checked(start[0] + common, ANGLE_TOL, name="cylinder theta"),
                start[1],
            )
            end_parameter = (
                _snap_checked(end[0] + common, ANGLE_TOL, name="cylinder theta"),
                end[1],
            )
            normalized.append(
                (
                    MatchingHalfEdge(
                        half_edge.curve,
                        half_edge.direction,
                        None
                        if half_edge.start is None
                        else MatchingWireVertex(half_edge.start.vertex, start_parameter),
                        None
                        if half_edge.end is None
                        else MatchingWireVertex(half_edge.end.vertex, end_parameter),
                    ),
                    start_parameter,
                    end_parameter,
                )
            )
        joined_items.add(tuple(normalized))

    def extend(
        shifted: list[tuple[MatchingHalfEdge, tuple[float, float], tuple[float, float]]],
        remaining: tuple[_CylinderPcurveOccurrence, ...],
    ) -> None:
        if not remaining:
            retain(shifted)
            return
        for index, occurrence in enumerate(remaining):
            budget.charge()
            rest = remaining[:index] + remaining[index + 1 :]
            for reverse in (False, True):
                budget.charge()
                item = oriented(occurrence, reverse)
                budget.charge()
                join = lifted_join(shifted[-1], item)
                if join is None:
                    continue
                _turns, offset = join
                half_edge, start, end = item
                shifted_item = (
                    MatchingHalfEdge(
                        half_edge.curve,
                        half_edge.direction,
                        None
                        if half_edge.start is None
                        else MatchingWireVertex(
                            half_edge.start.vertex,
                            (start[0] + offset, start[1]),
                        ),
                        None
                        if half_edge.end is None
                        else MatchingWireVertex(
                            half_edge.end.vertex,
                            (end[0] + offset, end[1]),
                        ),
                    ),
                    (start[0] + offset, start[1]),
                    (end[0] + offset, end[1]),
                )
                extend([*shifted, shifted_item], rest)

    for index, occurrence in enumerate(pcurves):
        budget.charge()
        remaining = pcurves[:index] + pcurves[index + 1 :]
        for reverse in (False, True):
            budget.charge()
            extend([oriented(occurrence, reverse)], remaining)
    if not joined_items:
        return set()
    accepted: set[MatchingWire] = set()
    expected_positive = (role == "outer") == (face.material_side > 0)
    for occurrence_cycle in joined_items:
        budget.charge()
        radius = face.parameters[6]
        parameter_points = tuple(
            (start[0], start[1] / radius) for _edge, start, _end in occurrence_cycle
        )
        tolerance = max(ANGLE_TOL, quantum / radius)

        def orientation(
            left: tuple[float, float],
            middle: tuple[float, float],
            right: tuple[float, float],
        ) -> float:
            return (middle[0] - left[0]) * (right[1] - left[1]) - (middle[1] - left[1]) * (
                right[0] - left[0]
            )

        def intersects(
            left_start: tuple[float, float],
            left_end: tuple[float, float],
            right_start: tuple[float, float],
            right_end: tuple[float, float],
            _tolerance: float = tolerance,
        ) -> bool:
            values = (
                orientation(left_start, left_end, right_start),
                orientation(left_start, left_end, right_end),
                orientation(right_start, right_end, left_start),
                orientation(right_start, right_end, left_end),
            )
            if all(abs(value) > _tolerance for value in values):
                return (values[0] > 0.0) != (values[1] > 0.0) and (
                    (values[2] > 0.0) != (values[3] > 0.0)
                )
            return not (
                max(left_start[0], left_end[0]) < min(right_start[0], right_end[0]) - _tolerance
                or max(right_start[0], right_end[0]) < min(left_start[0], left_end[0]) - _tolerance
                or max(left_start[1], left_end[1]) < min(right_start[1], right_end[1]) - _tolerance
                or max(right_start[1], right_end[1]) < min(left_start[1], left_end[1]) - _tolerance
            )

        simple = True
        for left_index, left_start in enumerate(parameter_points):
            left_end = parameter_points[(left_index + 1) % len(parameter_points)]
            if math.dist(left_start, left_end) <= tolerance:
                simple = False
                break
            for right_index in range(left_index + 1, len(parameter_points)):
                if right_index in {
                    left_index,
                    (left_index + 1) % len(parameter_points),
                } or left_index == (right_index + 1) % len(parameter_points):
                    continue
                right_start = parameter_points[right_index]
                right_end = parameter_points[(right_index + 1) % len(parameter_points)]
                if intersects(left_start, left_end, right_start, right_end):
                    simple = False
                    break
            if not simple:
                break
        if not simple:
            continue
        area = 0.5 * sum(
            left[0] * right[1] - right[0] * left[1] for _, left, right in occurrence_cycle
        )
        if abs(area) <= ANGLE_TOL * quantum:
            continue
        if (area > 0.0) != expected_positive:
            continue
        cycle = tuple(item for item, _, _ in occurrence_cycle)
        rotations = tuple(cycle[index:] + cycle[:index] for index in range(len(cycle)))
        theta_delta = sum(end[0] - start[0] for _, start, end in occurrence_cycle)
        theta_winding = int(round(theta_delta / (2.0 * math.pi)))
        if abs(theta_delta - theta_winding * 2.0 * math.pi) > 4.0 * ANGLE_TOL:
            continue
        # A simple closed curve on a cylindrical annulus represents only the zero
        # class or one primitive generator.  Larger winding repeats the same physical
        # cylinder and is not a distinct lawful boundary cycle.
        if abs(theta_winding) > 1:
            continue
        accepted.add(MatchingWire(role, theta_winding, min(rotations)))
    return accepted


def _cylinder_cycle(
    assignments: tuple[tuple[_CylinderPcurveOccurrence, ...], ...],
    curves: tuple[MatchingCurve, ...],
    role: str,
    face: FaceGeometry,
    quantum: float,
    budget: _MatchingConstructionBudget,
) -> MatchingWire:
    if not assignments:
        raise UnsupportedBodyGeometry("matching cylinder pcurve assignment roster is empty")
    accepted: set[MatchingWire] = set()
    for assignment in assignments:
        budget.charge()
        accepted.update(_cylinder_cycle_assignment(assignment, curves, role, face, quantum, budget))
    if len(accepted) != 1:
        raise UnsupportedBodyGeometry(
            "matching cylinder wire has no unique material-oriented cycle"
        )
    return accepted.pop()


def _matching_label_orders(
    values: tuple[Any, ...], budget: _MatchingConstructionBudget
) -> tuple[tuple[int, ...], ...]:
    groups = []
    grouped: dict[Any, list[int]] = {}
    for index, value in enumerate(values):
        grouped.setdefault(value, []).append(index)
    for value in sorted(grouped):
        group = tuple(grouped[value])
        choices = []
        for choice in permutations(group):
            budget.charge()
            choices.append(choice)
        groups.append(tuple(choices))
    orders = []
    for selection in product(*groups):
        budget.charge()
        orders.append(tuple(index for group in selection for index in group))
    return tuple(orders)


def _matching_graph_canonical(
    vertices: tuple[QPoint, ...],
    curves: tuple[MatchingCurve, ...],
    faces: tuple[MatchingFace, ...],
    budget: _MatchingConstructionBudget,
) -> MatchingBoundaryGraph:
    candidates: list[MatchingBoundaryGraph] = []
    for vertex_order in _matching_label_orders(vertices, budget):
        vertex_map = {old: new for new, old in enumerate(vertex_order)}
        ordered_vertices = tuple(vertices[index] for index in vertex_order)
        remapped_curves = tuple(
            MatchingCurve(
                curve.kind,
                None
                if curve.vertices is None
                else (vertex_map[curve.vertices[0]], vertex_map[curve.vertices[1]]),
                curve.length,
                curve.centre,
                curve.axis,
                curve.radius,
                curve.sweep,
                curve.full,
            )
            for curve in curves
        )
        for curve_order in _matching_label_orders(remapped_curves, budget):
            curve_map = {old: new for new, old in enumerate(curve_order)}
            ordered_curves = tuple(remapped_curves[index] for index in curve_order)
            remapped_faces = []
            for face in faces:
                remapped_wires = []
                for wire in face.wires:
                    cycle = tuple(
                        MatchingHalfEdge(
                            curve_map[item.curve],
                            item.direction,
                            None
                            if item.start is None
                            else MatchingWireVertex(
                                vertex_map[cast(int, item.start.vertex)],
                                item.start.parameter,
                            ),
                            None
                            if item.end is None
                            else MatchingWireVertex(
                                vertex_map[cast(int, item.end.vertex)], item.end.parameter
                            ),
                        )
                        for item in wire.cycle
                    )
                    rotations = tuple(cycle[index:] + cycle[:index] for index in range(len(cycle)))
                    remapped_wires.append(
                        MatchingWire(wire.role, wire.theta_winding, min(rotations))
                    )
                remapped_faces.append(
                    MatchingFace(
                        face.kind,
                        face.parameters,
                        face.area,
                        face.centroid,
                        face.material_side,
                        tuple(sorted(remapped_wires)),
                    )
                )
            remapped_faces_tuple = tuple(remapped_faces)
            for face_order in _matching_label_orders(remapped_faces_tuple, budget):
                budget.charge()
                ordered_faces = tuple(remapped_faces_tuple[index] for index in face_order)
                incidence: dict[int, list[tuple[int, int, int]]] = {}
                for face_index, face in enumerate(ordered_faces):
                    for wire_index, wire in enumerate(face.wires):
                        for occurrence, half_edge in enumerate(wire.cycle):
                            incidence.setdefault(half_edge.curve, []).append(
                                (face_index, wire_index, occurrence)
                            )
                if set(incidence) != set(range(len(ordered_curves))) or any(
                    len(occurrences) != 2 for occurrences in incidence.values()
                ):
                    raise UnsupportedBodyGeometry(
                        "matching curve incidence is not a closed-shell pair"
                    )
                candidates.append(
                    MatchingBoundaryGraph(
                        ordered_vertices,
                        ordered_curves,
                        ordered_faces,
                        tuple(
                            (index, tuple(sorted(incidence[index])))
                            for index in range(len(ordered_curves))
                        ),
                        len(ordered_faces),
                        sum(len(face.wires) for face in ordered_faces),
                        sum(len(wire.cycle) for face in ordered_faces for wire in face.wires),
                        False,
                    )
                )
    if not candidates:
        raise UnsupportedBodyGeometry("matching graph has no canonical serialization")

    def key(value: MatchingBoundaryGraph) -> tuple[object, ...]:
        return (
            value.vertices,
            value.curves,
            value.faces,
            value.incidence,
            value.face_count,
            value.wire_count,
            value.edge_occurrence_count,
        )

    selected = min(candidates, key=key)
    minimum = key(selected)
    return MatchingBoundaryGraph(
        selected.vertices,
        selected.curves,
        selected.faces,
        selected.incidence,
        selected.face_count,
        selected.wire_count,
        selected.edge_occurrence_count,
        sum(key(candidate) == minimum for candidate in candidates) > 1,
    )


def matching_boundary_for_solid(
    solid: Any,
    descriptor: BodyGeometryDescriptor,
    cached_face_builds: tuple[object, ...] | None = None,
) -> MatchingBoundaryGraph:
    """Build the bounded LINE/CIRCLE and PLANE/CYLINDER schema-three boundary graph."""

    centre = descriptor.placement.centre_of_mass
    quantum = descriptor.quantization.metric_quantum
    budget = _MatchingConstructionBudget()
    raw_vertices: list[Any] = []
    raw_curves: list[Any] = []
    raw_curve_indices: dict[Any, int] = {}
    raw_curve_vertices: dict[Any, tuple[int, ...]] = {}
    curve_adaptors: dict[Any, BRepAdaptor_Curve] = {}
    curve_examples: list[Any] = []
    faces = tuple(solid.faces())
    if cached_face_builds is None:
        face_builds = tuple(
            _face_geometry(face, centre, descriptor.quantization.characteristic_scale)
            for face in faces
        )
    elif len(cached_face_builds) != len(faces) or any(
        type(build) is not _FaceBuild for build in cached_face_builds
    ):
        raise UnsupportedBodyGeometry("matching cached face authority is malformed")
    else:
        face_builds = cast(tuple[_FaceBuild, ...], cached_face_builds)
    cached_edge_labels: dict[Any, EdgeGeometry] = {}
    for face_build in face_builds:
        for wire_build in face_build.wires:
            for alignment in wire_build.occurrences:
                for (geometry, _semantic_direction), (token, _direction) in zip(
                    wire_build.geometry.edges, alignment, strict=True
                ):
                    previous = cached_edge_labels.get(token)
                    if previous is not None and previous != geometry:
                        raise UnsupportedBodyGeometry("matching cached curve authority disagrees")
                    cached_edge_labels[cast(Any, token)] = geometry
    face_curve_indices: list[list[tuple[str, tuple[object, ...]]]] = []
    face_adaptors = tuple(BRepAdaptor_Surface(face.wrapped) for face in faces)
    for face, face_adaptor, face_build in zip(faces, face_adaptors, face_builds, strict=True):
        wires: list[tuple[str, tuple[object, ...]]] = []

        def vertex_index(vertex: Any) -> int:
            if vertex.IsNull():
                raise UnsupportedBodyGeometry("matching pcurve lost a topology vertex")
            found = next(
                (
                    existing_index
                    for existing_index, existing in enumerate(raw_vertices)
                    if existing.IsSame(vertex)
                ),
                None,
            )
            if found is None:
                raise UnsupportedBodyGeometry(
                    "matching pcurve vertex is outside its global curve roster"
                )
            return found

        def register(
            edge: Any,
            owner: Any = face,
            surface: BRepAdaptor_Surface = face_adaptor,
            *,
            validate_pcurve: bool = True,
        ) -> int:
            cached_label = cached_edge_labels.get(edge)
            if cached_label is None:
                raise UnsupportedBodyGeometry("matching cached curve authority is incomplete")
            curve_adaptor = curve_adaptors.get(edge)
            if curve_adaptor is None:
                curve_adaptor = BRepAdaptor_Curve(edge.wrapped)
                curve_adaptors[edge] = curve_adaptor
            if validate_pcurve:
                budget.charge()
                _validate_matching_pcurve(
                    edge,
                    owner,
                    curve_adaptor,
                    surface,
                    quantum,
                    cached_label.kind,
                    cached_label.full,
                )
            index = raw_curve_indices.get(edge)
            if index is not None:
                return index
            index = len(raw_curves)
            raw_curve_indices[edge] = index
            raw_curves.append(edge)
            curve_examples.append(edge)
            vertex_indices = []
            topology_vertices = (
                ()
                if cached_label.full
                else (
                    TopExp.FirstVertex_s(edge.wrapped, False),
                    TopExp.LastVertex_s(edge.wrapped, False),
                )
            )
            for vertex in topology_vertices:
                if vertex.IsNull():
                    continue
                vertex_index = next(
                    (
                        existing_index
                        for existing_index, existing in enumerate(raw_vertices)
                        if existing.IsSame(vertex)
                    ),
                    None,
                )
                if vertex_index is None:
                    vertex_index = len(raw_vertices)
                    raw_vertices.append(vertex)
                vertex_indices.append(vertex_index)
            raw_curve_vertices[edge] = tuple(vertex_indices)
            return index

        if face_build.geometry.kind == "PLANE":
            for wire_build in face_build.wires:
                if not wire_build.occurrences:
                    raise UnsupportedBodyGeometry("matching cached wire authority is empty")
                token_sets = tuple(
                    (
                        len(alignment),
                        frozenset(cast(Any, token) for token, _direction in alignment),
                    )
                    for alignment in wire_build.occurrences
                )
                if any(tokens != token_sets[0] for tokens in token_sets[1:]):
                    raise UnsupportedBodyGeometry("matching cached wire token roster disagrees")
                indices = tuple(
                    register(cast(Any, token)) for token, _direction in wire_build.occurrences[0]
                )
                wires.append((wire_build.geometry.role, indices))
            face_curve_indices.append(wires)
            continue
        outer = face.outer_wire()
        gauge = _cylinder_gauge(face_adaptor, face_build.geometry, centre)
        raw_wire_rosters: list[tuple[str, tuple[object, ...]]] = []
        for wire in face.wires():
            occurrence_groups: list[tuple[Any, list[Any]]] = []
            for edge in wire.edges():
                group = next(
                    (
                        occurrences
                        for token, occurrences in occurrence_groups
                        if _same_shape(token, edge)
                    ),
                    None,
                )
                if group is None:
                    group = []
                    occurrence_groups.append((edge, group))
                group.append(edge)
            if not occurrence_groups:
                raise UnsupportedBodyGeometry("matching cylinder wire is empty")
            group_assignments = []
            for edge, edge_occurrences in occurrence_groups:
                curve_index = register(edge, validate_pcurve=False)
                curve_label = cached_edge_labels.get(edge)
                if curve_label is None:
                    raise UnsupportedBodyGeometry("matching cylinder curve authority is incomplete")
                variants = _cylinder_pcurve_variants(
                    edge,
                    face,
                    face_adaptor,
                    centre,
                    quantum,
                    gauge,
                    curve_label.kind,
                    curve_label.full,
                    budget,
                )
                if len(variants) != len(edge_occurrences):
                    raise UnsupportedBodyGeometry(
                        "matching cylinder pcurve roster cardinality changed"
                    )
                if curve_label.full:
                    start_vertex = end_vertex = None
                else:
                    start_vertex = vertex_index(TopExp.FirstVertex_s(edge.wrapped, False))
                    end_vertex = vertex_index(TopExp.LastVertex_s(edge.wrapped, False))
                choices = []
                for assignment in permutations(variants):
                    budget.charge()
                    choices.append(
                        tuple(
                            sorted(
                                (
                                    _CylinderPcurveOccurrence(
                                        curve_index,
                                        start_vertex,
                                        end_vertex,
                                        start_parameter,
                                        end_parameter,
                                    )
                                    for start_parameter, end_parameter in assignment
                                ),
                                key=repr,
                            )
                        )
                    )
                group_assignments.append(tuple(sorted(set(choices), key=repr)))
            assignments = []
            for assignment_groups in product(*group_assignments):
                budget.charge()
                assignments.append(
                    tuple(
                        occurrence
                        for assignment_group in assignment_groups
                        for occurrence in assignment_group
                    )
                )
            role = "outer" if wire == outer else "inner"
            raw_wire_rosters.append((role, tuple(sorted(set(assignments), key=repr))))
        # Wire enumeration is only a complete occurrence roster.  Its order is erased
        # immediately; material orientation and cyclic order are reconstructed below.
        wires.extend(sorted(raw_wire_rosters, key=repr))
        face_curve_indices.append(wires)

    vertex_labels = tuple(
        _relative_point(
            tuple(
                value - origin
                for value, origin in zip(BRep_Tool.Pnt_s(vertex).Coord(), centre, strict=True)
            ),
            quantum,
            name="matching vertex",
        )
        for vertex in raw_vertices
    )
    matching_curves: list[MatchingCurve] = []
    for edge in curve_examples:
        edge_geometry = cached_edge_labels.get(edge)
        if edge_geometry is None:
            raise UnsupportedBodyGeometry("matching cached curve authority is incomplete")
        if edge_geometry.kind not in {"LINE", "CIRCLE"}:
            raise UnsupportedBodyGeometry("matching curve grammar is unsupported")
        if edge_geometry.full:
            ordered = None
        else:
            endpoints = raw_curve_vertices[edge]
            if len(endpoints) != 2:
                raise UnsupportedBodyGeometry("matching curve does not have two vertices")
            ordered = (
                endpoints
                if vertex_labels[endpoints[0]] == edge_geometry.start
                else (endpoints[1], endpoints[0])
            )
            if (
                vertex_labels[ordered[0]] != edge_geometry.start
                or vertex_labels[ordered[1]] != edge_geometry.end
            ):
                raise UnsupportedBodyGeometry("matching curve endpoints disagree with its geometry")
        matching_curves.append(
            MatchingCurve(
                edge_geometry.kind,
                ordered,
                edge_geometry.length,
                edge_geometry.centre,
                edge_geometry.axis,
                edge_geometry.radius,
                2.0 * math.pi if edge_geometry.full else edge_geometry.sweep,
                edge_geometry.full,
            )
        )
    curves = tuple(matching_curves)
    matching_faces = []
    incidence: dict[int, list[tuple[int, int, int]]] = {}
    for face_index, (build, wire_roster) in enumerate(
        zip(face_builds, face_curve_indices, strict=True)
    ):
        matching_wires = []
        for wire_index, (role, wire_authority) in enumerate(wire_roster):
            if build.geometry.kind == "PLANE":
                curve_indices = cast(tuple[int, ...], wire_authority)
                matching = _planar_cycle(
                    curve_indices,
                    curves,
                    role,
                    build.geometry,
                    quantum,
                    vertex_labels,
                    budget,
                )
            elif build.geometry.kind == "CYLINDER":
                matching = _cylinder_cycle(
                    cast(
                        tuple[tuple[_CylinderPcurveOccurrence, ...], ...],
                        wire_authority,
                    ),
                    curves,
                    role,
                    build.geometry,
                    quantum,
                    budget,
                )
            else:
                raise UnsupportedBodyGeometry("matching boundary surface is unsupported")
            matching_wires.append(matching)
            for occurrence, half_edge in enumerate(matching.cycle):
                incidence.setdefault(half_edge.curve, []).append(
                    (face_index, wire_index, occurrence)
                )
        face_geometry = build.geometry
        matching_faces.append(
            MatchingFace(
                face_geometry.kind,
                face_geometry.parameters,
                face_geometry.area,
                face_geometry.centroid,
                face_geometry.material_side,
                tuple(matching_wires),
            )
        )
    if set(incidence) != set(range(len(curves))) or any(
        len(occurrences) != 2 for occurrences in incidence.values()
    ):
        raise UnsupportedBodyGeometry("matching curve incidence is not a closed-shell pair")
    return _matching_graph_canonical(vertex_labels, curves, tuple(matching_faces), budget)
