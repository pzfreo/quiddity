# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""What a slot, a pocket and a channel each are, given the faces and the reductions.

The three families' own logic, and the top of the recess stack. Each `_recognise_*_one`
function is one solid's worth of one family: read the faces
(:mod:`quiddity._recess_faces`), propose candidates from wall pairs, recover the ones
whose walls do not pair (:mod:`quiddity._recess_obround`), and reduce what is left to
features (:mod:`quiddity._recess_reduce`). :mod:`quiddity._recess_features`
wraps them in the public recognisers.

The candidate predicates are here rather than below because they are where the three families
actually differ. `_candidate` is a through-void between two walls; `_floored_candidate` is the
same read with a floor found, which is what makes a pocket a pocket and a channel a channel;
`_recognise_corner_notches` is the case none of them covers, a void open on two adjacent sides
with only one wall to find it by.

This module was 1,200 lines carrying all four responsibilities. The split is by responsibility
rather than by family, because the families share almost everything below the predicates and
share nothing above them.
"""

from __future__ import annotations

import math
from collections import Counter
from dataclasses import dataclass, replace
from typing import cast

from quiddity._adjacency import (
    FaceEdges,
    FaceGraph,
    FaceNode,
    is_any_smooth,
    is_opposed_nonsmooth,
    same_arc_kind,
)
from quiddity._geometry import COORD_FLOOR
from quiddity._recess_faces import (
    _AXES,
    _FLOOR_TOL,
    _MERGE_TOL,
    _center,
    _end_cap_faces,
    _Face,
    _has_floor,
    _overlap_len,
    _planar_faces,
)
from quiddity._recess_obround import (
    _extend_obround_proposals,
    _recognise_obround_from_ends,
)
from quiddity._recess_records import Channel, Pocket, Slot
from quiddity._recess_reduce import (
    _Claims,
    _collapse_collinear_proposals,
    _merge_proposals,
    _prism_is_empty,
    _RecessProposal,
    _region_center,
)
from quiddity._typing import Part
from quiddity._wire_seed import wire_seed as _inner_wire_seed

_LENGTH_TIE_FRAC = 0.05

_SLOT_MAX_SPAN_FRAC = 0.9


def _concave_boundary_regions(wall: FaceNode, graph: FaceGraph) -> set[frozenset[FaceNode]]:
    """Distinct closing regions reached concavely from one proposed wall.

    One physical end or floor may arrive from STEP as several tangent face patches. A gAAG
    collapses those patches into one region; :meth:`FaceGraph.smooth_region` asks the same
    question without mutating this graph. Convex arcs are excluded because they lead onto outer
    stock or added material, neither of which closes the removed region.
    """

    return {
        graph.smooth_region(node)
        for node in graph.neighbours(wall)
        if graph.arc(wall, node) == "concave"
    }


def _shared_concave_boundaries(fa: _Face, fb: _Face, graph: FaceGraph) -> set[frozenset[FaceNode]]:
    if fa.node is None or fb.node is None:
        raise ValueError("recess walls require graph nodes")
    return _concave_boundary_regions(fa.node, graph) & _concave_boundary_regions(fb.node, graph)


def _uninterrupted_long_span(
    long_axis: str,
    long_span: tuple[float, float],
    fa: _Face,
    fb: _Face,
    graph: FaceGraph,
) -> tuple[float, float] | None:
    """The record span after graph-proved curved interruptions at its ends.

    A coaxial post can replace a slot's planar end entirely. It is common to both walls, curved,
    and the material turns oppositely at its two contacts; that is explicit AAG evidence of added
    material interrupting the end rather than of the void ending there. Remove exactly its
    bounding interval from the occupancy probe. Interior material is not trimmed: the simple
    rectangular record has no way to represent it.
    """

    if fa.node is None or fb.node is None:
        raise ValueError("recess walls require graph nodes")
    lo, hi = long_span
    common = set(graph.neighbours(fa.node)) & set(graph.neighbours(fb.node))
    for node in common:
        if graph.is_planar(node):
            continue
        if is_opposed_nonsmooth(graph.arc(fa.node, node), graph.arc(fb.node, node)):
            bounds = graph.bounds(node)[_AXES[long_axis]]
            if bounds[0] <= lo + COORD_FLOOR:
                lo = max(lo, bounds[1])
            if bounds[1] >= hi - COORD_FLOOR:
                hi = min(hi, bounds[0])
    return (lo, hi) if hi - lo > COORD_FLOOR else None


def _candidate_has_void_evidence(
    spans: dict[str, tuple[float, float]],
    long_axis: str,
    fa: _Face,
    fb: _Face,
    graph: FaceGraph,
    part: Part,
) -> bool:
    """Whether the record's uninterrupted, unrounded rectangular prism is empty.

    A complete outer AAG boundary still does not prove this record: a ring-shaped removal around
    a large rectangular island has two shared concave ends but is not a simple rectangular slot.
    The graph instead proves only which curved end interruptions are added material that may be
    trimmed from the claimed run. Everything left in the record's prism must be void.

    Candidate existence has no volume allowance: even a thin continuous membrane keeps two
    recesses distinct. ``COORD_FLOOR`` is only a kernel-coordinate inset which avoids asking an
    OCCT Boolean about coincident boundary faces; any barrier thicker than twice that supported
    numerical floor remains inside the probe. The separate collinear-arm policy deliberately has
    a much larger allowance and does not leak into this decision.
    """
    long_span = _uninterrupted_long_span(long_axis, spans[long_axis], fa, fb, graph)
    if long_span is None:
        return False
    probe = dict(spans)
    probe[long_axis] = long_span
    return _prism_is_empty(probe, part, inset=COORD_FLOOR)


def _has_smooth_depth_closure(
    fa: _Face,
    fb: _Face,
    graph: FaceGraph,
    depth_axis: str,
    depth_span: tuple[float, float],
) -> bool:
    """Whether a shared curved continuation closes either proposed through end.

    A deep obround pocket can have more wall extent along its machining depth than along its
    footprint.  Choosing the longer extent as ``Slot.long_axis`` then rotates the interpretation:
    the pocket's two tangent cylindrical end caps appear at the proposed Slot depth ends, while
    the real planar floor is sought on the wrong axis and missed.  The topology already says this
    is not through geometry.  A non-planar region smoothly adjacent to both proposed walls and
    ending exactly at a proposed depth boundary closes that boundary.

    Correctly oriented obround Slot caps lie at the *long-axis* ends, so they do not satisfy this
    test.  Added coaxial posts meet the walls nonsmoothly and likewise remain outside it.
    """

    if fa.node is None or fb.node is None:
        raise ValueError("recess walls require graph nodes")
    k = _AXES[depth_axis]
    lo, hi = depth_span
    for seed in graph.neighbours(fa.node):
        if graph.is_planar(seed) or not is_any_smooth(graph.arc(fa.node, seed)):
            continue
        # Restrict the cached maximal smooth component to its connected non-planar members.
        # Planar walls can smoothly join both physical end caps, but they must not make those
        # separate curved closures one region for the boundary-extent test.
        available = {node for node in graph.smooth_region(seed) if not graph.is_planar(node)}
        region = {seed}
        pending = [seed]
        while pending:
            current = pending.pop()
            for neighbour in graph.neighbours(current):
                if (
                    neighbour in available
                    and neighbour not in region
                    and is_any_smooth(graph.arc(current, neighbour))
                ):
                    region.add(neighbour)
                    pending.append(neighbour)
        if not any(
            fb.node in graph.neighbours(node) and is_any_smooth(graph.arc(node, fb.node))
            for node in region
        ):
            continue
        region_lo = min(graph.bounds(node)[k][0] for node in region)
        region_hi = max(graph.bounds(node)[k][1] for node in region)
        if abs(region_hi - lo) <= COORD_FLOOR or abs(region_lo - hi) <= COORD_FLOOR:
            return True
    return False


def _bounds_one_void(fa: _Face, fb: _Face, graph: FaceGraph) -> bool:
    """Whether two opposed walls participate in one coherent recess boundary.

    Facing bounding boxes are not enough: walls of neighbouring features can overlap by a
    sliver and manufacture a candidate spanning the space between them. Two separate slots cut
    into the same plate make the stronger counterexample: their outer walls share the plate's
    top and bottom with matching *convex* arcs, but no concave boundary region joins them across
    the solid rib.

    Where the walls share a boundary face, both arcs must agree. Planar common neighbours are
    preferred because an added coaxial post contributes a cylindrical neighbour with opposite
    turns but interrupts rather than defines the slot. Agreement is only a prerequisite: after
    the candidate has exact bounds, :func:`_candidate_has_void_evidence` additionally requires
    an empty uninterrupted prism.
    """

    if fa.node is None or fb.node is None:
        raise ValueError("recess walls require graph nodes")
    common = set(graph.neighbours(fa.node)) & set(graph.neighbours(fb.node))
    if common:
        # Prefer the planar boundary shared by these planar walls. A convex cylindrical post at
        # a slot end is also adjacent to both walls, with one convex and one concave arc, but is
        # added material inside the boundary rather than evidence that the walls bound different
        # voids. Keep curved neighbours as the fallback for boundaries with no planar member.
        boundary = {neighbour for neighbour in common if graph.is_planar(neighbour)} or common
        return all(
            same_arc_kind(graph.arc(fa.node, neighbour), graph.arc(fb.node, neighbour))
            for neighbour in boundary
        )

    # With no common neighbour, a fragmented boundary must still join the walls through one
    # concave region or the wall faces themselves must be smooth subdivisions of one region.
    if _shared_concave_boundaries(fa, fb, graph):
        return True
    return fb.node in graph.smooth_region(fa.node)


def _candidate(
    fa: _Face,
    fb: _Face,
    part: Part,
    part_ext: dict[str, float],
    axis: str,
    graph: FaceGraph,
) -> Slot | None:
    """Build a :class:`Slot` from two facing rectangular walls, or None if the
    pair is not a slot (not facing, not overlapping, wider than long, or
    spanning the full part).  Geometry only — the through/blind test is applied
    by the caller, which needs the whole face set."""
    # *axis* is the bucket both walls came from, passed rather than re-read off `fa`: it is
    # established once where the oblique walls are declined, so nothing downstream needs a
    # branch for a wall that cannot reach here.
    k = _AXES[axis]
    bb_a, bb_b = fa.bb, fb.bb
    # Anti-parallel outward normals.
    if fa.normal[k] * fb.normal[k] >= 0:
        return None
    c_a, c_b = _center(bb_a, k), _center(bb_b, k)
    # Facing each other: A's outward normal points towards B.  Outer faces of
    # the stock fail this (their normals point apart).
    if (c_b - c_a) * fa.normal[k] <= 0:
        return None
    if not _bounds_one_void(fa, fb, graph):
        return None
    # The walls must genuinely overlap in both perpendicular axes, otherwise
    # they are unrelated faces that merely happen to be parallel and facing.
    others = [a for a in "xyz" if a != axis]
    ov = [_overlap_len(bb_a, bb_b, a) for a in others]
    if min(ov) <= 0:
        return None
    width = abs(c_b - c_a)
    # The longer shared extent is the slot length; the shorter is depth.  When
    # the two are near-equal (a near-square slot) the choice is ambiguous, so
    # break the tie towards the part's longer axis — a slot on a bar runs along
    # the bar.
    (ax0, ov0), (ax1, ov1) = sorted(zip(others, ov, strict=False), key=lambda t: t[1], reverse=True)
    if (ov0 - ov1) <= _LENGTH_TIE_FRAC * ov0 and part_ext[ax1] > part_ext[ax0]:
        (long_axis, length), depth_axis = (ax1, ov1), ax0
    else:
        (long_axis, length), depth_axis = (ax0, ov0), ax1
    # A slot is elongated: its width (the wall separation) is not its largest
    # dimension.  A wider-than-long pair is a step/pocket or a sliver of two
    # incidental parallel faces.
    if width > length:
        return None
    # Reject open / full-span features along the length (see _SLOT_MAX_SPAN_FRAC).
    if length >= _SLOT_MAX_SPAN_FRAC * part_ext[long_axis]:
        return None
    lc = "XYZ"[_AXES[long_axis]]
    lo = max(getattr(bb_a.min, lc), getattr(bb_b.min, lc))
    hi = min(getattr(bb_a.max, lc), getattr(bb_b.max, lc))
    dc = "XYZ"[_AXES[depth_axis]]
    d_lo = max(getattr(bb_a.min, dc), getattr(bb_b.min, dc))
    d_hi = min(getattr(bb_a.max, dc), getattr(bb_b.max, dc))
    if _has_smooth_depth_closure(fa, fb, graph, depth_axis, (d_lo, d_hi)):
        return None
    spans = {
        axis: tuple(sorted((c_a, c_b))),
        long_axis: (lo, hi),
        depth_axis: (d_lo, d_hi),
    }
    if not _candidate_has_void_evidence(spans, long_axis, fa, fb, graph, part):
        return None
    return Slot(
        width_axis=axis,
        long_axis=long_axis,
        width=round(width, 2),
        length=round(hi - lo, 2),
        w_center=round((c_a + c_b) / 2, 2),
        lo=round(lo, 2),
        hi=round(hi, 2),
        d_lo=round(d_lo, 2),
        d_hi=round(d_hi, 2),
    )


def _slot_proposals_one(
    part: Part,
    face_edges: FaceEdges | None = None,
    graph: FaceGraph | None = None,
    *,
    strict_cap_ambiguity: bool = True,
) -> list[_RecessProposal[Slot]]:
    """Recognise one solid's Slot occurrences with exact original topology identity."""

    owner = FaceGraph(part, face_edges=face_edges) if graph is None else graph
    faces = _planar_faces(part, face_edges, owner)
    pbb = part.bounding_box()
    part_ext = {a: getattr(pbb.size, "XYZ"[_AXES[a]]) for a in "xyz"}
    # Only straight-walled faces can be slot walls; bucket them by axis so the
    # O(n^2) pairing runs within each axis instead of across all planar faces.
    by_axis: dict[str, list[_Face]] = {}
    for f in faces:
        # An oblique wall is declined here, by this family, rather than filtered out of the
        # shared reduction on three families' behalf (ADR 0009). This recogniser pairs walls
        # that share a normal axis, so a wall with no axis has nothing here to pair with --
        # that is a real limit of the pairing strategy and it is now visible as one.
        if f.wall and f.axis is not None:
            by_axis.setdefault(f.axis, []).append(f)
    candidates: list[_RecessProposal[Slot]] = []
    for axis, walls in by_axis.items():
        for i in range(len(walls)):
            for j in range(i + 1, len(walls)):
                s = _candidate(walls[i], walls[j], part, part_ext, axis, owner)
                # Keep only through-slots: a blind pocket (or the floored gap
                # between bosses) is capped by a floor and is out of scope.
                if s is not None and not _has_floor(faces, s):
                    nodes = frozenset(
                        node for node in (walls[i].node, walls[j].node) if node is not None
                    )
                    candidates.append(_RecessProposal(s, nodes))
    # Stubby obround through-slots (straight section < width) have no pairable flat walls, so
    # recover them from their end caps. Emitted at the straight-wall junctions like the
    # flat-wall path, so `_merge` folds any duplicate an elongated obround also produced.
    recovered = _recognise_obround_from_ends(part, faces, graph=owner, proposals=True)
    candidates.extend(cast(list[_RecessProposal[Slot]], recovered))
    # Recombine arms of a crossing channel split by the intersection, then extend any
    # radiused-end (obround) slot to its overall length.
    return _extend_obround_proposals(
        _collapse_collinear_proposals(_merge_proposals(candidates), part),
        part,
        owner,
        strict_ambiguity=strict_cap_ambiguity,
    )


def _recognise_slots_one(
    part: Part,
    face_edges: FaceEdges | None = None,
    graph: FaceGraph | None = None,
    claims: _Claims | None = None,
) -> list[Slot]:
    """Record-only compatibility projection; legacy claims are derived, never authoritative."""

    proposals = _slot_proposals_one(part, face_edges, graph, strict_cap_ambiguity=False)
    if claims is not None:
        for proposal in proposals:
            claims.setdefault(proposal.record, set()).update(proposal.planar)
    return [proposal.record for proposal in proposals]


def _floored_candidate(
    fa,
    fb,
    part,
    faces,
    part_ext,
    axis: str,
    graph: FaceGraph,
    *,
    channel_bounds: dict[str, tuple[float, float]] | None = None,
    floor_nodes: list[FaceNode] | None = None,
) -> Pocket | Channel | None:
    """Build a floored opposed-wall recess, with open-vs-enclosed semantics explicit.

    ``channel_bounds=None`` asks for an enclosed :class:`Pocket` and preserves the
    historical full-span rejection.  Bounds ask for a :class:`Channel`: its shared
    longitudinal wall range must meet both envelope ends, proving the feature is open
    there rather than merely a large pocket.

    Unlike :func:`_candidate` (which splits the two non-width axes into long/depth by
    *size*), the depth axis is read from the geometry: it is capped on exactly one end
    (the floor) and open on the other.  This keeps a recess deeper than it is long from
    having its floor mistaken for an end wall.
    """
    k = _AXES[axis]  # *axis* is the width axis: the bucket both walls came from
    if fa.normal[k] * fb.normal[k] >= 0:
        return None  # not anti-parallel — not a facing pair
    c_a, c_b = _center(fa.bb, k), _center(fb.bb, k)
    if (c_b - c_a) * fa.normal[k] <= 0:
        return None  # normals face away from each other (outer faces), not a cavity
    if not _bounds_one_void(fa, fb, graph):
        return None
    width = abs(c_b - c_a)
    others = [a for a in "xyz" if a != axis]
    ranges = {}  # per non-width axis: (lo, hi) overlap of the two walls
    for a in others:
        c = "XYZ"[_AXES[a]]
        lo = max(getattr(fa.bb.min, c), getattr(fb.bb.min, c))
        hi = min(getattr(fa.bb.max, c), getattr(fb.bb.max, c))
        if hi - lo <= 0:
            return None  # walls do not overlap on this axis — not a slot
        ranges[a] = (lo, hi)
    w_range = (c_a + c_b) / 2 - width / 2, (c_a + c_b) / 2 + width / 2
    # The depth axis is the non-width axis capped on exactly one end (floor + opening).
    candidates: list[tuple[Pocket | Channel, frozenset[FaceNode]]] = []
    for depth_axis in others:
        (long_axis,) = [a for a in others if a != depth_axis]
        d_lo, d_hi = ranges[depth_axis]
        l_lo, l_hi = ranges[long_axis]
        foot = {axis: w_range, long_axis: (l_lo, l_hi)}
        foot_area = width * (l_hi - l_lo)
        cap_lo = _end_cap_faces(faces, foot, foot_area, depth_axis, d_lo, 1.0)
        cap_hi = _end_cap_faces(faces, foot, foot_area, depth_axis, d_hi, -1.0)
        if int(bool(cap_lo)) + int(bool(cap_hi)) != 1:
            continue  # 0 = through on this axis; 2 = an enclosed end-cap pair, not a floor
        length = l_hi - l_lo
        if width > length and channel_bounds is None:
            continue  # width is the smaller footprint dim (the wrong interpretation)
        if channel_bounds is None:
            if length >= _SLOT_MAX_SPAN_FRAC * part_ext[long_axis]:
                continue  # footprint spans the part — an open feature, not a pocket
            spans = {
                axis: tuple(sorted((c_a, c_b))),
                long_axis: (l_lo, l_hi),
                depth_axis: (d_lo, d_hi),
            }
            if not _candidate_has_void_evidence(spans, long_axis, fa, fb, graph, part):
                continue
            pocket_record = Pocket(
                width_axis=axis,
                long_axis=long_axis,
                width=round(width, 2),
                length=round(length, 2),
                depth=round(d_hi - d_lo, 2),
                w_center=round((c_a + c_b) / 2, 2),
                lo=round(l_lo, 2),
                hi=round(l_hi, 2),
                d_lo=round(d_lo, 2),
                d_hi=round(d_hi, 2),
                open_sign=1 if cap_lo else -1,
            )
            selected = cap_lo if cap_lo else cap_hi
            candidates.append(
                (
                    pocket_record,
                    frozenset(face.node for face in selected if face.node is not None),
                )
            )
            continue
        part_lo, part_hi = channel_bounds[long_axis]
        if abs(l_lo - part_lo) > _FLOOR_TOL or abs(l_hi - part_hi) > _FLOOR_TOL:
            continue  # not open at both longitudinal envelope ends
        spans = {axis: tuple(sorted((c_a, c_b))), long_axis: (l_lo, l_hi), depth_axis: (d_lo, d_hi)}
        if not _candidate_has_void_evidence(spans, long_axis, fa, fb, graph, part):
            continue
        channel_record = Channel(
            width_axis=axis,
            long_axis=long_axis,
            width=round(width, 2),
            w_center=round((c_a + c_b) / 2, 2),
            lo=round(l_lo, 2),
            hi=round(l_hi, 2),
            d_lo=round(d_lo, 2),
            d_hi=round(d_hi, 2),
            open_sign=1 if cap_lo else -1,
        )
        selected = cap_lo if cap_lo else cap_hi
        candidates.append(
            (
                channel_record,
                frozenset(face.node for face in selected if face.node is not None),
            )
        )
    if len(candidates) != 1:
        # Two valid depth axes describe an opening-corner ambiguity, not one rectangular recess.
        # Refuse rather than selecting whichever axis happens to appear first in ``"xyz"``.
        return None
    record, selected_nodes = candidates[0]
    if floor_nodes is not None:
        floor_nodes.extend(selected_nodes)
    return record


def _pocket_candidate(
    fa: _Face,
    fb: _Face,
    part: Part,
    faces: list[_Face],
    part_ext: dict[str, float],
    axis: str,
    graph: FaceGraph,
    *,
    floor_nodes: list[FaceNode] | None = None,
) -> Pocket | None:
    candidate = _floored_candidate(
        fa, fb, part, faces, part_ext, axis, graph, floor_nodes=floor_nodes
    )
    return candidate if isinstance(candidate, Pocket) else None


def _channel_candidate(
    fa: _Face,
    fb: _Face,
    part: Part,
    faces: list[_Face],
    part_ext: dict[str, float],
    part_bounds,
    axis: str,
    graph: FaceGraph,
    *,
    floor_nodes: list[FaceNode] | None = None,
) -> Channel | None:
    candidate = _floored_candidate(
        fa,
        fb,
        part,
        faces,
        part_ext,
        axis,
        graph,
        channel_bounds=part_bounds,
        floor_nodes=floor_nodes,
    )
    return candidate if isinstance(candidate, Channel) else None


def _channel_sort_key(channel: Channel) -> tuple:
    """Geometry-only order, including depth to break cross-solid traversal ties."""
    return (
        channel.long_axis,
        channel.width_axis,
        channel.lo,
        channel.hi,
        channel.w_center,
        channel.width,
        channel.d_lo,
        channel.d_hi,
        channel.open_sign,
    )


@dataclass(frozen=True, slots=True)
class _ChannelProposal:
    """One exact Channel occurrence and its ordered original side walls."""

    record: Channel
    low_wall: FaceNode
    high_wall: FaceNode
    floor: frozenset[FaceNode]


def _pocket_proposals_one(
    part: Part,
    face_edges: FaceEdges | None = None,
    graph: FaceGraph | None = None,
    *,
    strict_cap_ambiguity: bool = True,
) -> list[_RecessProposal[Pocket]]:
    """Recognise one solid's Pocket occurrences with exact original topology identity.

    *graph* and *claims* travel together exactly as they do in
    :func:`_recognise_slots_one`, and the two paths below claim differently on purpose:

    - **From opposed walls**, the two walls are defining and the floor is not. The floor
      reaches this candidate through :func:`_end_capped`, which asks whether *something* caps
      the footprint; the pocket's own depth is the walls' overlap on the depth axis, not the
      floor's position. Same line the through-slot draws, for the same reason: consultation is
      not consumption, and claiming it would have every pocket contest whatever owns its floor.
    - **From a corner notch**, the floor *is* defining. That path iterates floors and reads the
      notch's whole footprint off the one it finds, so the floor established the record as
      literally as the two walls did.
    """

    owner = FaceGraph(part, face_edges=face_edges) if graph is None else graph
    faces = _planar_faces(part, face_edges, owner)
    pbb = part.bounding_box()
    part_ext = {a: getattr(pbb.size, "XYZ"[_AXES[a]]) for a in "xyz"}
    by_axis: dict[str, list[_Face]] = {}
    for f in faces:
        if f.wall and f.axis is not None:
            by_axis.setdefault(f.axis, []).append(f)  # oblique declined here -- see slots
    candidates: list[_RecessProposal[Pocket]] = []
    for axis, walls in by_axis.items():
        for i in range(len(walls)):
            for j in range(i + 1, len(walls)):
                floor_nodes: list[FaceNode] = []
                p = _pocket_candidate(
                    walls[i],
                    walls[j],
                    part,
                    faces,
                    part_ext,
                    axis,
                    owner,
                    floor_nodes=floor_nodes,
                )
                if p is not None:
                    nodes = frozenset(
                        node for node in (walls[i].node, walls[j].node) if node is not None
                    )
                    if not floor_nodes:
                        raise ValueError("Pocket floor identity is unavailable")
                    candidates.append(_RecessProposal(p, nodes, floors=frozenset(floor_nodes)))
    candidates.extend(_corner_notch_proposals(faces, pbb))
    # Stubby blind obround pockets (straight section < width) have no pairable flat walls, so
    # recover them from their end caps — the blind counterpart of the through-slot path, and
    # claiming nothing for the same reason: its evidence is two cylindrical caps, which
    # `_planar_faces` never yielded and which no consumer reconciling planar walls can want.
    recovered = _recognise_obround_from_ends(part, faces, blind=True, graph=owner, proposals=True)
    candidates.extend(cast(list[_RecessProposal[Pocket]], recovered))
    proposals = _extend_obround_proposals(
        _merge_proposals(candidates),
        part,
        owner,
        strict_ambiguity=strict_cap_ambiguity,
    )
    return _attach_complete_pocket_regions(proposals, owner)


def _bounded_inner_region(
    graph: FaceGraph, opening: FaceNode, seed: frozenset[FaceNode]
) -> frozenset[FaceNode]:
    region = set(seed)
    pending = list(seed)
    while pending:
        current = pending.pop()
        for neighbour in graph.neighbours(current):
            if neighbour is opening or neighbour in region:
                continue
            kind = graph.arc(current, neighbour)
            if not (kind == "concave" or kind == "smooth"):
                continue
            region.add(neighbour)
            pending.append(neighbour)
    return frozenset(region)


def _attach_complete_pocket_regions(
    proposals: list[_RecessProposal[Pocket]], graph: FaceGraph
) -> list[_RecessProposal[Pocket]]:
    """Retain a complete inner-loop region only for a one-to-one Pocket occurrence.

    This is part of the final Pocket proposal proof, before Candidate issuance.  It starts from
    exact graph-owned opening-wire occurrences and joins them to the exact wall/cap/floor nodes
    already carried by discovery.  Ambiguity changes no record and leaves the historical
    constituent set to the publishing adapter.
    """

    by_region: dict[frozenset[FaceNode], set[FaceNode]] = {}
    for opening in graph.nodes:
        for wire in graph.face(opening).inner_wires():
            seed = _inner_wire_seed(graph, opening, wire)
            if not seed:
                continue
            region = _bounded_inner_region(graph, opening, seed)
            by_region.setdefault(region, set()).add(opening)

    regions: list[tuple[frozenset[FaceNode], frozenset[FaceNode]]] = []
    for region, opening_set in by_region.items():
        openings = frozenset(opening_set)
        solid = graph.common_valid_solid(region | openings)
        if solid is None:
            continue
        solid_nodes = {node for node in graph.nodes if graph.common_valid_solid((node,)) is solid}
        if region >= solid_nodes - openings:
            continue
        regions.append((region, openings))
    regions.sort(key=lambda item: tuple(sorted(node.index for node in item[0])))
    intersecting = {
        index
        for index, (region, _openings) in enumerate(regions)
        if any(
            region & other
            for other_index, (other, _other_openings) in enumerate(regions)
            if other_index != index
        )
    }

    matches: list[list[int]] = []
    for proposal in proposals:
        defining = proposal.planar | frozenset(node for group in proposal.caps for node in group)
        anchors = defining | proposal.floors
        matches.append(
            [
                index
                for index, (region, _openings) in enumerate(regions)
                if index not in intersecting and anchors <= region
            ]
        )
    region_uses = Counter(index for proposal_matches in matches for index in proposal_matches)

    return [
        replace(proposal, constituent=regions[proposal_matches[0]][0])
        if len(proposal_matches) == 1 and region_uses[proposal_matches[0]] == 1
        else proposal
        for proposal, proposal_matches in zip(proposals, matches, strict=True)
    ]


def _recognise_pockets_one(
    part: Part,
    face_edges: FaceEdges | None = None,
    graph: FaceGraph | None = None,
    claims: _Claims | None = None,
) -> list[Pocket]:
    """Record-only compatibility projection; legacy claims are derived from occurrences."""

    proposals = _pocket_proposals_one(part, face_edges, graph, strict_cap_ambiguity=False)
    if claims is not None:
        for proposal in proposals:
            claims.setdefault(proposal.record, set()).update(proposal.planar)
    return [proposal.record for proposal in proposals]


def _recognise_channels_one(
    part: Part, face_edges: FaceEdges | None = None, graph: FaceGraph | None = None
) -> list[Channel]:
    """Recognise channels using one solid's faces and bounds."""
    return sorted(
        {proposal.record for proposal in _channel_proposals_one(part, face_edges, graph)},
        key=_channel_sort_key,
    )


def _channel_proposals_one(
    part: Part, face_edges: FaceEdges | None = None, graph: FaceGraph | None = None
) -> list[_ChannelProposal]:
    """Discover one solid's Channels while retaining exact ordered wall identity."""

    owner = FaceGraph(part, face_edges=face_edges) if graph is None else graph
    faces = _planar_faces(part, face_edges, owner)
    pbb = part.bounding_box()
    part_ext = {a: getattr(pbb.size, "XYZ"[_AXES[a]]) for a in "xyz"}
    part_bounds = {
        a: (
            getattr(pbb.min, "XYZ"[_AXES[a]]),
            getattr(pbb.max, "XYZ"[_AXES[a]]),
        )
        for a in "xyz"
    }
    by_axis: dict[str, list[_Face]] = {}
    for face in faces:
        if face.wall and face.axis is not None:
            by_axis.setdefault(face.axis, []).append(face)  # oblique declined here -- see slots
    proposals: list[_ChannelProposal] = []
    for axis, walls in by_axis.items():
        for i in range(len(walls)):
            for j in range(i + 1, len(walls)):
                floor_nodes: list[FaceNode] = []
                channel = _channel_candidate(
                    walls[i],
                    walls[j],
                    part,
                    faces,
                    part_ext,
                    part_bounds,
                    axis,
                    owner,
                    floor_nodes=floor_nodes,
                )
                if channel is not None:
                    first, second = walls[i], walls[j]
                    if first.node is None or second.node is None:
                        raise ValueError("Channel walls require graph nodes")
                    if not floor_nodes:
                        raise ValueError("Channel floor identity is unavailable")
                    first_node, second_node = first.node, second.node
                    k = _AXES[axis]
                    low_node, high_node = (
                        (first_node, second_node)
                        if _center(first.bb, k) <= _center(second.bb, k)
                        else (second_node, first_node)
                    )
                    proposals.append(
                        _ChannelProposal(channel, low_node, high_node, frozenset(floor_nodes))
                    )
    return proposals


def _corner_notch_proposals(faces: list[_Face], pbb) -> list[_RecessProposal[Pocket]]:
    """Recognise a principal-axis rectangular blind corner interruption.

    A conventional pocket has opposed wall pairs. A corner interruption has only one wall on
    each of two axes, so its three mutually perpendicular interior faces establish one removed
    box instead. No principal axis is intrinsically "up": enumerate each valid interpretation
    and use the uniquely shallowest removed leg as depth. That geometric convention is invariant
    to principal-axis permutation and sign, unlike the historical world-Z floor rule. A tied
    shallowest leg has no unique ``Pocket.depth_axis`` and is refused rather than broken by an
    axis-letter or traversal-order preference.
    """
    tol = _MERGE_TOL

    def limits(bb, axis) -> tuple[float, float]:
        c = "XYZ"[_AXES[axis]]
        return getattr(bb.min, c), getattr(bb.max, c)

    envelope = {axis: limits(pbb, axis) for axis in "xyz"}
    candidates: list[tuple[float, _RecessProposal[Pocket]]] = []
    for depth_axis in "xyz":
        footprint_axes = [axis for axis in "xyz" if axis != depth_axis]
        first_axis, second_axis = footprint_axes
        di = _AXES[depth_axis]
        for floor in (face for face in faces if face.axis == depth_axis and face.wall):
            first_lo, first_hi = limits(floor.bb, first_axis)
            second_lo, second_hi = limits(floor.bb, second_axis)
            depth_lo, depth_hi = limits(floor.bb, depth_axis)
            first_span = first_hi - first_lo
            second_span = second_hi - second_lo
            if first_span <= tol or second_span <= tol or abs(depth_hi - depth_lo) > tol:
                continue
            if first_span >= _SLOT_MAX_SPAN_FRAC * (
                envelope[first_axis][1] - envelope[first_axis][0]
            ) or second_span >= _SLOT_MAX_SPAN_FRAC * (
                envelope[second_axis][1] - envelope[second_axis][0]
            ):
                continue  # a full-span step face, not a bounded interruption
            first_at_low = abs(first_lo - envelope[first_axis][0]) <= tol
            first_at_high = abs(first_hi - envelope[first_axis][1]) <= tol
            second_at_low = abs(second_lo - envelope[second_axis][0]) <= tol
            second_at_high = abs(second_hi - envelope[second_axis][1]) <= tol
            if (
                not ((first_at_low or first_at_high) and (second_at_low or second_at_high))
                or min(abs(depth_lo - end) for end in envelope[depth_axis]) <= tol
            ):
                continue
            first_inner = first_hi if first_at_low else first_lo
            second_inner = second_hi if second_at_low else second_lo

            first_wall = next(
                (
                    face
                    for face in faces
                    if face.axis == first_axis
                    and abs(_center(face.bb, _AXES[first_axis]) - first_inner) <= tol
                    and _overlap_len(face.bb, floor.bb, second_axis) >= second_span - tol
                ),
                None,
            )
            second_wall = next(
                (
                    face
                    for face in faces
                    if face.axis == second_axis
                    and abs(_center(face.bb, _AXES[second_axis]) - second_inner) <= tol
                    and _overlap_len(face.bb, floor.bb, first_axis) >= first_span - tol
                ),
                None,
            )
            if first_wall is None or second_wall is None:
                continue
            first_depth = limits(first_wall.bb, depth_axis)
            second_depth = limits(second_wall.bb, depth_axis)
            if any(
                abs(first - second) > tol
                for first, second in zip(first_depth, second_depth, strict=True)
            ):
                continue  # a split/interrupted side cannot define the complete removed box
            d_lo = max(first_depth[0], second_depth[0])
            d_hi = min(first_depth[1], second_depth[1])
            if d_hi - d_lo <= tol or not (d_lo - tol <= depth_lo <= d_hi + tol):
                continue

            if first_span <= second_span:
                width_axis, long_axis = first_axis, second_axis
                width, length = first_span, second_span
                w_center, lo, hi = (first_lo + first_hi) / 2, second_lo, second_hi
            else:
                width_axis, long_axis = second_axis, first_axis
                width, length = second_span, first_span
                w_center, lo, hi = (second_lo + second_hi) / 2, first_lo, first_hi
            record = Pocket(
                width_axis=width_axis,
                long_axis=long_axis,
                width=round(width, 2),
                length=round(length, 2),
                depth=round(d_hi - d_lo, 2),
                w_center=round(w_center, 2),
                lo=round(lo, 2),
                hi=round(hi, 2),
                d_lo=round(d_lo, 2),
                d_hi=round(d_hi, 2),
                open_sign=1 if floor.normal[di] > 0 else -1,
                edge_anchored=True,
            )
            candidates.append(
                (
                    d_hi - d_lo,
                    _RecessProposal(
                        record,
                        frozenset(
                            node
                            for node in (floor.node, first_wall.node, second_wall.node)
                            if node is not None
                        ),
                    ),
                ),
            )

    # Interpretations of the same removed box have the same centre. Preserve distinct physical
    # corners, then select depth only when geometry establishes a unique shallowest leg.
    groups: list[list[tuple[float, _RecessProposal[Pocket]]]] = []
    for candidate in candidates:
        centre = _region_center(candidate[1].record)
        group = next(
            (
                existing
                for existing in groups
                if math.dist(centre, _region_center(existing[0][1].record)) <= tol
            ),
            None,
        )
        if group is None:
            groups.append([candidate])
        else:
            group.append(candidate)
    out: list[_RecessProposal[Pocket]] = []
    for group in groups:
        ordered = sorted(group, key=lambda item: item[0])
        if len(ordered) > 1 and ordered[1][0] - ordered[0][0] <= COORD_FLOOR:
            continue
        out.append(ordered[0][1])
    return out


def _recognise_corner_notches(
    faces: list[_Face], pbb, claims: _Claims | None = None
) -> list[Pocket]:
    """Compatibility projection of occurrence-safe corner-notch proposals."""

    proposals = _corner_notch_proposals(faces, pbb)
    if claims is not None:
        for proposal in proposals:
            claims.setdefault(proposal.record, set()).update(proposal.planar)
    return [proposal.record for proposal in proposals]
