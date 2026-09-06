# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Hole and boss records and recognition over the shared cylinder substrate."""

import math
from collections.abc import Sequence
from dataclasses import dataclass, replace
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
    FaceEdges,
    FaceNode,
    GraphRunToken,
    edge_face_map,
    frame_points_outward,
    neighbours,
)
from quiddity._candidates import CompletedOccurrence, FamilyId
from quiddity._claims import EvidenceWriter
from quiddity._cylinder_substrate import (
    _STACK_GAP_FRAC,
    _cyl_group_key,
    _line_key,
    _merge_runs,
    analyse_cylinders,
    full_cylinders,
)
from quiddity._effective_surfaces import (
    AnalyticSurfaceFact,
    EffectiveFaceSurfaceQuery,
    EffectiveSurfaceFact,
    SurfaceKind,
    SurfaceUse,
    SurfaceUseRefusal,
    SurfaceUseResult,
    cylinder_surface_dependency,
    effective_faces_for_graph,
    effective_faces_for_part,
)
from quiddity._geometry import _unit, length_tol, quantise
from quiddity._record import Record
from quiddity._typing import CylinderEvidence, CylinderInventory, FaceLike, Part, Vector3
from quiddity.countersinks import CounterSink, countersink_matches_hole


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
#: Two cylinder patches are the same diameter. NOT a machining allowance: `analyse_cylinders`
#: quantises every diameter to six significant figures, so this is an equality test on those
#: quantised values rather than a tolerance on a length. Named because a bare number here reads
#: as a machining allowance and would invite exactly that mistake.
#:
#: Relative, because the quantum it tests against is. It was 0.01 mm while the substrate rounded
#: to two decimals, and that pairing held only at millimetre scale: on a part modelled at a
#: twentieth, 0.01 mm is 8% of a 0.125 mm band, so patches of visibly different diameter
#: compared equal.
_SAME_DIAMETER_FRAC = 1e-4
#: Smallest diameter the proportional test will divide by; see `_same_diameter`.
_DIAMETER_FLOOR = 1e-9


def _dot(left: tuple[float, float, float], right: tuple[float, float, float]) -> float:
    return math.fsum(a * b for a, b in zip(left, right, strict=True))


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


def _same_diameter(a: float, b: float) -> bool:
    """Whether two quantised diameters are the same one, proportionally."""

    # The floor keeps a zero or near-zero diameter from making the test vacuous. It is well
    # below `quantise`'s own resolution at any size this package sees, so it never decides a
    # real comparison -- it only stops one from dividing the world by nothing.
    return abs(a - b) <= _SAME_DIAMETER_FRAC * max(abs(a), abs(b), _DIAMETER_FLOOR)


# A counterbore-like step shallower than this fraction of its diameter is a spotface.
_SPOTFACE_MAX_RATIO = 0.2


@dataclass(frozen=True)
class CounterBore(Record):
    """A cylindrical enlargement at a hole mouth: diameter and axial depth.

    The containing :class:`HoleRecord` distinguishes ``cbore`` from ``spotface``. Their
    geometry is otherwise identical, so classification uses the documented depth/diameter
    ratio: a shallow facing cut below ``_SPOTFACE_MAX_RATIO`` is a spotface; a deeper tool step
    is a counterbore. Diameter alone cannot establish that manufacturing distinction.
    """

    diameter: float
    depth: float


@dataclass(frozen=True)
class HoleRecord(Record):
    """A drilled hole: the bore plus optional counterbore/spotface steps.

    ``axis`` is the drilling direction (unit vector pointing from the opening
    into the hole) and ``location`` the axis point at the opening surface.
    ``diameter``/``depth`` describe the bore itself — the narrowest segment of
    the stack — with ``depth`` measured from the top of the bore to the hole's
    deep end (a bottom relief groove counts, a drill point's cone does not).
    ``bottom`` is ``"through"``, ``"flat"``, ``"drill_point"``, or
    ``"unknown"`` when the adjacent geometry matches none of those.
    """

    axis: Vector3
    location: Vector3
    diameter: float
    depth: float
    bottom: str
    cbore: CounterBore | None = None
    spotface: CounterBore | None = None
    # A countersink (flat-head seat) coaxial with the bore, or None. Composed
    # from the standalone :func:`recognise_countersinks` so the hole's spec/grouping and
    # the callout-width estimate all see it.
    csink: CounterSink | None = None


@dataclass(frozen=True, slots=True)
class _NearSideSelection:
    """Serialized near-side steps and the cylinder patches that establish them."""

    cbore: CounterBore | None
    spotface: CounterBore | None
    faces: tuple[Face, ...]


@dataclass(frozen=True, slots=True)
class _DepthSelection:
    """Serialized bore depth and the bore/deep-extension patches that establish it."""

    depth: float
    faces: tuple[Face, ...]


@dataclass(frozen=True, slots=True)
class _HoleProposal:
    """One exact Hole occurrence with its original cylindrical provenance."""

    record: HoleRecord
    cylindrical_faces: tuple[Face, ...]
    terminal_faces: tuple[Face, ...]
    matching_csinks: tuple[CounterSink, ...]


@dataclass(frozen=True)
class BossRecord(Record):
    """An external cylindrical boss (including a turned part's OD).

    ``axis`` points from the base toward the free end, ``location`` is the
    axis point at the free end, and ``height`` the axial extent.
    """

    axis: Vector3
    location: Vector3
    diameter: float
    height: float


@dataclass(frozen=True, slots=True)
class _BossProposal:
    record: BossRecord
    segment_faces: tuple[Face, ...]
    terminal_faces: tuple[Face, ...]


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


def _canonical_hole_axis_point(seg: SegmentEvidence, s: float) -> tuple[float, float, float]:
    """Remove the final kernel-noise digit from a reconstructed Hole location."""

    measured = _axis_point(seg, s)
    # Axis reconstruction combines independently measured anchors, directions and bounds. Eleven
    # significant figures removes their last kernel-noise digit while remaining far tighter than
    # the public record serializer and all length predicates.
    return tuple(quantise(value, figures=11) for value in measured)  # type: ignore[return-value]


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
    partners = []
    for face in seg["faces"]:
        for edge in face.edges():
            pts = [edge.center()] + [v.center() for v in edge.vertices()]
            if not all(abs(p.X * dx + p.Y * dy + p.Z * dz - s_end) <= margin for p in pts):
                continue
            for partner in edge_faces.get(edge, ()):
                if not any(partner.is_same(f) for f in seg["faces"]):
                    partners.append(partner)
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
            dot = _dot(normal, (dx, dy, dz)) * e_sign
            if dot < -0.5:
                return classified("flat", partner)
            if dot > 0.5:
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


def _shared_transition(
    a: SegmentEvidence, b: SegmentEvidence, edge_faces: dict, cache: dict | None = None
) -> bool:
    """True when a cone or torus face spans the gap between segment *a*'s
    high end and segment *b*'s low end — the shoulder chamfer or fillet that
    makes the two segments steps of one hole. The transition face touches
    one segment directly and may reach the other through the shoulder ring
    plane, so one adjacency hop is followed. Solid material between two
    unrelated coaxial features has no such connecting face."""
    a_partners = _end_partners(a, a["s_hi"], edge_faces, cache)
    b_partners = _end_partners(b, b["s_lo"], edge_faces, cache)
    for own, other in ((a_partners, b_partners), (b_partners, a_partners)):
        for t in own:
            if BRepAdaptor_Surface(t.wrapped).GetType() not in (GeomAbs_Cone, GeomAbs_Torus):
                continue
            if any(t.is_same(o) for o in other):
                return True
            if any(n.is_same(o) for n in neighbours(t, edge_faces) for o in other):
                return True
    return False


def _merge_stacks(
    stacks: list[list[SegmentEvidence]],
    edge_faces: dict,
    cache: dict | None = None,
    face_surfaces: EffectiveFaceSurfaceQuery | None = None,
) -> list[list[SegmentEvidence]]:
    """Recombine coaxial stacks that are one hole:

    - same bore diameter on both sides of a crossing void, neither facing
      end closed (a flat bottom or drill point means genuinely separate
      holes, e.g. blind holes drilled from opposite faces);
    - different diameters whose gap is bridged by a shoulder chamfer or
      fillet face (the steps of a counterbored hole with a deburred
      shoulder).
    """
    by_line: dict[tuple, list[list[SegmentEvidence]]] = {}
    for stack in stacks:
        by_line.setdefault(_line_key(stack[0]), []).append(stack)
    merged: list[list[SegmentEvidence]] = []
    for line_stacks in by_line.values():
        line_stacks.sort(key=lambda st: min(s["s_lo"] for s in st))
        cur = line_stacks[0]
        for nxt in line_stacks[1:]:
            a = max(cur, key=lambda s: s["s_hi"])
            b = min(nxt, key=lambda s: s["s_lo"])
            closed = ("flat", "drill_point")
            if (
                _same_diameter(a["diameter"], b["diameter"])
                and _classify_end(a, a["s_hi"], True, edge_faces, cache, face_surfaces)
                not in closed
                and _classify_end(b, b["s_lo"], False, edge_faces, cache, face_surfaces)
                not in closed
            ):
                joined = cast(
                    SegmentEvidence,
                    dict(a, s_hi=b["s_hi"], faces=a["faces"] + b["faces"]),
                )
                cur = [s for s in cur if s is not a] + [joined] + [s for s in nxt if s is not b]
            elif b["s_lo"] - a["s_hi"] <= length_tol(
                max(a["diameter"], b["diameter"]), rel=_STACK_GAP_FRAC
            ) + abs(a["diameter"] - b["diameter"]) and _shared_transition(a, b, edge_faces, cache):
                cur = cur + nxt
            else:
                merged.append(cur)
                cur = nxt
        merged.append(cur)
    return merged


def _csink_for_hole(h: HoleRecord, csinks: Sequence[CounterSink]) -> CounterSink | None:
    """The countersink seated on hole *h*, or None. A countersink belongs to the
    hole when its **minor circle** — where the cone meets the drilled bore — sits coaxially
    at one of the hole's bore *ends*: the opening (``s`` ≈ 0) or, for a through hole (open
    at both faces), the far end (``s`` ≈ depth). Keying on the minor-at-a-bore-end (not the
    opening face, nor the axis direction) makes association independent of which end
    ``recognise_holes`` happened to call the opening, and still excludes a *separate*
    coaxial hole on the opposite face — whose own bore ends are elsewhere."""
    for cs in csinks:
        if countersink_matches_hole(cs, h):
            return cs
    return None


def _drilled_from(
    stack: list[SegmentEvidence],
    edge_faces: dict,
    cache: dict,
    face_surfaces: EffectiveFaceSurfaceQuery | None = None,
    *,
    bottom_faces: list[Face] | None = None,
) -> tuple[bool, SegmentEvidence, float, str]:
    """Which end of a coaxial stack is the opening, and what closes the other.

    Returns ``(from_hi, opening_seg, opening_s, bottom)``. The opening is the open end; when
    both ends are open — a through hole — the wider segment's end wins, because counterbores sit
    at the opening, and a tie falls to the high-coordinate end on the convention that a part is
    drilled from the top.

    Getting this backwards would not fail loudly: the hole would still be reported, with its
    counterbore read as a far-side step and its depth measured from the wrong face.
    """

    lo_seg = min(stack, key=lambda s: s["s_lo"])
    hi_seg = max(stack, key=lambda s: s["s_hi"])
    lo_faces: list[Face] = []
    hi_faces: list[Face] = []
    lo_state = _classify_end(
        lo_seg,
        lo_seg["s_lo"],
        False,
        edge_faces,
        cache,
        face_surfaces,
        terminal_faces=lo_faces,
    )
    hi_state = _classify_end(
        hi_seg,
        hi_seg["s_hi"],
        True,
        edge_faces,
        cache,
        face_surfaces,
        terminal_faces=hi_faces,
    )

    if lo_state == "open" and hi_state != "open":
        from_hi = False
    elif hi_state == "open" and lo_state != "open":
        from_hi = True
    else:
        from_hi = hi_seg["diameter"] >= lo_seg["diameter"]

    opening_seg, opening_s = (hi_seg, hi_seg["s_hi"]) if from_hi else (lo_seg, lo_seg["s_lo"])
    bottom_state = lo_state if from_hi else hi_state
    if bottom_faces is not None and bottom_state in ("flat", "drill_point"):
        bottom_faces.extend(lo_faces if from_hi else hi_faces)
    return from_hi, opening_seg, opening_s, {"open": "through"}.get(bottom_state, bottom_state)


def _near_side_steps(steps: list[SegmentEvidence]) -> _NearSideSelection:
    """Classify the segments between the opening and the bore as counterbore and spotface.

    *steps* is ordered from the opening inward. Diameters must narrow monotonically: a segment
    wider than one already seen is a groove — an O-ring gland inside a counterbore — rather than
    a step, and is skipped. Lands of one step are unioned so a groove between them does not
    split the step into two shallower ones.

    Depth relative to diameter then separates the two: a shallow step is a spotface, a deep one
    a counterbore. The first of each kind wins, which is the one nearest the opening.
    """

    spans: dict = {}
    contributors: dict[float, list[SegmentEvidence]] = {}
    step_order = []
    min_d = math.inf
    for step in steps:
        if step["diameter"] > min_d and not _same_diameter(step["diameter"], min_d):
            continue
        min_d = step["diameter"]
        key = quantise(step["diameter"], figures=4)
        if key not in spans:
            spans[key] = [step["s_lo"], step["s_hi"]]
            contributors[key] = [step]
            step_order.append(key)
        else:
            spans[key][0] = min(spans[key][0], step["s_lo"])
            spans[key][1] = max(spans[key][1], step["s_hi"])
            contributors[key].append(step)

    cbore = spotface = None
    selected_keys: list[float] = []
    for key in step_order:
        lo, hi = spans[key]
        spec = CounterBore(key, round(hi - lo, 2))
        if spec.depth < _SPOTFACE_MAX_RATIO * spec.diameter:
            if spotface is None:
                spotface = spec
                selected_keys.append(key)
        else:
            if cbore is None:
                cbore = spec
                selected_keys.append(key)
    faces = tuple(
        face for key in selected_keys for segment in contributors[key] for face in segment["faces"]
    )
    return _NearSideSelection(cbore, spotface, faces)


def _bore_depth(
    stack: list[SegmentEvidence], bore: SegmentEvidence, *, bottom: str, from_hi: bool
) -> _DepthSelection:
    """Depth from the top of the bore to the hole's deep end.

    The two ends are measured against different segment sets on purpose. The near end is the
    bore's own top, so a counterbore above it is excluded. The deep end is the whole stack for a
    blind hole — a bottom relief groove is part of the depth — but only the bore segments for a
    through hole, where a far-side counterbore is a separate feature and must not extend it.
    """

    bore_segs = [s for s in stack if _same_diameter(s["diameter"], bore["diameter"])]
    deep_segs = bore_segs if bottom == "through" else stack
    # float() rather than a cast: the segment dicts are untyped, so the arithmetic is Any and
    # the annotation would be a claim rather than a guarantee.
    defining = list(bore_segs)
    if from_hi:
        deep_end = min(s["s_lo"] for s in deep_segs)
        depth = float(max(s["s_hi"] for s in bore_segs)) - float(deep_end)
        if bottom != "through":
            defining.extend(s for s in deep_segs if s["s_lo"] == deep_end)
    else:
        deep_end = max(s["s_hi"] for s in deep_segs)
        depth = float(deep_end) - float(min(s["s_lo"] for s in bore_segs))
        if bottom != "through":
            defining.extend(s for s in deep_segs if s["s_hi"] == deep_end)
    return _DepthSelection(
        depth,
        tuple(face for segment in defining for face in segment["faces"]),
    )


def recognise_holes(
    part: Part,
    *,
    cyls: CylinderInventory | None = None,
    csinks: Sequence[CounterSink] | None = None,
    face_edges: FaceEdges | None = None,
) -> list[HoleRecord]:
    """Recognise drilled holes on *part* (see :class:`HoleRecord`).

    Coaxial internal cylinders are grouped into stacks — drill + optional
    counterbore + optional spotface become one hole, and a bore interrupted
    by a crossing hole is recombined.  The bottom is classified by probing
    the face adjacent to the deep end.  Countersinks are not recognised as
    steps (the cone is treated as an opening); steps on the far side of the
    bore (e.g. a second counterbore from the back face) are not reported.

    Pass *cyls* — a precomputed ``analyse_cylinders(part)`` result — to avoid
    re-scanning the solid (mirrors ``lint_feature_coverage``'s parameter).

    Pass *csinks* — a precomputed ``recognise_countersinks(part)`` result — to
    compose each countersink onto the hole it flares. Per the package ADR 0002
    contract this recogniser does **not** recognise countersinks itself; the
    caller owns the single inventory and injects it.
    With ``csinks=None`` the holes come back without countersink attribution.
    """
    return _discover_holes(part, cyls=cyls, csinks=csinks, face_edges=face_edges)


def _discover_holes(
    part: Part,
    *,
    cyls: CylinderInventory | None = None,
    csinks: Sequence[CounterSink] | None = None,
    face_edges: FaceEdges | None = None,
    writer: EvidenceWriter | None = None,
    predecessor_occurrences: Sequence[CompletedOccurrence] = (),
    face_surfaces: EffectiveFaceSurfaceQuery | None = None,
) -> list[HoleRecord]:
    """Discover Holes and stage exact cylindrical/predecessor evidence atomically."""

    effective = _family_surface_query(part, writer, face_surfaces)
    z_cyls, cross_cyls = (
        cyls if cyls is not None else analyse_cylinders(part, face_surfaces=effective)
    )
    internal = [c for c in _full_cyls(z_cyls) + _full_cyls(cross_cyls) if not c["external"]]
    if not internal:
        return []
    edge_faces = edge_face_map(part.faces(), face_edges=face_edges)
    # one end-classification cache for the whole call: the same (seg, end) is
    # classified by _merge_stacks and again in the loop below, each scan walking
    # every face's edges.
    cache: dict = {}
    stacks = _merge_stacks(
        _merge_runs(_segments(internal), _line_key),
        edge_faces,
        cache,
        effective,
    )

    proposals: list[_HoleProposal] = []
    for stack in stacks:
        d = stack[0]["dir_xyz"]
        terminal_faces: list[Face] = []
        from_hi, opening_seg, opening_s, bottom = _drilled_from(
            stack,
            edge_faces,
            cache,
            effective,
            bottom_faces=terminal_faces,
        )

        # Order segments from the opening inward; the bore is the narrowest
        # (not the farthest — a through hole counterbored from both sides has
        # a step beyond the bore) and only steps on the opening side count.
        ordered = sorted(stack, key=lambda s: s["s_hi"], reverse=from_hi)
        bore_i = min(range(len(ordered)), key=lambda i: ordered[i]["diameter"])
        bore = ordered[bore_i]

        # Steps narrow monotonically from the opening to the bore; a wider
        # segment between same-diameter lands is a groove (e.g. an O-ring
        # gland inside a counterbore), not a step. Lands of one step span
        # their groove.
        near = _near_side_steps(ordered[:bore_i])

        # The bore's depth runs from its top to the hole's deep end: bore
        # lands span a mid-bore groove, and a blind hole's depth includes a
        # bottom relief groove — but not a through hole's far-side steps.
        depth = _bore_depth(stack, bore, bottom=bottom, from_hi=from_hi)
        record = HoleRecord(
            axis=_unit(tuple(-c for c in d) if from_hi else d),
            location=_canonical_hole_axis_point(opening_seg, opening_s),
            diameter=bore["diameter"],
            depth=round(depth.depth, 2),
            bottom=bottom,
            cbore=near.cbore,
            spotface=near.spotface,
        )
        proposals.append(_HoleProposal(record, depth.faces + near.faces, tuple(terminal_faces), ()))
    # Compose the injected countersinks: a coaxial cone flaring from the bore is
    # a hole attribute (like a counterbore), so it rides on the HoleRecord — HoleSpec
    # grouping and the callout-width estimate then see it for free. The caller injects the
    # inventory (package ADR 0002 — no sibling re-recognition here).
    if csinks:
        composed: list[_HoleProposal] = []
        for proposal in proposals:
            matches = tuple(cs for cs in csinks if countersink_matches_hole(cs, proposal.record))
            record = replace(proposal.record, csink=matches[0]) if matches else proposal.record
            composed.append(replace(proposal, record=record, matching_csinks=matches))
        proposals = composed

    if writer is not None:
        assert effective is not None
        occurrences_by_record: dict[int, list[CompletedOccurrence]] = {}
        for occurrence in predecessor_occurrences:
            predecessor_record = occurrence.record(CounterSink)
            occurrences_by_record.setdefault(id(predecessor_record), []).append(occurrence)

        pending: list[tuple[HoleRecord, tuple[FaceNode, ...], tuple[FaceNode, ...]]] = []
        used_nodes: set[FaceNode] = set()
        used_predecessors: set[int] = set()
        for proposal in proposals:
            resolved = {writer.graph.require_node(face) for face in proposal.cylindrical_faces}
            nodes = tuple(node for node in writer.graph.nodes if node in resolved)
            if not nodes:
                raise ValueError("Hole cylindrical evidence does not prove one valid solid")
            if used_nodes & resolved:
                raise ValueError("Hole occurrences share defining cylindrical faces")
            terminal_resolved = {
                writer.graph.require_node(face) for face in proposal.terminal_faces
            }
            if resolved & terminal_resolved:
                raise ValueError("Hole terminal identity aliases cylindrical evidence")
            terminal_nodes = tuple(node for node in writer.graph.nodes if node in terminal_resolved)
            members = (*nodes, *terminal_nodes)
            solid = writer.graph.common_valid_solid(members)
            if solid is None:
                raise ValueError("Hole cylindrical evidence does not prove one valid solid")

            if proposal.matching_csinks:
                selected = proposal.matching_csinks[0]
                if sum(item is selected for item in proposal.matching_csinks) != 1:
                    raise ValueError("Hole has ambiguous matching CounterSink occurrences")
                predecessor_matches = occurrences_by_record.get(id(selected), ())
                if (
                    len(predecessor_matches) != 1
                    or predecessor_matches[0].record(CounterSink) is not selected
                ):
                    raise ValueError("Hole CounterSink predecessor identity is unavailable")
                occurrence = predecessor_matches[0]
                if id(occurrence) in used_predecessors:
                    raise ValueError("CounterSink predecessor is shared by Hole occurrences")
                if occurrence.solid() != solid:
                    raise ValueError("Hole and CounterSink predecessors belong to different solids")
                used_predecessors.add(id(occurrence))

            used_nodes.update(resolved)
            pending.append((proposal.record, nodes, members))

        issued_pending: list[
            tuple[HoleRecord, tuple[FaceNode, ...], tuple[FaceNode, ...], tuple[SurfaceUse, ...]]
        ] = []
        for record, nodes, members in pending:
            issued = tuple(
                cylinder_surface_dependency(effective, writer.graph.face(node)) for node in nodes
            )
            if any(isinstance(use, SurfaceUseRefusal) for use in issued):
                raise ValueError("Hole cylinder provenance is unavailable")
            uses = tuple(use for use in issued if isinstance(use, SurfaceUse))
            issued_pending.append((record, nodes, members, uses))

        for record, nodes, members, uses in issued_pending:
            writer.add_defining(
                record,
                nodes,
                family=FamilyId.HOLES,
                constituent=members,
                surfaces=uses,
            )

    return [proposal.record for proposal in proposals]


def recognise_bosses(
    part: Part, *, cyls: CylinderInventory | None = None, face_edges: FaceEdges | None = None
) -> list[BossRecord]:
    """Recognise external cylindrical bosses on *part* (one
    :class:`BossRecord` per coaxial external cylinder segment, including a
    turned part's OD — callers wanting only local bosses can filter on
    diameter against the part envelope).

    Pass *cyls* — a precomputed ``analyse_cylinders(part)`` result — to avoid
    re-scanning the solid (mirrors ``recognise_holes``'s parameter, so a caller
    computing both holes and bosses can share one analysis).
    """
    return _discover_bosses(part, cyls=cyls, face_edges=face_edges)


def _discover_bosses(
    part: Part,
    *,
    cyls: CylinderInventory | None = None,
    face_edges: FaceEdges | None = None,
    writer: EvidenceWriter | None = None,
    face_surfaces: EffectiveFaceSurfaceQuery | None = None,
) -> list[BossRecord]:
    """Discover Bosses and validate every defining segment before publication."""

    effective = _family_surface_query(part, writer, face_surfaces)
    z_cyls, cross_cyls = (
        cyls if cyls is not None else analyse_cylinders(part, face_surfaces=effective)
    )
    external = [c for c in _full_cyls(z_cyls) + _full_cyls(cross_cyls) if c["external"]]
    if not external:
        return []
    edge_faces = edge_face_map(part.faces(), face_edges=face_edges)
    cache: dict = {}

    proposals: list[_BossProposal] = []
    for seg in _segments(external):
        d = seg["dir_xyz"]
        lo_faces: list[Face] = []
        hi_faces: list[Face] = []
        lo_state = _classify_end(
            seg,
            seg["s_lo"],
            False,
            edge_faces,
            cache,
            effective,
            terminal_faces=lo_faces,
        )
        hi_state = _classify_end(
            seg,
            seg["s_hi"],
            True,
            edge_faces,
            cache,
            effective,
            terminal_faces=hi_faces,
        )
        # The free end is the open one (its cap faces away from the segment);
        # default to the high end when both or neither are open.
        from_hi = not (lo_state == "open" and hi_state != "open")
        proposals.append(
            _BossProposal(
                BossRecord(
                    axis=_unit(d if from_hi else tuple(-c for c in d)),
                    location=_axis_point(seg, seg["s_hi"] if from_hi else seg["s_lo"]),
                    diameter=seg["diameter"],
                    height=round(seg["s_hi"] - seg["s_lo"], 2),
                ),
                tuple(seg["faces"]),
                tuple(hi_faces if from_hi and hi_state == "open" else lo_faces)
                if (hi_state if from_hi else lo_state) == "open"
                else (),
            )
        )

    if writer is not None:
        assert effective is not None
        pending: list[tuple[BossRecord, tuple[FaceNode, ...], tuple[FaceNode, ...]]] = []
        for proposal in proposals:
            resolved = {writer.graph.require_node(face) for face in proposal.segment_faces}
            nodes = tuple(node for node in writer.graph.nodes if node in resolved)
            if not nodes:
                raise ValueError("Boss defining faces do not prove one valid solid")
            terminal_resolved = {
                writer.graph.require_node(face) for face in proposal.terminal_faces
            }
            if resolved & terminal_resolved:
                raise ValueError("Boss terminal identity aliases cylindrical evidence")
            terminal_nodes = tuple(node for node in writer.graph.nodes if node in terminal_resolved)
            members = (*nodes, *terminal_nodes)
            solid = writer.graph.common_valid_solid(members)
            if solid is None:
                raise ValueError("Boss defining faces do not prove one valid solid")
            pending.append((proposal.record, nodes, members))
        issued_pending: list[
            tuple[BossRecord, tuple[FaceNode, ...], tuple[FaceNode, ...], tuple[SurfaceUse, ...]]
        ] = []
        for record, nodes, members in pending:
            issued = tuple(
                cylinder_surface_dependency(effective, writer.graph.face(node)) for node in nodes
            )
            if any(isinstance(use, SurfaceUseRefusal) for use in issued):
                raise ValueError("Boss cylinder provenance is unavailable")
            uses = tuple(use for use in issued if isinstance(use, SurfaceUse))
            issued_pending.append((record, nodes, members, uses))
        for record, nodes, members, uses in issued_pending:
            writer.add_defining(
                record,
                nodes,
                family=FamilyId.BOSSES,
                constituent=members,
                surfaces=uses,
            )

    return [proposal.record for proposal in proposals]


def feature_diameters(
    part: Part,
    cyls: CylinderInventory | None = None,
    holes: Sequence[HoleRecord] | None = None,
    bosses: Sequence[BossRecord] | None = None,
) -> list[float]:
    """Sorted unique diameters of the *recognised* dimensionable cylindrical
    features on *part*: every hole bore, each hole's counterbore/spotface step,
    and every boss.

    This is the inventory to use for coverage checks ("is each dimensionable
    diameter called out?"). It is deliberately built from
    :func:`recognise_holes` / :func:`recognise_bosses`, not the raw :func:`full_cylinders`
    patch list, so partial cylinders that never become a real feature — slot ends and
    interrupted recesses (an exact half-cylinder pair sums to a full turn and
    fools an angle-only test, but is not a bore) — are excluded, while genuine
    counterbore/spotface steps are kept.

    Pass *cyls* — a precomputed ``analyse_cylinders(part)`` result — to share one
    scan between ``recognise_holes`` and ``recognise_bosses``. Pass *holes* — a precomputed
    ``recognise_holes`` result — to reuse the single feature inventory instead of
    re-detecting (the single-inventory rule).
    """
    cyls = analyse_cylinders(part) if cyls is None else cyls
    if holes is None:
        holes = recognise_holes(part, cyls=cyls)
    diams: list[float] = []
    for h in holes:
        diams.append(h.diameter)
        if h.cbore is not None:
            diams.append(h.cbore.diameter)
        if h.spotface is not None:
            diams.append(h.spotface.diameter)
    for b in recognise_bosses(part, cyls=cyls) if bosses is None else bosses:
        diams.append(b.diameter)
    return sorted(set(diams))


# Preserve the historical implementation-module path used by repr/pickle tooling.
for _record_type in (CounterBore, HoleRecord, BossRecord):
    _record_type.__module__ = "quiddity._features"
