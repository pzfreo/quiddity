# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Recognition of bounded principal-axis rectangular raised pads."""

from __future__ import annotations

import math
from collections import Counter
from dataclasses import dataclass
from typing import Any, cast

from build123d import Vector
from OCP.BRepGProp import BRepGProp
from OCP.GProp import GProp_GProps

from quiddity._analytic_surfaces import SurfaceKind
from quiddity._candidates import FamilyId
from quiddity._claims import EvidenceWriter
from quiddity._effective_surfaces import (
    AnalyticSurfaceFact,
    EffectiveFaceSurfaceQuery,
    SurfaceUse,
    SurfaceUseRefusal,
    effective_faces_for_graph,
    effective_faces_for_part,
)
from quiddity._geometry import AXIS_ALIGNED_COS, AXIS_ZERO_COS
from quiddity._record import Record
from quiddity._typing import FaceLike, Part
from quiddity.experimental_geometry import (
    AnalyticSurface,
    BlendFact,
    FaceRef,
    GeometryGraph,
)
from quiddity.experimental_geometry import SurfaceKind as InspectionSurfaceKind

#: **A minimum-evidence threshold, not a tolerance — deliberately absolute (ADR 0008).**
#: Scaling it to the part makes a feature's existence depend on what surrounds it, so a small
#: feature on a large part disappears. Whether such a feature is worth dimensioning is consumer
#: policy, and ADR 0001 puts policy with the consumer; recognition reports it either way.
#: Also the pad-footprint minimum on both in-plane axes.
_TOL = 0.2


@dataclass(frozen=True, order=True)
class RaisedPad(Record):
    """A bounded rectangular island, including local XYZ bounds and orientation."""

    x0: float
    x1: float
    y0: float
    y1: float
    z0: float
    z1: float
    axis: str = "z"
    direction: int = 1


@dataclass(frozen=True, slots=True)
class _PadProposal:
    record: RaisedPad
    top_face: FaceLike
    wall_roles: tuple[tuple[FaceLike, ...], ...]


@dataclass(frozen=True, slots=True)
class _PlanarFace:
    face: FaceLike
    bounds: Any
    normal: tuple[float, float, float]
    area: float


_AXIS_INDEX = {"x": 0, "y": 1, "z": 2}
_TRANSVERSE_AXES = {"x": ("y", "z"), "y": ("x", "z"), "z": ("x", "y")}


def _component(value: Any, axis: str) -> float:
    return float(getattr(value, axis.upper()))


def _span(bounds: Any, axis: str) -> tuple[float, float]:
    if isinstance(bounds, tuple):
        return cast(tuple[float, float], bounds[_AXIS_INDEX[axis]])
    return _component(bounds.min, axis), _component(bounds.max, axis)


def _surface_area(face: FaceLike) -> float:
    properties = GProp_GProps()
    BRepGProp.SurfaceProperties_s(face.wrapped, properties)
    return float(properties.Mass())


def _face_vertex_bounds(face: FaceLike) -> tuple[tuple[float, float], ...]:
    """Return exact coordinate spans from a planar candidate's boundary vertices."""

    vertices = tuple(tuple(vertex) for vertex in face.vertices())
    return tuple((min(values), max(values)) for values in zip(*vertices, strict=True))


def _planar_faces(
    part: Part,
    face_surfaces: EffectiveFaceSurfaceQuery,
) -> tuple[_PlanarFace, ...]:
    """Resolve run-owned planar facts and bounds once for all six orientations."""

    resolved = []
    for face in part.faces():
        fact = face_surfaces.fact(face)
        if isinstance(fact, AnalyticSurfaceFact) and fact.kind is SurfaceKind.PLANE:
            normal = fact.parameters
            if max(abs(normal[0]), abs(normal[1]), abs(normal[2])) < AXIS_ALIGNED_COS:
                continue
            resolved.append(
                _PlanarFace(
                    face,
                    _face_vertex_bounds(face),
                    (normal[0], normal[1], normal[2]),
                    _surface_area(face),
                )
            )
    return tuple(resolved)


def _record_bounds(record: RaisedPad) -> tuple[tuple[float, float], ...]:
    return ((record.x0, record.x1), (record.y0, record.y1), (record.z0, record.z1))


def _axial_extent(record: RaisedPad) -> float:
    """Return the attachment-to-terminal span of an oriented pad."""

    lo, hi = _record_bounds(record)[_AXIS_INDEX[record.axis]]
    return hi - lo


def _proposal_faces(proposal: _PadProposal, geometry: GeometryGraph) -> frozenset[FaceRef]:
    """Return original boundary evidence used by one interpretation."""

    return frozenset(
        geometry.ref(face)
        for face in (proposal.top_face, *(face for role in proposal.wall_roles for face in role))
    )


def _resolve_axis_ambiguity(
    proposals: list[_PadProposal], geometry: GeometryGraph, *, tol: float
) -> list[_PadProposal]:
    """Choose a unique shallowest interpretation among overlapping axis evidence.

    A rectilinear union can make one physical island look like several pads when
    viewed along different principal axes.  Boundary-disjoint occurrences remain
    independent.  Within each overlapping component, the unique shortest
    attachment span owns the evidence; a tied minimum is geometrically ambiguous
    and is refused without preferring a world axis.
    """

    evidence = [_proposal_faces(proposal, geometry) for proposal in proposals]
    remaining = set(range(len(proposals)))
    selected: list[_PadProposal] = []
    while remaining:
        component = {remaining.pop()}
        frontier = list(component)
        while frontier:
            current = frontier.pop()
            neighbours = {index for index in remaining if evidence[current] & evidence[index]}
            remaining.difference_update(neighbours)
            component.update(neighbours)
            frontier.extend(neighbours)
        by_axis: dict[tuple[str, int], list[int]] = {}
        for index in component:
            record = proposals[index].record
            by_axis.setdefault((record.axis, record.direction), []).append(index)
        minimum = min(
            min(_axial_extent(proposals[index].record) for index in indices)
            for indices in by_axis.values()
        )
        winner_axes = {
            orientation
            for orientation, indices in by_axis.items()
            if abs(min(_axial_extent(proposals[index].record) for index in indices) - minimum)
            <= tol
        }
        if len(winner_axes) == 1:
            selected.extend(proposals[index] for index in sorted(by_axis[next(iter(winner_axes))]))
    return selected


def _pad_record(
    *,
    axis: str,
    axis_sign: int,
    u_axis: str,
    u0: float,
    u1: float,
    v_axis: str,
    v0: float,
    v1: float,
    base: float,
    top: float,
) -> RaisedPad:
    bounds = {axis: tuple(sorted((base, top))), u_axis: (u0, u1), v_axis: (v0, v1)}
    x_bounds, y_bounds, z_bounds = (bounds[coordinate_axis] for coordinate_axis in ("x", "y", "z"))
    return RaisedPad(
        x0=round(x_bounds[0], 3),
        x1=round(x_bounds[1], 3),
        y0=round(y_bounds[0], 3),
        y1=round(y_bounds[1], 3),
        z0=round(z_bounds[0], 3),
        z1=round(z_bounds[1], 3),
        axis=axis,
        direction=axis_sign,
    )


def _wall_role(
    vertical_faces: list[tuple[FaceLike, Any, Any]],
    *,
    axis: str,
    pos: float,
    lo: float,
    hi: float,
    top: float,
    tol: float,
    height_axis: str = "z",
    axis_sign: int = 1,
) -> tuple[float, tuple[FaceLike, ...]] | None:
    """Return the current maximal-base original faces for one perimeter role."""

    matches = []
    for face, bounds, normal in vertical_faces:
        n_axis = abs(_component(normal, axis))
        if n_axis < AXIS_ALIGNED_COS:
            continue
        plane_lo, plane_hi = _span(bounds, axis)
        plane_pos = (plane_lo + plane_hi) / 2
        cross_axis = next(
            candidate for candidate in ("x", "y", "z") if candidate not in (axis, height_axis)
        )
        cross_lo, cross_hi = _span(bounds, cross_axis)
        height_lo, height_hi = _span(bounds, height_axis)
        wall_top = height_hi if axis_sign > 0 else height_lo
        candidate_base = height_lo if axis_sign > 0 else height_hi
        if (
            abs(plane_pos - pos) <= tol
            and abs(wall_top - top) <= tol
            and axis_sign * top - tol > axis_sign * candidate_base
            and cross_lo <= lo + tol
            and cross_hi >= hi - tol
        ):
            matches.append((float(candidate_base), face))
    if not matches:
        return None
    base = max(matches, key=lambda item: axis_sign * item[0])[0]
    return base, tuple(face for candidate_base, face in matches if candidate_base == base)


def _touches_plan(a: RaisedPad, b: RaisedPad, *, tol: float) -> bool:
    """Return tolerance-inclusive contact in a pad's transverse plane."""

    if a.axis != b.axis or a.direction != b.direction:
        return False
    bounds_a = _record_bounds(a)
    bounds_b = _record_bounds(b)
    transverse = tuple(index for index in range(3) if index != _AXIS_INDEX[a.axis])
    return all(
        min(bounds_a[index][1], bounds_b[index][1]) - max(bounds_a[index][0], bounds_b[index][0])
        >= -tol
        for index in transverse
    )


def _tier_suppresses(pad: RaisedPad, region: RaisedPad, *, tol: float) -> bool:
    """Return whether one raw top is the current touching-tier suppression context."""

    axial = _AXIS_INDEX[pad.axis]
    pad_span = _record_bounds(pad)[axial]
    region_span = _record_bounds(region)[axial]
    pad_base = pad_span[0] if pad.direction > 0 else pad_span[1]
    region_top = region_span[1] if region.direction > 0 else region_span[0]
    return abs(region_top - pad_base) <= tol and _touches_plan(pad, region, tol=tol)


def _recognise_rectangular_pads_one(
    part,
    *,
    tol: float | None,
    face_surfaces: EffectiveFaceSurfaceQuery,
    axis: str = "z",
    axis_sign: int = 1,
    planar_faces: tuple[_PlanarFace, ...] | None = None,
    material_side_cache: dict[int, SurfaceUse | SurfaceUseRefusal] | None = None,
    part_bounds: Any | None = None,
) -> list[_PadProposal]:
    """Recognise pads using one solid's faces and bounds."""
    bb = part.bounding_box() if part_bounds is None else part_bounds
    tol = _TOL if tol is None else tol
    axis_index = _AXIS_INDEX[axis]
    u_axis, v_axis = _TRANSVERSE_AXES[axis]
    geometric_tops: list[tuple[float, float, float, float, float, FaceLike]] = []
    planar_faces = _planar_faces(part, face_surfaces) if planar_faces is None else planar_faces
    for planar in planar_faces:
        face, fb, normal = planar.face, planar.bounds, planar.normal
        if abs(normal[axis_index]) < AXIS_ALIGNED_COS:
            continue
        u0, u1 = _span(fb, u_axis)
        v0, v1 = _span(fb, v_axis)
        top_lo, top_hi = _span(fb, axis)
        top_coordinate = (top_lo + top_hi) / 2
        bb_lo, bb_hi = _span(bb, axis)
        if (
            u1 - u0 <= tol
            or v1 - v0 <= tol
            or axis_sign * (top_coordinate - (bb_lo if axis_sign > 0 else bb_hi)) <= tol
        ):
            continue
        rectangle_area = (u1 - u0) * (v1 - v0)
        if abs(planar.area - rectangle_area) > max(tol * tol, 0.005 * rectangle_area):
            continue
        bb_u0, bb_u1 = _span(bb, u_axis)
        bb_v0, bb_v1 = _span(bb, v_axis)
        full_u = bb_u0 + tol >= u0 and bb_u1 - tol <= u1
        full_v = bb_v0 + tol >= v0 and bb_v1 - tol <= v1
        if full_u or full_v:
            continue
        top_entry = (
            round(u0, 3),
            round(u1, 3),
            round(v0, 3),
            round(v1, 3),
            round(top_coordinate, 3),
            face,
        )
        geometric_tops.append(top_entry)

    # Recover each pad's base from its own four downward perimeter walls. A
    # part-global "highest horizontal level below the top" is wrong when another
    # feature has an unrelated intervening Z level.
    vertical_faces = []
    for planar in planar_faces:
        face, fb, normal = planar.face, planar.bounds, planar.normal
        if abs(normal[axis_index]) > AXIS_ZERO_COS:
            continue
        vertical_faces.append((face, fb, Vector(*normal)))

    proposals: list[_PadProposal] = []
    for u0, u1, v0, v1, top_coordinate, top_face in geometric_tops:
        roles = (
            _wall_role(
                vertical_faces,
                axis=u_axis,
                pos=u0,
                lo=v0,
                hi=v1,
                top=top_coordinate,
                tol=tol,
                height_axis=axis,
                axis_sign=axis_sign,
            ),
            _wall_role(
                vertical_faces,
                axis=u_axis,
                pos=u1,
                lo=v0,
                hi=v1,
                top=top_coordinate,
                tol=tol,
                height_axis=axis,
                axis_sign=axis_sign,
            ),
            _wall_role(
                vertical_faces,
                axis=v_axis,
                pos=v0,
                lo=u0,
                hi=u1,
                top=top_coordinate,
                tol=tol,
                height_axis=axis,
                axis_sign=axis_sign,
            ),
            _wall_role(
                vertical_faces,
                axis=v_axis,
                pos=v1,
                lo=u0,
                hi=u1,
                top=top_coordinate,
                tol=tol,
                height_axis=axis,
                axis_sign=axis_sign,
            ),
        )
        if any(role is None for role in roles):
            continue
        complete_roles = tuple(role for role in roles if role is not None)
        numeric_bases = [role[0] for role in complete_roles]
        # A pad touching the part envelope may have one exterior wall merged all
        # the way to the stock base. The highest perimeter-wall base is the local
        # support plane; the other three walls still prove the bounded island.
        base = max(numeric_bases, key=lambda value: axis_sign * value)
        proposals.append(
            _PadProposal(
                _pad_record(
                    axis=axis,
                    axis_sign=axis_sign,
                    u_axis=u_axis,
                    u0=u0,
                    u1=u1,
                    v_axis=v_axis,
                    v0=v0,
                    v1=v1,
                    base=base,
                    top=top_coordinate,
                ),
                top_face,
                tuple(role[1] for role in complete_roles),
            )
        )

    if not proposals:
        return []

    raw_regions = [
        _pad_record(
            axis=axis,
            axis_sign=axis_sign,
            u_axis=u_axis,
            u0=u0,
            u1=u1,
            v_axis=v_axis,
            v0=v0,
            v1=v1,
            base=top,
            top=top,
        )
        for u0, u1, v0, v1, top, _face in geometric_tops
    ]
    needed_top_ids = {id(proposal.top_face) for proposal in proposals}
    suppression_needed_ids = {
        id(top_entry[-1])
        for top_entry, region in zip(geometric_tops, raw_regions, strict=True)
        if any(_tier_suppresses(proposal.record, region, tol=tol) for proposal in proposals)
    }
    needed_top_ids.update(suppression_needed_ids)

    # Material certification is the expensive exact-solid query.  Establish the
    # complete rectangular terminal/wall grammar first; parts with no structural
    # proposal never mesh irrelevant planar faces merely to reject them. Tops
    # unable to define or suppress a structural proposal are equally irrelevant.
    suppression_tops: list[tuple[float, float, float, float, float, FaceLike]] = []
    certified_top_ids: set[int] = set()
    for top_entry in geometric_tops:
        face = top_entry[-1]
        cache_key = id(face)
        if cache_key not in needed_top_ids:
            continue
        top_surface = (
            material_side_cache.get(cache_key) if material_side_cache is not None else None
        )
        if top_surface is None:
            top_surface = face_surfaces.use(face, material_side=True)
            if material_side_cache is not None:
                material_side_cache[cache_key] = top_surface
        if isinstance(top_surface, SurfaceUseRefusal) or top_surface.material_side is None:
            # Tier suppression is conservative context, not a feature claim.
            # Keep unverified geometric ledges in that context; refusing a ledge
            # must never introduce a Pad claim on the tier above it.
            suppression_tops.append(top_entry)
            continue
        if top_surface.material_side.outward[axis_index] * axis_sign < AXIS_ALIGNED_COS:
            continue
        suppression_tops.append(top_entry)
        certified_top_ids.add(cache_key)

    proposals = [proposal for proposal in proposals if id(proposal.top_face) in certified_top_ids]

    # A tiered/staircase tower has rectangular ledges touching the candidate at its
    # recovered local base.  Lower ledges on a sloped support can touch the pad in plan
    # without belonging to that stack; comparing every different Z discarded the
    # real upper pad.  Disjoint pads may legitimately have any number of heights.
    suppression_ids = {id(top_entry[-1]) for top_entry in suppression_tops}
    suppression_regions = [
        region
        for top_entry, region in zip(geometric_tops, raw_regions, strict=True)
        if id(top_entry[-1]) in suppression_ids
    ]
    return [
        proposal
        for proposal in proposals
        if not any(
            _tier_suppresses(proposal.record, other, tol=tol) for other in suppression_regions
        )
    ]


def _recognise_blended_rectangular_pads_one(
    part,
    *,
    tol: float | None,
    face_surfaces: EffectiveFaceSurfaceQuery,
    geometry: GeometryGraph,
    axis: str = "z",
    axis_sign: int = 1,
    blend_facts_cache: list[tuple[BlendFact, ...]] | None = None,
    planar_faces: tuple[_PlanarFace, ...] | None = None,
    material_side_cache: dict[int, SurfaceUse | SurfaceUseRefusal] | None = None,
    part_bounds: Any | None = None,
) -> list[_PadProposal]:
    """Recognise one complete four-corner convex blend cycle around a rectangular pad."""

    tol = _TOL if tol is None else tol
    axis_index = _AXIS_INDEX[axis]
    u_axis, v_axis = _TRANSVERSE_AXES[axis]
    u_index, v_index = _AXIS_INDEX[u_axis], _AXIS_INDEX[v_axis]
    bb = part.bounding_box() if part_bounds is None else part_bounds
    faces = tuple(part.faces())
    refs = {geometry.ref(face): face for face in faces}
    local_refs = set(refs)
    planar_faces = _planar_faces(part, face_surfaces) if planar_faces is None else planar_faces

    vertical: dict[FaceRef, tuple[FaceLike, Any, tuple[float, float, float]]] = {}
    for planar in planar_faces:
        face = planar.face
        ref = geometry.ref(face)
        normal = geometry.normal(ref)
        if normal is not None and abs(normal[axis_index]) <= AXIS_ZERO_COS:
            vertical[ref] = (face, planar.bounds, normal)

    eligible_chains: list[BlendFact] | None = None

    proposals: list[_PadProposal] = []
    for planar in planar_faces:
        top_face, top_bounds = planar.face, planar.bounds
        top_ref = geometry.ref(top_face)
        top_lo, top_hi = _span(top_bounds, axis)
        top = round((top_lo + top_hi) / 2, 3)
        bb_lo, bb_hi = _span(bb, axis)
        if axis_sign * top - tol <= axis_sign * (bb_lo if axis_sign > 0 else bb_hi):
            continue

        adjacent_vertical = set(geometry.neighbours(top_ref)) & set(vertical)
        roles: dict[str, FaceRef] = {}
        for ref in adjacent_vertical:
            _face, bounds, normal = vertical[ref]
            axial_span = _span(bounds, axis)
            wall_top = axial_span[1] if axis_sign > 0 else axial_span[0]
            if abs(wall_top - top) > tol:
                continue
            if abs(normal[u_index]) >= AXIS_ALIGNED_COS and abs(normal[v_index]) <= AXIS_ZERO_COS:
                role = "u1" if normal[u_index] > 0 else "u0"
            elif abs(normal[v_index]) >= AXIS_ALIGNED_COS and abs(normal[u_index]) <= AXIS_ZERO_COS:
                role = "v1" if normal[v_index] > 0 else "v0"
            else:
                continue
            if role in roles:
                roles = {}
                break
            roles[role] = ref
        if set(roles) != {"u0", "u1", "v0", "v1"}:
            continue

        ordered_refs = tuple(roles[name] for name in ("u0", "u1", "v0", "v1"))
        u0 = round(sum(geometry.bounds(roles["u0"])[u_index]) / 2, 3)
        u1 = round(sum(geometry.bounds(roles["u1"])[u_index]) / 2, 3)
        v0 = round(sum(geometry.bounds(roles["v0"])[v_index]) / 2, 3)
        v1 = round(sum(geometry.bounds(roles["v1"])[v_index]) / 2, 3)
        if u1 - u0 <= tol or v1 - v0 <= tol:
            continue
        bb_u0, bb_u1 = _span(bb, u_axis)
        bb_v0, bb_v1 = _span(bb, v_axis)
        full_u = bb_u0 + tol >= u0 and bb_u1 - tol <= u1
        full_v = bb_v0 + tol >= v0 and bb_v1 - tol <= v1
        if full_u or full_v:
            continue
        role_cross_spans = (
            geometry.bounds(roles["u0"])[v_index],
            geometry.bounds(roles["u1"])[v_index],
            geometry.bounds(roles["v0"])[u_index],
            geometry.bounds(roles["v1"])[u_index],
        )
        expected_cross_spans = ((v0, v1), (v0, v1), (u0, u1), (u0, u1))
        if all(
            actual[0] <= expected[0] + tol and actual[1] >= expected[1] - tol
            for actual, expected in zip(role_cross_spans, expected_cross_spans, strict=True)
        ):
            continue  # the unchanged sharp path owns uninterrupted wall roles

        if eligible_chains is None:  # pragma: no branch - cached after the first eligible top
            eligible_chains = []
            if blend_facts_cache is None:
                all_chains = geometry.blend_facts()
            else:
                if not blend_facts_cache:
                    blend_facts_cache.append(tuple(geometry.blend_facts()))
                all_chains = blend_facts_cache[0]
            for chain in all_chains:
                if (
                    chain.side != "convex"
                    or len(chain.blend_faces) != 1
                    or any(len(support) != 1 for support in chain.supports)
                ):
                    continue
                left = next(iter(chain.supports[0]))
                right = next(iter(chain.supports[1]))
                if left not in vertical or right not in vertical:
                    continue  # pragma: no cover - graph-issued support refs are local
                if not chain.blend_faces <= local_refs:
                    continue  # pragma: no cover - graph-issued blend refs are local
                blend_fact = geometry.surface_fact(next(iter(chain.blend_faces)))
                if (
                    not isinstance(blend_fact, AnalyticSurface)
                    or blend_fact.kind is not InspectionSurfaceKind.CYLINDER
                ):
                    continue
                left_span = geometry.bounds(left)[axis_index]
                right_span = geometry.bounds(right)[axis_index]
                terminal_index = 1 if axis_sign > 0 else 0
                if abs(left_span[terminal_index] - right_span[terminal_index]) > tol:
                    continue  # pragma: no cover - one native chain has one shared axial span
                eligible_chains.append(chain)

        expected_pairs = (
            frozenset((roles["u0"], roles["v0"])),
            frozenset((roles["u0"], roles["v1"])),
            frozenset((roles["u1"], roles["v0"])),
            frozenset((roles["u1"], roles["v1"])),
        )
        expected_pair_set = set(expected_pairs)
        by_pair: dict[frozenset[FaceRef], list[BlendFact]] = {}
        for chain in eligible_chains:
            pair = frozenset((next(iter(chain.supports[0])), next(iter(chain.supports[1]))))
            if pair in expected_pair_set:
                by_pair.setdefault(pair, []).append(chain)
        if set(by_pair) != expected_pair_set or any(
            len(chains) != 1 for chains in by_pair.values()
        ):
            continue
        selected = tuple(by_pair[pair][0] for pair in expected_pairs)

        spans = [geometry.bounds(ref)[axis_index] for ref in ordered_refs]
        terminal_index = 1 if axis_sign > 0 else 0
        base_index = 0 if axis_sign > 0 else 1
        if any(abs(span[terminal_index] - top) > tol for span in spans):
            continue  # pragma: no cover - adjacency to this planar top fixes the upper span
        base = max((span[base_index] for span in spans), key=lambda value: axis_sign * value)
        if axis_sign * top - tol <= axis_sign * base:
            continue  # pragma: no cover - eligible vertical faces have positive height

        # Four quarter-circle removals explain the rounded top exactly; another trim or hole
        # cannot borrow the blend cycle's permission to become a Pad.
        if len(top_face.wires()) != 1:
            continue
        removed = math.fsum((1.0 - math.pi / 4.0) * chain.radius**2 for chain in selected)
        expected_area = (u1 - u0) * (v1 - v0) - removed
        if abs(planar.area - expected_area) > max(tol * tol, 0.005 * expected_area):
            continue

        bridges = geometry.collapsed_bridges(tuple(chain.ref for chain in selected))
        if len(bridges) != 4:
            raise ValueError("selected Pad blend cycle has no unique logical bridges")
        for chain in selected:
            pair = frozenset((next(iter(chain.supports[0])), next(iter(chain.supports[1]))))
            matching = [bridge for bridge in bridges if frozenset(bridge.supports) == pair]
            if len(matching) != 1:
                raise ValueError("selected Pad blend chain has no unique logical bridge")
            expected_faces = frozenset((*chain.blend_faces, *chain.supports[0], *chain.supports[1]))
            if matching[0].provenance.faces != expected_faces or Counter(
                matching[0].provenance.boundary
            ) != Counter(chain.boundary):
                raise ValueError("selected Pad blend bridge lost original provenance")

        cache_key = id(top_face)
        top_use = material_side_cache.get(cache_key) if material_side_cache is not None else None
        if top_use is None:
            top_use = face_surfaces.use(top_face, material_side=True)
            if material_side_cache is not None:
                material_side_cache[cache_key] = top_use
        if isinstance(top_use, SurfaceUseRefusal) or top_use.material_side is None:
            continue
        if top_use.material_side.outward[axis_index] * axis_sign < AXIS_ALIGNED_COS:
            continue

        proposals.append(
            _PadProposal(
                _pad_record(
                    axis=axis,
                    axis_sign=axis_sign,
                    u_axis=u_axis,
                    u0=u0,
                    u1=u1,
                    v_axis=v_axis,
                    v0=v0,
                    v1=v1,
                    base=base,
                    top=top,
                ),
                top_face,
                tuple((vertical[ref][0],) for ref in ordered_refs),
            )
        )
    return proposals


def recognise_rectangular_pads(part: Part, *, tol: float | None = None) -> list[RaisedPad]:
    """Return bounded rectangular raised faces independently per solid.

    A candidate is a planar principal-axis face whose area fills its transverse bounding rectangle
    and is bounded on both transverse axes. Full-span steps are excluded;
    non-rectangular pocket floors and perforated plate faces fail the area test.
    Body-local walls and bounds prevent a detached component from being treated
    as a pad raised from another component. Each input face must have one unique
    owner in a valid closed solid; open, invalid, or ambiguous body ownership is
    refused and returns no Pad records.
    """
    return _discover_rectangular_pads(part, tol=tol)


def _discover_rectangular_pads(
    part: Part,
    *,
    tol: float | None = None,
    writer: EvidenceWriter | None = None,
    face_surfaces: EffectiveFaceSurfaceQuery | None = None,
    geometry: GeometryGraph | None = None,
) -> list[RaisedPad]:
    """Shared rectangular-pad discovery with optional aggregate evidence issuance."""

    if face_surfaces is None:
        face_surfaces = (
            effective_faces_for_part(part)
            if writer is None
            else effective_faces_for_graph(writer.graph)
        )
    elif writer is not None and face_surfaces.run_token is not writer.graph.run_token:
        raise ValueError("Pad surface facts and evidence writer belong to different runs")
    if geometry is None:
        geometry = (
            GeometryGraph._from_graph(writer.graph) if writer is not None else GeometryGraph(part)
        )
    elif writer is not None and not geometry._uses_graph(writer.graph):
        raise ValueError("Pad geometry and evidence writer belong to different runs")

    solids = list(part.solids())
    sources = solids if len(solids) > 1 else [part]
    occurrences: list[tuple[RaisedPad, tuple[_PadProposal, ...]]] = []
    for solid in sources:
        by_record: dict[RaisedPad, list[_PadProposal]] = {}
        proposals: list[_PadProposal] = []
        blend_facts_cache: list[tuple[BlendFact, ...]] = []
        planar_faces = _planar_faces(solid, face_surfaces)
        material_side_cache: dict[int, SurfaceUse | SurfaceUseRefusal] = {}
        part_bounds = solid.bounding_box()
        for axis in ("z", "x", "y"):
            for axis_sign in (1, -1):
                proposals.extend(
                    _recognise_rectangular_pads_one(
                        solid,
                        tol=tol,
                        face_surfaces=face_surfaces,
                        axis=axis,
                        axis_sign=axis_sign,
                        planar_faces=planar_faces,
                        material_side_cache=material_side_cache,
                        part_bounds=part_bounds,
                    )
                )
                proposals.extend(
                    _recognise_blended_rectangular_pads_one(
                        solid,
                        tol=tol,
                        face_surfaces=face_surfaces,
                        geometry=geometry,
                        axis=axis,
                        axis_sign=axis_sign,
                        blend_facts_cache=blend_facts_cache,
                        planar_faces=planar_faces,
                        material_side_cache=material_side_cache,
                        part_bounds=part_bounds,
                    )
                )
        proposals = _resolve_axis_ambiguity(proposals, geometry, tol=_TOL if tol is None else tol)
        for proposal in proposals:
            by_record.setdefault(proposal.record, []).append(proposal)
        occurrences.extend((record, tuple(group)) for record, group in by_record.items())
    occurrences.sort(key=lambda item: item[0])
    records = [record for record, _group in occurrences]
    if writer is None:
        return records

    pending: list[tuple[RaisedPad, tuple[Any, ...], tuple[SurfaceUse, ...]]] = []
    used_tops: set[Any] = set()
    for record, alternatives in occurrences:
        identity_signatures: list[tuple[Any, ...]] = []
        for proposal in alternatives:
            top = writer.graph.require_node(proposal.top_face)
            roles: list[Any] = []
            for faces in proposal.wall_roles:
                resolved = {writer.graph.require_node(face) for face in faces}
                if len(resolved) != 1:
                    raise ValueError("a Pad wall role has ambiguous maximal-base faces")
                roles.append(next(iter(resolved)))
            signature = (top, *roles)
            if len(set(signature)) != 5:
                raise ValueError("a Pad requires five pairwise-distinct defining faces")
            identity_signatures.append(signature)
        distinct = set(identity_signatures)
        if len(distinct) != 1:
            raise ValueError("equal Pad values have ambiguous defining occurrences")
        signature = next(iter(distinct))
        node_set = frozenset(signature)
        if signature[0] in used_tops:
            raise ValueError("Pad occurrences share a defining top face")
        ordered = tuple(node for node in writer.graph.nodes if node in node_set)
        if writer.graph.common_valid_solid(ordered) is None:
            raise ValueError("Pad defining faces do not belong to one valid solid")
        used_tops.add(signature[0])
        selected = alternatives[0]
        selected_faces = (selected.top_face, *(faces[0] for faces in selected.wall_roles))
        issued_uses = tuple(
            face_surfaces.use(face, material_side=face is selected.top_face)
            for face in selected_faces
        )
        if any(isinstance(use, SurfaceUseRefusal) for use in issued_uses):
            raise ValueError("Pad surface provenance became unavailable before issuance")
        surface_by_node = {use.node: use for use in issued_uses if isinstance(use, SurfaceUse)}
        surface_uses = tuple(surface_by_node[node] for node in ordered)
        pending.append((record, ordered, surface_uses))
    for record, nodes, surface_uses in pending:
        writer.add_defining(
            record,
            nodes,
            family=FamilyId.PADS,
            surfaces=surface_uses,
        )
    return records
