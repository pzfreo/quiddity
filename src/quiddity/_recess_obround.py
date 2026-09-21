# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Slots and pockets whose ends are cylinders.

A slot milled with a round cutter has semi-cylindrical ends, and if it is short enough its two
flat walls never pair -- the wall-pairing scan that finds every other slot finds nothing at all.
This module recovers those from the ends instead: `_obround_ends` inventories the cylindrical
faces that could be a cap, `_obround_end` decides whether one really is, and
`_recognise_obround_from_ends` rebuilds the void between a matched pair.

`_extend_obround_ends` is the other direction -- a slot already found by its walls, whose
rounded ends were not part of what found it, and whose length is short by a radius at each end
until they are.

Imports :mod:`quiddity._recess_faces` for the face read and
:mod:`quiddity._recess_reduce` for claim bookkeeping; nothing imports it but
:mod:`quiddity._recess_core`.
"""

from __future__ import annotations

from dataclasses import replace
from typing import cast

from quiddity._adjacency import FaceGraph, FaceNode
from quiddity._geometry import length_tol
from quiddity._recess_faces import (
    _AXES,
    _MERGE_TOL,
    _center,
    _cylinder_faces,
    _Face,
    _floor_end_faces,
    _has_side_walls,
    _union_bb,
)
from quiddity._recess_records import Pocket, Slot
from quiddity._recess_reduce import _R, _absorb, _Claims, _RecessProposal
from quiddity._typing import Part

# A radiused-end (obround) slot has semicircular end caps whose radius is the slot's
# half-width; a cylindrical face counts as such a cap when its radius is within this fraction
# of width/2 (ADR 0008). Tight, so a filleted-corner rectangular slot (small corner radii) is
# not extended. The fraction is the 0.15 mm constant it replaces over the corpus's 4 mm
# reference radius.
_END_RADIUS_FRAC = 0.0375


def _end_cap_at(caps: list[tuple], s: _R, coord: float) -> bool:
    """True when a semicircular obround end cap sits at ``coord`` along the long axis: a
    **concave** (void-bounding) cylinder of radius ≈ ``s.width/2`` about the depth axis, on the
    slot centreline, **spanning the slot's own depth extent** ``[d_lo, d_hi]``. Two clauses
    separate a real cap from a coaxial impostor at the same place: the depth-extent test rejects
    a boss/hole at a *different* depth, and the concavity test rejects added material — a post or
    boss protruding *into* the slot end at the slot's depth."""
    r = s.width / 2
    li, wi, di = _AXES[s.long_axis], _AXES[s.width_axis], _AXES[s.depth_axis]
    dc = "XYZ"[di]
    return any(
        concave
        and abs(rad - r) <= length_tol(r, rel=_END_RADIUS_FRAC)
        and ax == s.depth_axis
        and abs(loc[li] - coord) <= _MERGE_TOL
        and abs(loc[wi] - s.w_center) <= _MERGE_TOL
        and abs(getattr(bb.min, dc) - s.d_lo) <= _MERGE_TOL
        and abs(getattr(bb.max, dc) - s.d_hi) <= _MERGE_TOL
        for rad, ax, loc, bb, concave, _node in caps
    )


def _extend_obround_ends(records: list[_R], part: Part, claims: _Claims | None = None) -> list[_R]:
    """Extend radiused-end (obround) slots/pockets to their **overall** length.

    The recogniser pairs the two flat side walls, so the raw ``lo``/``hi`` stop at the straight
    portion — a slot with semicircular ends under-reports its length by the two end radii (each
    ``width/2``). A true obround is symmetric, so a slot is extended only when a semicircular end
    cap (:func:`_end_cap_at`) is present at **both** ends; each end is then pushed out by the
    radius, keeping the centre fixed. Requiring both ends (not one) avoids a lone coaxial
    cylinder shifting a flat-ended slot's centre, and matches explicit-object readers that
    derive overall length from the object bounding box. Rectangular slots have no caps
    and are returned unchanged."""
    caps = _cylinder_faces(part)
    if not caps:
        return records
    out: list[_R] = []
    for s in records:
        r = s.width / 2
        if _end_cap_at(caps, s, s.lo) and _end_cap_at(caps, s, s.hi):
            extended = replace(
                s,
                lo=round(s.lo - r, 2),
                hi=round(s.hi + r, 2),
                length=round(s.hi - s.lo + 2 * r, 2),
                end_radius=round(r, 2),
            )
            _absorb(claims, extended, s)
            out.append(extended)
        else:
            out.append(s)
    return out


def _matching_end_groups(
    ends: list[tuple], record: _R, coord: float
) -> tuple[frozenset[FaceNode], ...]:
    """Every distinct physical cap cluster matching one serialized endpoint."""

    radius = record.width / 2
    groups: list[frozenset[FaceNode]] = []
    for end in ends:
        _wa, _la, axis, rad, wc, flat, _direction, d_lo, d_hi, patches = end
        if (
            axis == record.depth_axis
            and abs(rad - radius) <= length_tol(radius, rel=_END_RADIUS_FRAC)
            and abs(flat - coord) <= _MERGE_TOL
            and abs(wc - record.w_center) <= _MERGE_TOL
            and abs(d_lo - record.d_lo) <= _MERGE_TOL
            and abs(d_hi - record.d_hi) <= _MERGE_TOL
            and patches
            and patches not in groups
        ):
            groups.append(patches)
    return tuple(groups)


def _extend_obround_proposals(
    proposals: list[_RecessProposal[_R]],
    part: Part,
    graph: FaceGraph,
    *,
    strict_ambiguity: bool = True,
    cylinders: list[tuple] | None = None,
) -> list[_RecessProposal[_R]]:
    """Extend records while retaining both exact cap clusters; ambiguity refuses."""

    ends = _obround_ends(part, graph, cylinders=cylinders)
    out: list[_RecessProposal[_R]] = []
    for proposal in proposals:
        record = proposal.record
        low = _matching_end_groups(ends, record, record.lo)
        high = _matching_end_groups(ends, record, record.hi)
        if strict_ambiguity and (len(low) > 1 or len(high) > 1):
            raise ValueError("multiple distinct obround cap clusters compete for one endpoint")
        if low and high:
            radius = record.width / 2
            extended = replace(
                record,
                lo=round(record.lo - radius, 2),
                hi=round(record.hi + radius, 2),
                length=round(record.hi - record.lo + 2 * radius, 2),
                end_radius=round(radius, 2),
            )
            cap_groups = list(proposal.caps)
            for group in (low[0], high[0]):
                if group not in cap_groups:
                    cap_groups.append(group)
            out.append(
                _RecessProposal(
                    extended,
                    proposal.planar,
                    tuple(cap_groups),
                    proposal.floors,
                )
            )
        else:
            out.append(proposal)
    return out


# An obround through-slot whose straight section is shorter than its width has flat side
# walls too short to pair as an elongated slot: `_candidate` either rejects it (width > the short
# straight length) or mistakes the full through-thickness for the length (a full-span cut). Its
# length lives in the two semicircular ends, so recognise it directly from the end caps — a pair of
# concave HALF-cylinders (in-plane bbox ≈ 2r × r) of equal radius, coaxial about a through depth
# axis, on a shared centreline, bulging apart along the long axis. A round hole is a FULL cylinder
# (bbox 2r × 2r) so it is never read as an end; a lone unpaired end is a fillet/round.
_OBROUND_RATIO_TOL = 0.1  # a half-cylinder's in-plane extents are 2r (across) / r (bulge) — match

# Coaxial cap faces (a semicircle a STEP importer split into two quarter-cylinders) are clustered
# when their axis lines sit within this fraction of the cap radius; importer split noise is a
# fraction of the face, well inside it, and a real slot's two ends are separated by its straight
# run, well outside. The fraction is the 0.3 mm constant it replaces over the 4 mm reference.
_CAP_CLUSTER_FRAC = 0.075


def _compatible_end_groups(ends: list[tuple]) -> tuple[tuple[tuple, ...], ...]:
    """Extend legacy decimal groups only for a lost stubby opposing pair.

    Existing exact rounded-centre groups remain byte-for-byte authoritative. Imported or
    near-principal caps can place the two ends of one stubby recess on opposite sides of a decimal
    boundary, however. Merge exactly two singleton groups only when the existing radius-scaled cap
    authority says their centrelines agree *and* their opposed flats retain the documented stubby
    span (straight run no longer than the width). The latter prevents end recovery from duplicating
    an elongated wall-derived Pocket.
    """

    legacy: dict[tuple, list[tuple]] = {}
    for end in ends:
        wa, la, da, rad, wc, _flat, _direction, d_lo, d_hi, _patches = end
        key = (
            wa,
            la,
            da,
            round(rad, 2),
            round(wc, 2),
            round(d_lo, 2),
            round(d_hi, 2),
        )
        legacy.setdefault(key, []).append(end)

    groups = [tuple(group) for group in legacy.values()]
    partners: dict[int, list[tuple[int, tuple, tuple]]] = {}
    for index, group in enumerate(groups):
        if len(group) != 1:
            continue
        for other_index in range(index + 1, len(groups)):
            other = groups[other_index]
            if len(other) != 1:
                continue
            low, high = sorted((group[0], other[0]), key=lambda end: end[5])
            radius = round(low[3], 2)
            tolerance = length_tol(radius, rel=_CAP_CLUSTER_FRAC)
            base_matches = (
                low[0:3] == high[0:3]
                and round(low[3], 2) == round(high[3], 2)
                and round(low[7], 2) == round(high[7], 2)
                and round(low[8], 2) == round(high[8], 2)
            )
            if (
                base_matches
                and abs(low[4] - high[4]) <= tolerance
                and low[6] == -1
                and high[6] == 1
                and high[5] - low[5] <= 2 * radius + tolerance
            ):
                partners.setdefault(index, []).append((other_index, low, high))
                partners.setdefault(other_index, []).append((index, low, high))

    out: list[tuple[tuple, ...]] = []
    consumed: set[int] = set()
    for index, group in enumerate(groups):
        if index in consumed:
            continue
        if len(group) != 1:
            out.append(group)
            continue
        matches = partners.get(index, [])
        # A pair is admissible only when both ends name each other as their sole partner. Multiple
        # plausible partners are ambiguous topology, not authority to choose the nearest or first
        # traversal occurrence. Preserve every legacy bucket and let recognition refuse.
        if len(matches) == 1 and len(partners.get(matches[0][0], [])) == 1:
            other_index, low, high = matches[0]
            consumed.add(other_index)
            out.append((low, high))
        else:
            out.append(group)
    return tuple(out)


def _obround_end(cap: tuple, patches: frozenset[FaceNode] = frozenset()) -> tuple | None:
    """Classify a concave cylinder *cap* (from :func:`_cylinder_faces`) as a half-cylinder obround
    end, or None. A half-cylinder end's in-plane bounding box is ≈ 2r across (its width axis) by
    ≈ r along the bulge (its long axis) — a full cylinder (round hole) is 2r × 2r and is rejected.
    The 2r/r test is by RATIO to the radius so the classifier holds at any scale (fixing the
    absolute-tolerance collapse for small radii). Returns
    ``(width_axis, long_axis, depth_axis,
    radius, w_center, flat, direction, d_lo, d_hi)`` — ``flat`` is the cylinder-axis position on the
    long axis (the straight-wall junction), ``direction`` (±1) the side the cap bulges toward. The
    through/blind split is left to the caller's :func:`_has_floor` (authoritative and local, so a
    through-slot in a thin step of stepped stock is not rejected by a global-thickness
    assumption)."""
    rad, ax, loc, bb, concave, _node = cap
    if not concave or rad <= 0:
        return None
    others = [a for a in "xyz" if a != ax]
    ext = {a: getattr(bb.max, "XYZ"[_AXES[a]]) - getattr(bb.min, "XYZ"[_AXES[a]]) for a in others}
    across = [a for a in others if abs(ext[a] / rad - 2.0) <= _OBROUND_RATIO_TOL]
    bulge = [a for a in others if abs(ext[a] / rad - 1.0) <= _OBROUND_RATIO_TOL]
    if len(across) != 1 or len(bulge) != 1 or across[0] == bulge[0]:
        return None
    width_axis, long_axis = across[0], bulge[0]
    dc = "XYZ"[_AXES[ax]]
    d_lo, d_hi = getattr(bb.min, dc), getattr(bb.max, dc)
    lc = "XYZ"[_AXES[long_axis]]
    flat = loc[_AXES[long_axis]]
    direction = -1 if (flat - getattr(bb.min, lc)) > (getattr(bb.max, lc) - flat) else 1
    # w_center from the (merged) bbox centre, not loc — an imported STEP splits an end into two
    # quarter faces whose axis Location differs by ~0.02 mm across the diameter, so loc[width] is
    # unreliable while the union bbox centre is exact. flat (loc[long]) stays consistent.
    return (
        width_axis,
        long_axis,
        ax,
        rad,
        _center(bb, _AXES[width_axis]),
        flat,
        direction,
        d_lo,
        d_hi,
        patches,
    )


def _obround_ends(
    part: Part,
    graph: FaceGraph | None = None,
    *,
    cylinders: list[tuple] | None = None,
) -> list[tuple]:
    """The obround end caps of *part*, robust to the imported-STEP topology that splits a
    semicircular end into two quarter-cylinder faces.

    A physical end is one or more **coaxial** concave cylinder faces sharing an axis line and depth:
    build123d emits it as a single half-cylinder face (in-plane bbox ``2r × r``), while a STEP
    importer commonly splits it into two quarter-cylinders (``r × r`` each) whose axis Locations
    differ by ~0.02 mm across the diameter. Faces are therefore **clustered by proximity** of their
    axis line (same axis + radius + depth, in-plane position within ``_CAP_CLUSTER_FRAC`` of the
    cap radius) rather
    than exact-key grouping, and the UNION in-plane bbox is classified via
    :func:`_obround_end` — the union of the two quarters is the same ``2r × r`` a single
    half-cylinder gives. (A round hole is a full cylinder, ``2r × 2r``, so its union still
    fails the ratio test.) The core may supply its graph-owned cylinder inventory so obround and
    rounded-corner proofs share one topology scan; standalone callers retain the local scan.
    """
    clusters: list[dict] = []
    inventory = _cylinder_faces(part, graph) if cylinders is None else cylinders
    for rad, ax, loc, bb, concave, node in inventory:
        if not concave or rad <= 0:
            continue
        o0, o1 = [a for a in "xyz" if a != ax]
        dc = "XYZ"[_AXES[ax]]
        ip = (loc[_AXES[o0]], loc[_AXES[o1]])
        dz = (round(getattr(bb.min, dc), 1), round(getattr(bb.max, dc), 1))
        for cl in clusters:
            if (
                cl["ax"] == ax
                and abs(cl["rad"] - rad) <= length_tol(rad, rel=_END_RADIUS_FRAC)
                and cl["dz"] == dz
                and abs(cl["ip"][0] - ip[0]) <= length_tol(rad, rel=_CAP_CLUSTER_FRAC)
                and abs(cl["ip"][1] - ip[1]) <= length_tol(rad, rel=_CAP_CLUSTER_FRAC)
            ):
                cl["bb"] = _union_bb(cl["bb"], bb)
                if node is not None:
                    cl["nodes"].add(node)
                break
        else:
            clusters.append(
                {
                    "ax": ax,
                    "rad": rad,
                    "loc": loc,
                    "ip": ip,
                    "dz": dz,
                    "bb": bb,
                    "nodes": set() if node is None else {node},
                }
            )
    ends = []
    for cl in clusters:
        e = _obround_end(
            (cl["rad"], cl["ax"], cl["loc"], cl["bb"], True, None),
            frozenset(cl["nodes"]),
        )
        if e is not None:
            ends.append(e)
    return ends


def _recognise_obround_from_ends(
    part: Part,
    faces: list[_Face],
    *,
    blind: bool = False,
    graph: FaceGraph | None = None,
    proposals: bool = False,
    cylinders: list[tuple] | None = None,
) -> list[Slot] | list[Pocket] | list[_RecessProposal]:
    """Recognise obround recesses from their semicircular end caps — the path for recesses whose
    flat walls are too short for :func:`_candidate`/:func:`_pocket_candidate` to pair.
    Merged ends (:func:`_obround_ends`, quarter/half-cylinder agnostic) are grouped by centreline/
    radius/depth, then within a group sorted along the run and paired: a cap bulging toward -long
    immediately followed by one bulging toward +long is one recess's two ends (the void lies between
    their flats); the reverse adjacency is the solid gap between two recesses and is skipped. Each
    pair is confirmed by :func:`_has_side_walls` (a real channel connects the ends — not two
    D-cutouts across solid), then :func:`_has_floor` routes it: ``blind=False`` keeps only through
    recesses (:class:`Slot`), ``blind=True`` keeps only floored ones (:class:`Pocket`, depth =
    ``d_hi - d_lo``). ``lo``/``hi`` are emitted at the straight-wall junctions so
    :func:`_extend_obround_ends` adds the two radii (uniform with the flat-wall path, and `_merge`
    folds any duplicate an elongated obround's flat walls also produced).

    The two flats must be more than ``_MERGE_TOL`` apart to count as distinct ends — so a genuinely
    sub-millimetre obround (straight run < 0.5 mm) is not recovered; supporting that would need the
    module-wide absolute ``_MERGE_TOL`` to go relative, out of scope here."""
    # The list is homogeneous per call -- `blind` decides which record kind is appended -- but
    # that is a fact about the flag, not about this local, so the overloads above carry it and
    # the local is typed as what it structurally is.
    out: list[Slot | Pocket] = []
    proposed: list[_RecessProposal] = []
    ends = _obround_ends(part, graph, cylinders=cylinders)
    for grp in _compatible_end_groups(ends):
        wa, la, _da, raw_rad, _wc, _flat, _direction, raw_dlo, raw_dhi, _patches = grp[0]
        rad, dlo, dhi = round(raw_rad, 2), round(raw_dlo, 2), round(raw_dhi, 2)
        wc = round(sum(end[4] for end in grp) / len(grp), 2)
        run = sorted(grp, key=lambda e: e[5])  # by flat along the long axis
        i = 0
        while i < len(run) - 1:
            lo_end, hi_end = run[i], run[i + 1]
            # A recess's two ends bulge APART (low end toward -long, high end toward +long), so the
            # void is between their flats. The reverse pair is solid stock between recesses — skip.
            if not (lo_end[6] == -1 and hi_end[6] == 1 and hi_end[5] - lo_end[5] > _MERGE_TOL):
                i += 1
                continue
            lo_f, hi_f = round(lo_end[5], 2), round(hi_end[5], 2)
            s = Slot(
                width_axis=wa,
                long_axis=la,
                width=round(2 * rad, 2),
                length=round(hi_f - lo_f, 2),
                w_center=round(wc, 2),
                lo=lo_f,
                hi=hi_f,
                d_lo=round(dlo, 2),
                d_hi=round(dhi, 2),
            )
            # Confirm a real channel (side walls) joins the ends, rather than two D-cutouts
            # bridging solid. On failure the caps may belong to a later valid pair, so advance
            # by one.
            if not _has_side_walls(faces, s):
                i += 1
                continue
            # Route on the EXACT floor count: a pocket is capped on ONE end (floor + opening); a
            # through-slot on neither; a sealed internal void (both ends capped) is neither — do not
            # emit it as a full-thickness-deep pocket.
            floor_ends = _floor_end_faces(faces, s)
            n_floor = sum(bool(end) for end in floor_ends)
            if blind and n_floor == 1:
                open_sign = 1 if floor_ends[0] else -1
                record = Pocket(
                    width_axis=wa,
                    long_axis=la,
                    width=round(2 * rad, 2),
                    length=round(hi_f - lo_f, 2),
                    depth=round(dhi - dlo, 2),
                    w_center=round(wc, 2),
                    lo=lo_f,
                    hi=hi_f,
                    d_lo=round(dlo, 2),
                    d_hi=round(dhi, 2),
                    # which face this obround pocket opens through
                    open_sign=open_sign,
                )
                out.append(record)
                selected_floor = floor_ends[0] if floor_ends[0] else floor_ends[1]
                floors = frozenset(face.node for face in selected_floor if face.node is not None)
                proposed.append(
                    _RecessProposal(
                        record,
                        caps=(lo_end[9], hi_end[9]),
                        floors=floors,
                    )
                )
                i += 2
            elif not blind and n_floor == 0:
                out.append(s)
                proposed.append(_RecessProposal(s, caps=(lo_end[9], hi_end[9])))
                i += 2
            else:
                i += 1  # not ours: a pocket in a slot scan, a slot in a pocket scan, or a void
    if proposals:
        return proposed
    return cast(list[Slot] | list[Pocket], out)
