# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Which faces established which candidate: run-local and append-only during discovery.

Two recognisers can describe the same physical region, and until now the package could only ask
*where* their records were, not *what they were built from*. `recognise_passages` suppressed a
passage whose XY centre matched a slot's to within 1e-6 — comparing two quantities derived by
different procedures, ignoring the passage's own axis, and making the census depend on the order
its families run in. The question it needed was "do these two claim the same faces?", and nothing
here could answer it.

**The ledger is not the graph.** :class:`quiddity._adjacency.FaceGraph` holds geometric
fact: a face's bounds, its normal, which faces it meets. Those are true of the solid whoever is
looking. A claim is an *interpretation* — this recogniser believes these faces make a passage —
and a competing recogniser may interpret the same faces differently. Keeping interpretations out
of the graph is what lets the graph stay immutable and shared while each run, or a rerun of one
family, gets a fresh ledger.

**Write-only during discovery.** Migrated recogniser cores receive only `EvidenceSink`, so they
cannot read what another family proposed. Once all participants in one current conflict have run,
orchestration may copy their issued prefix into an immutable `EvidenceIndex` for that reconciler.
Standalone compatibility code may still take a point-in-time snapshot without closing issuance.
Aggregate orchestration instead seals once after every physical family has completed. A recogniser
that declined a face because another family had claimed it would make output order-dependent, and
that stays forbidden by ADR 0003.

**A claim is defining ownership, not every face a predicate touched.** Treating contextual stock
as consumed would manufacture conflicts between features that legitimately share it. Candidate
evidence therefore records only defining nodes. The one demonstrated failed-predicate diagnostic
uses a separate Observation for its subject and terminal context.

**A claim is an object, not an index into a table.** `add_defining` hands back the claim
itself, so it carries its own claimant and its own defining faces and there is no id to look up
in the wrong ledger. Claims compare by identity, which is what keeps two equal-valued candidates
apart: records compare by value, so a table keyed on the record would have merged them into one
entry covering both their faces, and a reconciler asked which to keep would have found one claim
naming both sets.

**Overlap is not exclusion.** Two claims sharing a defining face is evidence for a reconciler to
weigh, not a verdict. Which of them survives — or whether both do, as a pattern and its members
both legitimately do — is the reconciler's policy under ADR 0003, and is not decided here.
"""

from __future__ import annotations

from collections.abc import Iterable, Sequence
from dataclasses import dataclass

from quiddity._adjacency import FaceGraph, FaceNode
from quiddity._candidates import (
    Candidate,
    CandidateSet,
    CompletedInputs,
    EvidenceIndex,
    EvidenceSink,
    FamilyId,
    _CandidateIssuer,
)
from quiddity._effective_surfaces import SurfaceUse


@dataclass(frozen=True, eq=False, repr=False, slots=True)
class Claim:
    """What one recogniser believes established one candidate.

    Compared by identity, deliberately. Two candidates whose records are equal are still two
    candidates, and a reconciler has to be able to tell them apart.

    Frozen, because the ledger indexes each claim under the nodes it named. A writable
    ``defining`` would let a claim say it owns one face while ``claims_of`` still answered for
    another, so the two views of one ledger could contradict each other -- append-only in name
    only.
    """

    claimant: object
    defining: frozenset[FaceNode]
    _candidate: Candidate[object]

    def __repr__(self) -> str:
        return f"Claim({self.claimant!r}, defining={len(self.defining)} faces)"


class EvidenceWriter:
    """Aggregate discovery's graph-bound, write-only compatibility capability."""

    __slots__ = ("graph", "sink")

    def __init__(self, graph: FaceGraph, sink: EvidenceSink) -> None:
        self.graph = graph
        self.sink = sink

    def add_defining(
        self,
        claimant: object,
        nodes: Iterable[FaceNode],
        *,
        family: FamilyId = FamilyId.LEGACY,
        constituent: Iterable[FaceNode] | None = None,
        surfaces: Iterable[SurfaceUse] = (),
        groups: Iterable[Iterable[FaceNode]] = (),
    ) -> Candidate[object]:
        """Issue defining and optional wider constituent evidence without read authority."""

        defining = tuple(nodes)
        if not defining:
            raise ValueError(f"{claimant!r} claims no defining face")
        return self.sink.propose(
            family,
            claimant,
            defining=defining,
            constituent=None if constituent is None else tuple(constituent),
            surfaces=surfaces,
            groups=groups,
        )


class ClaimLedger:
    """Append-only claims against the nodes of one :class:`FaceGraph`.

    Bound to a graph, and the binding is enforced rather than assumed: a node the graph did not
    issue is refused, so a ledger paired with the wrong graph cannot report overlaps between the
    faces of different solids.
    """

    def __init__(self, graph: FaceGraph, *, definitions: Sequence[object] = ()) -> None:
        self._graph = graph
        self._claims: dict[int, Claim] = {}
        self._issuer = _CandidateIssuer(
            graph, on_issued=self._candidate_issued, definitions=definitions
        )

    def _candidate_issued(self, candidate: Candidate[object]) -> None:
        """Maintain the legacy Claim view for every non-empty sink issuance."""

        if candidate.evidence.defining:
            self._claims[id(candidate)] = Claim(
                candidate.record,
                candidate.evidence.defining,
                candidate,
            )

    def __len__(self) -> int:
        return len(self._claims)

    @property
    def graph(self) -> FaceGraph:
        """The graph whose nodes this ledger accepts.

        Exposed because a recogniser handed a ledger needs the graph to turn its faces into
        nodes, and passing both would let a caller pair the wrong two.
        """

        return self._graph

    @property
    def sink(self) -> EvidenceSink:
        """The write-only proposal capability for migrated discovery paths."""

        return self._issuer.sink

    @property
    def writer(self) -> EvidenceWriter:
        """The graph-bound write-only adapter used by aggregate discovery."""

        return EvidenceWriter(self._graph, self.sink)

    def propose(
        self,
        family: FamilyId,
        record: object,
        nodes: Iterable[FaceNode] = (),
    ) -> Candidate[object]:
        """Issue one candidate and retain a legacy Claim view for non-empty evidence."""

        return self.sink.propose(family, record, defining=nodes)

    def add_defining(
        self,
        claimant: object,
        nodes: Iterable[FaceNode],
        *,
        family: FamilyId = FamilyId.LEGACY,
    ) -> Claim:
        """Record that *nodes* are what established *claimant*, and return the claim.

        *claimant* is normally the record itself; nothing here interprets it.

        Refuses an empty set. A claim naming no face can never appear in :meth:`claims_of`, so
        it can take no part in reconciliation or in per-face scoring -- the only two things
        claims exist for. A candidate with no defining face belongs outside this ledger until a
        family gives absence an explicit meaning.
        """

        defining = tuple(nodes)
        if not defining:
            raise ValueError(f"{claimant!r} claims no defining face")
        candidate = self.propose(family, claimant, defining)
        return self._claims[id(candidate)]

    @property
    def claims(self) -> tuple[Claim, ...]:
        """Every claim, in the order it was made."""

        return tuple(
            self._claims[id(candidate)]
            for candidate in self._issuer.candidates
            if id(candidate) in self._claims
        )

    def candidate_set(self, family: FamilyId) -> CandidateSet[object]:
        """Candidates issued for *family*, preserving proposal order."""

        return self._issuer.candidate_set(family)

    def candidate_set_for(
        self, family: FamilyId, records: Iterable[object]
    ) -> CandidateSet[object]:
        """Bind returned record occurrences to their one family candidate."""

        return self._issuer.candidate_set_for(family, records)

    def restricted_inputs(self, definition: object) -> CompletedInputs:
        """Issue an opaque view of exactly one consumer's completed predecessors."""

        return self._issuer.restricted_inputs(definition)

    def snapshot_index(self) -> EvidenceIndex:
        """Freeze the evidence issued so far into a point-in-time read capability.

        This does not close issuance. Later proposals remain visible to a later snapshot but
        cannot change this one; aggregate orchestration uses :meth:`freeze_index` instead.
        """

        return self._issuer.snapshot_index()

    def freeze_index(self) -> EvidenceIndex:
        """Seal aggregate issuance and return the complete evidence index."""

        return self._issuer.freeze_index()

    def defining_of(self, claimant: object) -> frozenset[FaceNode]:
        """The faces *this* candidate was established by, or empty when it claimed none.

        Record lookup fails closed when the same record object backs multiple candidates; callers
        at that boundary must use Candidate identity instead.

        **By identity, deliberately**, and it is the direction a reconciler reads. A rule holds a
        record and needs its evidence; the alternative it replaces was pairing the returned list
        against the ledger's claims *by position*, which requires the recogniser to write claims
        in exactly the order it returns records and never say so anywhere the reader can check.
        `strict=True` catches a count that drifts. It cannot catch a permutation, and a
        permutation attaches every record to another record's faces while the counts stay right.

        That is not hypothetical: a mutation in the chamfer/angled-step work that claimed before
        sorting rather than after did precisely this, kept the slant and dropped the real
        chamfer, and survived the whole golden corpus. It was caught only by a part built so the
        kernel's traversal order and the record sort order disagree.

        Keyed on ``id`` and safe to be: the ledger holds the claim, the claim holds the claimant,
        so nothing here can be collected and have its address reused while the entry lives. That
        is the property the identity-keyed caches this project got wrong twice did not have.

        Empty rather than None for a record that claimed nothing -- a stubby obround slot
        recovered from its end caps is the case -- because "established by no face I can name" is
        an answer a rule can act on, and every rule here asks whether some other family's faces
        are inside this record's.
        """

        return self._issuer.defining_of(claimant)

    def claims_of(self, node: FaceNode) -> tuple[Claim, ...]:
        """Every claim naming *node* as defining, in claim order; empty for an unclaimed node.

        A foreign node raises rather than reporting nothing. Answering ``()`` would let a
        reconciler read "belongs to another graph" as "this face is unclaimed" -- a silent false
        negative in exactly the ownership logic this layer exists to protect, and the one shape
        of error a read-side check can catch that the write-side one cannot.

        This is the direction both consumers read. A reconciler asks it of a candidate's own
        nodes to find what else claims them, and per-face corpus scoring asks it of every node
        -- which ``docs/capabilities.md`` records as impossible today, attribution there being
        statistical rather than per-face.
        """

        return tuple(self._claims[id(candidate)] for candidate in self._issuer.candidates_of(node))
