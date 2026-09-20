# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""What a solid's faces say about recesses, before any family has an opinion.

The bottom of the recess stack. `_planar_faces` reduces a solid to the axis-aligned planar
faces the three recess families work from -- ADR 0009's worked example, and the reason that
record exists -- and `_Face` is the cached read each of them is wrapped in. Above that sit the
probes that ask a *candidate* what its ends and floor look like: whether a void is capped,
how many of its ends the floor reaches, which way it opens, whether it has side walls at all.

The coincidence bands live here too, with the reads they qualify rather than with the families
that consult them. `_MERGE_TOL` and `_FLOOR_TOL` are what "the same coordinate" and "the same
floor plane" mean across every module in this group, so putting them beside one caller would
have made the other three look like they had borrowed someone else's number.

Nothing here imports another recess module. That is what makes it the bottom, and it is
checked by the architecture tests rather than left as an intention.
"""

from __future__ import annotations

import math
from dataclasses import dataclass
from types import SimpleNamespace

from build123d import GeomType
from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.GeomAbs import GeomAbs_Cylinder

from quiddity._adjacency import FaceEdges, FaceGraph, FaceNode, frame_points_outward
from quiddity._recess_records import Slot
from quiddity._typing import Bounds, FaceLike, Part, Vector3

_AXES = {"x": 0, "y": 1, "z": 2}

_AXIS_ALIGNED_TOL = 1e-3

#
# 0.2.3 scaled these to the solid's largest extent on the reasoning that they have no smaller
# feature to measure against. That reference was too coarse: whether two pockets are one pocket
# depends on the pockets, not on the plate they sit in, so a large plate merged recesses that a
# small one kept. _MERGE_TOL also gates the minimum separation of two slot ends, so the same
# change simultaneously raised a threshold. Both directions lose records, which is why the
# downstream report that prompted this saw nineteen losses across six parts and no gains.
_MERGE_TOL = 0.5

_FLOOR_TOL = 0.3

_FLOOR_COVER_FRAC = 0.5


def _dominant_axis(nrm: Vector3) -> str | None:
    """Return the axis letter when ``nrm`` is axis-aligned, else None."""
    for axis, k in _AXES.items():
        if abs(abs(nrm[k]) - 1.0) <= _AXIS_ALIGNED_TOL:
            return axis
    return None


@dataclass(frozen=True)
class _Face:
    """A planar face reduced to the data the recogniser needs."""

    normal: Vector3
    #: The principal axis this face's normal aligns with, or None when it aligns with none.
    #: Total by ADR 0009: an oblique wall is carried with ``axis=None`` rather than dropped, so
    #: the family that cannot use it is the one seen to decline it.
    axis: str | None
    #: Typed rather than left as ``object``: every use reads ``.min``/``.max``, so the
    #: placeholder silently disabled checking on the field this module touches most.
    bb: Bounds
    wall: bool  # a valid slot wall: LINE/CIRCLE edges, at least one straight LINE
    #: The graph node for this face, when the caller asked for claims; None otherwise. The
    #: reduction above is what the recogniser needs to *decide*, and it deliberately drops the
    #: face -- so before this field there was no way to say which faces a slot was built from,
    #: and passage/slot reconciliation compared record coordinates instead.
    node: FaceNode | None = None


def _is_wall(face: FaceLike, face_edges: FaceEdges | None = None) -> bool:
    """True when *face* can be a slot wall: bounded only by straight (LINE) or circular-arc
    (CIRCLE) edges, with **at least one** straight edge and **at most one** arc. A fully
    rectangular wall qualifies (all LINE); a slot cut into round stock has a wall the OD clips
    into a *single* arc + a straight floor/chord, which now qualifies too.

    A turned groove / circlip recess is still rejected — its annular wall is a washer bounded
    by *two* concentric arcs (the outer OD + the inner floor circle), so the ``<= 1`` arc cap
    excludes it. That cap holds even when a keyway / flat / cross-hole notches a straight edge
    into the annulus, which is why the arc-count proof is stronger than an
    "all edges must be straight" test: the
    two concentric arcs survive, so a keyed groove never reads as a slot. A freeform
    (spline/ellipse) face is rejected outright."""
    edges = face_edges.of(face) if face_edges is not None else face.edges()
    types = [e.geom_type for e in edges]
    if not types or any(t not in (GeomType.LINE, GeomType.CIRCLE) for t in types):
        return False
    n_line = sum(1 for t in types if t == GeomType.LINE)
    n_circle = sum(1 for t in types if t == GeomType.CIRCLE)
    return n_line >= 1 and n_circle <= 1


def _planar_faces(
    part: Part, face_edges: FaceEdges | None = None, graph: FaceGraph | None = None
) -> list[_Face]:
    """Every planar face as an :class:`_Face` record (computed once).

    **A view over the graph, not a parallel computation.** Each face's outward normal is read
    from :meth:`FaceGraph.normal` rather than derived here, so there is one owner of that fact
    instead of two -- which turned out to be two spellings of the same thing rather than two
    conventions, though that took measuring to find out. `_Face` carried a node before this,
    but only
    as an identity tag for claiming -- every actual fact about the face it recomputed, which is
    how a graph ends up being used as a name tag.

    *graph* is now built when a caller does not supply one, rather than left absent. The graph is
    lazy, so a family that reads only normals pays only for normals, and sharing the caller's is
    still worth doing because the attributes are then computed once across every family in a
    census rather than once per family.

    A compound is scanned per solid while the graph covers the whole part, and the faces of a
    solid are the same shapes the part yields, so they resolve against it. A face that does not
    resolve is refused: it can only mean the caller paired a graph with a different part, and
    while no *wrong* claim would then be made, no claim would be made either -- leaving the
    caller unable to tell "this part has no claimable slots" from "you handed me the wrong
    graph". A reconciler reading that empty ledger concludes there is no overlap and reports
    the duplicate feature it exists to suppress. `ClaimLedger.claims_of` refuses a foreign node
    for the same reason rather than answering "no claims"; this is that check one layer up.
    """

    owner = FaceGraph(part, face_edges=face_edges) if graph is None else graph
    faces = []
    for face in part.faces():
        node = owner.require_node(face)
        # This function's declared domain is a planar face with a readable normal, and both
        # halves are asked here rather than one riding on the other. It used to rest entirely on
        # a normal reader that answered None for a curved face -- which worked, and broke
        # silently the moment a reader that answers for cylinders too was substituted.
        nrm = owner.normal(node) if owner.is_planar(node) else None
        if nrm is None:
            continue
        # `axis` is None for an oblique planar face, and the face is carried anyway. It used to
        # be dropped here, which made an axis-aligned-walls restriction that three families
        # inherit invisible to all three and impossible to count -- see ADR 0009. Each family
        # now declines it for itself, where the rejection can be named and measured.
        faces.append(
            _Face(
                nrm,
                _dominant_axis(nrm),
                face.bounding_box(),
                _is_wall(face, face_edges),
                # Also retained for a standalone recogniser's internal graph: candidate
                # predicates use adjacency and arc attributes to prove two walls bound one
                # void. Claims are still written only when a caller supplied a ledger.
                node,
            )
        )
    return faces


def _center(bb: Bounds, k: int) -> float:
    return float(getattr(bb.min, "XYZ"[k]) + getattr(bb.max, "XYZ"[k])) / 2


def _overlap_len(bb_a: Bounds, bb_b: Bounds, axis: str) -> float:
    """Length of the overlap of two bboxes along ``axis`` (0 if disjoint)."""
    c = "XYZ"[_AXES[axis]]
    lo = max(getattr(bb_a.min, c), getattr(bb_b.min, c))
    hi = min(getattr(bb_a.max, c), getattr(bb_b.max, c))
    return float(hi - lo)


def _end_cap_faces(
    faces: list[_Face],
    foot: dict[str, tuple[float, float]],
    foot_area: float,
    depth_axis: str,
    end: float,
    want: float,
) -> tuple[_Face, ...]:
    """Return exact inward faces when they cover one end of the required footprint.

    ``want`` is the sign the covering normal must point (+depth at the low end, -depth at the
    high end) so the cap faces *into* the cavity; the part's own outer face at that level
    faces the other way and is excluded (that is what separates a real floor from a
    through-open end). Coverage is *aggregated* over all qualifying faces (not tested per
    face), so a floor split by a rib/divider still counts. The sum (not union) is
    exact for the coplanar floor faces of a valid solid; overlaps only arise from
    interpenetrating solids (degenerate input)."""
    dk = _AXES[depth_axis]
    covered = 0.0
    selected: list[_Face] = []
    for f in faces:
        if f.axis != depth_axis or abs(_center(f.bb, dk) - end) > _FLOOR_TOL:
            continue
        if f.normal[dk] * want <= 0:
            continue
        area = 1.0
        for ax, (lo, hi) in foot.items():
            c = "XYZ"[_AXES[ax]]
            ov = min(getattr(f.bb.max, c), hi) - max(getattr(f.bb.min, c), lo)
            area *= max(ov, 0.0)
        covered += area
        if area > 0.0:
            selected.append(f)
    return tuple(selected) if covered >= _FLOOR_COVER_FRAC * foot_area else ()


def _end_capped(
    faces: list[_Face],
    foot: dict[str, tuple[float, float]],
    foot_area: float,
    depth_axis: str,
    end: float,
    want: float,
) -> bool:
    """Whether :func:`_end_cap_faces` proves one capped footprint end."""

    return bool(_end_cap_faces(faces, foot, foot_area, depth_axis, end, want))


def _floor_end_faces(faces: list[_Face], s: Slot) -> tuple[tuple[_Face, ...], tuple[_Face, ...]]:
    """Read both depth ends once and retain the exact accepted floor clusters."""

    foot = {
        s.width_axis: (s.w_center - s.width / 2, s.w_center + s.width / 2),
        s.long_axis: (s.lo, s.hi),
    }
    foot_area = math.prod(hi - lo for lo, hi in foot.values())
    return (
        _end_cap_faces(faces, foot, foot_area, s.depth_axis, s.d_lo, 1.0),
        _end_cap_faces(faces, foot, foot_area, s.depth_axis, s.d_hi, -1.0),
    )


def _floor_ends(faces: list[_Face], s: Slot) -> int:
    """How many of *s*'s two depth ends are capped by accepted planar floor clusters."""

    return sum(bool(end) for end in _floor_end_faces(faces, s))


def _has_floor(faces: list[_Face], s: Slot) -> bool:
    """True when a planar floor caps the slot at *either* depth end — i.e. it is not a through
    slot. The through/blind split for :func:`recognise_slots`'s flat-wall path; the obround end-cap
    path uses the finer :func:`_floor_ends` count (a pocket is capped on exactly one end)."""
    return _floor_ends(faces, s) >= 1


def _cylinder_faces(part: Part, graph: FaceGraph | None = None) -> list[tuple]:
    """``(radius, axis_letter, axis_location, bbox, concave, node)`` per cylinder patch.

    ``node`` is present when a caller supplies the run graph.  Keeping the exact patch identity
    here lets obround clustering retain every STEP-split quarter cylinder without a second scan.
    The public geometry-only path supplies no graph and receives ``None`` in that private field.

    Each entry describes an axis-aligned cylindrical
    face of *part* — the candidate obround end caps. ``axis_location`` is a point on the
    cylinder axis; ``bbox`` bounds the face (used to confirm the cap spans the slot's depth, not
    some unrelated cylinder at a different depth); ``concave`` is True when the face bounds a
    *void* (its material-outward normal points inward, toward the axis) — a recess wall — rather
    than added material (a boss/post). Asks `frame_points_outward`, which answers *which side*
    for a whole face without needing a point on it -- what a cylinder needs, since it has no one
    outward vector. A planar face wants `FaceGraph.normal`, which is already material-side."""
    out = []
    for face in part.faces():
        surf = BRepAdaptor_Surface(face.wrapped)
        if surf.GetType() != GeomAbs_Cylinder:
            continue
        cyl = surf.Cylinder()
        d = cyl.Axis().Direction()
        axis = _dominant_axis((d.X(), d.Y(), d.Z()))
        if axis is None:
            continue
        loc = cyl.Axis().Location()
        concave = not frame_points_outward(face)
        node = None if graph is None else graph.require_node(face)
        out.append(
            (
                cyl.Radius(),
                axis,
                (loc.X(), loc.Y(), loc.Z()),
                face.bounding_box(),
                concave,
                node,
            )
        )
    return out


def _has_side_walls(faces: list[_Face], s: Slot) -> bool:
    """True when the two flat side walls of obround slot *s* are present: **inward-facing** wall
    faces on the width axis at ``w_center - width/2`` (material-outward normal toward
    +width) and ``w_center + width/2`` (normal toward -width), each overlapping the
    straight run ``[s.lo, s.hi]``. Confirms a genuine channel connects the two end caps —
    rejects two independent D-cutouts whose caps merely alternate ``-1, +1`` across solid.
    The normal-direction test is essential: the stock's own OUTWARD-facing side faces sit
    at the same ``w_center ± width/2`` when the stock is exactly as wide as the slot, and
    would otherwise be mistaken for the channel walls.
    """
    wk, lk = _AXES[s.width_axis], _AXES[s.long_axis]
    lo_wall, hi_wall = False, False
    for f in faces:
        if not f.wall or f.axis != s.width_axis:
            continue
        lo, hi = getattr(f.bb.min, "XYZ"[lk]), getattr(f.bb.max, "XYZ"[lk])
        if min(hi, s.hi) - max(lo, s.lo) <= 0:  # wall does not span the straight run
            continue
        c = _center(f.bb, wk)
        # inward-facing: the low-side wall's outward normal points toward the centreline (+width),
        # the high-side wall's toward -width. The stock's exterior faces point the other way.
        if abs(c - (s.w_center - s.width / 2)) <= _MERGE_TOL and f.normal[wk] > 0:
            lo_wall = True
        if abs(c - (s.w_center + s.width / 2)) <= _MERGE_TOL and f.normal[wk] < 0:
            hi_wall = True
    return lo_wall and hi_wall


def _union_bb(a: Bounds | SimpleNamespace, b: Bounds) -> SimpleNamespace:
    """Axis-aligned union of two bounding boxes, as a ``min``/``max`` namespace matching the
    build123d ``BoundBox`` interface (``.min.X`` …) the cap helpers read."""
    mn = SimpleNamespace(X=min(a.min.X, b.min.X), Y=min(a.min.Y, b.min.Y), Z=min(a.min.Z, b.min.Z))
    mx = SimpleNamespace(X=max(a.max.X, b.max.X), Y=max(a.max.Y, b.max.Y), Z=max(a.max.Z, b.max.Z))
    return SimpleNamespace(min=mn, max=mx)
