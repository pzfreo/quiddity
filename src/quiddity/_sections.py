# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Private canonical line/arc sections and run-local placement values.

This is a geometry-value leaf for epic 0004.  Nothing here is a public record, kernel value,
candidate, or persistent body identifier.  The deliberately small issuer gives adapters a
run-local provenance boundary without coupling this module to recognition orchestration.
"""

from __future__ import annotations

import math
from dataclasses import dataclass
from typing import TypeAlias

Vector2: TypeAlias = tuple[float, float]
Vector3: TypeAlias = tuple[float, float, float]

_EPS = 1e-9
_POSITION_TOL = 8e-4
_OCCURRENCE_TOL = 2e-3
_BULGE_DIGITS = 12


def _finite(values: tuple[float, ...]) -> bool:
    return all(math.isfinite(value) for value in values)


def _dot(a: Vector3, b: Vector3) -> float:
    return sum(x * y for x, y in zip(a, b, strict=True))


def _cross(a: Vector3, b: Vector3) -> Vector3:
    return (
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    )


def _unit(value: Vector3) -> Vector3:
    length = math.sqrt(_dot(value, value))
    if not math.isfinite(length) or length <= _EPS:
        raise ValueError("frame direction must be finite and nonzero")
    return tuple(component / length for component in value)  # type: ignore[return-value]


@dataclass(frozen=True, slots=True)
class LocalFrame:
    """A private right-handed placement frame."""

    origin: Vector3
    run: Vector3
    u: Vector3
    v: Vector3

    def __post_init__(self) -> None:
        if not _finite(self.origin + self.run + self.u + self.v):
            raise ValueError("frame values must be finite")
        for direction in (self.run, self.u, self.v):
            if not math.isclose(_dot(direction, direction), 1.0, abs_tol=_EPS):
                raise ValueError("frame directions must be unit length")
        if any(
            abs(_dot(a, b)) > _EPS
            for a, b in ((self.run, self.u), (self.run, self.v), (self.u, self.v))
        ):
            raise ValueError("frame directions must be orthogonal")
        if any(abs(a - b) > _EPS for a, b in zip(_cross(self.run, self.u), self.v, strict=True)):
            raise ValueError("frame must be right handed: run cross u equals v")

    @classmethod
    def principal(cls, axis: str, centroid: Vector3) -> LocalFrame:
        """Place the package's canonical principal-axis basis through *centroid*."""

        bases: dict[str, tuple[Vector3, Vector3, Vector3]] = {
            "x": ((1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0)),
            "y": ((0.0, 1.0, 0.0), (0.0, 0.0, 1.0), (1.0, 0.0, 0.0)),
            "z": ((0.0, 0.0, 1.0), (1.0, 0.0, 0.0), (0.0, 1.0, 0.0)),
        }
        try:
            run, u, v = bases[axis]
        except KeyError as exc:
            raise ValueError("axis must be 'x', 'y', or 'z'") from exc
        if not _finite(centroid):
            raise ValueError("centroid must be finite")
        along = _dot(centroid, run)
        origin = tuple(centroid[i] - along * run[i] for i in range(3))
        return cls(origin=origin, run=run, u=u, v=v)  # type: ignore[arg-type]

    @classmethod
    def canonical(cls, run: Vector3, centroid: Vector3) -> LocalFrame:
        """Construct the deterministic free-axis frame described by epic 0004."""

        direction = _unit(run)
        serialized_direction = tuple(_round_clean(value, 6) for value in direction)
        components = tuple(abs(value) for value in serialized_direction)
        peak = max(components)
        dominant = next(index for index in (2, 1, 0) if components[index] == peak)
        if serialized_direction[dominant] < 0:
            direction = tuple(-value for value in direction)  # type: ignore[assignment]
        seeds: tuple[Vector3, ...] = (
            (0.0, 1.0, 0.0),  # x -> y
            (0.0, 0.0, 1.0),  # y -> z
            (1.0, 0.0, 0.0),  # z -> x
        )
        seed = seeds[dominant]
        projection = tuple(seed[i] - _dot(seed, direction) * direction[i] for i in range(3))
        u = _unit(projection)  # type: ignore[arg-type]
        v = _cross(direction, u)
        if not _finite(centroid):
            raise ValueError("centroid must be finite")
        along = _dot(centroid, direction)
        origin = tuple(centroid[i] - along * direction[i] for i in range(3))
        return cls(origin=origin, run=direction, u=u, v=v)  # type: ignore[arg-type]


@dataclass(frozen=True, slots=True)
class SectionVertex:
    """A 2-D vertex whose bulge describes the edge to the next vertex."""

    point: Vector2
    bulge: float = 0.0

    def __post_init__(self) -> None:
        if not _finite(self.point + (self.bulge,)):
            raise ValueError("section vertex must be finite")
        object.__setattr__(
            self,
            "point",
            tuple(0.0 if coordinate == 0.0 else coordinate for coordinate in self.point),
        )
        if self.bulge == 0.0:
            object.__setattr__(self, "bulge", 0.0)


@dataclass(frozen=True, slots=True)
class _Arc:
    centre: Vector2
    radius: float
    start: float
    sweep: float


def _arc(a: SectionVertex, b: SectionVertex) -> _Arc | None:
    bulge = a.bulge
    if bulge == 0.0:
        return None
    dx, dy = b.point[0] - a.point[0], b.point[1] - a.point[1]
    chord = math.hypot(dx, dy)
    if chord <= _EPS:
        raise ValueError("arc endpoints must be distinct")
    offset = chord * (1.0 - bulge * bulge) / (4.0 * bulge)
    centre = (
        0.5 * (a.point[0] + b.point[0]) - dy * offset / chord,
        0.5 * (a.point[1] + b.point[1]) + dx * offset / chord,
    )
    radius = chord * (1.0 + bulge * bulge) / (4.0 * abs(bulge))
    return _Arc(
        centre=centre,
        radius=radius,
        start=math.atan2(a.point[1] - centre[1], a.point[0] - centre[0]),
        sweep=4.0 * math.atan(bulge),
    )


def _line_moments(a: Vector2, b: Vector2) -> tuple[float, float, float]:
    cross = a[0] * b[1] - b[0] * a[1]
    # Use the same Green-theorem fields as ``_arc_moments``.  The familiar polygon
    # centroid formula is equivalent only after summing a wholly linear closed loop;
    # mixing its per-edge contributions with curved-edge integrals is not translation
    # invariant.
    dx, dy = b[0] - a[0], b[1] - a[1]
    mx = dy * (a[0] * a[0] + a[0] * b[0] + b[0] * b[0]) / 6.0
    my = -dx * (a[1] * a[1] + a[1] * b[1] + b[1] * b[1]) / 6.0
    return (0.5 * cross, mx, my)


def _arc_moments(arc: _Arc) -> tuple[float, float, float]:
    cx, cy = arc.centre
    radius, start, end = arc.radius, arc.start, arc.start + arc.sweep
    dsin = math.sin(end) - math.sin(start)
    dcos = math.cos(end) - math.cos(start)
    dsin2 = math.sin(2 * end) - math.sin(2 * start)
    area = 0.5 * (radius * cx * dsin - radius * cy * dcos + radius * radius * arc.sweep)

    int_cos2 = 0.5 * arc.sweep + 0.25 * dsin2
    int_cos3 = (math.sin(end) - math.sin(end) ** 3 / 3) - (
        math.sin(start) - math.sin(start) ** 3 / 3
    )
    x2dy = radius * (cx * cx * dsin + 2 * cx * radius * int_cos2 + radius**2 * int_cos3)

    int_sin = -dcos
    int_sin2 = 0.5 * arc.sweep - 0.25 * dsin2
    int_sin3 = (-math.cos(end) + math.cos(end) ** 3 / 3) - (
        -math.cos(start) + math.cos(start) ** 3 / 3
    )
    y2dx = -radius * (cy * cy * int_sin + 2 * cy * radius * int_sin2 + radius**2 * int_sin3)
    return area, 0.5 * x2dy, -0.5 * y2dx


def _moments(vertices: tuple[SectionVertex, ...]) -> tuple[float, Vector2]:
    anchor = vertices[0].point
    local = tuple(
        SectionVertex((vertex.point[0] - anchor[0], vertex.point[1] - anchor[1]), vertex.bulge)
        for vertex in vertices
    )
    area = mx = my = 0.0
    for index, vertex in enumerate(local):
        following = local[(index + 1) % len(local)]
        contribution = (
            _line_moments(vertex.point, following.point)
            if vertex.bulge == 0.0
            else _arc_moments(_arc(vertex, following))  # type: ignore[arg-type]
        )
        area += contribution[0]
        mx += contribution[1]
        my += contribution[2]
    if not math.isfinite(area) or abs(area) <= _EPS:
        raise ValueError("section must enclose nonzero area")
    return area, (mx / area + anchor[0], my / area + anchor[1])


def _reverse(vertices: tuple[SectionVertex, ...]) -> tuple[SectionVertex, ...]:
    size = len(vertices)
    return tuple(
        SectionVertex(vertices[-index % size].point, -vertices[-index - 1].bulge)
        for index in range(size)
    )


def _serialized(vertex: SectionVertex) -> tuple[float, float, float]:
    bulge = round(vertex.bulge, _BULGE_DIGITS)
    if vertex.bulge != 0.0 and bulge == 0.0:
        raise ValueError("serialization would collapse a nonzero arc")
    return (
        _round_clean(vertex.point[0], 3),
        _round_clean(vertex.point[1], 3),
        0.0 if bulge == 0.0 else bulge,
    )


def _canonical_start(vertices: tuple[SectionVertex, ...]) -> tuple[SectionVertex, ...]:
    candidates = tuple(vertices[index:] + vertices[:index] for index in range(len(vertices)))
    return min(candidates, key=lambda candidate: tuple(_serialized(vertex) for vertex in candidate))


def _projection_bound(vertices: tuple[SectionVertex, ...]) -> float:
    serialized = tuple(_serialized(vertex) for vertex in vertices)
    projected = tuple(SectionVertex((value[0], value[1]), value[2]) for value in serialized)
    bound = 0.0
    for index, vertex in enumerate(vertices):
        bound = max(bound, math.dist(vertex.point, projected[index].point))
        if vertex.bulge != 0.0:
            original_arc = _arc(vertex, vertices[(index + 1) % len(vertices)])
            projected_arc = _arc(projected[index], projected[(index + 1) % len(vertices)])
            assert original_arc is not None and projected_arc is not None
            # At equal normalized sweep, the difference of two equal-sweep arcs
            # is C + R(t)D. Its maximum norm is at an endpoint or where R(t)D
            # aligns with C. Treat the tiny serialized sweep change separately
            # with the chord <= arc-length bound; no sampling or tolerance widening.
            centre_delta = complex(*original_arc.centre) - complex(*projected_arc.centre)
            radial_delta = original_arc.radius * complex(
                math.cos(original_arc.start), math.sin(original_arc.start)
            ) - projected_arc.radius * complex(
                math.cos(projected_arc.start), math.sin(projected_arc.start)
            )
            angles = [0.0, original_arc.sweep]
            if abs(centre_delta) > 0 and abs(radial_delta) > 0:
                aligned = math.atan2(centre_delta.imag, centre_delta.real) - math.atan2(
                    radial_delta.imag, radial_delta.real
                )
                for turn in (-2 * math.pi, 0.0, 2 * math.pi):
                    angle = aligned + turn
                    if min(0.0, original_arc.sweep) <= angle <= max(0.0, original_arc.sweep):
                        angles.append(angle)
            displacement_bound = max(
                abs(centre_delta + radial_delta * complex(math.cos(angle), math.sin(angle)))
                for angle in angles
            ) + projected_arc.radius * abs(original_arc.sweep - projected_arc.sweep)
            bound = max(bound, displacement_bound)
    return bound


def _validate_projection(vertices: tuple[SectionVertex, ...]) -> None:
    if _projection_bound(vertices) > _POSITION_TOL:
        raise ValueError("serialized section moves its boundary beyond local tolerance")


def _line_intersection(a: Vector2, b: Vector2, c: Vector2, d: Vector2) -> bool:
    def orient(p: Vector2, q: Vector2, r: Vector2) -> float:
        return (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])

    def on_segment(p: Vector2, q: Vector2, r: Vector2) -> bool:
        return (
            min(p[0], r[0]) - _EPS <= q[0] <= max(p[0], r[0]) + _EPS
            and min(p[1], r[1]) - _EPS <= q[1] <= max(p[1], r[1]) + _EPS
        )

    o1, o2, o3, o4 = orient(a, b, c), orient(a, b, d), orient(c, d, a), orient(c, d, b)
    if abs(o1) <= _EPS and on_segment(a, c, b):
        return True
    if abs(o2) <= _EPS and on_segment(a, d, b):
        return True
    if abs(o3) <= _EPS and on_segment(c, a, d):
        return True
    if abs(o4) <= _EPS and on_segment(c, b, d):
        return True
    return (o1 > 0) != (o2 > 0) and (o3 > 0) != (o4 > 0)


def _angle_on_arc(angle: float, arc: _Arc, *, interior: bool = False) -> bool:
    delta = (angle - arc.start) % (2 * math.pi)
    sweep = arc.sweep
    if sweep < 0:
        delta = (arc.start - angle) % (2 * math.pi)
        sweep = -sweep
    tolerance = _EPS if not interior else -_EPS
    return -tolerance <= delta <= sweep + tolerance


def _point_on_arc(point: Vector2, arc: _Arc) -> bool:
    if abs(math.hypot(point[0] - arc.centre[0], point[1] - arc.centre[1]) - arc.radius) > _EPS:
        return False
    return _angle_on_arc(math.atan2(point[1] - arc.centre[1], point[0] - arc.centre[0]), arc)


def _line_arc_points(a: Vector2, b: Vector2, arc: _Arc) -> tuple[Vector2, ...]:
    dx, dy = b[0] - a[0], b[1] - a[1]
    fx, fy = a[0] - arc.centre[0], a[1] - arc.centre[1]
    aa = dx * dx + dy * dy
    bb = 2 * (fx * dx + fy * dy)
    cc = fx * fx + fy * fy - arc.radius * arc.radius
    discriminant = bb * bb - 4 * aa * cc
    if discriminant < -_EPS:
        return ()
    roots = (
        ((-bb) / (2 * aa),)
        if abs(discriminant) <= _EPS
        else (
            (-bb - math.sqrt(discriminant)) / (2 * aa),
            (-bb + math.sqrt(discriminant)) / (2 * aa),
        )
    )
    return tuple(
        point
        for root in roots
        if -_EPS <= root <= 1 + _EPS
        and _point_on_arc(point := (a[0] + root * dx, a[1] + root * dy), arc)
    )


def _line_arc_intersection(a: Vector2, b: Vector2, arc: _Arc) -> bool:
    return bool(_line_arc_points(a, b, arc))


def _arc_arc_points(first: _Arc, second: _Arc) -> tuple[Vector2, ...] | None:
    dx, dy = second.centre[0] - first.centre[0], second.centre[1] - first.centre[1]
    distance = math.hypot(dx, dy)
    if distance <= _EPS:
        if abs(first.radius - second.radius) > _EPS:
            return ()
        return None
    if (
        distance > first.radius + second.radius + _EPS
        or distance < abs(first.radius - second.radius) - _EPS
    ):
        return ()
    along = (first.radius**2 - second.radius**2 + distance**2) / (2 * distance)
    height2 = first.radius**2 - along**2
    if height2 < -_EPS:
        return ()
    height = math.sqrt(max(0.0, height2))
    base = (first.centre[0] + along * dx / distance, first.centre[1] + along * dy / distance)
    points = (
        (base[0] - height * dy / distance, base[1] + height * dx / distance),
        (base[0] + height * dy / distance, base[1] - height * dx / distance),
    )
    return tuple(
        point for point in points if _point_on_arc(point, first) and _point_on_arc(point, second)
    )


def _arc_arc_intersection(first: _Arc, second: _Arc) -> bool:
    points = _arc_arc_points(first, second)
    if points is not None:
        return bool(points)
    endpoints = (
        first.start,
        first.start + first.sweep,
        second.start,
        second.start + second.sweep,
    )
    return any(
        _angle_on_arc(angle, second if index < 2 else first)
        for index, angle in enumerate(endpoints)
    )


def _adjacent_line_arc_crosses(shared: Vector2, end: Vector2, arc: _Arc) -> bool:
    """Test the second intersection, factoring out the known shared endpoint.

    A generic quadratic can split a tangent double root through cancellation.
    Adjacency supplies the exact first root; solving only the remaining linear
    factor preserves that topology without increasing the positional tolerance.
    """

    dx, dy = end[0] - shared[0], end[1] - shared[1]
    fx, fy = shared[0] - arc.centre[0], shared[1] - arc.centre[1]
    fraction = -2 * (fx * dx + fy * dy) / (dx * dx + dy * dy)
    if not -_EPS <= fraction <= 1 + _EPS:
        return False
    point = (shared[0] + fraction * dx, shared[1] + fraction * dy)
    return math.dist(point, shared) > _EPS and _point_on_arc(point, arc)


def _validate_adjacent(
    first_start: SectionVertex, shared: SectionVertex, second_end: SectionVertex
) -> None:
    first = _arc(first_start, shared)
    second = _arc(shared, second_end)
    if first is None and second is None:
        left = (
            shared.point[0] - first_start.point[0],
            shared.point[1] - first_start.point[1],
        )
        right = (
            second_end.point[0] - shared.point[0],
            second_end.point[1] - shared.point[1],
        )
        cross = left[0] * right[1] - left[1] * right[0]
        if abs(cross) <= _EPS and left[0] * right[0] + left[1] * right[1] <= 0:
            raise ValueError("adjacent section edges overlap or backtrack")
        return
    if first is None:
        if _adjacent_line_arc_crosses(shared.point, first_start.point, second):  # type: ignore[arg-type]
            raise ValueError("adjacent section edges meet away from their shared endpoint")
        return
    if second is None:
        if _adjacent_line_arc_crosses(shared.point, second_end.point, first):
            raise ValueError("adjacent section edges meet away from their shared endpoint")
        return
    arc_points = _arc_arc_points(first, second)
    if arc_points is None:
        first_end = first.start + first.sweep
        first_tangent = (
            math.copysign(1.0, first.sweep) * -math.sin(first_end),
            math.copysign(1.0, first.sweep) * math.cos(first_end),
        )
        second_tangent = (
            math.copysign(1.0, second.sweep) * -math.sin(second.start),
            math.copysign(1.0, second.sweep) * math.cos(second.start),
        )
        aligned = sum(a * b for a, b in zip(first_tangent, second_tangent, strict=True))
        if aligned < 1.0 - _EPS or abs(first.sweep) + abs(second.sweep) > 2 * math.pi + _EPS:
            raise ValueError("adjacent circular arcs overlap or backtrack")
    elif any(math.dist(point, shared.point) > _EPS for point in arc_points):
        raise ValueError("adjacent section edges meet away from their shared endpoint")


def _validate_simple(vertices: tuple[SectionVertex, ...], *, closed: bool = True) -> None:
    size = len(vertices)
    if size > 2:
        for index in range(size) if closed else range(1, size - 1):
            _validate_adjacent(vertices[index - 1], vertices[index], vertices[(index + 1) % size])
    edge_count = size if closed else size - 1
    for left in range(edge_count):
        a, b = vertices[left], vertices[(left + 1) % size]
        for right in range(left + 1, edge_count):
            if right == left + 1 or (closed and left == 0 and right == size - 1):
                continue
            c, d = vertices[right], vertices[(right + 1) % size]
            first, second = _arc(a, b), _arc(c, d)
            intersects = (
                _line_intersection(a.point, b.point, c.point, d.point)
                if first is None and second is None
                else _line_arc_intersection(a.point, b.point, second)  # type: ignore[arg-type]
                if first is None
                else _line_arc_intersection(c.point, d.point, first)
                if second is None
                else _arc_arc_intersection(first, second)
            )
            if intersects:
                raise ValueError("section boundary must be simple")


def validate_section_end_separation(
    vertices: tuple[SectionVertex, ...],
    span: float,
    gradient: Vector2,
    *,
    closed: bool,
) -> None:
    """Prove positive high-minus-low span on each physical line/arc segment.

    A linear function attains its extrema at vertices or where its gradient is
    parallel to an arc radius. No synthetic closing edge is needed for open chains.
    """
    points = [vertex.point for vertex in vertices]
    angle = math.atan2(gradient[1], gradient[0])
    for index in range(len(vertices) if closed else len(vertices) - 1):
        arc = _arc(vertices[index], vertices[(index + 1) % len(vertices)])
        if arc is not None and gradient != (0.0, 0.0):
            for extremum in (angle, angle + math.pi):
                if _angle_on_arc(extremum, arc):
                    points.append(
                        (
                            arc.centre[0] + arc.radius * math.cos(extremum),
                            arc.centre[1] + arc.radius * math.sin(extremum),
                        )
                    )
    if any(span + gradient[0] * x + gradient[1] * y <= _EPS for x, y in points):
        raise ValueError("section termination planes must not cross or touch the profile")


@dataclass(frozen=True, slots=True)
class PlanarSection:
    """An intrinsic canonical closed line/arc loop."""

    boundary: tuple[SectionVertex, ...]

    def __post_init__(self) -> None:
        vertices = tuple(self.boundary)
        if len(vertices) < 2:
            raise ValueError("section needs at least two vertices")
        if any(
            math.dist(vertex.point, vertices[(i + 1) % len(vertices)].point) <= _EPS
            for i, vertex in enumerate(vertices)
        ):
            raise ValueError("adjacent section vertices must be distinct")
        _validate_simple(vertices)
        area, _ = _moments(vertices)
        if area < 0:
            vertices = _reverse(vertices)
        vertices = _canonical_start(vertices)
        serialized = tuple(_serialized(vertex) for vertex in vertices)
        if len({item[:2] for item in serialized}) != len(serialized):
            raise ValueError("serialization would collapse distinct vertices")
        _validate_projection(vertices)
        object.__setattr__(self, "boundary", vertices)

    @property
    def area(self) -> float:
        return _moments(self.boundary)[0]

    @property
    def centroid(self) -> Vector2:
        return _moments(self.boundary)[1]


@dataclass(frozen=True, slots=True)
class SectionEnds:
    low_capped: bool
    high_capped: bool

    def __post_init__(self) -> None:
        if type(self.low_capped) is not bool or type(self.high_capped) is not bool:
            raise ValueError("section end conditions must be booleans")
        if self.low_capped and self.high_capped:
            raise ValueError("an occurrence cannot be capped at both ends")


@dataclass(frozen=True, slots=True, eq=False, init=False)
class BodyRef:
    signature: str | None
    _issuer: object


@dataclass(frozen=True, slots=True)
class _IssuedBodyRef:
    outward: BodyRef
    signature: str | None


class BodyRefIssuer:
    """Issue and validate run-local body occurrence identities."""

    def __init__(self) -> None:
        self._token = object()
        self._issued: dict[int, _IssuedBodyRef] = {}
        self._signatures: set[str] = set()

    def issue(self, *, signature: str | None = None) -> BodyRef:
        if signature is not None and (not isinstance(signature, str) or not signature):
            raise ValueError("body signature must be a nonempty string or None")
        if signature is not None and signature in self._signatures:
            raise ValueError("body signature must be unambiguous within one run")
        outward = object.__new__(BodyRef)
        object.__setattr__(outward, "signature", signature)
        object.__setattr__(outward, "_issuer", self._token)
        self._issued[id(outward)] = _IssuedBodyRef(outward, signature)
        if signature is not None:
            self._signatures.add(signature)
        return outward

    def validate(self, body: BodyRef) -> None:
        issued = self._issued.get(id(body))
        if (
            issued is None
            or issued.outward is not body
            or body._issuer is not self._token
            or body.signature is not issued.signature
        ):
            raise ValueError("body reference was not issued by this run or was mutated")


@dataclass(frozen=True, slots=True, eq=False)
class SectionOccurrence:
    body: BodyRef
    frame: LocalFrame
    run_interval: tuple[float, float]
    section: PlanarSection
    ends: SectionEnds

    def __post_init__(self) -> None:
        _validate_occurrence_value(self)


def _validate_occurrence_value(occurrence: SectionOccurrence) -> None:
    if not isinstance(occurrence.frame, LocalFrame):
        raise ValueError("section occurrence requires a LocalFrame")
    validated_frame = LocalFrame(
        occurrence.frame.origin,
        occurrence.frame.run,
        occurrence.frame.u,
        occurrence.frame.v,
    )
    canonical_frame = LocalFrame.canonical(validated_frame.run, validated_frame.origin)
    if any(
        abs(actual - expected) > _EPS
        for actual_vector, expected_vector in (
            (validated_frame.run, canonical_frame.run),
            (validated_frame.u, canonical_frame.u),
            (validated_frame.v, canonical_frame.v),
        )
        for actual, expected in zip(actual_vector, expected_vector, strict=True)
    ):
        raise ValueError("section occurrence requires the canonical run and in-plane basis")

    if not isinstance(occurrence.section, PlanarSection):
        raise ValueError("section occurrence requires a PlanarSection")
    try:
        rebuilt_section = PlanarSection(
            tuple(
                SectionVertex(vertex.point, vertex.bulge) for vertex in occurrence.section.boundary
            )
        )
    except (AttributeError, TypeError) as exc:
        raise ValueError("section occurrence contains an invalid section") from exc
    if rebuilt_section != occurrence.section:
        raise ValueError("section occurrence section is not canonical or was mutated")

    if not isinstance(occurrence.ends, SectionEnds):
        raise ValueError("section occurrence requires SectionEnds")
    SectionEnds(occurrence.ends.low_capped, occurrence.ends.high_capped)

    lo, hi = occurrence.run_interval
    if not _finite((lo, hi)) or hi - lo <= _EPS:
        raise ValueError("run interval must be finite and increasing")
    if math.hypot(*occurrence.section.centroid) > _EPS:
        raise ValueError("section occurrence requires an origin-centred intrinsic section")
    if abs(_dot(occurrence.frame.origin, occurrence.frame.run)) > _EPS:
        raise ValueError("section occurrence frame origin must be perpendicular to its run")


def validate_occurrence(occurrence: SectionOccurrence, *, body_refs: BodyRefIssuer) -> None:
    """Revalidate one private occurrence and its run-owned body at every read boundary."""

    if not isinstance(occurrence, SectionOccurrence):
        raise ValueError("expected a SectionOccurrence")
    body_refs.validate(occurrence.body)
    _validate_occurrence_value(occurrence)


def section_vertex_dict(vertex: SectionVertex) -> dict[str, object]:
    """Return the proposal's primitive-only serialized vertex shape."""

    x, y, bulge = _serialized(vertex)
    return {"point": [x, y], "bulge": bulge}


def _rounded_vector(vector: Vector3, digits: int) -> Vector3:
    return tuple(_round_clean(component, digits) for component in vector)  # type: ignore[return-value]


def _round_clean(value: float, digits: int) -> float:
    rounded = round(value, digits)
    return 0.0 if rounded == 0.0 else rounded


def occurrence_geometry_dict(
    occurrence: SectionOccurrence, *, body_refs: BodyRefIssuer
) -> dict[str, object]:
    """Project the private occurrence to the primitive-only version-1 proposal shape.

    Run-local body identity deliberately does not cross this value boundary.
    """

    validate_occurrence(occurrence, body_refs=body_refs)
    projected_origin = _rounded_vector(occurrence.frame.origin, 3)
    projected_run = _rounded_vector(occurrence.frame.run, 6)
    projected_u = _rounded_vector(occurrence.frame.u, 6)
    projected_v = _rounded_vector(occurrence.frame.v, 6)
    projected_interval = tuple(_round_clean(value, 3) for value in occurrence.run_interval)
    if projected_interval[1] <= projected_interval[0]:
        raise ValueError("serialized run interval collapses")
    projected_frame = LocalFrame.canonical(projected_run, (0.0, 0.0, 0.0))
    if (
        any(
            abs(math.sqrt(_dot(vector, vector)) - 1.0) > 1e-6
            for vector in (projected_run, projected_u, projected_v)
        )
        or any(
            abs(_dot(left, right)) > 2e-6
            for left, right in (
                (projected_run, projected_u),
                (projected_run, projected_v),
                (projected_u, projected_v),
            )
        )
        or math.dist(_cross(projected_run, projected_u), projected_v) > 3e-6
        or math.dist(projected_run, projected_frame.run) > 1e-6
        or math.dist(projected_u, projected_frame.u) > 3e-6
        or math.dist(projected_v, projected_frame.v) > 3e-6
    ):
        raise ValueError("serialized frame exceeds its canonical validation tolerances")
    perpendicular_bound = 0.000868 + 1e-6 * math.sqrt(_dot(projected_origin, projected_origin))
    if abs(_dot(projected_origin, projected_run)) > perpendicular_bound:
        raise ValueError("serialized frame origin exceeds its perpendicularity tolerance")
    projected_section = PlanarSection(
        tuple(
            SectionVertex(
                (_serialized(vertex)[0], _serialized(vertex)[1]),
                _serialized(vertex)[2],
            )
            for vertex in occurrence.section.boundary
        )
    )
    if math.hypot(*projected_section.centroid) > _POSITION_TOL:
        raise ValueError("serialized section centroid exceeds its canonical tolerance")

    origin_error = math.dist(occurrence.frame.origin, projected_origin)
    run_error = math.dist(occurrence.frame.run, projected_run)
    u_error = math.dist(occurrence.frame.u, projected_u)
    v_error = math.dist(occurrence.frame.v, projected_v)
    interval_error = max(
        abs(value - projected)
        for value, projected in zip(occurrence.run_interval, projected_interval, strict=True)
    )
    extents: list[float] = []
    for index, vertex in enumerate(occurrence.section.boundary):
        arc = _arc(
            vertex,
            occurrence.section.boundary[(index + 1) % len(occurrence.section.boundary)],
        )
        extents.append(
            math.hypot(*vertex.point) if arc is None else math.hypot(*arc.centre) + arc.radius
        )
    transverse_extent = max(extents)
    world_bound = (
        origin_error
        + max(abs(value) for value in occurrence.run_interval) * run_error
        + interval_error * math.sqrt(sum(value * value for value in projected_run))
        + transverse_extent * (u_error + v_error)
        + _projection_bound(occurrence.section.boundary)
        # A section coordinate error is one 2-D Euclidean vector, not two independent
        # full-size errors.  The Frobenius norm safely bounds its mapping through the
        # rounded (u, v) basis without falsely adding orthogonal components as collinear.
        * math.sqrt(
            sum(value * value for value in projected_u)
            + sum(value * value for value in projected_v)
        )
    )
    if world_bound > _OCCURRENCE_TOL:
        raise ValueError("serialized occurrence moves its geometry beyond local tolerance")

    return {
        "frame": {
            "origin": list(projected_origin),
            "run": list(projected_run),
            "u": list(projected_u),
            "v": list(projected_v),
        },
        "run_interval": list(projected_interval),
        "section": {
            "boundary": [section_vertex_dict(vertex) for vertex in occurrence.section.boundary]
        },
        "ends": {
            "low_capped": occurrence.ends.low_capped,
            "high_capped": occurrence.ends.high_capped,
        },
    }
