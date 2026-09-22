# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Coaxial cylinder segments and the classification of their ends.

The layer holes and bosses both stand on. A bore and a raised cylinder are found by the same
walk -- merge the coaxial patches, then decide what each end of the stack meets -- and only the
decision about what the stack *is* differs, which is why that decision lives in each family
module and this does not know either of them.
"""

from typing import cast

from build123d import Face
from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.GeomAbs import (
    GeomAbs_BezierSurface,
    GeomAbs_BSplineSurface,
    GeomAbs_Cone,
    GeomAbs_Cylinder,
    GeomAbs_Plane,
    GeomAbs_Sphere,
    GeomAbs_Torus,
)

from quiddity._adjacency import (
    GraphRunToken,
    frame_points_outward,
)
from quiddity._claims import EvidenceWriter
from quiddity._cylinder_substrate import (
    _STACK_GAP_FRAC,
    _cyl_group_key,
    _merge_runs,
    full_cylinders,
)
from quiddity._effective_surfaces import (
    AnalyticSurfaceFact,
    EffectiveFaceSurfaceQuery,
    EffectiveSurfaceFact,
    SurfaceKind,
    SurfaceUse,
    SurfaceUseResult,
    effective_faces_for_graph,
    effective_faces_for_part,
)
from quiddity._geometry import dot, length_tol
from quiddity._typing import CylinderEvidence, FaceLike, Part


class SegmentEvidence(CylinderEvidence):
    """One coaxial cylinder segment: a :class:`CylinderEvidence` plus the patches it merged.

    Defined here rather than in ``_typing`` because it is internal — ``CylinderEvidence`` is
    published for downstream compatibility, this is not. It exists because ``_segments`` widens
    each cylinder record with the faces of every patch it absorbed, so annotating a segment
    ``CylinderEvidence`` would be *wrong* rather than merely imprecise: the extra key is what
    the end-classification walk reads.
    """

    faces: list[Face]


_full_cyls = full_cylinders


class _LazyPartSurfaceQuery:
    """Defer the standalone recovery graph until a spline face actually needs it."""

    def __init__(self, part: Part) -> None:
        self._part = part
        self._delegate: EffectiveFaceSurfaceQuery | None = None

    def _query(self) -> EffectiveFaceSurfaceQuery:
        if self._delegate is None:
            self._delegate = effective_faces_for_part(self._part)
        return self._delegate

    @property
    def run_token(self) -> GraphRunToken:
        return self._query().run_token

    def fact(self, face: FaceLike) -> EffectiveSurfaceFact:
        return self._query().fact(face)

    def use(self, face: FaceLike, *, material_side: bool = False) -> SurfaceUseResult:
        return self._query().use(face, material_side=material_side)


def _family_surface_query(
    part: Part,
    writer: EvidenceWriter | None,
    supplied: EffectiveFaceSurfaceQuery | None,
) -> EffectiveFaceSurfaceQuery | None:
    if supplied is not None:
        return supplied
    if writer is not None:
        return effective_faces_for_graph(writer.graph)
    return _LazyPartSurfaceQuery(part)


def _segments(cyls: list[CylinderEvidence]) -> list[SegmentEvidence]:
    """Collapse cylinder patches into segments: one per (axis line, diameter,
    contiguous axial range). Keyway-split patches of one bore merge; coaxial
    same-diameter holes from opposite faces stay separate."""
    # cast rather than a TypedDict literal: `dict(run[0], ...)` carries the ten keys of the
    # source record forwards, and respelling them here would be a second place to keep in step
    # with `CylinderEvidence`.
    return [
        cast(
            SegmentEvidence,
            dict(
                run[0],
                s_lo=min(p["s_lo"] for p in run),
                s_hi=max(p["s_hi"] for p in run),
                faces=[p["face"] for p in run],
            ),
        )
        for run in _merge_runs(cyls, _cyl_group_key)
    ]


def _axis_point(seg: SegmentEvidence, s: float) -> tuple[float, float, float]:
    """The 3D point on *seg*'s axis at axial coordinate *s*."""
    ax, ay, az = seg["axis_xyz"]
    dx, dy, dz = seg["dir_xyz"]
    s_ap = ax * dx + ay * dy + az * dz
    t = s - s_ap
    return (ax + t * dx, ay + t * dy, az + t * dz)


def _end_partners(
    seg: SegmentEvidence, s_end: float, edge_faces: dict, cache: dict | None = None
) -> list:
    """The faces beyond one axial end of *seg*: partners of edges that lie at
    that end. An opening edge on a slanted or curved surface dips away from
    the end plane (by the lip sagitta), so edges match within a margin — but
    stay well clear of the segment's other end.

    *cache* (optional) memoises the result per ``(seg, s_end)`` within one
    ``recognise_holes``/``recognise_bosses`` call — the same end is classified several
    times (``_merge_stacks`` plus the main loop), and each scan walks every
    face's edges. The seg is stored in the cached value so an ``is`` check
    rejects (and pins against) any ``id`` reuse."""
    if cache is not None:
        key = ("ep", id(seg), round(s_end, 9))
        hit = cache.get(key)
        if hit is not None and hit[0] is seg:
            # cast, not a copy: the cache exists to avoid re-walking every face's edges, and
            # rebuilding the list on each hit would undo that. The value's type is fixed by
            # where it is written, a few lines below.
            return cast(list, hit[1])
    dx, dy, dz = seg["dir_xyz"]
    margin = max(
        length_tol(seg["diameter"], rel=_STACK_GAP_FRAC),
        min(0.45 * (seg["s_hi"] - seg["s_lo"]), 0.5 * seg["diameter"]),
    )
    # Keep evidence nearest the requested end first. A curved or slanted
    # opening may legitimately dip away from the nominal end, which is why the
    # broad admission margin exists, but a distant edge must not outvote an
    # exact cap merely because OCCT happened to enumerate it first.
    ranked: list[tuple[float, int, Face]] = []
    order = 0
    for face in seg["faces"]:
        for edge in face.edges():
            pts = [edge.center()] + [v.center() for v in edge.vertices()]
            distance = max(abs(p.X * dx + p.Y * dy + p.Z * dz - s_end) for p in pts)
            if distance > margin:
                continue
            for partner in edge_faces.get(edge, ()):
                if not any(partner.is_same(f) for f in seg["faces"]):
                    previous = next(
                        (at for at, (_, _, found) in enumerate(ranked) if partner.is_same(found)),
                        None,
                    )
                    if previous is None:
                        ranked.append((distance, order, partner))
                        order += 1
                    elif distance < ranked[previous][0]:
                        ranked[previous] = (distance, ranked[previous][1], partner)
    partners = [partner for _, _, partner in sorted(ranked, key=lambda item: item[:2])]
    if cache is not None:
        cache[key] = (seg, partners)
    return partners


def _classify_end(
    seg: SegmentEvidence,
    s_end: float,
    hi_end: bool,
    edge_faces: dict,
    cache: dict | None = None,
    face_surfaces: EffectiveFaceSurfaceQuery | None = None,
    *,
    terminal_faces: list[Face] | None = None,
) -> str:
    """Cached wrapper over :func:`_classify_end_uncached` (see *cache* there)."""
    retained: list[Face] = []
    if cache is None:
        result = _classify_end_uncached(
            seg,
            s_end,
            hi_end,
            edge_faces,
            face_surfaces=face_surfaces,
            terminal_faces=retained,
        )
        if terminal_faces is not None:
            terminal_faces.extend(retained)
        return result
    key = ("ce", id(seg), round(s_end, 9), hi_end)
    hit = cache.get(key)
    if hit is not None and hit[0] is seg:
        if terminal_faces is not None:
            terminal_faces.extend(hit[2])
        return cast(str, hit[1])
    result = _classify_end_uncached(
        seg,
        s_end,
        hi_end,
        edge_faces,
        cache,
        face_surfaces=face_surfaces,
        terminal_faces=retained,
    )
    cached_faces = tuple(retained)
    cache[key] = (seg, result, cached_faces)
    if terminal_faces is not None:
        terminal_faces.extend(cached_faces)
    return result


def _classify_end_uncached(
    seg: SegmentEvidence,
    s_end: float,
    hi_end: bool,
    edge_faces: dict,
    cache: dict | None = None,
    face_surfaces: EffectiveFaceSurfaceQuery | None = None,
    *,
    terminal_faces: list[Face] | None = None,
) -> str:
    """Classify one axial end of a cylinder segment from the face beyond it.

    Returns ``"open"`` (the bore exits, or the boss's free end), ``"flat"``
    (closed by a plane facing back into the segment, or a boss's base),
    ``"drill_point"`` (a bore closed by a cone), or ``"unknown"``.

    Planes, cones, and tori are decisive; a curved wall (cylinder/sphere) is
    a weak signal — an exit for a bore, a base for a boss — that only counts
    when no decisive partner is present (a crossing port near a flat bottom
    must not outvote the bottom).

    An adjacent cone is read through the segment's internal/external context
    and its apex direction: for a bore, apex outward closes it (drill point)
    while apex inward widens it (an entry chamfer or countersink — open);
    for a boss the senses flip (apex outward is a chamfered free end, apex
    inward a base draft).  Tori follow the corner they round: one curling
    inward (major radius below the segment's) is a closed corner — a blind
    bore's bottom or a boss's base — and one flaring outward is an opening
    lip or a free end.
    """
    dx, dy, dz = seg["dir_xyz"]
    e_sign = 1.0 if hi_end else -1.0
    weak: tuple[str, tuple[Face, ...]] | None = None

    def classified(state: str, *faces: Face) -> str:
        if terminal_faces is not None:
            terminal_faces.extend(faces)
        return state

    for partner in _end_partners(seg, s_end, edge_faces, cache):
        surf = BRepAdaptor_Surface(partner.wrapped)
        kind = surf.GetType()
        if kind == GeomAbs_Cone:
            cone = surf.Cone()
            apex = cone.Apex()
            apex_s = apex.X() * dx + apex.Y() * dy + apex.Z() * dz
            outward = (apex_s - s_end) * e_sign > 0
            if not seg["external"]:
                if outward:
                    # A deburr chamfer on a flat floor's rim is also an
                    # apex-outward cone — closed either way, but it has the
                    # floor plane right next to it where a true drill point
                    # has nothing beyond its apex.
                    for e2 in partner.edges():
                        for n in edge_faces.get(e2, ()):
                            if n.is_same(partner) or any(n.is_same(f) for f in seg["faces"]):
                                continue
                            n_surf = BRepAdaptor_Surface(n.wrapped)
                            if n_surf.GetType() != GeomAbs_Plane:
                                continue
                            nv = n.normal_at(n.center())
                            if abs(nv.X * dx + nv.Y * dy + nv.Z * dz) > 0.9:
                                return classified("flat", n)
                    return classified("drill_point", partner)
                return "open"
            return "open" if outward else "flat"
        if kind == GeomAbs_Torus:
            curls_in = surf.Torus().MajorRadius() < seg["diameter"] / 2
            if not seg["external"]:
                return "flat" if curls_in else "open"
            return "open" if curls_in else "flat"
        if kind == GeomAbs_Plane:
            n = partner.normal_at(partner.center())
            normal = (n.X, n.Y, n.Z)
        elif kind in (GeomAbs_BSplineSurface, GeomAbs_BezierSurface) and face_surfaces is not None:
            fact = face_surfaces.fact(partner)
            if not isinstance(fact, AnalyticSurfaceFact) or fact.kind is not SurfaceKind.PLANE:
                continue
            plane_use = face_surfaces.use(partner, material_side=True)
            if not isinstance(plane_use, SurfaceUse) or plane_use.material_side is None:
                continue
            normal = plane_use.material_side.outward
        else:
            normal = None
        if normal is not None:
            alignment = dot(normal, (dx, dy, dz)) * e_sign
            if alignment < -0.5:
                return classified("flat", partner)
            if alignment > 0.5:
                return classified("open", partner)
        if kind == GeomAbs_Sphere:
            # Convex (material inside the sphere): the bore exits through a
            # spherical surface. Concave (a ball-nose cavity): a closed
            # bottom — reported as "flat" (no rounded-bottom category).
            convex = bool(frame_points_outward(partner))
            if not seg["external"]:
                weak = ("open" if convex else "flat", (partner,))
            else:
                weak = ("flat" if convex else "open", (partner,))
        elif kind == GeomAbs_Cylinder:
            weak = ("open" if not seg["external"] else "flat", ())
    return classified(weak[0], *weak[1]) if weak is not None else "unknown"
