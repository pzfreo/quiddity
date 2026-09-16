# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Recognition of cylindrical holes, their counterbores and spotfaces, and their patterns."""

import math
from collections.abc import Sequence
from dataclasses import dataclass, replace
from typing import cast

from build123d import Face
from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.GeomAbs import (
    GeomAbs_Cone,
    GeomAbs_Torus,
)

from quiddity._adjacency import (
    FaceEdges,
    FaceNode,
    edge_face_map,
    neighbours,
)
from quiddity._candidates import CompletedInputs, CompletedOccurrence, DerivedId, FamilyId
from quiddity._claims import EvidenceWriter
from quiddity._cylinder_stacks import (
    SegmentEvidence,
    _axis_point,
    _classify_end,
    _end_partners,
    _family_surface_query,
    _full_cyls,
    _segments,
)
from quiddity._cylinder_substrate import (
    _STACK_GAP_FRAC,
    _line_key,
    _merge_runs,
    analyse_cylinders,
)
from quiddity._definitions import (
    AcceptedInputs,
    Counted,
    DerivedDefinition,
    DiscoveryServices,
    FullyAttributed,
    ManifestEvidence,
    PhysicalDefinition,
    always,
)
from quiddity._effective_surfaces import (
    EffectiveFaceSurfaceQuery,
    SurfaceUse,
    SurfaceUseRefusal,
    cylinder_surface_dependency,
)
from quiddity._geometry import dot, length_tol, quantise, without_negative_zero
from quiddity._pattern_geometry import (
    _PATTERN_ABS_TOL,
    _linear_array_candidates,
    _pattern_tol,
    _plane_uv,
    _rect_grid,
)
from quiddity._record import Record
from quiddity._typing import CylinderInventory, Part, Vector3
from quiddity.countersinks import CounterSink, countersink_matches_hole

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


def _same_diameter(a: float, b: float) -> bool:
    """Whether two quantised diameters are the same one, proportionally."""

    # The floor keeps a zero or near-zero diameter from making the test vacuous. It is well
    # below `quantise`'s own resolution at any size this package sees, so it never decides a
    # real comparison -- it only stops one from dividing the world by nothing.
    return abs(a - b) <= _SAME_DIAMETER_FRAC * max(abs(a), abs(b), _DIAMETER_FLOOR)


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


def _canonical_hole_axis_point(seg: SegmentEvidence, s: float) -> tuple[float, float, float]:
    """Remove the final kernel-noise digit from a reconstructed Hole location."""

    measured = _axis_point(seg, s)
    # Axis reconstruction combines independently measured anchors, directions and bounds. Eleven
    # significant figures removes their last kernel-noise digit while remaining far tighter than
    # the public record serializer and all length predicates.
    return tuple(quantise(value, figures=11) for value in measured)  # type: ignore[return-value]


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

    - same bore diameter on both sides of one observed internal cylindrical
      segment, with neither facing end closed. The interruption's original faces
      must adjoin both lands and its finite cylinder must contain the bore-axis
      gap; exterior air between separate lugs is not a bridge;
    - different diameters whose gap is bridged by a shoulder chamfer or
      fillet face (the steps of a counterbored hole with a deburred
      shoulder).
    """
    # The input already contains only full internal cylinder segments. Their
    # source-face membership identifies a possible common interruption, including
    # a cylinder split into several patches at a seam. Keep the original inventory: merged
    # spans must never become evidence for a later merge across exterior air.
    original_segments = [seg for stack in stacks for seg in stack]
    interruptions = {
        (seg.get("solid_idx", 0), face): index
        for index, seg in enumerate(original_segments)
        for face in seg["faces"]
    }
    source_neighbours: dict[tuple[int, Face], frozenset[int]] = {}

    def shares_interruption(a: SegmentEvidence, b: SegmentEvidence) -> bool:
        def sources(seg: SegmentEvidence) -> set[int]:
            found: set[int] = set()
            for face in seg["faces"]:
                key = (seg.get("solid_idx", 0), face)
                if key not in source_neighbours:
                    # Use exact source-edge adjacency. _end_partners is only a
                    # bounded end-classification heuristic: an oblique crossing
                    # rim can extend much farther axially than its search margin.
                    # Exclude seams between patches of this same source segment.
                    own = interruptions[key]
                    source_neighbours[key] = frozenset(
                        index
                        for other in neighbours(face, edge_faces)
                        if (index := interruptions.get((key[0], other))) is not None
                        and index != own
                    )
                found.update(source_neighbours[key])
            return found

        endpoints = (_axis_point(a, a["s_hi"]), _axis_point(b, b["s_lo"]))
        for index in sources(a) & sources(b):
            interruption = original_segments[index]
            tolerance = length_tol(interruption["diameter"], rel=_STACK_GAP_FRAC)
            radius = interruption["diameter"] / 2
            # Inventory diameters have six significant figures. Half a quantum
            # is at most 5e-6 of the value; do not reject a genuine crossing just
            # because its measured rim lies beyond the rounded-down radius.
            radial_tolerance = length_tol(radius, rel=5e-6)
            # Adjacency alone also admits a cavity running along the sides of
            # separate lugs. Require the common source's finite cylinder to
            # contain the bore axis across the gap. A cylinder is convex, so
            # containment of both endpoints proves containment of the interval.
            # This supplements source topology; exterior air alone proves nothing.
            for point in endpoints:
                axial = dot(point, interruption["dir_xyz"])
                centre = _axis_point(interruption, axial)
                if not (
                    interruption["s_lo"] - tolerance <= axial <= interruption["s_hi"] + tolerance
                    and math.dist(point, centre) < radius + radial_tolerance
                ):
                    break
            else:
                return True
        return False

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
                and shares_interruption(a, b)
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


# A counterbore-like step shallower than this fraction of its diameter is a spotface.
_SPOTFACE_MAX_RATIO = 0.2


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
            axis=without_negative_zero(tuple(-c for c in d) if from_hi else d),
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


_BC_SPACING_FRAC = 0.04


@dataclass(frozen=True)
class BoltCircle(Record):
    """≥3 identical holes equally spaced on a circle.

    ``center`` is the world point at the holes' opening plane, ``diameter``
    the bolt-circle diameter (BCD), ``holes`` the member features.
    """

    holes: tuple[HoleRecord, ...]
    center: Vector3
    diameter: float


@dataclass(frozen=True)
class LinearArray(Record):
    """≥3 identical holes collinear at constant pitch.

    ``direction`` is the unit vector from the first hole toward the last
    (members are ordered along it).
    """

    holes: tuple[HoleRecord, ...]
    pitch: float
    direction: Vector3


@dataclass(frozen=True)
class RectGrid(Record):
    """A fully-populated rectangular grid of identical holes (an N×M lattice).

    ``rows``×``cols`` holes sit on a regular rectangular lattice; every lattice
    position is occupied (``rows * cols == len(holes)``). ``center`` is the world
    point at the grid centroid (opening plane).

    The serialized lattice convention is self-contained: **columns** are spaced
    ``col_pitch`` apart along the lattice's first basis direction and **rows**
    ``row_pitch`` apart along the second, and ``angle`` is the COLUMN direction's
    orientation in degrees within the holes' opening plane, measured in the
    :func:`quiddity._geometry.plane_axes` frame and normalised to ``[0, 180)``.

    ``[0, 180)`` and not ``[0, 90)``: the lattice is unchanged by a half-turn (its
    cell set is symmetric about ``center``, so the basis sign carries no
    information) but a QUARTER-turn swaps rows for columns, which these fields
    distinguish. Note the first basis is the SHORTEST pairwise vector rather than
    anything world-aligned, so a grid may legitimately come back with its rows and
    columns named the other way round — what is fixed is that each count keeps its
    own pitch, and that the fields describe the same lattice.

    A rectangular *ring* / perimeter (holes only around the edge, interior
    empty) is not a grid — it is reported as its constituent edge
    :class:`LinearArray` rows instead.
    """

    holes: tuple[HoleRecord, ...]
    rows: int
    cols: int
    row_pitch: float
    col_pitch: float
    angle: float
    center: Vector3


@dataclass(frozen=True)
class HoleSpec(Record):
    """The machining spec shared by holes that are the *same drilled feature*.

    Two holes drilled with the same tool, in the same direction, with the same
    counterbore/spotface stack have equal :class:`HoleSpec` values (a through
    drill is the same spec whatever wall it pierces). Because the dataclass is
    frozen it hashes and compares by value, so it is a stable dict/set key for
    grouping holes — pattern detection and callout grouping agree when they key
    on the same :class:`HoleSpec`.

    Build one with :meth:`from_hole`; do not construct the fields by hand (the
    normalisation in :meth:`from_hole` is part of the contract). ``axis`` is the
    drilling direction snapped to 6 dp (boolean ops leave ~1e-16 noise on the
    components, and exact float keys would split a pattern silently). ``depth``
    is ``None`` for a through hole — its depth is irrelevant to the spec —
    otherwise the bore depth. Public and stable for downstream consumers.
    """

    axis: Vector3
    diameter: float
    depth: float | None
    bottom: str
    cbore: CounterBore | None
    spotface: CounterBore | None
    # The countersink's *size* only — ``(major_diameter, included_angle)`` — never its
    # location, so identical countersunk holes at different positions share one spec.
    csink: tuple[float, float] | None = None

    @classmethod
    def from_hole(cls, hole: HoleRecord) -> "HoleSpec":
        """The :class:`HoleSpec` for *hole* (a :class:`HoleRecord`)."""
        depth = None if hole.bottom == "through" else hole.depth
        axis = tuple(0.0 if abs(c) < 1e-6 else round(c, 6) for c in hole.axis)
        csink = (hole.csink.major_diameter, hole.csink.included_angle) if hole.csink else None
        return cls(
            (axis[0], axis[1], axis[2]),
            hole.diameter,
            depth,
            hole.bottom,
            hole.cbore,
            hole.spotface,
            csink,
        )


def _spec_key(h) -> HoleSpec:
    return HoleSpec.from_hole(h)


def _as_bolt_circle(holes, pts: Sequence[tuple[float, float]]) -> BoltCircle | None:
    """BoltCircle when *pts* (2D) are equally spaced on a common circle."""
    n = len(pts)
    cx = sum(p[0] for p in pts) / n
    cy = sum(p[1] for p in pts) / n
    radii = [math.hypot(p[0] - cx, p[1] - cy) for p in pts]
    r = sum(radii) / n
    if r < _PATTERN_ABS_TOL or max(abs(ri - r) for ri in radii) > _pattern_tol(r):
        return None
    angles = sorted(math.atan2(p[1] - cy, p[0] - cx) for p in pts)
    gaps = [angles[i + 1] - angles[i] for i in range(n - 1)]
    gaps.append(2 * math.pi - (angles[-1] - angles[0]))
    even = 2 * math.pi / n
    if max(abs(g - even) for g in gaps) > _BC_SPACING_FRAC * even:
        return None
    center = tuple(sum(c) / n for c in zip(*(h.location for h in holes), strict=True))
    return BoltCircle(holes=tuple(holes), center=center, diameter=round(2 * r, 2))


def _circumcircle(p0, p1, p2) -> tuple[float, float, float] | None:
    """Centre and radius ``(cx, cy, r)`` of the circle through three 2D points,
    or ``None`` when they are collinear (so a collinear triple can never seed a
    bolt circle — collinearity must win, per :func:`recognise_hole_patterns`)."""
    ax, ay = p0
    bx, by = p1
    cx, cy = p2
    d = 2 * (ax * (by - cy) + bx * (cy - ay) + cx * (ay - by))
    if abs(d) < 1e-9:
        return None
    a2, b2, c2 = ax * ax + ay * ay, bx * bx + by * by, cx * cx + cy * cy
    ux = (a2 * (by - cy) + b2 * (cy - ay) + c2 * (ay - by)) / d
    uy = (a2 * (cx - bx) + b2 * (ax - cx) + c2 * (bx - ax)) / d
    return ux, uy, math.hypot(ax - ux, ay - uy)


def _bolt_circle_candidates(
    members, pts: Sequence[tuple[float, float]]
) -> list[tuple[BoltCircle, frozenset[int]]]:
    """All bolt circles within a spec group: every triple seeds a candidate
    circle, the group's points lying on it are gathered, and the set is kept
    only if :func:`_as_bolt_circle` confirms it is fully, evenly populated.
    Returns ``(BoltCircle, frozenset(member indices))`` candidates."""
    n = len(pts)
    out, seen = [], set()
    for i in range(n):
        for j in range(i + 1, n):
            for k in range(j + 1, n):
                circ = _circumcircle(pts[i], pts[j], pts[k])
                if circ is None:
                    continue
                cx, cy, r = circ
                if r < _PATTERN_ABS_TOL:
                    continue
                key = (round(cx, 2), round(cy, 2), round(r, 2))
                if key in seen:
                    continue
                seen.add(key)
                tol = _pattern_tol(r)
                idx = [
                    m
                    for m in range(n)
                    if abs(math.hypot(pts[m][0] - cx, pts[m][1] - cy) - r) <= tol
                ]
                if len(idx) < 3:
                    continue
                pat = _as_bolt_circle([members[m] for m in idx], [pts[m] for m in idx])
                if pat is not None:
                    out.append((pat, frozenset(idx)))
    return out


def _mk_hole_linear(members, pitch, direction) -> LinearArray:
    return LinearArray(holes=tuple(members), pitch=pitch, direction=direction)


def _mk_hole_grid(members, rows, cols, row_pitch, col_pitch, angle, center) -> RectGrid:
    return RectGrid(
        holes=tuple(members),
        rows=rows,
        cols=cols,
        row_pitch=row_pitch,
        col_pitch=col_pitch,
        angle=angle,
        center=center,
    )


def recognise_hole_patterns(
    holes: Sequence[HoleRecord],
) -> list[BoltCircle | LinearArray | RectGrid]:
    """Recognise :class:`BoltCircle`, :class:`LinearArray`, and
    :class:`RectGrid` patterns among *holes* (``HoleRecord`` records, e.g.
    from :func:`recognise_holes`).

    Holes are grouped by machining spec and drilling axis, then each group is
    *sub-clustered* — a single spec can contribute several patterns (two
    separate bolt circles, the rows of a rectangular perimeter, a grid). All
    candidate sub-patterns are enumerated and allocated greedily largest-first,
    so each hole belongs to at most one pattern and the richest interpretation
    wins. Precedence is deterministic: a full grid claims its complete same-spec group first;
    remaining candidates sort by member count with stable family order. A filled N×M lattice
    becomes one :class:`RectGrid`; a rectangular
    ring or perimeter is reported as its edge :class:`LinearArray` rows.

    Collinearity is tested ahead of concyclicity (any three points are
    concyclic, so a 3-hole "bolt circle" must really be an equilateral
    triangle); unpatterned holes are simply absent from the result.
    """
    groups: dict = {}
    for h in holes:
        groups.setdefault(_spec_key(h), []).append(h)

    patterns: list[BoltCircle | LinearArray | RectGrid] = []
    for spec, members in groups.items():
        if len(members) < 3:
            continue
        u, v = _plane_uv(spec.axis)
        pts = [
            (
                sum(a * b for a, b in zip(h.location, u, strict=True)),
                sum(a * b for a, b in zip(h.location, v, strict=True)),
            )
            for h in members
        ]
        grid = _rect_grid(members, pts, _mk_hole_grid)
        if grid is not None:
            # `_rect_grid` succeeds only when this entire same-spec group fills one
            # lattice. The grid therefore claims every member, sorts ahead of every
            # equal-sized candidate, and makes all circle/linear candidates impossible
            # to allocate. Return it now instead of doing O(n^4) work whose results are
            # guaranteed to be discarded.
            patterns.append(grid)
            continue
        candidates: list = []
        candidates += _bolt_circle_candidates(members, pts)
        candidates += _linear_array_candidates(members, pts, _mk_hole_linear)
        # allocate largest-first; a hole used by one pattern is off the table
        # for the rest (stable sort keeps grids ahead of circles ahead of rows
        # at equal size)
        candidates.sort(key=lambda c: -len(c[1]))
        used: set = set()
        for pattern, idx in candidates:
            if idx & used:
                continue
            patterns.append(pattern)
            used |= idx
    return patterns


# Holes read the countersinks that completed before them, so the declaration hands the core both
# the records and their occurrences; `_registry` places this family after COUNTERSINKS.
def _discover(services: DiscoveryServices, inputs: CompletedInputs) -> list[object]:
    countersinks = list(inputs.records(FamilyId.COUNTERSINKS, CounterSink))
    occurrences = inputs.occurrences(FamilyId.COUNTERSINKS, CounterSink)
    return list(
        _discover_holes(
            services.context.part,
            cyls=services.cylinders,
            csinks=countersinks,
            face_edges=services.context.face_edges,
            writer=services.writer,
            predecessor_occurrences=occurrences,
            face_surfaces=services.context.face_surfaces,
        )
    )


def _derive_patterns(inputs: AcceptedInputs) -> list[object]:
    return list(recognise_hole_patterns(inputs.records(FamilyId.HOLES, HoleRecord)))


DEFINITION = PhysicalDefinition(
    family=FamilyId.HOLES,
    record_types=(HoleRecord,),
    result_field="holes",
    public_entrypoint=recognise_holes.__name__,
    dependencies=(FamilyId.COUNTERSINKS,),
    applicable=always,
    discover=_discover,
    census=Counted("hole"),
    attribution=FullyAttributed(
        "every returned Hole claims its complete original cylindrical occurrence faces"
    ),
    evidence=ManifestEvidence(
        goldens=("simple_through_hole", "counterbored_and_countersunk_holes"),
        extra_records=(
            (
                "CounterBore",
                "nested",
                ("RecognitionResult.holes.cbore", "RecognitionResult.holes.spotface"),
            ),
            ("HoleSpec", "evidence", ()),
        ),
    ),
)

PATTERNS = DerivedDefinition(
    identifier=DerivedId.HOLE_PATTERNS,
    record_types=(BoltCircle, LinearArray, RectGrid),
    result_field="hole_patterns",
    public_entrypoint=recognise_hole_patterns.__name__,
    sources=(FamilyId.HOLES,),
    derive=_derive_patterns,
    census=Counted("hole_pattern"),
    evidence=ManifestEvidence(goldens=("bolt_circle_and_rectangular_grid",)),
)
