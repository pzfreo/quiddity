# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Pure schema-three facts for bounded geometric partition correspondence."""

from __future__ import annotations

import math
from collections.abc import Callable
from dataclasses import dataclass
from typing import cast

from quiddity._body_geometry import (
    DIRECTION_TOL,
    DescriptorQuantization,
    FaceGeometry,
    MatchingBoundaryGraph,
    MatchingFace,
)

Vector3 = tuple[float, float, float]


@dataclass(frozen=True, slots=True)
class _PrismCurve:
    kind: str
    length: float
    full: bool
    start: Vector3 | None
    end: Vector3 | None
    centre: Vector3 | None
    axis: Vector3 | None
    radius: float | None
    sweep: float | None
    direction: int
    start_parameter: tuple[float, float] | None
    end_parameter: tuple[float, float] | None


@dataclass(frozen=True, slots=True)
class _PrismCap:
    face: MatchingFace
    face_position: int
    axial_position: float
    section_curves: tuple[_PrismCurve, ...]
    side_faces: tuple[int, ...]
    theta_winding: int


@dataclass(frozen=True, slots=True)
class _PrismFact:
    axis: Vector3
    interval: tuple[float, float]
    low_cap: _PrismCap
    high_cap: _PrismCap
    section_signature: object
    repeat_count: int
    edge_count: int
    section_points: tuple[Vector3, ...]
    volume: float
    centre_of_mass: Vector3
    quantization: DescriptorQuantization


def _dot(left: Vector3, right: Vector3) -> float:
    return sum(a * b for a, b in zip(left, right, strict=True))


def _parallel(left: Vector3, right: Vector3) -> bool:
    return 1.0 - abs(_dot(left, right)) <= 4.0 * DIRECTION_TOL


def _axis_vector(axis: str) -> Vector3:
    at = "xyz".index(axis)
    return tuple(1.0 if index == at else 0.0 for index in range(3))  # type: ignore[return-value]


def _curve_roster(
    graph: MatchingBoundaryGraph, face: MatchingFace
) -> tuple[_PrismCurve, ...] | None:
    if len(face.wires) != 1 or face.wires[0].role != "outer" or not face.wires[0].cycle:
        return None
    values = []
    for half_edge in face.wires[0].cycle:
        curve = graph.curves[half_edge.curve]
        values.append(
            _PrismCurve(
                curve.kind,
                curve.length,
                curve.full,
                None
                if half_edge.start is None or half_edge.start.vertex is None
                else graph.vertices[half_edge.start.vertex],
                None
                if half_edge.end is None or half_edge.end.vertex is None
                else graph.vertices[half_edge.end.vertex],
                curve.centre,
                curve.axis,
                curve.radius,
                curve.sweep,
                half_edge.direction,
                None if half_edge.start is None else half_edge.start.parameter,
                None if half_edge.end is None else half_edge.end.parameter,
            )
        )
    return tuple(values)


def _plane_basis(normal: Vector3) -> tuple[Vector3, Vector3]:
    axes: tuple[Vector3, ...] = (
        (1.0, 0.0, 0.0),
        (0.0, 1.0, 0.0),
        (0.0, 0.0, 1.0),
    )
    reference = min(axes, key=lambda value: abs(_dot(value, normal)))
    projection = tuple(
        value - _dot(reference, normal) * component
        for value, component in zip(reference, normal, strict=True)
    )
    length = math.sqrt(sum(value * value for value in projection))
    if length <= DIRECTION_TOL:
        raise ValueError("prism plane basis is degenerate")
    u = cast(Vector3, tuple(value / length for value in projection))
    v = (
        normal[1] * u[2] - normal[2] * u[1],
        normal[2] * u[0] - normal[0] * u[2],
        normal[0] * u[1] - normal[1] * u[0],
    )
    return u, v


def _plane_curve_parameters_match(curve: _PrismCurve, face: MatchingFace, metric: float) -> bool:
    if curve.start is None or curve.end is None:
        return curve.start_parameter is None and curve.end_parameter is None
    if curve.start_parameter is None or curve.end_parameter is None:
        return False
    normal = cast(Vector3, face.parameters[:3])
    u, v = _plane_basis(normal)
    origin = cast(Vector3, tuple(face.parameters[3] * value for value in normal))
    for point, parameter in (
        (curve.start, curve.start_parameter),
        (curve.end, curve.end_parameter),
    ):
        delta = tuple(value - offset for value, offset in zip(point, origin, strict=True))
        expected = (_dot(cast(Vector3, delta), u), _dot(cast(Vector3, delta), v))
        if math.dist(expected, parameter) > 4.0 * metric:
            return False
    return True


def _translated_cap_curves_match(
    low: tuple[_PrismCurve, ...],
    high: tuple[_PrismCurve, ...],
    axis: Vector3,
    metric: float,
    charged: Callable[[], None],
) -> bool:
    """Prove two material-oriented cap cycles differ only along the extrusion axis."""

    def transverse_point(left: Vector3 | None, right: Vector3 | None) -> bool:
        if left is None or right is None:
            return left is right
        delta = tuple(b - a for a, b in zip(left, right, strict=True))
        along = _dot(cast(Vector3, delta), axis)
        return (
            sum(
                (value - along * direction) ** 2
                for value, direction in zip(delta, axis, strict=True)
            )
            <= metric**2
        )

    def curve_matches(left: _PrismCurve, right: _PrismCurve, presentation: int) -> bool:
        expected_start, expected_end = (
            (left.start, left.end) if presentation == 1 else (left.end, left.start)
        )
        expected_start_parameter, expected_end_parameter = (
            (left.start_parameter, left.end_parameter)
            if presentation == 1
            else (left.end_parameter, left.start_parameter)
        )
        return (
            left.kind == right.kind
            and left.full == right.full
            and abs(left.length - right.length) <= metric
            and transverse_point(expected_start, right.start)
            and transverse_point(expected_end, right.end)
            and transverse_point(left.centre, right.centre)
            and left.axis == right.axis
            and (left.radius is None) == (right.radius is None)
            and (
                left.radius is None
                or right.radius is not None
                and abs(left.radius - right.radius) <= metric
            )
            and (left.sweep is None) == (right.sweep is None)
            and (
                left.sweep is None
                or right.sweep is not None
                and abs(left.sweep - right.sweep) <= 4.0 * DIRECTION_TOL
            )
            and right.direction == left.direction * presentation
            and expected_start_parameter == right.start_parameter
            and expected_end_parameter == right.end_parameter
        )

    for presentation in (1, -1):
        source = low if presentation == 1 else tuple(reversed(low))
        for shift in range(len(source)):
            charged()
            aligned = source[shift:] + source[:shift]
            if all(
                curve_matches(left, right, presentation)
                for left, right in zip(aligned, high, strict=True)
            ):
                return True
    return False


def _polar_signature(
    points: tuple[Vector3, ...], centre: Vector3, axis: Vector3
) -> tuple[tuple[float, float], ...]:
    transverse = tuple(at for at, value in enumerate(axis) if value == 0.0)
    if len(transverse) != 2:
        raise ValueError("prism axis is not principal")

    def one_direction(candidate: tuple[Vector3, ...]) -> tuple[tuple[float, float], ...]:
        polar = []
        for point in candidate:
            left = point[transverse[0]] - centre[transverse[0]]
            right = point[transverse[1]] - centre[transverse[1]]
            polar.append((math.hypot(left, right), math.atan2(right, left)))
        unwrapped = [polar[0][1]]
        for _radius, angle in polar[1:]:
            previous = unwrapped[-1]
            while angle - previous > math.pi:
                angle -= 2.0 * math.pi
            while angle - previous < -math.pi:
                angle += 2.0 * math.pi
            unwrapped.append(angle)
        phase = unwrapped[0]
        relative = tuple(
            (round(radius, 6) or 0.0, round(angle - phase, 6) or 0.0)
            for (radius, _raw), angle in zip(polar, unwrapped, strict=True)
        )
        reflected = tuple((radius, -angle) for radius, angle in relative)
        return min(relative, reflected)

    return min(one_direction(points), one_direction(tuple(reversed(points))))


def _sample_curve(curve: _PrismCurve) -> tuple[Vector3, ...] | None:
    if curve.start is None or curve.end is None:
        return None
    if curve.kind == "LINE":
        return tuple(
            cast(
                Vector3,
                tuple(
                    left + fraction * (right - left)
                    for left, right in zip(curve.start, curve.end, strict=True)
                ),
            )
            for fraction in (index / 8.0 for index in range(9))
        )
    if (
        curve.kind != "CIRCLE"
        or curve.centre is None
        or curve.axis is None
        or curve.sweep is None
        or curve.full
    ):
        return None
    radial = tuple(value - origin for value, origin in zip(curve.start, curve.centre, strict=True))
    samples = []
    for index in range(9):
        angle = curve.sweep * index / 8.0
        cosine = math.cos(angle)
        sine = math.sin(angle)
        cross = (
            curve.axis[1] * radial[2] - curve.axis[2] * radial[1],
            curve.axis[2] * radial[0] - curve.axis[0] * radial[2],
            curve.axis[0] * radial[1] - curve.axis[1] * radial[0],
        )
        samples.append(
            cast(
                Vector3,
                tuple(
                    origin + cosine * value + sine * cross_value
                    for origin, value, cross_value in zip(curve.centre, radial, cross, strict=True)
                ),
            )
        )
    return tuple(samples)


def prism_fact(
    graph: MatchingBoundaryGraph,
    *,
    axis_name: str,
    span: tuple[float, float],
    profile_centre: Vector3,
    section_signature: object,
    defining: tuple[FaceGeometry, FaceGeometry],
    repeat_count: int,
    edge_count: int,
    volume: float,
    centre_of_mass: Vector3,
    quantization: DescriptorQuantization,
    charge: Callable[[], None] | None = None,
) -> _PrismFact | None:
    """Return one exact bounded extrusion fact, or ``None`` when ineligible."""

    def charged() -> None:
        if charge is not None:
            charge()

    axis = _axis_vector(axis_name)
    cap_candidates = []
    for position, face in enumerate(graph.faces):
        charged()
        if (
            face.kind == "PLANE"
            and len(face.parameters) == 4
            and _parallel(face.parameters[:3], axis)
        ):
            cap_candidates.append(position)
    cap_positions = tuple(cap_candidates)
    if len(cap_positions) != 2:
        return None
    ordered_caps = tuple(
        sorted(cap_positions, key=lambda position: _dot(graph.faces[position].centroid, axis))
    )
    low_position, high_position = ordered_caps
    low_face, high_face = graph.faces[low_position], graph.faces[high_position]
    if (
        low_face.material_side == high_face.material_side
        or low_face.wires[0].theta_winding != 0
        or high_face.wires[0].theta_winding != 0
    ):
        return None
    defining_rows = tuple(
        tuple(
            at
            for at, candidate in enumerate(defining)
            if candidate.kind == face.kind
            and candidate.parameters == face.parameters
            and candidate.area == face.area
            and candidate.centroid == face.centroid
            and candidate.material_side == face.material_side
        )
        for face in (low_face, high_face)
    )
    if any(len(row) != 1 for row in defining_rows) or defining_rows[0] == defining_rows[1]:
        return None
    low_curves = _curve_roster(graph, low_face)
    high_curves = _curve_roster(graph, high_face)
    if low_curves is None or high_curves is None or len(low_curves) != len(high_curves):
        return None
    parameter_metric = 2.0 * quantization.metric_quantum
    if any(
        not _plane_curve_parameters_match(curve, face, parameter_metric)
        for face, curves in ((low_face, low_curves), (high_face, high_curves))
        for curve in curves
    ):
        return None
    if (
        type(section_signature) is not tuple
        or repeat_count <= 0
        or edge_count != len(low_curves)
        or repeat_count * len(section_signature) != edge_count
    ):
        return None
    signature_roster: list[tuple[str, float, tuple[tuple[float, float], ...]]] = []
    relative_profile_centre = cast(
        Vector3,
        tuple(value - origin for value, origin in zip(profile_centre, centre_of_mass, strict=True)),
    )
    for value in section_signature:
        charged()
        if (
            type(value) is not tuple
            or len(value) != 3
            or value[0] not in {"LINE", "CIRCLE"}
            or type(value[1]) is not float
            or not math.isfinite(value[1])
            or type(value[2]) is not tuple
        ):
            return None
        signature_roster.extend(
            cast(tuple[str, float, tuple[tuple[float, float], ...]], value)
            for _ in range(repeat_count)
        )
    cap_roster: list[tuple[str, float, tuple[tuple[float, float], ...]]] = []
    for cap_curve in low_curves:
        charged()
        samples = _sample_curve(cap_curve)
        if samples is None:
            return None
        cap_roster.append(
            (
                cap_curve.kind,
                round(cap_curve.length, 6),
                _polar_signature(samples, relative_profile_centre, axis),
            )
        )
    expected_signature = sorted(signature_roster)
    derived_signature = sorted(cap_roster)
    # The accepted signature and schema-three graph are independently snapped
    # to six decimals, so their closed comparison carries both half-quanta.
    signature_metric = max(2.0 * quantization.metric_quantum, 2.0e-6)
    signature_angle = max(4.0 * DIRECTION_TOL, 2.0e-6)
    if len(expected_signature) != len(derived_signature):
        return None
    for expected, derived in zip(expected_signature, derived_signature, strict=True):
        charged()
        if (
            type(expected) is not tuple
            or len(expected) != 3
            or expected[0] != derived[0]
            or abs(expected[1] - derived[1]) > signature_metric
            or type(expected[2]) is not tuple
            or len(expected[2]) != len(derived[2])
            or any(
                abs(expected_point[0] - derived_point[0]) > signature_metric
                or abs(expected_point[1] - derived_point[1]) > signature_angle
                for expected_point, derived_point in zip(expected[2], derived[2], strict=True)
            )
        ):
            return None
    if not _translated_cap_curves_match(
        low_curves,
        high_curves,
        axis,
        2.0 * quantization.metric_quantum,
        charged,
    ):
        return None
    if any(face.kind not in {"PLANE", "CYLINDER"} for face in graph.faces):
        return None
    if any(any(wire.role != "outer" for wire in face.wires) for face in graph.faces):
        return None

    incidence = dict(graph.incidence)

    def side_for(cap_position: int, curve_position: int) -> int | None:
        charged()
        occurrences = incidence.get(curve_position, ())
        if len(occurrences) != 2 or len(set(occurrences)) != 2:
            return None
        owners = {face_position for face_position, _wire_position, _edge_position in occurrences}
        if cap_position not in owners or len(owners) != 2:
            return None
        (side,) = owners - {cap_position}
        return side

    low_curve_positions = tuple(item.curve for item in low_face.wires[0].cycle)
    high_curve_positions = tuple(item.curve for item in high_face.wires[0].cycle)
    low_sides = tuple(side_for(low_position, curve) for curve in low_curve_positions)
    high_sides = tuple(side_for(high_position, curve) for curve in high_curve_positions)
    if any(side is None for side in (*low_sides, *high_sides)):
        return None
    closed_low_sides = cast(tuple[int, ...], low_sides)
    closed_high_sides = cast(tuple[int, ...], high_sides)
    low_side_set = set(closed_low_sides)
    high_side_set = set(closed_high_sides)
    side_faces = set(range(len(graph.faces))) - {low_position, high_position}
    if low_side_set != side_faces or high_side_set != side_faces:
        return None
    if len(low_sides) != len(side_faces) or len(high_sides) != len(side_faces):
        return None

    low_by_side = dict(zip(low_sides, low_curves, strict=True))
    high_by_side = dict(zip(high_sides, high_curves, strict=True))
    metric = 2.0 * quantization.metric_quantum
    lo = _dot(low_face.centroid, axis)
    hi = _dot(high_face.centroid, axis)
    joining_neighbours: dict[int, set[int]] = {side: set() for side in side_faces}
    for side_position in side_faces:
        charged()
        low_curve = low_by_side[side_position]
        high_curve = high_by_side[side_position]
        side = graph.faces[side_position]
        if (
            low_curve.kind != high_curve.kind
            or low_curve.full != high_curve.full
            or abs(low_curve.length - high_curve.length) > metric
            or (low_curve.radius is None) != (high_curve.radius is None)
            or (
                low_curve.radius is not None
                and high_curve.radius is not None
                and abs(low_curve.radius - high_curve.radius) > metric
            )
            or (low_curve.sweep is None) != (high_curve.sweep is None)
            or (
                low_curve.sweep is not None
                and high_curve.sweep is not None
                and abs(abs(low_curve.sweep) - abs(high_curve.sweep)) > 4.0 * DIRECTION_TOL
            )
        ):
            return None
        if low_curve.kind == "LINE" and side.kind != "PLANE":
            return None
        if low_curve.kind == "CIRCLE" and side.kind != "CYLINDER":
            return None
        if (
            side.kind == "PLANE"
            and abs(_dot(cast(Vector3, side.parameters[:3]), axis)) > 4.0 * DIRECTION_TOL
        ):
            return None
        if side.kind == "CYLINDER" and (
            not _parallel(cast(Vector3, side.parameters[:3]), axis)
            or low_curve.radius is None
            or len(side.parameters) != 7
            or abs(side.parameters[6] - low_curve.radius) > metric
        ):
            return None
        if len(side.wires) != 1 or side.wires[0].role != "outer":
            return None
        side_curve_positions = tuple(item.curve for item in side.wires[0].cycle)
        if low_curve_positions[low_sides.index(side_position)] not in side_curve_positions:
            return None
        if high_curve_positions[high_sides.index(side_position)] not in side_curve_positions:
            return None
        joining = tuple(
            graph.curves[position]
            for position in side_curve_positions
            if position
            not in {
                low_curve_positions[low_sides.index(side_position)],
                high_curve_positions[high_sides.index(side_position)],
            }
        )
        expected_joining = 0 if low_curve.full else 2
        if len(joining) != expected_joining or any(
            curve.kind != "LINE" or curve.vertices is None for curve in joining
        ):
            return None
        joining_positions = tuple(
            position
            for position in side_curve_positions
            if position
            not in {
                low_curve_positions[low_sides.index(side_position)],
                high_curve_positions[high_sides.index(side_position)],
            }
        )
        for curve_position, curve in zip(joining_positions, joining, strict=True):
            charged()
            owners = incidence.get(curve_position, ())
            owner_faces = {face_position for face_position, _wire, _edge in owners}
            if (
                len(owners) != 2
                or len(set(owners)) != 2
                or len(owner_faces) != 2
                or side_position not in owner_faces
                or owner_faces & {low_position, high_position}
            ):
                return None
            (other_side,) = owner_faces - {side_position}
            joining_neighbours[side_position].add(other_side)
            assert curve.vertices is not None
            start, end = (graph.vertices[position] for position in curve.vertices)
            delta = cast(
                Vector3,
                tuple(right - left for left, right in zip(start, end, strict=True)),
            )
            along = _dot(delta, axis)
            transverse = tuple(
                value - along * direction for value, direction in zip(delta, axis, strict=True)
            )
            if (
                sum(value * value for value in transverse) > metric**2
                or abs(abs(along) - (hi - lo)) > metric
            ):
                return None

    side_cycle = closed_low_sides
    for index, side_position in enumerate(side_cycle):
        charged()
        if len(side_cycle) == 1:
            expected_neighbours: set[int] = set()
        else:
            expected_neighbours = {
                side_cycle[(index - 1) % len(side_cycle)],
                side_cycle[(index + 1) % len(side_cycle)],
            }
        if joining_neighbours[side_position] != expected_neighbours:
            return None

    if not math.isfinite(lo) or not math.isfinite(hi) or hi <= lo:
        return None
    record_lo, record_hi = span
    bound = 2.0 * quantization.metric_quantum
    placement = _dot(centre_of_mass, axis)
    if abs((placement + lo) - record_lo) > bound or abs((placement + hi) - record_hi) > bound:
        return None
    if (
        abs(_dot(profile_centre, axis) - (record_lo + record_hi) / 2.0) > bound
        or sum(
            (relative_profile_centre[at] - low_face.centroid[at]) ** 2
            for at, value in enumerate(axis)
            if value == 0.0
        )
        > bound**2
    ):
        return None

    return _PrismFact(
        axis,
        (record_lo, record_hi),
        _PrismCap(
            low_face,
            low_position,
            lo,
            low_curves,
            closed_low_sides,
            low_face.wires[0].theta_winding,
        ),
        _PrismCap(
            high_face,
            high_position,
            hi,
            high_curves,
            closed_high_sides,
            high_face.wires[0].theta_winding,
        ),
        section_signature,
        repeat_count,
        edge_count,
        tuple(
            graph.vertices[index]
            for index in sorted(
                {
                    int(vertex)
                    for half_edge in low_face.wires[0].cycle
                    for vertex in (
                        None if half_edge.start is None else half_edge.start.vertex,
                        None if half_edge.end is None else half_edge.end.vertex,
                    )
                    if vertex is not None
                }
            )
        ),
        volume,
        centre_of_mass,
        quantization,
    )
