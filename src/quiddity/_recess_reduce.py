# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Many candidates into the features they describe.

A recess scan proposes the same void more than once -- seen through its other wall pair, split
into arms by a channel crossing it, or found again from its end caps. Nothing above this module
should have to know that, so this is where a list of candidates becomes a list of features:
`_merge` folds co-located ones together, `_collapse_collinear` rejoins the arms a crossing
feature split, and `_absorb` keeps the claims straight while they do it -- a folded candidate's
faces belong to the feature that survives, or the ledger would report a void nobody built.

`_body_scoped_pairs` is the other half of the same idea at a larger grain: a compound is scanned
per solid, so faces from separate components cannot combine into a feature spanning the gap
between them, and identical bodies are recognised once and their answer reused.

Reduction only. Nothing here reads a face; it works on candidates that
:mod:`quiddity._recess_faces` has already produced.
"""

from __future__ import annotations

import math
from dataclasses import dataclass, replace
from typing import Generic, TypeVar

from quiddity._adjacency import FaceNode
from quiddity._body_identity import body_signature, unambiguous_body_keys
from quiddity._recess_faces import _MERGE_TOL
from quiddity._recess_records import Pocket, Slot
from quiddity._solid_properties import SolidProperties
from quiddity._typing import Part
from quiddity._volume_probe import prism_is_empty, prism_material_fraction

_R = TypeVar("_R", Slot, Pocket)


@dataclass(frozen=True, eq=False, slots=True)
class _RecessProposal(Generic[_R]):
    """One exact recess occurrence and the original topology carried through reduction.

    ``record`` remains the public value.  Proposal identity is deliberately object identity:
    equal records on separate solids are separate occurrences until the body-scoped projection.
    Planar defining nodes, cylindrical cap groups and accepted floors stay separate because this
    is neutral plumbing; family publication decides which roles are defining or constituent.
    """

    record: _R
    planar: frozenset[FaceNode] = frozenset()
    caps: tuple[frozenset[FaceNode], ...] = ()
    floors: frozenset[FaceNode] = frozenset()
    constituent: frozenset[FaceNode] = frozenset()


def _replace_proposal(proposal: _RecessProposal[_R], record: _R) -> _RecessProposal[_R]:
    return _RecessProposal(
        record, proposal.planar, proposal.caps, proposal.floors, proposal.constituent
    )


def _combine_proposals(record: _R, proposals: list[_RecessProposal[_R]]) -> _RecessProposal[_R]:
    planar = frozenset(node for proposal in proposals for node in proposal.planar)
    floors = frozenset(node for proposal in proposals for node in proposal.floors)
    constituent = frozenset(node for proposal in proposals for node in proposal.constituent)
    cap_groups: list[frozenset[FaceNode]] = []
    for proposal in proposals:
        for group in proposal.caps:
            if group not in cap_groups:
                cap_groups.append(group)
    return _RecessProposal(record, planar, tuple(cap_groups), floors, constituent)


#: Which faces a record was built from, while it is being built. Keyed by the record's *value*,
#: which is safe here and nowhere else: this map lives inside one recognition of one part, and
#: `_merge` already treats two candidates within `_MERGE_TOL` of each other as one feature -- so
#: two value-equal candidates are, by the pipeline's own definition, the same slot. The ledger
#: the values end up in keys by claim identity instead, because there two equal-valued *records*
#: really can be two features.
#:
#: Spelled out rather than left as a bare ``dict``: the alias is the only description this map
#: has, and an unparameterised one type-checks ``setdefault(<anything>, 1).no_such_method()``
#: clean. `_Face.bb` below records what that costs on a field this module touches constantly.
_Claims = dict[Slot | Pocket, set[FaceNode]]

_VOID_INSET = 0.1

_VOID_VOL_FRAC = 0.01


def _prism_material_fraction(
    spans: dict[str, tuple[float, float]], part: Part, *, inset: float = _VOID_INSET
) -> float:
    """Compatibility facade over the policy-neutral volumetric measurement."""

    return prism_material_fraction(spans, part, inset=inset)


def _prism_is_empty(spans: dict[str, tuple[float, float]], part: Part, *, inset: float) -> bool:
    """Compatibility facade over the exact-empty volumetric measurement."""

    return prism_is_empty(spans, part, inset=inset)


def _absorb(claims: _Claims | None, into: Slot | Pocket, *from_: Slot | Pocket) -> None:
    """Give *into* the nodes of every record in *from_*, the records the pipeline replaces by it.

    Every transform below rebuilds records rather than mutating them -- `_merge` keeps one of a
    group, `_collapse_collinear` spans several into one, `_extend_obround_ends` `replace`s
    fields -- so without this the claim would be attached to a record that never reaches the
    caller. `_body_scoped_pairs` `replace`s a field too, but reads the map before it does rather
    than going through here, because that is also where the map is scoped to one solid.
    """

    if claims is None:
        return
    nodes = claims.setdefault(into, set())
    for record in from_:
        nodes |= claims.get(record, set())


# Backward-compatible private import used by the aggregate registry and tests.
_body_signature = body_signature


def _body_scoped_pairs(
    sources,
    recognise_one,
    claims: _Claims | None = None,
    *,
    properties: SolidProperties | None = None,
) -> list[tuple]:
    """The same, paired with the nodes each record was built from.

    The claim is read **per solid**, before the next one runs, and the map is cleared between
    them. Reading it afterwards would have been wrong for a compound: the map is keyed by record
    value, and two solids occupying the same space produce value-equal slots *and* duplicate body
    signatures -- so both records would have come back carrying the union of both solids' faces.
    That is precisely the cross-solid confusion `body_key` fails closed on, and it would have
    reappeared in the claims.
    """

    keys = unambiguous_body_keys(sources, properties=properties)
    out: list[tuple] = []
    for solid, body_key in zip(sources, keys, strict=True):
        if claims is not None:
            claims.clear()
        for record in recognise_one(solid):
            keyed = replace(record, body_key=body_key)
            nodes = frozenset() if claims is None else frozenset(claims.get(record, ()))
            out.append((keyed, nodes))
    return out


def _body_scoped_proposals(
    sources, recognise_one, *, properties: SolidProperties | None = None
) -> list[_RecessProposal]:
    """Body-scope exact occurrences without using record values as provenance authority."""

    keys = unambiguous_body_keys(sources, properties=properties)
    out: list[_RecessProposal] = []
    for solid, body_key in zip(sources, keys, strict=True):
        for proposal in recognise_one(solid):
            keyed = replace(
                proposal.record,
                body_key=body_key,
            )
            out.append(_replace_proposal(proposal, keyed))
    return out


def _same_channel_line(a: Slot, b: Slot) -> tuple[float, float] | None:
    """When ``a`` and ``b`` are collinear co-axial slot *arms* — same wall plane
    (width axis, centreline, width and depth extent) but disjoint along their run
    — return the gap ``(g_lo, g_hi)`` between them along ``long_axis``; else None.

    Two arms of one channel that a crossing cut has split share every
    dimension but their run; two genuinely parallel slots have different
    centrelines (``w_center``) and never reach here."""
    if a.width_axis != b.width_axis or a.long_axis != b.long_axis:
        return None
    if abs(a.w_center - b.w_center) > _MERGE_TOL or abs(a.width - b.width) > _MERGE_TOL:
        return None
    if abs(a.d_lo - b.d_lo) > _MERGE_TOL or abs(a.d_hi - b.d_hi) > _MERGE_TOL:
        return None
    if a.hi <= b.lo:
        gap = (a.hi, b.lo)
    elif b.hi <= a.lo:
        gap = (b.hi, a.lo)
    else:
        return None  # overlapping along the run — not two disjoint arms
    return gap if gap[1] - gap[0] > 0 else None


def _gap_is_void(gap, arm: Slot, part: Part) -> bool:
    """Whether the whole gap between collinear slot arms is near-empty.

    A crossing channel of matching cross-section clears it. Solid stock or a small incidental
    hole leaves substantial material, so the arms remain separate. The 1% allowance belongs to
    this reduction policy only; candidate existence uses exact emptiness instead.

    Known limitation: a wide enclosed void flush with the arm ends also empties the box and
    fuses the arms. Distinguishing that from the supported crossing-channel case needs an
    aspect-ratio policy with no clean line, so it remains deliberately out of scope.
    """

    spans = {
        arm.long_axis: (gap[0], gap[1]),
        arm.width_axis: (arm.w_center - arm.width / 2, arm.w_center + arm.width / 2),
        arm.depth_axis: (arm.d_lo, arm.d_hi),
    }
    return _prism_material_fraction(spans, part) <= _VOID_VOL_FRAC


def _collapse_collinear(slots: list[Slot], part: Part, claims: _Claims | None = None) -> list[Slot]:
    """Recombine slot arms split by a crossing channel into whole channels.

    A ``+`` of two intersecting through-channels is milled as one continuous slot
    each, but the central intersection removes the middle of both channels' walls,
    so the wall scan yields two collinear arm-slots per channel (four total).
    Union collinear co-axial arms whose gap is void (a crossing channel passes
    between them), and span each group into a single slot running its full length.
    Arms separated by solid material — two genuinely distinct slots — are left as
    separate features."""
    parent = list(range(len(slots)))

    def find(i: int) -> int:
        while parent[i] != i:
            parent[i] = parent[parent[i]]
            i = parent[i]
        return i

    for i in range(len(slots)):
        for j in range(i + 1, len(slots)):
            gap = _same_channel_line(slots[i], slots[j])
            if gap is not None and _gap_is_void(gap, slots[i], part):
                parent[find(i)] = find(j)

    groups: dict[int, list[Slot]] = {}
    for idx, s in enumerate(slots):
        groups.setdefault(find(idx), []).append(s)

    out: list[Slot] = []
    for members in groups.values():
        if len(members) == 1:
            out.append(members[0])
            continue
        base = members[0]
        lo = min(m.lo for m in members)
        hi = max(m.hi for m in members)
        spanned = Slot(
            width_axis=base.width_axis,
            long_axis=base.long_axis,
            width=base.width,
            length=round(hi - lo, 2),
            w_center=base.w_center,
            lo=round(lo, 2),
            hi=round(hi, 2),
            d_lo=base.d_lo,
            d_hi=base.d_hi,
        )
        # One channel milled through, split into arms by a crossing channel: every arm's walls
        # bound the whole feature.
        _absorb(claims, spanned, *members)
        out.append(spanned)
    return sorted(out, key=lambda c: (c.width, _region_center(c)))


def _collapse_collinear_proposals(
    proposals: list[_RecessProposal[Slot]], part: Part
) -> list[_RecessProposal[Slot]]:
    """Occurrence-safe counterpart of :func:`_collapse_collinear`."""

    slots = [proposal.record for proposal in proposals]
    parent = list(range(len(slots)))

    def find(index: int) -> int:
        while parent[index] != index:
            parent[index] = parent[parent[index]]
            index = parent[index]
        return index

    for left in range(len(slots)):
        for right in range(left + 1, len(slots)):
            gap = _same_channel_line(slots[left], slots[right])
            if gap is not None and _gap_is_void(gap, slots[left], part):
                parent[find(left)] = find(right)
    groups: dict[int, list[_RecessProposal[Slot]]] = {}
    for index, proposal in enumerate(proposals):
        groups.setdefault(find(index), []).append(proposal)
    out: list[_RecessProposal[Slot]] = []
    for members in groups.values():
        if len(members) == 1:
            out.append(members[0])
            continue
        base = members[0].record
        lo = min(member.record.lo for member in members)
        hi = max(member.record.hi for member in members)
        spanned = Slot(
            width_axis=base.width_axis,
            long_axis=base.long_axis,
            width=base.width,
            length=round(hi - lo, 2),
            w_center=base.w_center,
            lo=round(lo, 2),
            hi=round(hi, 2),
            d_lo=base.d_lo,
            d_hi=base.d_hi,
        )
        out.append(_combine_proposals(spanned, members))
    return sorted(
        out,
        key=lambda proposal: (
            proposal.record.width,
            _region_center(proposal.record),
        ),
    )


def _region_center(s: Slot | Pocket) -> tuple[float, float, float]:
    """The slot's mid-point in part coordinates (axis-ordered)."""
    c = {
        s.width_axis: s.w_center,
        s.long_axis: (s.lo + s.hi) / 2,
        s.depth_axis: (s.d_lo + s.d_hi) / 2,
    }
    return (c["x"], c["y"], c["z"])


def _merge(candidates: list[_R], claims: _Claims | None = None) -> list[_R]:
    """A rectangular slot is bounded by two orthogonal opposed-wall pairs (the
    width walls and the length end-caps), so the same feature is detected twice
    — once per pair.  Collapse candidates that occupy the same region, keeping
    the one with the smallest width (the true across-flats).

    Sorted by ``(width, region_centre)`` so the output order — and therefore the
    ``slot{i}`` annotation names downstream — is determined by geometry alone,
    not by OCC face-iteration order (which is not stable across kernels)."""
    kept: list[_R] = []
    for s in sorted(candidates, key=lambda c: (c.width, _region_center(c))):
        cs = _region_center(s)
        keeper = next((k for k in kept if math.dist(cs, _region_center(k)) <= _MERGE_TOL), None)
        if keeper is not None:
            # The dropped candidate is the *same* feature seen through its other wall pair, so
            # its walls are as much this slot's evidence as the ones that survived.
            _absorb(claims, keeper, s)
            continue
        kept.append(s)
    return kept


def _merge_proposals(proposals: list[_RecessProposal[_R]]) -> list[_RecessProposal[_R]]:
    """Preserve first-win public reduction while unioning exact occurrence provenance."""

    kept: list[_RecessProposal[_R]] = []
    for proposal in sorted(
        proposals, key=lambda item: (item.record.width, _region_center(item.record))
    ):
        centre = _region_center(proposal.record)
        keeper = next(
            (item for item in kept if math.dist(centre, _region_center(item.record)) <= _MERGE_TOL),
            None,
        )
        if keeper is None:
            kept.append(proposal)
            continue
        index = kept.index(keeper)
        kept[index] = _combine_proposals(keeper.record, [keeper, proposal])
    return kept
