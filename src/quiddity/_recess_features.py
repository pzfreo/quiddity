# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Public slot, pocket, and channel recognisers over one shared recess core."""

from __future__ import annotations

from dataclasses import replace
from functools import partial

from quiddity._adjacency import FaceEdges, FaceGraph, FaceNode, SolidRef
from quiddity._body_identity import unambiguous_body_keys
from quiddity._candidates import FamilyId
from quiddity._claims import ClaimLedger, EvidenceWriter
from quiddity._recess_core import (
    _channel_proposals_one,
    _channel_sort_key,
    _ChannelProposal,
    _pocket_proposals_one,
    _recognise_pockets_one,
    _recognise_slots_one,
    _slot_proposals_one,
)
from quiddity._recess_records import Channel, Pocket, Slot
from quiddity._recess_reduce import (
    _body_scoped_pairs,
    _body_scoped_proposals,
    _region_center,
)
from quiddity._solid_properties import solid_properties
from quiddity._typing import Part


class _SlotAttributionError(ValueError):
    """A public Slot occurrence whose complete original ownership cannot be issued."""


class _PocketAttributionError(ValueError):
    """A Pocket occurrence whose complete original ownership cannot be issued."""


def recognise_slots(
    part: Part,
    *,
    face_edges: FaceEdges | None = None,
    ledger: ClaimLedger | EvidenceWriter | None = None,
) -> list[Slot]:
    """Recognise enclosed through-slots independently within each solid in *part*.

    Returns a list of :class:`Slot`, one per physical feature, in a
    deterministic order (co-located candidate pairs within a solid are merged,
    keeping the narrower width). See the module docstring for the recognition
    predicate and its deliberately narrow scope. Obround slots too stubby for
    their flat walls to pair are recovered from their end caps.

    A compound is scanned per solid so faces from separate components cannot
    combine into a fictitious slot across the gap between them.

    *ledger* is injected the way *face_edges* is, and records which faces each returned slot was
    built from -- its two walls, plus the walls of every candidate folded into it: the same void
    seen through its other wall pair, and the arms a crossing channel split it into. It changes
    nothing about what is returned: claims are written and never read here, so no slot's
    existence can depend on another family having run. It exists so a second family can ask
    whether it is describing the same void, instead of comparing record coordinates.

    *ledger*'s graph must have been built from *part*; a face that does not resolve against it
    is refused rather than silently claiming nothing, because an empty ledger would otherwise
    read as "no overlap" to the reconciler it exists to serve.

    Writer-enabled discovery records the complete source set selected by the occurrence route:
    every planar wall intentionally retained by merge/collapse plus every patch in the selected
    low/high cylindrical cap groups. Consumers that compare planar overlap continue to see the
    same wall subset; cap-recovered occurrences now carry the evidence that establishes them.
    """
    solids = list(part.solids())
    sources = solids if len(solids) > 1 else [part]
    if ledger is None:
        pairs = _body_scoped_pairs(
            sources,
            partial(_recognise_slots_one, face_edges=face_edges),
        )
        pairs.sort(key=lambda pair: (pair[0].width, _region_center(pair[0])))
        return [record for record, _nodes in pairs]
    writer = ledger.writer if isinstance(ledger, ClaimLedger) else ledger
    return _discover_slots(part, face_edges=face_edges, writer=writer, _wrap_identity_errors=False)


def _discover_slots(
    part: Part,
    *,
    face_edges: FaceEdges | None = None,
    graph: FaceGraph | None = None,
    writer: EvidenceWriter | None = None,
    _wrap_identity_errors: bool = True,
) -> list[Slot]:
    """Discover Slots and optionally issue every selected wall and cap source patch."""

    if graph is not None and writer is not None and graph is not writer.graph:
        raise _SlotAttributionError("Slot graph and writer must share one authority")
    owner = writer.graph if writer is not None else graph
    solids = list(part.solids())
    sources = solids if len(solids) > 1 else [part]
    properties = solid_properties(owner)
    recognise_one = partial(_slot_proposals_one, face_edges=face_edges, graph=owner)
    if writer is None:
        proposals = _body_scoped_proposals(sources, recognise_one, properties=properties)
    else:
        # Close the graph/run authority boundary before geometry discovery. Once every source
        # face resolves, unrelated kernel and predicate defects remain geometry failures and are
        # deliberately not relabelled as attribution errors.
        try:
            for face in part.faces():
                writer.graph.require_node(face)
        except ValueError as exc:
            if not _wrap_identity_errors:
                raise
            raise _SlotAttributionError("Slot source identity does not belong to this run") from exc
        try:
            proposals = _body_scoped_proposals(sources, recognise_one, properties=properties)
        except ValueError as exc:
            if "obround cap clusters compete" not in str(exc):
                raise
            raise _SlotAttributionError("Slot endpoint cap ownership is ambiguous") from exc
    proposals.sort(key=lambda proposal: (proposal.record.width, _region_center(proposal.record)))
    records = [proposal.record for proposal in proposals]
    if writer is None:
        return records

    pending: list[tuple[Slot, frozenset[FaceNode], SolidRef]] = []
    try:
        for proposal in proposals:
            nodes = frozenset(
                (*proposal.planar, *(node for group in proposal.caps for node in group))
            )
            if not nodes:
                raise _SlotAttributionError("Slot occurrence has no defining source faces")
            for node in nodes:
                writer.graph.face(node)
            solid = writer.graph.common_valid_solid(nodes)
            if solid is None:
                raise _SlotAttributionError("Slot source faces do not prove one valid solid")
            duplicate = False
            for other_record, other_nodes, other_solid in pending:
                if proposal.record == other_record and solid == other_solid:
                    if nodes == other_nodes:
                        duplicate = True
                        break
                    raise _SlotAttributionError(
                        "equal Slot record has competing source roles on one solid"
                    )
                if not nodes.isdisjoint(other_nodes) and solid != other_solid:
                    raise _SlotAttributionError(
                        "Slot source face is ambiguously reused by another occurrence"
                    )
            if duplicate:
                continue
            pending.append((proposal.record, nodes, solid))
    except _SlotAttributionError:
        raise
    except (IndexError, KeyError, ValueError) as exc:
        if not _wrap_identity_errors:
            raise
        raise _SlotAttributionError("Slot source identity does not belong to this run") from exc
    records = [record for record, _nodes, _solid in pending]
    for record, nodes, _solid in pending:
        writer.add_defining(record, nodes, family=FamilyId.SLOTS)
    return records


def recognise_pockets(
    part: Part,
    *,
    face_edges: FaceEdges | None = None,
    ledger: ClaimLedger | EvidenceWriter | None = None,
) -> list[Pocket]:
    """Recognise blind rectangular recesses independently within each solid.

    The blind counterpart of :func:`recognise_slots`: the same facing-rectangular-wall
    candidate scan, but keeping the pairs a floor caps. The depth (open-face-to-floor
    extent) is read from the axis the floor is normal to -- see
    :func:`_pocket_candidate` -- not from a size heuristic, so a pocket deeper than it
    is long is dimensioned correctly. A compound is scanned per solid so separate
    components cannot supply walls or floors for one fictitious recess.

    *ledger* is injected the way it is on :func:`recognise_slots`, and changes nothing about
    what is returned: claims are written and never read here. What a pocket claims depends on
    how it was found. From opposed walls it claims the two walls, and *not* the floor, which
    only had to exist -- the same line the through-slot draws, since the depth is the walls'
    own overlap rather than the floor's position. From a corner notch it claims the floor too,
    because that path iterates floors and reads the notch's footprint off the one it finds. A
    stubby obround pocket owns the complete low/high cylindrical cap patch clusters that establish
    it. An elongated obround owns those cap patches in addition to its retained planar walls.

    *ledger*'s graph must have been built from *part*; a face that does not resolve is refused
    rather than silently claiming nothing.
    """
    solids = list(part.solids())
    sources = solids if len(solids) > 1 else [part]
    if ledger is None:
        pairs = _body_scoped_pairs(
            sources,
            partial(_recognise_pockets_one, face_edges=face_edges),
        )
        pairs.sort(key=lambda pair: (pair[0].width, _region_center(pair[0])))
        return [record for record, _nodes in pairs]
    writer = ledger.writer if isinstance(ledger, ClaimLedger) else ledger
    return _discover_pockets(part, face_edges=face_edges, writer=writer, _wrap_errors=False)


def _discover_pockets(
    part: Part,
    *,
    face_edges: FaceEdges | None = None,
    graph: FaceGraph | None = None,
    writer: EvidenceWriter | None = None,
    _wrap_errors: bool = True,
) -> list[Pocket]:
    """Discover Pockets and optionally issue complete route-selected source faces."""

    if graph is not None and writer is not None and graph is not writer.graph:
        raise _PocketAttributionError("Pocket graph and writer must share one authority")
    owner = writer.graph if writer is not None else graph
    solids = list(part.solids())
    sources = solids if len(solids) > 1 else [part]
    if writer is not None:
        try:
            for face in part.faces():
                writer.graph.require_node(face)
        except ValueError as exc:
            if not _wrap_errors:
                raise
            raise _PocketAttributionError(
                "Pocket source identity does not belong to this run"
            ) from exc
    try:
        proposals = _body_scoped_proposals(
            sources,
            partial(_pocket_proposals_one, face_edges=face_edges, graph=owner),
            properties=solid_properties(owner),
        )
    except ValueError as exc:
        if "obround cap clusters compete" not in str(exc):
            raise
        raise _PocketAttributionError("Pocket endpoint cap ownership is ambiguous") from exc
    proposals.sort(key=lambda proposal: (proposal.record.width, _region_center(proposal.record)))
    records = [proposal.record for proposal in proposals]
    if writer is None:
        return records

    staged = []
    try:
        for proposal in proposals:
            nodes = frozenset(
                (*proposal.planar, *(node for group in proposal.caps for node in group))
            )
            if not nodes:
                raise _PocketAttributionError("Pocket occurrence has no defining source faces")
            if not proposal.record.edge_anchored and not proposal.floors:
                raise _PocketAttributionError("Pocket floor identity is unavailable")
            members = nodes | proposal.floors | proposal.constituent
            for node in members:
                writer.graph.face(node)
            solid = writer.graph.common_valid_solid(members)
            if solid is None:
                raise _PocketAttributionError("Pocket source faces do not prove one valid solid")
            staged.append((proposal.record, nodes, members, solid))
    except _PocketAttributionError:
        raise
    except (IndexError, KeyError, ValueError) as exc:
        if not _wrap_errors:
            raise
        raise _PocketAttributionError("Pocket source identity does not belong to this run") from exc
    pending = []
    seen: dict[tuple[Pocket, object], tuple[frozenset, frozenset]] = {}
    for record, nodes, members, solid in staged:
        key = (record, solid)
        prior = seen.get(key)
        if prior is not None:
            if prior != (nodes, members):
                raise _PocketAttributionError("equal Pocket value has competing source assignments")
            continue
        seen[key] = (nodes, members)
        pending.append((record, nodes, members))
    for record, nodes, members in pending:
        writer.add_defining(
            record,
            nodes,
            family=FamilyId.POCKETS,
            constituent=members,
        )
    return [record for record, _nodes, _members in pending]


def recognise_channels(
    part: Part,
    *,
    face_edges: FaceEdges | None = None,
    ledger: ClaimLedger | EvidenceWriter | None = None,
) -> list[Channel]:
    """Recognise full-span floored channels independently within each solid.

    This is deliberately separate from :func:`recognise_pockets`: a channel's length
    and depth participate in the surrounding envelope/plate scheme, while only its
    wall-to-wall width is an independent defining measurement. Body-local bounds prove
    that the channel reaches the ends of the same solid whose faces bound it.

    *ledger* is accepted but **never written to**: this family claims nothing, because no rule
    needs to ask what a channel was built from. What the parameter is for is the *graph* --
    `_planar_faces` reads each face's material-side normal from it, and a family without one
    builds its own. Passing the run's keeps a census to a single graph rather than one per solid
    for this family alone.
    """
    return _discover_channels(
        part,
        face_edges=face_edges,
        graph=None if ledger is None else ledger.graph,
    )


def _discover_channels(
    part: Part,
    *,
    face_edges: FaceEdges | None = None,
    graph: FaceGraph | None = None,
    writer: EvidenceWriter | None = None,
) -> list[Channel]:
    """Discover Channels and optionally issue their exact opposed-wall evidence."""

    if graph is not None and writer is not None and graph is not writer.graph:
        raise ValueError("Channel graph and writer must share one authority")
    owner = writer.graph if writer is not None else graph
    solids = list(part.solids())
    # Use the physical solid even through a one-solid Compound wrapper. Plate uses the same
    # scope, so signing the wrapper here (whose aggregate volume can be zero) would make two
    # records from one body carry different keys.
    sources = solids or [part]
    retained: list[_ChannelProposal] = []
    body_keys = unambiguous_body_keys(
        sources, require_valid_solid=True, properties=solid_properties(owner)
    )
    for solid, body_key in zip(sources, body_keys, strict=True):
        proposals = _channel_proposals_one(solid, face_edges, owner)
        by_record: dict[Channel, list[_ChannelProposal]] = {}
        for proposal in proposals:
            by_record.setdefault(proposal.record, []).append(proposal)
        for record in sorted(by_record, key=_channel_sort_key):
            unique = {}
            for proposal in by_record[record]:
                unique[(proposal.low_wall, proposal.high_wall)] = proposal
            if writer is not None and len(unique) != 1:
                raise ValueError("Channel value has ambiguous opposed-wall occurrences")
            proposal = next(iter(unique.values()))
            retained.append(replace(proposal, record=replace(proposal.record, body_key=body_key)))

    retained.sort(key=lambda proposal: _channel_sort_key(proposal.record))
    if writer is not None:
        pending = []
        for proposal in retained:
            nodes = (proposal.low_wall, proposal.high_wall)
            if nodes[0] == nodes[1]:
                raise ValueError("Channel side walls must be distinct")
            if not proposal.floor or not proposal.floor.isdisjoint(nodes):
                raise ValueError("Channel floor identity is unavailable")
            # Revalidate the graph-issued snapshots immediately before publication.
            writer.graph.face(nodes[0])
            writer.graph.face(nodes[1])
            members = (*nodes, *proposal.floor)
            if writer.graph.common_valid_solid(members) is None:
                raise ValueError("Channel faces do not prove one valid solid")
            pending.append((proposal.record, nodes, members))
        for record, nodes, members in pending:
            writer.add_defining(
                record,
                nodes,
                family=FamilyId.CHANNELS,
                constituent=members,
            )
    return [proposal.record for proposal in retained]
