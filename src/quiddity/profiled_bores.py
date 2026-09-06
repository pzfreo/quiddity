# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Recognition of non-circular bores whose profile has explicit drafting semantics.

The first supported profile is a through double-D: two parallel chords joined by two arcs
of one circle.  That correspondence matters.  Two lines plus two circular edges also
describes an obround, while arbitrary circular arcs can describe a lens; neither is a
double-D bore. Recognition is limited to principal-axis through bores: equal opposed boundary
profiles are paired, then the complete profile prism is proved void so two blind recesses cannot
masquerade as one through feature.
"""

from __future__ import annotations

from dataclasses import dataclass
from math import asin, isfinite, sqrt

from build123d import Face, GeomType, Solid, Vector
from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.Standard import Standard_Failure

from quiddity._adjacency import FaceEdges, edge_face_map
from quiddity._candidates import FamilyId
from quiddity._claims import EvidenceWriter
from quiddity._geometry import part_scale
from quiddity._record import Record
from quiddity._typing import FaceLike, Part, Vector3
from quiddity._volume_probe import intersection_volume


@dataclass(frozen=True)
class DoubleDBore(Record):
    """A geometrically proven principal-axis through double-D bore.

    ``major_diameter`` is the parent circle diameter; ``across_flats`` is the distance
    between its parallel chords. ``flat_direction`` is their canonical unit normal and
    preserves the profile's in-plane orientation independently of the bore axis. The
    current recogniser emits ``through=True`` only: blind double-D recesses need a
    separate floor/depth proof and are deliberately unsupported.
    """

    axis: tuple[float, float, float]
    location: tuple[float, float, float]
    major_diameter: float
    across_flats: float
    depth: float
    through: bool
    flat_direction: tuple[float, float, float]


@dataclass(frozen=True)
class DoubleDProfile:
    """Private B-rep reading shared by recognition, declaration and physical critique."""

    centre: tuple[float, float, float]
    major_diameter: float
    across_flats: float
    flat_direction: tuple[float, float, float]


@dataclass(frozen=True, slots=True)
class _DoubleDBoreProposal:
    record: DoubleDBore
    wall_faces: tuple[FaceLike, ...]


@dataclass(slots=True)
class _ProposalContext:
    opening_faces: dict[int, FaceLike]
    face_edges: FaceEdges
    proposals: list[_DoubleDBoreProposal]


def _valid_wall_chain_facts(
    chains: tuple[tuple[int, ...], ...],
    high_assignments: tuple[int, ...],
    intervals: dict[int, tuple[float, float]],
    edges: tuple[tuple[int, int], ...],
    *,
    lo: float,
    hi: float,
    tol: float,
) -> bool:
    """Validate the immutable topology facts behind four logical lateral-wall chains."""

    if not all(isfinite(value) for value in (lo, hi, tol)) or tol < 0 or hi <= lo:
        return False
    if len(chains) != 4 or any(not chain for chain in chains):
        return False
    if sorted(high_assignments) != list(range(4)):
        return False
    flattened = [patch for chain in chains for patch in chain]
    if len(flattened) != len(set(flattened)):
        return False
    required = set(flattened)
    if set(intervals) != required:
        return False
    if any(
        not all(isfinite(value) for value in interval)
        or interval[1] <= interval[0]
        for interval in intervals.values()
    ):
        return False
    if any(left not in required or right not in required for left, right in edges):
        return False
    for chain in chains:
        ordered = sorted(intervals[patch] for patch in chain)
        cursor = lo
        for start, end in ordered:
            if start > cursor + tol or start < cursor - tol or end <= start:
                return False
            cursor = end
        if abs(cursor - hi) > tol:
            return False
        if len(chain) == 1:
            if any(left in chain and right in chain for left, right in edges):
                return False
            continue
        chain_edges = [(left, right) for left, right in edges if left in chain and right in chain]
        if len(chain_edges) != len(chain) - 1:
            return False
        degrees = {patch: 0 for patch in chain}
        for left, right in chain_edges:
            if left == right:
                return False
            degrees[left] += 1
            degrees[right] += 1
        if any(degree > 2 for degree in degrees.values()) or list(degrees.values()).count(1) != 2:
            return False
    return True


def _same_shape(left, right) -> bool:
    return bool(left.wrapped.IsSame(right.wrapped))


def _complete_wall_component(
    part,
    low_wire,
    high_wire,
    low_face,
    high_face,
    axis: str,
    profile: DoubleDProfile,
    lo: float,
    hi: float,
    tol: float,
    *,
    face_edges: FaceEdges,
) -> tuple[FaceLike, ...]:
    """Return the connected original lateral wall patches between two exact openings.

    Opening edges seed both ends. Traversal never crosses either consulted extremal plane,
    so middle axial subdivisions are retained while exterior stock reachable only through an
    end plane is excluded. The four-edge profile grammar has already been proved by
    :func:`double_d_profile`.
    """

    faces = list(part.faces())
    incidence = edge_face_map(faces, face_edges=face_edges)

    def seeds(wire, boundary_face) -> list:
        found: list = []
        for edge in wire.edges():
            partners = [
                face
                for face in incidence.get(edge, ())
                if not _same_shape(face, boundary_face)
            ]
            if len(partners) != 1:
                return []
            found.append(partners[0])
        return found

    low_seeds = seeds(low_wire, low_face)
    high_seeds = seeds(high_wire, high_face)
    if len(low_seeds) != 4 or len(high_seeds) != 4:
        return ()

    axis_i = "xyz".index(axis)
    attr = axis.upper()
    centre = profile.centre
    flat_direction = profile.flat_direction
    metric_tol = max(tol, profile.major_diameter * 1e-3)

    def lateral(face) -> bool:
        if _same_shape(face, low_face) or _same_shape(face, high_face):
            return False
        if face.geom_type not in (GeomType.PLANE, GeomType.CYLINDER):
            return False
        bbox = face.bounding_box()
        face_lo = float(getattr(bbox.min, attr))
        face_hi = float(getattr(bbox.max, attr))
        if face_hi - face_lo <= 1e-9 or face_lo < lo - metric_tol or face_hi > hi + metric_tol:
            return False
        surface = BRepAdaptor_Surface(face.wrapped)
        if face.geom_type == GeomType.PLANE:
            plane = surface.Plane()
            normal = plane.Axis().Direction()
            values = (normal.X(), normal.Y(), normal.Z())
            if abs(abs(sum(values[i] * flat_direction[i] for i in range(3))) - 1.0) > 1e-4:
                return False
            location = plane.Location()
            offset = abs(
                sum(
                    ((location.X(), location.Y(), location.Z())[i] - centre[i])
                    * flat_direction[i]
                    for i in range(3)
                )
            )
            valid_support = abs(offset - profile.across_flats / 2.0) <= metric_tol
        else:
            cylinder = surface.Cylinder()
            direction = cylinder.Axis().Direction()
            components = (direction.X(), direction.Y(), direction.Z())
            if abs(abs(components[axis_i]) - 1.0) > 1e-4:
                return False
            location = cylinder.Axis().Location()
            axis_point = (location.X(), location.Y(), location.Z())
            valid_support = (
                abs(cylinder.Radius() - profile.major_diameter / 2.0) <= metric_tol
                and all(
                    abs(axis_point[i] - centre[i]) <= metric_tol
                    for i in range(3)
                    if i != axis_i
                )
            )
        if not valid_support:
            return False
        native_point = surface.Value(
            0.5 * (surface.FirstUParameter() + surface.LastUParameter()),
            0.5 * (surface.FirstVParameter() + surface.LastVParameter()),
        )
        point = Vector(native_point.X(), native_point.Y(), native_point.Z())
        normal = face.normal_at(point)
        point_values = (float(point.X), float(point.Y), float(point.Z))
        normal_values = (float(normal.X), float(normal.Y), float(normal.Z))
        # A void wall's material-outward normal points from the wall into the profile void.
        radial_to_void = [centre[i] - point_values[i] for i in range(3)]
        radial_to_void[axis_i] = 0.0
        return sum(radial_to_void[i] * normal_values[i] for i in range(3)) > metric_tol

    def support(face):
        surface = BRepAdaptor_Surface(face.wrapped)
        if face.geom_type == GeomType.PLANE:
            plane = surface.Plane()
            normal = plane.Axis().Direction()
            values = (normal.X(), normal.Y(), normal.Z())
            location = plane.Location()
            offset = sum(
                (location.X(), location.Y(), location.Z())[i] * values[i]
                for i in range(3)
            )
            if next(value for value in values if abs(value) > 1e-9) < 0:
                values = tuple(-value for value in values)
                offset = -offset
            return ("plane", *values, offset)
        cylinder = surface.Cylinder()
        direction = cylinder.Axis().Direction()
        components = [direction.X(), direction.Y(), direction.Z()]
        first = next(value for value in components if abs(value) > 1e-12)
        if first < 0:
            components = [-value for value in components]
        location = cylinder.Axis().Location()
        axis_point = (location.X(), location.Y(), location.Z())
        return (
            "cylinder",
            *components,
            *(axis_point[i] for i in range(3) if i != axis_i),
            cylinder.Radius(),
        )

    def same_support(left, right) -> bool:
        if left[0] != right[0] or len(left) != len(right):
            return False
        if left[0] == "plane":
            return all(abs(left[i] - right[i]) <= 1e-4 for i in range(1, 4)) and abs(
                left[4] - right[4]
            ) <= metric_tol
        return (
            all(abs(left[i] - right[i]) <= 1e-4 for i in range(1, 4))
            and all(abs(left[i] - right[i]) <= metric_tol for i in range(4, len(left)))
        )

    def chain(seed) -> tuple[FaceLike, ...]:
        if not lateral(seed):
            return ()
        role = support(seed)
        pending = [seed]
        found: list[FaceLike] = []
        while pending:
            face = pending.pop()
            if any(_same_shape(face, known) for known in found):
                continue
            if not lateral(face) or not same_support(support(face), role):
                continue
            found.append(face)
            for edge in face_edges.of(face):
                for other in incidence.get(edge, ()):
                    if not any(_same_shape(other, known) for known in found):
                        pending.append(other)
        return tuple(found)

    chains = [chain(seed) for seed in low_seeds]
    if any(not chain_faces for chain_faces in chains):
        return ()
    # Each low role must reach exactly one high role, and every high role is consumed once.
    high_assignments: list[int] = []
    for chain_faces in chains:
        matches = [
            at
            for at, seed in enumerate(high_seeds)
            if any(_same_shape(seed, face) for face in chain_faces)
        ]
        if len(matches) != 1:
            return ()
        high_assignments.append(matches[0])
    intervals: dict[int, tuple[float, float]] = {}
    edge_pairs: list[tuple[int, int]] = []
    visited_edges: set = set()
    for chain_faces in chains:
        for face in chain_faces:
            key = id(face)
            bbox = face.bounding_box()
            intervals[key] = (
                float(getattr(bbox.min, attr)),
                float(getattr(bbox.max, attr)),
            )
            for edge in face_edges.of(face):
                if edge in visited_edges:
                    continue
                partners = [
                    other
                    for other in incidence.get(edge, ())
                    if any(_same_shape(other, candidate) for candidate in chain_faces)
                    and not _same_shape(other, face)
                ]
                if partners:
                    visited_edges.add(edge)
                    other = next(
                        candidate
                        for candidate in chain_faces
                        if _same_shape(candidate, partners[0])
                    )
                    edge_pairs.append((key, id(other)))
    chain_ids = tuple(tuple(id(face) for face in chain_faces) for chain_faces in chains)
    if not _valid_wall_chain_facts(
        chain_ids,
        tuple(high_assignments),
        intervals,
        tuple(edge_pairs),
        lo=lo,
        hi=hi,
        tol=metric_tol,
    ):
        return ()

    seen = [face for chain_faces in chains for face in chain_faces]

    return tuple(face for face in faces if any(_same_shape(face, wall) for wall in seen))


def principal_boundary_plane(face, bbox) -> tuple[str, tuple[str, str], float] | None:
    """Return ``(normal axis, in-plane axes, boundary coordinate)`` for an extremal face."""
    if face.geom_type != GeomType.PLANE:
        return None
    fbb = face.bounding_box()
    extent = {axis: float(getattr(fbb.size, axis.upper())) for axis in "xyz"}
    part_extent = (float(bbox.size.X), float(bbox.size.Y), float(bbox.size.Z))
    tol = max(1e-5, max(part_extent) * 1e-5)
    flat = [axis for axis, value in extent.items() if value <= tol]
    if len(flat) != 1:
        return None
    axis = flat[0]
    attr = axis.upper()
    at = float(getattr(fbb.center(), attr))
    if (
        min(
            abs(at - float(getattr(bbox.min, attr))),
            abs(at - float(getattr(bbox.max, attr))),
        )
        > tol
    ):
        return None
    plane_axes = tuple(candidate for candidate in "xyz" if candidate != axis)
    return axis, (plane_axes[0], plane_axes[1]), at


def double_d_profile(wire, plane_axes: tuple[str, str], *, tol: float) -> DoubleDProfile | None:
    """Read a double-D wire, rejecting merely topology-similar loops.

    The proof checks one common parent circle, opposed parallel chords, chord length and
    arc length.  Those metric correspondences distinguish the profile from true obrounds,
    lenses and arbitrary line/arc loops.
    """
    edges = list(wire.edges())
    lines = [edge for edge in edges if edge.geom_type == GeomType.LINE]
    arcs = [edge for edge in edges if edge.geom_type == GeomType.CIRCLE]
    if len(edges) != 4 or len(lines) != 2 or len(arcs) != 2:
        return None

    def coord(obj, axis: str) -> float:
        return float(getattr(obj, axis.upper()))

    wbb = wire.bounding_box()
    profile_scale = max(coord(wbb.size, axis) for axis in plane_axes)
    metric_tol = max(8 * tol, profile_scale * 1e-3)
    try:
        radius = float(arcs[0].radius)
        centre = arcs[0].arc_center
        if radius <= tol:
            return None
        if any(
            abs(float(edge.radius) - radius) > metric_tol
            or any(
                abs(coord(edge.arc_center, axis) - coord(centre, axis)) > metric_tol
                for axis in plane_axes
            )
            for edge in arcs[1:]
        ):
            return None
    except (AttributeError, ValueError):
        return None

    centre_xyz = (float(centre.X), float(centre.Y), float(centre.Z))
    axis_i = "xyz".index(next(axis for axis in "xyz" if axis not in plane_axes))
    directions: list[tuple[float, float, float]] = []
    midpoints: list[tuple[float, float, float]] = []
    for line in lines:
        vertices = list(line.vertices())
        if len(vertices) != 2:
            return None
        ends = [(float(vertex.X), float(vertex.Y), float(vertex.Z)) for vertex in vertices]
        delta = tuple(ends[1][i] - ends[0][i] for i in range(3))
        length = sqrt(sum(component * component for component in delta))
        if length <= tol or abs(delta[axis_i]) > metric_tol:
            return None
        directions.append((delta[0] / length, delta[1] / length, delta[2] / length))
        midpoints.append(
            (
                (ends[0][0] + ends[1][0]) / 2.0,
                (ends[0][1] + ends[1][1]) / 2.0,
                (ends[0][2] + ends[1][2]) / 2.0,
            )
        )
        if any(
            abs(sqrt(sum((end[i] - centre_xyz[i]) ** 2 for i in range(3))) - radius) > metric_tol
            for end in ends
        ):
            return None
    parallel = abs(sum(directions[0][i] * directions[1][i] for i in range(3)))
    if abs(parallel - 1.0) > 1e-4:
        return None

    u_i, v_i = ("xyz".index(axis) for axis in plane_axes)
    direction = directions[0]
    flat_direction = [0.0, 0.0, 0.0]
    flat_direction[u_i] = -direction[v_i]
    flat_direction[v_i] = direction[u_i]
    first = next(component for component in flat_direction if abs(component) > 1e-12)
    if first < 0:
        flat_direction = [-component for component in flat_direction]
    offsets = sorted(
        sum((midpoint[i] - centre_xyz[i]) * flat_direction[i] for i in range(3))
        for midpoint in midpoints
    )
    if offsets[0] >= -tol or offsets[1] <= tol or abs(offsets[0] + offsets[1]) > metric_tol:
        return None
    half_af = (offsets[1] - offsets[0]) / 2.0
    if half_af <= tol or half_af >= radius - tol:
        return None

    expected_chord = 2.0 * sqrt(radius * radius - half_af * half_af)
    if any(abs(float(line.length) - expected_chord) > metric_tol for line in lines):
        return None
    expected_arc = 2.0 * radius * asin(half_af / radius)
    if any(abs(float(arc.length) - expected_arc) > metric_tol for arc in arcs):
        return None

    clean_direction = tuple(
        0.0 if abs(component) <= 1e-12 else round(component, 12) for component in flat_direction
    )
    canonical_direction = (clean_direction[0], clean_direction[1], clean_direction[2])
    return DoubleDProfile(
        centre=centre_xyz,
        major_diameter=round(2.0 * radius, 4),
        across_flats=round(2.0 * half_af, 4),
        flat_direction=canonical_direction,
    )


def _profiles_correspond(
    first: DoubleDProfile,
    second: DoubleDProfile,
    plane_axes: tuple[str, str],
    *,
    tol: float,
) -> bool:
    """Whether two end profiles prove the same coaxial double-D cross-section."""
    return (
        abs(first.major_diameter - second.major_diameter) <= tol
        and abs(first.across_flats - second.across_flats) <= tol
        and all(
            abs(a - b) <= max(tol, 1e-6)
            for a, b in zip(first.flat_direction, second.flat_direction, strict=True)
        )
        and all(
            abs(first.centre["xyz".index(axis)] - second.centre["xyz".index(axis)]) <= tol
            for axis in plane_axes
        )
    )


def double_d_bores_from_openings(
    openings: list[tuple[str, float, DoubleDProfile, object]],
    bbox,
    *,
    part,
    tol: float,
    proposal_context: _ProposalContext | None = None,
) -> list[DoubleDBore]:
    """Pair extremal openings and prove their entire profile prism is void.

    Opposed equal openings are not sufficient: two blind recesses can leave an internal web.
    Extruding the actual opening wire across the claimed depth and intersecting that prism with
    the part proves the whole cross-section is open, not merely a sampled centre line. Physical
    critique uses the same proof over the openings in its independent boundary scan.
    """
    out: list[DoubleDBore] = []
    for axis in "xyz":
        attr = axis.upper()
        lo = float(getattr(bbox.min, attr))
        hi = float(getattr(bbox.max, attr))
        low = [(at, p, wire) for a, at, p, wire in openings if a == axis and abs(at - lo) <= tol]
        high = [(at, p, wire) for a, at, p, wire in openings if a == axis and abs(at - hi) <= tol]
        in_plane = [candidate for candidate in "xyz" if candidate != axis]
        plane_axes = (in_plane[0], in_plane[1])
        for _lo_at, low_profile, low_wire in low:
            high_match = next(
                (
                    (profile, high_wire)
                    for _hi_at, profile, high_wire in high
                    if _profiles_correspond(profile, low_profile, plane_axes, tol=tol)
                ),
                None,
            )
            if high_match is None:
                continue
            high_profile, high_wire = high_match
            axis_vector = (
                1.0 if axis == "x" else 0.0,
                1.0 if axis == "y" else 0.0,
                1.0 if axis == "z" else 0.0,
            )
            try:
                prism = Solid.extrude(
                    Face(low_wire),
                    Vector(*(component * (hi - lo) for component in axis_vector)),
                )
                overlap = part & prism
                volume_tol = max(tol**3, float(prism.volume) * 1e-6)
                overlap_volume = intersection_volume(overlap)
                if overlap_volume > volume_tol:
                    continue
            except (Standard_Failure, RuntimeError, ValueError):
                continue
            location = list(high_profile.centre)
            location["xyz".index(axis)] = hi
            record = DoubleDBore(
                axis=axis_vector,
                location=(location[0], location[1], location[2]),
                major_diameter=high_profile.major_diameter,
                across_flats=high_profile.across_flats,
                depth=round(hi - lo, 4),
                through=True,
                flat_direction=high_profile.flat_direction,
            )
            out.append(record)
            if proposal_context is not None:
                low_face = proposal_context.opening_faces.get(id(low_wire))
                high_face = proposal_context.opening_faces.get(id(high_wire))
                if low_face is None or high_face is None:
                    proposal_context.proposals.append(_DoubleDBoreProposal(record, ()))
                else:
                    walls = _complete_wall_component(
                        part,
                        low_wire,
                        high_wire,
                        low_face,
                        high_face,
                        axis,
                        low_profile,
                        lo,
                        hi,
                        tol,
                        face_edges=proposal_context.face_edges,
                    )
                    proposal_context.proposals.append(_DoubleDBoreProposal(record, walls))
    return sorted(out, key=lambda bore: (bore.axis, bore.location, bore.major_diameter))


def _recognise_double_d_bores_one(
    part,
    *,
    tol: float,
    face_edges: FaceEdges | None = None,
    proposals: list[_DoubleDBoreProposal] | None = None,
) -> list[DoubleDBore]:
    """Recognise double-D bores within one solid's own boundary."""
    bbox = part.bounding_box()
    scan_tol = max(tol, part_scale(bbox) * 1e-5)
    openings: list[tuple[str, float, DoubleDProfile, object]] = []
    opening_faces: dict[int, FaceLike] = {}
    for face in part.faces():
        boundary = principal_boundary_plane(face, bbox)
        if boundary is None:
            continue
        axis, plane_axes, at = boundary
        for wire in face.inner_wires():
            profile = double_d_profile(wire, plane_axes, tol=scan_tol)
            if profile is not None:
                openings.append((axis, at, profile, wire))
                opening_faces[id(wire)] = face
    context = None
    if proposals is not None:
        context = _ProposalContext(opening_faces, face_edges or FaceEdges(), proposals)
    return double_d_bores_from_openings(
        openings,
        bbox,
        part=part,
        tol=scan_tol,
        proposal_context=context,
    )


def recognise_double_d_bores(part: Part, *, tol: float = 1e-5) -> list[DoubleDBore]:
    """Recognise principal-axis through double-D bores, independently per solid.

    This slice is deliberately through-only: the same profile must appear at both opposite
    extremal faces.  A single opening could be a blind recess, whose floor/depth needs a
    separate proof before it can be called a bore. Each solid owns its own extrema so two
    components in an assembly cannot be paired into one fictitious bore across their gap.
    """
    return _discover_double_d_bores(part, tol=tol)


def _discover_double_d_bores(
    part: Part,
    *,
    tol: float = 1e-5,
    face_edges: FaceEdges | None = None,
    writer: EvidenceWriter | None = None,
) -> list[DoubleDBore]:
    """Shared public/aggregate discovery with optional write-only wall evidence."""

    solids = list(part.solids())
    sources = solids if len(solids) > 1 else [part]
    proposals: list[_DoubleDBoreProposal] | None = [] if writer is not None else None
    bores = [
        bore
        for solid in sources
        for bore in _recognise_double_d_bores_one(
            solid,
            tol=tol,
            face_edges=face_edges,
            proposals=proposals,
        )
    ]
    ordered = sorted(bores, key=lambda bore: (bore.axis, bore.location, bore.major_diameter))
    if writer is None:
        return ordered

    assert proposals is not None
    by_occurrence = {id(proposal.record): proposal for proposal in proposals}
    ordered_proposals = [by_occurrence[id(record)] for record in ordered]
    pending: list[tuple[DoubleDBore, tuple]] = []
    assigned_nodes: set[int] = set()
    for proposal in ordered_proposals:
        if not proposal.wall_faces:
            raise ValueError("Double-D bore has no complete original wall component")
        resolved = tuple(writer.graph.require_node(face) for face in proposal.wall_faces)
        node_ids = {id(node) for node in resolved}
        nodes = tuple(node for node in writer.graph.nodes if id(node) in node_ids)
        if not nodes or writer.graph.common_valid_solid(nodes) is None:
            raise ValueError("Double-D wall evidence has no one valid owner solid")
        if assigned_nodes & node_ids:
            raise ValueError("Double-D wall evidence is assigned across occurrences")
        assigned_nodes.update(node_ids)
        pending.append((proposal.record, nodes))
    for record, nodes in pending:
        writer.add_defining(record, nodes, family=FamilyId.DOUBLE_D_BORES)
    return ordered


def read_double_d_tool(
    obj: Part, *, tol: float = 1e-5
) -> tuple[str, float, float, Vector3, float, Vector3]:
    """Read one constant double-D extrusion, rejecting merely matching end geometry.

    Returns ``(axis, major_diameter, across_flats, origin, depth,
    profile_direction)``. ``axis`` is the principal-axis name ``"x"``, ``"y"`` or
    ``"z"``; diameters, ``origin`` coordinates and ``depth`` use model-length units;
    ``profile_direction`` is a unitless direction in the profile plane.
    """
    bbox = obj.bounding_box()
    scan_tol = max(tol, part_scale(bbox) * 1e-5)
    ends: list[tuple[str, float, DoubleDProfile, object]] = []
    for face in obj.faces():
        boundary = principal_boundary_plane(face, bbox)
        if boundary is None:
            continue
        axis, plane_axes, at = boundary
        profile = double_d_profile(face.outer_wire(), plane_axes, tol=scan_tol)
        if profile is not None:
            ends.append((axis, at, profile, face.outer_wire()))

    for axis in "xyz":
        i = "xyz".index(axis)
        attr = axis.upper()
        lo = float(getattr(bbox.min, attr))
        hi = float(getattr(bbox.max, attr))
        in_plane = [candidate for candidate in "xyz" if candidate != axis]
        plane_axes = (in_plane[0], in_plane[1])
        low = [
            (profile, wire)
            for a, at, profile, wire in ends
            if a == axis and abs(at - lo) <= scan_tol
        ]
        high = [
            profile for a, at, profile, _wire in ends if a == axis and abs(at - hi) <= scan_tol
        ]
        for low_profile, low_wire in low:
            high_profile = next(
                (
                    profile
                    for profile in high
                    if _profiles_correspond(
                        low_profile,
                        profile,
                        (plane_axes[0], plane_axes[1]),
                        tol=scan_tol,
                    )
                ),
                None,
            )
            if high_profile is None:
                continue
            axis_vector = [0.0, 0.0, 0.0]
            axis_vector[i] = hi - lo
            try:
                prism = Solid.extrude(Face(low_wire), Vector(*axis_vector))
                overlap = obj & prism
                overlap_volume = intersection_volume(overlap)
                volume_tol = max(scan_tol**3, float(prism.volume) * 1e-6)
                if (
                    abs(overlap_volume - float(prism.volume)) > volume_tol
                    or abs(float(obj.volume) - float(prism.volume)) > volume_tol
                ):
                    continue
            except (Standard_Failure, RuntimeError, ValueError):
                continue
            centre = list(low_profile.centre)
            centre[i] = (lo + hi) / 2.0
            return (
                axis,
                low_profile.major_diameter,
                low_profile.across_flats,
                (centre[0], centre[1], centre[2]),
                hi - lo,
                low_profile.flat_direction,
            )
    raise ValueError(
        "double_d_bore(object) needs one constant extrusion of a two-chord, "
        "common-circle double-D profile"
    )
