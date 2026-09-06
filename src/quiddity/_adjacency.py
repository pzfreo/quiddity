# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Internal face adjacency: which faces of a part meet along an edge.

Every recogniser that reasons about a face's surroundings needs this, and none owns it, so
it sits in ADR 0007's base layer — depending on nothing but the kernel and
:mod:`quiddity._geometry`'s thresholds, depended on by anything.

It was previously answered five separate ways: an edge→faces dict in ``_hole_features``, a
second one inline in ``fillets``, memoised pairwise closures in ``polygonal_bosses``, and
raw ``IsSame`` sweeps in ``chamfers`` and ``flats``. The line count of that duplication was
small; the risk was not, because nothing tested that five implementations of face identity
agreed. ``tests/test_adjacency.py`` now pins the one answer they share.

**Identity is build123d shape equality**, which is ``TShape`` plus ``Location`` and
orientation-insensitive — exactly the ``IsSame`` predicate the pairwise sweeps used, so
routing them through a dict is behaviour-preserving rather than merely equivalent-looking.
That equivalence is measured, not assumed: :func:`tests.test_adjacency` proves both
predicates induce the same partition of the edges *and* the faces of every pinned fixture.
"""

from __future__ import annotations

import math
from collections.abc import Callable, Iterable
from dataclasses import dataclass
from typing import Literal, Protocol, TypeVar, cast

from build123d import Edge, Solid
from OCP.BRep import BRep_Tool
from OCP.BRepAdaptor import BRepAdaptor_Curve, BRepAdaptor_Surface
from OCP.GCPnts import GCPnts_AbscissaPoint
from OCP.GeomAbs import GeomAbs_Cone, GeomAbs_Cylinder, GeomAbs_Plane, GeomAbs_Sphere
from OCP.gp import gp_Pnt, gp_Vec
from OCP.ShapeAnalysis import ShapeAnalysis_Surface
from OCP.TopAbs import TopAbs_EDGE, TopAbs_Orientation, TopAbs_WIRE
from OCP.TopExp import TopExp_Explorer
from OCP.TopoDS import TopoDS

from quiddity._analytic_surfaces import (
    SurfaceKind,
    equivalent_parameters,
    native_primitive,
    validated_parameters,
)
from quiddity._body_geometry import (
    BodyGeometryDescriptor,
    FaceGeometry,
    MatchingBoundaryGraph,
    describe_solid,
    matching_boundary_for_solid,
)
from quiddity._geometry import AXIS_ALIGNED_COS, SMOOTH_ARC_GAP, length_tol
from quiddity._typing import EdgeLike, FaceLike

_T = TypeVar("_T")


#: What an arc can be. Closed, so a caller cannot compare against a value the graph never
#: returns -- which a bare ``str`` allowed. Note that closing the set does not by itself make
#: mypy reject a non-exhaustive consumer: that needs narrowing ending in ``assert_never``, which
#: is the consumer's business rather than this alias's.
#:
#: ``"unknown"`` means *adjacent, but no single classification applies*: the geometry at the edge
#: is too degenerate to read, or the pair shares several edges that do not agree. It is not the
#: same as :meth:`FaceGraph.arc` returning None, which means the faces do not meet at all, and
#: the difference matters -- a traversal rule reading "not smooth" from an absence would be
#: concluding from silence, which `require_node` and `claims_of` both refuse to allow elsewhere.
ArcKind = Literal["convex", "concave", "smooth", "unknown"]
SmoothSide = Literal["neutral", "convex", "concave", "unproven"]

_SMOOTH_CURVATURE_GAP = 1e-6
_SIDE_SAMPLES = (0.25, 0.5, 0.75)
_ANALYTIC_KINDS = {
    GeomAbs_Plane: SurfaceKind.PLANE,
    GeomAbs_Cylinder: SurfaceKind.CYLINDER,
    GeomAbs_Cone: SurfaceKind.CONE,
    GeomAbs_Sphere: SurfaceKind.SPHERE,
}


def is_any_smooth(kind: ArcKind | None) -> bool:
    """Whether the legacy first-order arc fact proves tangent continuity."""

    return kind == "smooth"


def is_opposed_nonsmooth(left: ArcKind | None, right: ArcKind | None) -> bool:
    """Whether two proved joins are exactly one convex and one concave turn."""

    return {left, right} == {"convex", "concave"}


def same_arc_kind(left: ArcKind | None, right: ArcKind | None) -> bool:
    """Whether two legacy pair facts agree, preserving every closed/absent state exactly."""

    return left == right


@dataclass(frozen=True, slots=True)
class _SmoothSideObservation:
    samples: tuple[SmoothSide, ...]
    result: SmoothSide


class FaceEdges:
    """Per-face edge lists, computed once and shared for the length of one recognition run.

    ``Face.edges()`` is the single most expensive derived query the suite makes — measured at
    24% of a full :func:`quiddity.census.feature_census` over the pinned corpus, at
    ~107 µs a call — and every recogniser asks it of the same faces of the same part.

    **The sharing has to cross recogniser boundaries to pay.** Memoising within one
    recogniser call is worth about 2% and is a net loss for several of them; sharing one memo
    across a whole census is worth about 20%. That asymmetry is the whole reason this is a
    threaded parameter rather than a detail each recogniser could keep to itself, and it is
    why ``part.faces()`` sharing — the obvious candidate — is not the answer: re-walking the
    part's faces is only 1.9% of a census, because the walk is cheap and the *derivation*
    hanging off each face is not.

    Keyed on the face itself. build123d shape equality is ``TShape`` + ``Location``, i.e.
    ``IsSame``, proven over every fixture in :mod:`tests.test_adjacency` — so two wrappers for
    the same face, from two different ``part.faces()`` calls in two different recognisers, hit
    the same entry. That proof is what makes this safe; without it the memo would silently
    miss and quietly cost more than it saved.

    Scope it to a single run over a single part. It holds its faces alive, and it must not
    outlive geometry that could be rebuilt.

    The returned list is the memo's own, not a copy: **callers must not mutate it.** Every
    call site either iterates it or derives a new list with ``filter_by``/``sorted``.
    """

    def __init__(self) -> None:
        self._of: dict = {}

    def of(self, face: FaceLike) -> list[EdgeLike]:
        """The edges of *face*, computed on first ask and reused thereafter."""

        edges = self._of.get(face)
        if edges is None:
            self._of[face] = edges = face.edges()
        return edges


@dataclass(frozen=True, eq=False, slots=True)
class FaceNode:
    """A node of one :class:`FaceGraph`, and of no other.

    A bare integer would have carried no provenance: node 0 of one part is a perfectly valid
    index into another, so a node from the wrong graph would have been accepted and silently
    addressed the wrong face -- the accidental identity this substrate exists to remove. These
    are created only by the graph that owns them and compared by identity, so a foreign node is
    rejected rather than misread.

    Frozen, because a writable ``index`` would let a caller invalidate a handle the graph had
    issued -- ``owns`` would then refuse a node that genuinely came from this graph. ``eq=False``
    keeps comparison by identity, which is the whole mechanism.

    Opaque by design. ``index`` is a position in one part's face list, exposed because the graph
    itself needs it; nothing outside this module should read it, and it means nothing once the
    part changes.
    """

    index: int


@dataclass(frozen=True, eq=False, slots=True)
class GraphRunToken:
    """Opaque identity of one graph/run; equality is deliberately object identity."""


@dataclass(frozen=True, eq=False, slots=True)
class SolidRef:
    """Opaque run-local identity of one graph-owned valid solid."""

    ordinal: int


class BodyGeometryAuthorityError(ValueError):
    """A solid reference is foreign, stale, copied, or no longer graph-authorized."""


@dataclass(frozen=True, slots=True)
class BodyGeometryFact:
    """One graph-authorized run-local body fact with a handle-free descriptor."""

    _solid: SolidRef
    descriptor: BodyGeometryDescriptor
    _faces: tuple[tuple[FaceNode, FaceGeometry], ...]
    _matching_faces: tuple[tuple[FaceNode, object], ...]

    def _defining_face(self, node: FaceNode) -> FaceGeometry:
        for issued, geometry in self._faces:
            if issued is node:
                return geometry
        raise BodyGeometryAuthorityError("face node is not part of this graph-authorized body fact")

    def _matching_face(self, node: FaceNode) -> object:
        for issued, geometry in self._matching_faces:
            if issued is node:
                return geometry
        raise BodyGeometryAuthorityError("face node is not part of this matching body fact")


@dataclass(frozen=True, eq=False, slots=True)
class EdgeOccurrenceRef:
    """One exact oriented edge occurrence in one original face wire traversal."""

    owner: FaceNode
    wire_ordinal: int
    ordinal: int
    reversed: bool
    edge: EdgeLike


@dataclass(frozen=True, eq=False, slots=True)
class SharedEdgeOccurrenceRef:
    """One adjacency occurrence paired from the two original oriented half-edges."""

    endpoints: tuple[FaceNode, FaceNode]
    halves: tuple[EdgeOccurrenceRef, EdgeOccurrenceRef]
    edge: EdgeLike


@dataclass(frozen=True, slots=True)
class EdgeOwnershipFact:
    """Closed same-solid/two-face proof for one original adjacency occurrence."""

    solid: SolidRef
    occurrence: SharedEdgeOccurrenceRef


class FaceGraphQuery(Protocol):
    @property
    def run_token(self) -> GraphRunToken: ...

    @property
    def nodes(self) -> tuple[FaceNode, ...]: ...

    def owns(self, node: FaceNode) -> bool: ...

    def face(self, node: FaceNode) -> FaceLike: ...

    def neighbours(self, node: FaceNode) -> tuple[FaceNode, ...]: ...

    def arc(self, a: FaceNode, b: FaceNode) -> ArcKind | None: ...

    def smooth_side(self, a: FaceNode, b: FaceNode) -> SmoothSide | None: ...

    def shared_occurrences(
        self, a: FaceNode, b: FaceNode
    ) -> tuple[SharedEdgeOccurrenceRef, ...]: ...

    def edge_occurrences(self, node: FaceNode) -> tuple[EdgeOccurrenceRef, ...]: ...

    def ownership(self, occurrence: SharedEdgeOccurrenceRef) -> EdgeOwnershipFact | None: ...

    def common_valid_solid(self, nodes: Iterable[FaceNode]) -> SolidRef | None: ...

    def solid_shape(self, solid: SolidRef) -> Solid: ...

    def body_geometry(self, solid: SolidRef) -> BodyGeometryFact: ...

    def matching_boundary(self, solid: SolidRef) -> MatchingBoundaryGraph: ...


class FaceGraph:
    """One node per face of one part, for the length of one recognition run.

    Seven modules privately re-derive the same handful of things about a face: its bounding
    box (six of them), its surface type (five), its normal (three), its edges, and which faces
    it meets. The derivations are duplicated, the caches are not shared, and two recognisers
    that disagree about a face have no way to say so except by comparing coordinates. This is
    the node half of that: one place where a face's attributes are derived, once.

    **What it deliberately is not.** Not a persistent identity -- node ids are positions in
    this part's face list and mean nothing outside this object. Not a public schema: no record
    gains a field. Not sampled UV geometry or anything a learned recogniser would want. Not a
    subgraph-matching engine; the recognisers stay procedural.

    **It carries no recogniser ownership.** Original solid/edge incidence is a neutral topology
    fact, now exposed through issuer-owned private references. Which recogniser claimed which face
    is an interpretation of these facts, not one of them, and it lives in
    :class:`quiddity._claims.ClaimLedger`
    instead -- an append-only sidecar built against one graph. Keeping them apart is what lets
    this object stay immutable after construction and be reused across a whole census while
    each run, or a rerun of one recogniser, keeps its own ledger. Analysis Situs writes
    recognition results back onto the AAG itself as attributes; the separation here is the one
    deliberate divergence, and it buys immutability at the cost of a second object.

    **Every attribute is lazy**, because measurement says eagerness does not
    pay: sharing the walk over ``part.faces()`` was worth 1.9% of a census, while sharing the
    ``Face.edges()`` derivation hanging off it was worth about a fifth. Attributes are derived
    on first ask and kept; a consumer that never asks for normals never pays for them.

    Scope it to one run over one part, as :class:`FaceEdges` is scoped -- it holds the part's
    faces alive, and its node ids stop meaning anything the moment the part changes.
    """

    def __init__(self, part, *, face_edges: FaceEdges | None = None) -> None:
        self._run_token = GraphRunToken()
        self._part = part
        self._faces: list[FaceLike] = list(part.faces())
        self._nodes = tuple(FaceNode(at) for at in range(len(self._faces)))
        self._index = {face: at for at, face in enumerate(self._faces)}
        self._face_edges = face_edges
        self._edges: dict[int, tuple[EdgeLike, ...]] = {}
        self._surface: dict[int, int] = {}
        self._normal: dict[int, tuple[float, float, float] | None] = {}
        self._bounds: dict[int, tuple] = {}
        self._edge_faces: dict | None = None
        self._neighbours: dict[int, tuple[FaceNode, ...]] = {}
        self._arcs: dict[tuple[int, int], ArcKind | None] = {}
        self._smooth_regions: dict[int, frozenset[FaceNode]] = {}
        self._face_solids: tuple[tuple[int, ...], ...] | None = None
        self._closed_solids: frozenset[int] | None = None
        self._smooth_side_edges: dict[tuple[int, int, EdgeLike], _SmoothSideObservation] = {}
        self._smooth_sides: dict[tuple[int, int], SmoothSide | None] = {}
        self._solid_refs: tuple[SolidRef, ...] | None = None
        self._issued_solid_refs: dict[SolidRef, int] = {}
        self._solids: tuple | None = None
        self._body_geometry: dict[SolidRef, BodyGeometryFact] = {}
        self._edge_occurrences: dict[FaceNode, tuple[EdgeOccurrenceRef, ...]] = {}
        self._issued_edge_occurrences: dict[EdgeOccurrenceRef, tuple] = {}
        self._shared_occurrences: dict[tuple[int, int], tuple[SharedEdgeOccurrenceRef, ...]] = {}
        self._issued_shared_occurrences: dict[SharedEdgeOccurrenceRef, tuple] = {}

    @property
    def run_token(self) -> GraphRunToken:
        return self._run_token

    def __len__(self) -> int:
        return len(self._faces)

    @property
    def nodes(self) -> tuple[FaceNode, ...]:
        """Every node, in the order the kernel yielded the faces."""

        return self._nodes

    def owns(self, node: FaceNode) -> bool:
        """Whether *node* was issued by this graph, checked by identity rather than by value."""

        at = node.index
        return 0 <= at < len(self._nodes) and self._nodes[at] is node

    def _at(self, node: FaceNode) -> int:
        if not self.owns(node):
            raise ValueError(f"{node!r} was not issued by this graph")
        return node.index

    def face(self, node: FaceNode) -> FaceLike:
        return self._faces[self._at(node)]

    def node_of(self, face: FaceLike) -> FaceNode | None:
        """The node for *face*, or None when it belongs to another part.

        Keyed on build123d shape equality, so a face from a second ``part.faces()`` walk
        resolves to the same node -- the property the whole graph rests on, and the one proved
        over every pinned fixture in :mod:`tests.test_adjacency`.
        """

        at = self._index.get(face)
        return None if at is None else self._nodes[at]

    def require_node(self, face: FaceLike) -> FaceNode:
        """This graph's node for *face*, or a refusal that names the mistake.

        The counterpart to :meth:`node_of`, for the callers that cannot do anything useful with
        ``None``. A recogniser resolving faces against a graph built from a different part
        would claim nothing and hand back an empty ledger, which reads downstream as "these
        families claim nothing" rather than as "you paired the wrong graph" -- and a reconciler
        reading that concludes there is no overlap and reports the duplicate it exists to
        suppress.

        Here rather than in each caller: `_recess_core` and `grooves` had grown the same three
        lines and the same message independently, and sharing them through either module would
        have made one recogniser import another for no reason a reader could justify.
        """

        node = self.node_of(face)
        if node is None:
            raise ValueError(
                "the claim ledger's graph was built from a different part: it has no "
                f"{face.bounding_box()}"
            )
        return node

    def edges(self, node: FaceNode) -> tuple[EdgeLike, ...]:
        """The edges of *node*, computed on first ask.

        A tuple, not the memo's own list. A returned list would have made the graph mutable
        through its own API -- clearing it would change adjacency and every later answer -- and
        a "do not mutate" comment is weaker than a type that cannot be. The lazy caches behind
        this do mutate; the answers handed out do not.
        """

        at = self._at(node)
        got = self._edges.get(at)
        if got is None:
            face = self._faces[at]
            self._edges[at] = got = tuple(
                face.edges() if self._face_edges is None else self._face_edges.of(face)
            )
        return got

    def surface(self, node: FaceNode) -> int:
        """The ``GeomAbs`` surface type, so callers stop constructing their own adaptor."""

        at = self._at(node)
        got = self._surface.get(at)
        if got is None:
            self._surface[at] = got = BRepAdaptor_Surface(self._faces[at].wrapped).GetType()
        return got

    def is_planar(self, node: FaceNode) -> bool:
        return bool(self.surface(node) == GeomAbs_Plane)

    def normal(self, node: FaceNode) -> tuple[float, float, float] | None:
        """The unit normal, or None for a face too degenerate to have one.

        **Geometric, not material-side**, and the distinction is load-bearing rather than
        pedantic. This is ``normal_at()``: it points whichever way the surface's parameterisation
        does, so on a ``REVERSED`` face it points *into* the solid. A recogniser asking "which
        side is the material" wants :func:`frame_points_outward` instead, and the two differ by a
        sign on exactly the faces where it matters most. Nothing but this paragraph said so
        before, and it is why ``_recess_core._Face`` cannot simply be replaced by a node.

        None rather than an exception because every caller today wraps the kernel call in a
        ``try`` and skips the face; returning the skip makes that one decision instead of
        seven.
        """

        at = self._at(node)
        if at not in self._normal:
            try:
                unit = self._faces[at].normal_at()
            except Exception:  # noqa: BLE001 - a degenerate face has no normal to read
                self._normal[at] = None
            else:
                self._normal[at] = (unit.X, unit.Y, unit.Z)
        return self._normal[at]

    def bounds(
        self, node: FaceNode
    ) -> tuple[tuple[float, float], tuple[float, float], tuple[float, float]]:
        """``((x_lo, x_hi), (y_lo, y_hi), (z_lo, z_hi))``, indexable by axis number.

        By axis rather than as a build123d box because every caller indexes it by an axis it
        computed, and each was unpacking the box by hand to do so.
        """

        at = self._at(node)
        got = self._bounds.get(at)
        if got is None:
            bb = self._faces[at].bounding_box()
            self._bounds[at] = got = (
                (bb.min.X, bb.max.X),
                (bb.min.Y, bb.max.Y),
                (bb.min.Z, bb.max.Z),
            )
        return got

    def neighbours(self, node: FaceNode) -> tuple[FaceNode, ...]:
        """The nodes sharing an edge with *node*, excluding itself, each once.

        Order follows the face's own edge order, so it inherits the part's traversal order and
        nothing more: a caller needing determinism must sort or reduce, as the blend
        recognisers do by keeping the nearest neighbour per axis.
        """

        at = self._at(node)
        got = self._neighbours.get(at)
        if got is None:
            found: list[FaceNode] = []
            seen = {at}
            for edge in self.edges(node):
                for other in self._edge_face_map().get(edge, ()):
                    neighbour = self._index.get(other)
                    if neighbour is None or neighbour in seen:
                        continue
                    seen.add(neighbour)
                    found.append(self._nodes[neighbour])
            self._neighbours[at] = got = tuple(found)
        return got

    def arc(self, a: FaceNode, b: FaceNode) -> ArcKind | None:
        """How the solid turns where two faces meet, or None when they do not meet.

        The attribute an attributed adjacency graph is *for*, and the half this package went
        without. Nodes carry facts about a face; until this, nothing said what happened *between*
        two of them, so a recogniser needing that inferred it at the point of use or did not ask.

        - **convex** / **concave** -- whether the material forms a wedge at the edge or wraps
          around it, from the direction the boundary of *a* walks the shared edge: turning left
          from that direction, in *a*'s surface, points into *a*, and which side of *b* that
          lands on is the answer.
        - **smooth** -- the outward normals agree to :data:`SMOOTH_ARC_GAP` where the faces meet.
          A face split in two by a neighbouring feature is the exact case, its halves coplanar;
          a tangential blend is the other. This is why ADR 0004's amendment treats seeing
          *through* a blend and *across* a split as one mechanism: a split is the zero-angle
          blend.
        - **unknown** -- adjacent, but no single answer applies. See :data:`ArcKind`.

        **A property of the face pair, not of one edge**, and where they share several the
        classification must be the same at each or the answer is ``"unknown"``. Two faces meeting
        along two edges are common -- 49 such pairs across the checked-in STEP corpus -- and an
        earlier version read only ``shared_edges(...)[0]``, which made the answer depend on
        traversal order that :meth:`neighbours` does not promise. No disagreeing pair has been
        observed; requiring agreement therefore costs nothing today and makes the first one loud
        rather than silently order-dependent.

        Evaluated at a point on each shared edge, from the faces' own normals there, so it is
        total rather than planar-only: a plane has one normal everywhere and a cone does not, and
        a groove's conical lead-in is precisely the face a recogniser must classify an arc
        against.

        Cached per unordered pair. The answer is symmetric by construction -- the two faces of a
        manifold edge walk it in opposite directions, which cancels -- and the cache is keyed so
        that ``arc(a, b)`` and ``arc(b, a)`` are one entry rather than two that could drift.

        **Not a generalisation of** :func:`quiddity._bevel.convex_bevel`, which asks a
        different question -- whether the corner a bevel *replaces* is convex, that is whether
        material was removed there or added. A gusset filling a re-entrant corner has convex arcs
        to both walls it meets while filling a concave corner, and both answers are right.
        """

        # `_at` rather than `.index`, and *before* the lookup: a cache hit would otherwise
        # answer for a node this graph never issued, because the validation on the miss path
        # only happens inside `_classify_arc`. That is the provenance contract `require_node`
        # and `claims_of` both enforce, silently skipped for exactly the second caller onward.
        key_a, key_b = self._at(a), self._at(b)
        key = (min(key_a, key_b), max(key_a, key_b))
        if key not in self._arcs:
            self._arcs[key] = self._classify_arc(a, b)
        return self._arcs[key]

    def smooth_region(self, node: FaceNode) -> frozenset[FaceNode]:
        """The maximal face region reachable across only smooth arcs.

        This is the non-mutating gAAG operation used when one physical boundary arrives as
        several tangent STEP patches. Every member is cached against the same immutable set, so
        repeated wall-pair queries traverse a smooth component once per recognition run rather
        than once per candidate. Set membership, not traversal order, is the contract.
        """

        at = self._at(node)
        cached = self._smooth_regions.get(at)
        if cached is not None:
            return cached
        found = {node}
        pending = [node]
        while pending:
            current = pending.pop()
            for neighbour in self.neighbours(current):
                if neighbour in found or not is_any_smooth(self.arc(current, neighbour)):
                    continue
                found.add(neighbour)
                pending.append(neighbour)
        region = frozenset(found)
        for member in region:
            self._smooth_regions[member.index] = region
        return region

    def smooth_side(self, a: FaceNode, b: FaceNode) -> SmoothSide | None:
        """The proved material side of a legacy-smooth pair, else None or unproven.

        ``None`` means the pair is not a legacy first-order smooth join.  Enrichment never
        rewrites :meth:`arc`; every unavailable ownership or differential fact is ``unproven``.
        """

        key_a, key_b = self._at(a), self._at(b)
        key = (min(key_a, key_b), max(key_a, key_b))
        if key in self._smooth_sides:
            return self._smooth_sides[key]
        if not is_any_smooth(self.arc(a, b)):
            self._smooth_sides[key] = None
            return None
        shared = self.shared_edges(a, b)
        observations = tuple(self._smooth_side_edge(a, b, edge) for edge in shared)
        answers = {observation.result for observation in observations}
        result: SmoothSide = answers.pop() if len(answers) == 1 else "unproven"
        self._smooth_sides[key] = result
        return result

    def _smooth_side_edge(self, a: FaceNode, b: FaceNode, edge: EdgeLike) -> _SmoothSideObservation:
        key_a, key_b = self._at(a), self._at(b)
        key = (min(key_a, key_b), max(key_a, key_b), edge)
        cached = self._smooth_side_edges.get(key)
        if cached is not None:
            return cached
        result = self._derive_smooth_side_edge(a, b, edge)
        self._smooth_side_edges[key] = result
        return result

    def _derive_smooth_side_edge(
        self, a: FaceNode, b: FaceNode, edge: EdgeLike
    ) -> _SmoothSideObservation:
        if not self._eligible_side_edge(a, b, edge):
            return _SmoothSideObservation((), "unproven")
        local = min(
            float(edge.length),
            math.sqrt(float(self.face(a).area)),
            math.sqrt(float(self.face(b).area)),
        )
        if not math.isfinite(local) or local <= 0.0:
            return _SmoothSideObservation((), "unproven")
        if self._native_continuation(a, b, local=local):
            neutral: SmoothSide = "neutral"
            return _SmoothSideObservation((neutral,) * len(_SIDE_SAMPLES), neutral)
        samples = tuple(
            self._smooth_side_sample(a, b, edge, fraction, local) for fraction in _SIDE_SAMPLES
        )
        answers = set(samples)
        result: SmoothSide = answers.pop() if len(answers) == 1 else "unproven"
        return _SmoothSideObservation(samples, result)

    def _eligible_side_edge(self, a: FaceNode, b: FaceNode, edge: EdgeLike) -> bool:
        self._build_solid_ownership()
        assert self._face_solids is not None
        assert self._closed_solids is not None
        owned_a = self._face_solids[a.index]
        owned_b = self._face_solids[b.index]
        if len(owned_a) != 1 or owned_a != owned_b or owned_a[0] not in self._closed_solids:
            return False
        incident = self._edge_face_map().get(edge, ())
        nodes = tuple(self.node_of(face) for face in incident)
        return len(nodes) == 2 and None not in nodes and set(nodes) == {a, b}

    def _build_solid_ownership(self) -> None:
        if self._face_solids is not None:
            return
        owned: list[list[int]] = [[] for _ in self._nodes]
        closed: set[int] = set()
        try:
            solids = tuple(self._part.solids())
        except Exception:  # noqa: BLE001 - open/non-solid input has no ownership proof
            solids = ()
        for solid_at, solid in enumerate(solids):
            try:
                # A valid TopoDS_Solid is the closed ownership unit.  The shape's optional
                # ``Closed`` cache flag is not reliably populated by OCCT booleans.
                if solid.is_valid:
                    closed.add(solid_at)
                faces = solid.faces()
            except Exception:  # noqa: BLE001 - invalid topology cannot prove material side
                continue
            for face in faces:
                node_at = self._index.get(face)
                if node_at is not None:
                    owned[node_at].append(solid_at)
        self._face_solids = tuple(tuple(entries) for entries in owned)
        self._closed_solids = frozenset(closed)
        self._solids = solids
        self._solid_refs = tuple(SolidRef(at) for at in range(len(solids)))
        self._issued_solid_refs = {solid: solid.ordinal for solid in self._solid_refs}

    def common_valid_solid(self, nodes: Iterable[FaceNode]) -> SolidRef | None:
        """Prove that non-empty original nodes belong to one valid closed solid.

        Foreign nodes are caller errors. Ambiguous, open, non-solid, or cross-solid sets have no
        proof and return ``None``. The returned reference is run-owned and revalidated on every
        read; it is not persistent identity.
        """

        defining = tuple(nodes)
        if not defining:
            return None
        for node in defining:
            self._at(node)
        self._build_solid_ownership()
        assert self._face_solids is not None
        assert self._closed_solids is not None
        assert self._solid_refs is not None
        memberships = tuple(self._face_solids[node.index] for node in defining)
        if (
            any(len(membership) != 1 for membership in memberships)
            or any(membership != memberships[0] for membership in memberships[1:])
            or memberships[0][0] not in self._closed_solids
        ):
            return None
        solid = self._solid_refs[memberships[0][0]]
        if self._issued_solid_refs.get(solid) != solid.ordinal:
            raise ValueError("solid reference changed after issuance")
        return solid

    def body_geometry(self, solid: SolidRef) -> BodyGeometryFact:
        """Return the complete supported descriptor for one exact graph-issued solid."""

        self._build_solid_ownership()
        issued = self._issued_solid_refs.get(solid)
        if issued is None or issued != solid.ordinal:
            raise BodyGeometryAuthorityError("solid reference was not issued by this graph")
        assert self._solid_refs is not None
        assert self._solids is not None
        assert self._closed_solids is not None
        if not (0 <= issued < len(self._solid_refs)) or self._solid_refs[issued] is not solid:
            raise BodyGeometryAuthorityError("solid reference identity changed after issuance")
        if issued not in self._closed_solids:
            raise BodyGeometryAuthorityError(
                "solid reference no longer maps to a valid closed solid"
            )
        cached = self._body_geometry.get(solid)
        if cached is not None:
            return cached
        described = describe_solid(self._solids[issued])
        face_facts: list[tuple[FaceNode, FaceGeometry]] = []
        matching_face_facts: list[tuple[FaceNode, object]] = []
        for face, geometry, face_build in zip(
            described.faces, described.face_geometry, described.face_builds, strict=True
        ):
            node = self.node_of(face)
            if node is None:
                raise BodyGeometryAuthorityError("described solid face is not owned by this graph")
            face_facts.append((node, geometry))
            matching_face_facts.append((node, face_build))
        fact = BodyGeometryFact(
            solid, described.descriptor, tuple(face_facts), tuple(matching_face_facts)
        )
        self._body_geometry[solid] = fact
        return fact

    def matching_boundary(self, solid: SolidRef) -> MatchingBoundaryGraph:
        """Return the lazy schema-three graph for one exact graph-issued solid."""

        self._build_solid_ownership()
        issued = self._issued_solid_refs.get(solid)
        assert self._solid_refs is not None
        assert self._solids is not None
        assert self._closed_solids is not None
        if (
            issued is None
            or issued != solid.ordinal
            or not 0 <= issued < len(self._solid_refs)
            or self._solid_refs[issued] is not solid
            or issued not in self._closed_solids
        ):
            raise BodyGeometryAuthorityError(
                "matching boundary solid reference is no longer graph-authorized"
            )
        fact = self._body_geometry.get(solid)
        if fact is None:
            fact = self.body_geometry(solid)
        if fact._solid is not solid:
            raise BodyGeometryAuthorityError("matching boundary lost its graph-issued solid")
        solid_shape = self._solids[issued]
        matching_builds = []
        for face in solid_shape.faces():
            node = self.node_of(face)
            if node is None:
                raise BodyGeometryAuthorityError("matching solid face is not graph-owned")
            matching_builds.append(fact._matching_face(node))
        return matching_boundary_for_solid(solid_shape, fact.descriptor, tuple(matching_builds))

    def solid_shape(self, solid: SolidRef) -> Solid:
        """Return the borrowed exact solid for an issuer-owned reference.

        This is a private recognition query for same-solid kernel classification.  The reference,
        not its ordinal or shape equality, remains the authority on every read.
        """

        self._build_solid_ownership()
        issued = self._issued_solid_refs.get(solid)
        if issued is None or issued != solid.ordinal:
            raise ValueError("solid reference was not issued by this graph")
        assert self._solid_refs is not None
        assert self._solids is not None
        assert self._closed_solids is not None
        if not (0 <= issued < len(self._solid_refs)) or self._solid_refs[issued] is not solid:
            raise ValueError("solid reference identity changed after issuance")
        if issued not in self._closed_solids:
            raise ValueError("solid reference no longer maps to a valid closed solid")
        return cast(Solid, self._solids[issued])

    def _native_continuation(self, a: FaceNode, b: FaceNode, *, local: float) -> bool:
        try:
            left = BRepAdaptor_Surface(self.face(a).wrapped)
            right = BRepAdaptor_Surface(self.face(b).wrapped)
            left_kind = _ANALYTIC_KINDS.get(left.GetType())
            right_kind = _ANALYTIC_KINDS.get(right.GetType())
            if left_kind is None or left_kind is not right_kind:
                return False
            left_parameters = validated_parameters(left_kind, native_primitive(left, left_kind))
            right_parameters = validated_parameters(right_kind, native_primitive(right, right_kind))
            return equivalent_parameters(left_kind, left_parameters, right_parameters, local=local)
        except Exception:  # noqa: BLE001 - no analytic continuation proof
            return False

    def _smooth_side_sample(
        self, a: FaceNode, b: FaceNode, edge: EdgeLike, fraction: float, local: float
    ) -> SmoothSide:
        left = self._normal_curvature(a, edge, fraction)
        right = self._normal_curvature(b, edge, fraction)
        if left is None or right is None:
            return "unproven"
        values = []
        for curvature, planar in (left, right):
            scaled = curvature * local
            if abs(scaled) <= _SMOOTH_CURVATURE_GAP:
                if planar:
                    continue
                return "unproven"
            values.append(scaled)
        if not values or abs((left[0] - right[0]) * local) <= _SMOOTH_CURVATURE_GAP:
            return "unproven"
        if all(value < 0.0 for value in values):
            return "convex"
        if all(value > 0.0 for value in values):
            return "concave"
        return "unproven"

    def _normal_curvature(
        self, node: FaceNode, edge: EdgeLike, fraction: float
    ) -> tuple[float, bool] | None:
        try:
            curve = BRepAdaptor_Curve(edge.wrapped)
            parameter = GCPnts_AbscissaPoint(
                curve, fraction * float(edge.length), curve.FirstParameter()
            ).Parameter()
            point, tangent = gp_Pnt(), gp_Vec()
            curve.D1(parameter, point, tangent)
            if tangent.Magnitude() < 1e-12:
                return None
            tangent.Normalize()
            if self._edge_reversed_in_face(node, edge):
                tangent.Reverse()

            face = self.face(node)
            surface = BRepAdaptor_Surface(face.wrapped)
            uv = ShapeAnalysis_Surface(BRep_Tool.Surface_s(face.wrapped)).ValueOfUV(point, 1e-6)
            here, du, dv = gp_Pnt(), gp_Vec(), gp_Vec()
            duu, dvv, duv = gp_Vec(), gp_Vec(), gp_Vec()
            surface.D2(uv.X(), uv.Y(), here, du, dv, duu, dvv, duv)
            normal = du.Crossed(dv)
            if normal.Magnitude() < 1e-12:
                return None
            normal.Normalize()
            if face.wrapped.Orientation() == TopAbs_Orientation.TopAbs_REVERSED:
                normal.Reverse()
            inward = normal.Crossed(tangent)
            if inward.Magnitude() < 1e-12:
                return None
            inward.Normalize()

            e, f, g = du.Dot(du), du.Dot(dv), dv.Dot(dv)
            det = e * g - f * f
            if not math.isfinite(det) or abs(det) < 1e-18:
                return None
            rhs_u, rhs_v = inward.Dot(du), inward.Dot(dv)
            along_u = (rhs_u * g - rhs_v * f) / det
            along_v = (rhs_v * e - rhs_u * f) / det
            first = du.Multiplied(along_u).Added(dv.Multiplied(along_v))
            denominator = first.SquareMagnitude()
            if not math.isfinite(denominator) or denominator < 1e-18:
                return None
            second = (
                duu.Multiplied(along_u * along_u)
                .Added(duv.Multiplied(2.0 * along_u * along_v))
                .Added(dvv.Multiplied(along_v * along_v))
            )
            curvature = normal.Dot(second) / denominator
            if not math.isfinite(curvature):
                return None
            return curvature, surface.GetType() == GeomAbs_Plane
        except Exception:  # noqa: BLE001 - differential failure is side-unproven
            return None

    def _classify_arc(self, a: FaceNode, b: FaceNode) -> ArcKind | None:
        """:meth:`arc` without the cache: every shared edge classified, and made to agree."""

        shared = self.shared_edges(a, b)
        if not shared:
            return None
        answers = {self._classify_at(a, b, edge) for edge in shared}
        if len(answers) != 1:
            return "unknown"  # the edges disagree: no one answer describes this pair
        return answers.pop()

    def _classify_at(self, a: FaceNode, b: FaceNode, edge: EdgeLike) -> ArcKind:
        """The classification along one shared edge."""

        # One guard, not three: a direction that cannot be walked and a normal that cannot be
        # read are the same answer -- the geometry here is too degenerate to classify.
        walk = self._boundary_direction(a, edge)
        direction, point = walk if walk else (None, None)
        na = self._normal_at(a, point) if point else None
        nb = self._normal_at(b, point) if point else None
        if direction is None or na is None or nb is None:
            return "unknown"
        if 1.0 - sum(x * y for x, y in zip(na, nb, strict=True)) <= SMOOTH_ARC_GAP:
            return "smooth"
        into = (
            na[1] * direction[2] - na[2] * direction[1],
            na[2] * direction[0] - na[0] * direction[2],
            na[0] * direction[1] - na[1] * direction[0],
        )
        return "convex" if sum(x * y for x, y in zip(into, nb, strict=True)) < 0 else "concave"

    def _normal_at(self, node: FaceNode, point) -> tuple[float, float, float] | None:
        """This face's outward normal *at a point*, or None where it has none.

        Not cached, unlike :meth:`normal`: the answer varies over a curved face, so there is no
        one value to keep. The point is the whole reason this exists -- a plane has one normal
        everywhere and a cone does not, and an arc has to be read where the faces actually meet.

        **Not** ``normal_at``, which ignores the point it is given: asked at 0, 90 and 180
        degrees around a cylinder it returns the same vector three times, so a per-point reader
        built on it silently reads the patch's middle instead. The point is projected to a
        surface parameter and the surface differentiated there.

        The sign correction is a single orientation flip, unlike
        :func:`frame_points_outward`'s two terms: ``du x dv`` is built from the parameterisation
        and so already carries the frame's handedness, where a stored axis direction does not.
        Checked on 659 faces across generated solids and imported STEP -- ``FORWARD`` agrees with
        the material side every time, ``REVERSED`` never does.
        """

        face = self._faces[self._at(node)]
        try:
            parameters = ShapeAnalysis_Surface(BRep_Tool.Surface_s(face.wrapped)).ValueOfUV(
                gp_Pnt(*point), 1e-6
            )
            adaptor = BRepAdaptor_Surface(face.wrapped)
            here, along_u, along_v = gp_Pnt(), gp_Vec(), gp_Vec()
            adaptor.D1(parameters.X(), parameters.Y(), here, along_u, along_v)
            cross = along_u.Crossed(along_v)
        except Exception:  # noqa: BLE001 - a degenerate patch has no normal there
            cross = gp_Vec()
        if cross.Magnitude() < 1e-12:
            return None  # degenerate: the surface has no normal at this point
        cross.Normalize()
        forward = face.wrapped.Orientation() != TopAbs_Orientation.TopAbs_REVERSED
        sign = 1.0 if forward else -1.0
        return (sign * cross.X(), sign * cross.Y(), sign * cross.Z())

    def _boundary_direction(self, node: FaceNode, edge: EdgeLike):
        """``(direction, point)``: how *node*'s boundary walks *edge*, and where it was measured.

        The edge's own parameterisation is not it -- OCP does not orient that consistently, and a
        classification resting on it reports both signs on a plain box where every edge is
        convex. What is consistent is the orientation the edge carries *within this face*, which
        is what makes the two faces of a manifold edge walk it in opposite directions and the
        arc's answer the same from either end.

        Deliberately **not** flipped for a ``REVERSED`` face. ``normal_at`` already accounts for
        face orientation, so flipping here too double-corrects -- measured, that broke the
        opposite-directions property on exactly half a box's edges.
        """

        # No "edge not found" branch: every caller reaches this through `shared_edges`, which
        # returns edges of *this* face, so the explorer always finds it. A guard for a case the
        # call graph excludes is a branch no test can reach, which this epic has removed twice
        # already rather than carry.
        reversed_here = self._edge_reversed_in_face(node, edge)
        curve = BRepAdaptor_Curve(edge.wrapped)
        middle = 0.5 * (curve.FirstParameter() + curve.LastParameter())
        point, tangent = gp_Pnt(), gp_Vec()
        curve.D1(middle, point, tangent)
        raw = (tangent.X(), tangent.Y(), tangent.Z())
        length = sum(x * x for x in raw) ** 0.5
        if length < 1e-12:
            return None  # a degenerate edge has no direction to walk
        sign = -1.0 / length if reversed_here else 1.0 / length
        return tuple(x * sign for x in raw), (point.X(), point.Y(), point.Z())

    def _edge_reversed_in_face(self, node: FaceNode, edge: EdgeLike) -> bool:
        """Whether the shared edge walks backward in this original face."""

        face = self._faces[self._at(node)]
        explorer = TopExp_Explorer(face.wrapped, TopAbs_EDGE)
        while explorer.More():
            current = explorer.Current()
            if current.IsSame(edge.wrapped):
                return bool(current.Orientation() == TopAbs_Orientation.TopAbs_REVERSED)
            explorer.Next()
        raise ValueError("shared edge is absent from its original face")

    def _face_edge_occurrences(self, node: FaceNode) -> tuple[EdgeOccurrenceRef, ...]:
        self._at(node)
        cached = self._edge_occurrences.get(node)
        if cached is not None:
            for occurrence in cached:
                self._validate_edge_occurrence(occurrence)
            return cached
        found: list[EdgeOccurrenceRef] = []
        wires = TopExp_Explorer(self.face(node).wrapped, TopAbs_WIRE)
        wire_ordinal = 0
        while wires.More():
            edges = TopExp_Explorer(wires.Current(), TopAbs_EDGE)
            ordinal = 0
            while edges.More():
                current = edges.Current()
                edge = Edge(TopoDS.Edge_s(current))
                occurrence = EdgeOccurrenceRef(
                    owner=node,
                    wire_ordinal=wire_ordinal,
                    ordinal=ordinal,
                    reversed=current.Orientation() == TopAbs_Orientation.TopAbs_REVERSED,
                    edge=edge,
                )
                snapshot = (node, wire_ordinal, ordinal, occurrence.reversed, edge)
                self._issued_edge_occurrences[occurrence] = snapshot
                found.append(occurrence)
                ordinal += 1
                edges.Next()
            wire_ordinal += 1
            wires.Next()
        result = tuple(found)
        self._edge_occurrences[node] = result
        return result

    def edge_occurrences(self, node: FaceNode) -> tuple[EdgeOccurrenceRef, ...]:
        """Every exact oriented edge occurrence in the original face-wire traversal."""

        return self._face_edge_occurrences(node)

    def _validate_edge_occurrence(self, occurrence: EdgeOccurrenceRef) -> None:
        snapshot = self._issued_edge_occurrences.get(occurrence)
        if snapshot is None:
            raise ValueError("edge occurrence was not issued by this graph")
        owner, wire_ordinal, ordinal, reversed_here, edge = snapshot
        if (
            occurrence.owner is not owner
            or occurrence.wire_ordinal != wire_ordinal
            or occurrence.ordinal != ordinal
            or occurrence.reversed is not reversed_here
            or occurrence.edge is not edge
            or not self.owns(owner)
        ):
            raise ValueError("edge occurrence changed after issuance")

    def _validate_shared_occurrence(self, occurrence: SharedEdgeOccurrenceRef) -> None:
        snapshot = self._issued_shared_occurrences.get(occurrence)
        if snapshot is None:
            raise ValueError("shared-edge occurrence was not issued by this graph")
        endpoints, halves, edge = snapshot
        if (
            occurrence.endpoints != endpoints
            or any(
                actual is not expected
                for actual, expected in zip(occurrence.endpoints, endpoints, strict=True)
            )
            or occurrence.halves != halves
            or any(
                actual is not expected
                for actual, expected in zip(occurrence.halves, halves, strict=True)
            )
            or occurrence.edge is not edge
        ):
            raise ValueError("shared-edge occurrence changed after issuance")
        for half in halves:
            self._validate_edge_occurrence(half)

    def shared_occurrences(self, a: FaceNode, b: FaceNode) -> tuple[SharedEdgeOccurrenceRef, ...]:
        """Exact paired oriented occurrences shared by two original nodes."""

        at_a, at_b = self._at(a), self._at(b)
        if at_a == at_b:
            return ()
        left, right = (a, b) if at_a < at_b else (b, a)
        key = (min(at_a, at_b), max(at_a, at_b))
        cached = self._shared_occurrences.get(key)
        if cached is not None:
            for occurrence in cached:
                self._validate_shared_occurrence(occurrence)
            return cached
        left_halves = self._face_edge_occurrences(left)
        right_halves = self._face_edge_occurrences(right)
        pairs: list[SharedEdgeOccurrenceRef] = []
        pending_left = list(left_halves)
        while pending_left:
            seed = pending_left.pop(0)
            left_group = [seed]
            for half in tuple(pending_left):
                if half.edge.wrapped.IsSame(seed.edge.wrapped):
                    pending_left.remove(half)
                    left_group.append(half)
            right_group = [
                half for half in right_halves if half.edge.wrapped.IsSame(seed.edge.wrapped)
            ]
            candidate_pairs = {
                left_half: tuple(
                    right_half
                    for right_half in right_group
                    if right_half.reversed is not left_half.reversed
                )
                for left_half in left_group
            }
            reverse_counts = {
                right_half: sum(right_half in matches for matches in candidate_pairs.values())
                for right_half in right_group
            }
            if (
                len(left_group) != len(right_group)
                or any(len(matches) != 1 for matches in candidate_pairs.values())
                or any(count != 1 for count in reverse_counts.values())
            ):
                continue  # no traversal-independent unique pairing
            for left_half, matches in candidate_pairs.items():
                right_half = matches[0]
                occurrence = SharedEdgeOccurrenceRef(
                    endpoints=(left, right),
                    halves=(left_half, right_half),
                    edge=left_half.edge,
                )
                self._issued_shared_occurrences[occurrence] = (
                    occurrence.endpoints,
                    occurrence.halves,
                    occurrence.edge,
                )
                pairs.append(occurrence)
        result = tuple(pairs)
        self._shared_occurrences[key] = result
        return result

    def ownership(self, occurrence: SharedEdgeOccurrenceRef) -> EdgeOwnershipFact | None:
        """Same-valid-solid/two-incident-face proof for one issued adjacency occurrence."""

        self._validate_shared_occurrence(occurrence)
        a, b = occurrence.endpoints
        self._build_solid_ownership()
        assert self._face_solids is not None
        assert self._closed_solids is not None
        assert self._solid_refs is not None
        owned_a = self._face_solids[a.index]
        owned_b = self._face_solids[b.index]
        if len(owned_a) != 1 or owned_a != owned_b or owned_a[0] not in self._closed_solids:
            return None
        incident = self._edge_face_map().get(occurrence.edge, ())
        nodes = tuple(self.node_of(face) for face in incident)
        if len(nodes) != 2 or None in nodes or set(nodes) != {a, b}:
            return None
        solid = self._solid_refs[owned_a[0]]
        if self._issued_solid_refs.get(solid) != solid.ordinal:
            raise ValueError("solid reference changed after issuance")
        return EdgeOwnershipFact(solid, occurrence)

    def shared_edges(self, a: FaceNode, b: FaceNode) -> tuple[EdgeLike, ...]:
        """The edges along which two faces meet, which an arc's classification will need.

        Retained rather than discarded because "is this arc convex" is a question about the
        edge, and answering it later without re-deriving the adjacency requires keeping it.
        """

        other = set(self.edges(b))
        return tuple(edge for edge in self.edges(a) if edge in other)

    def _edge_face_map(self) -> dict:
        if self._edge_faces is None:
            built: dict = {}
            for node in self._nodes:
                for edge in self.edges(node):
                    built.setdefault(edge, []).append(self._faces[node.index])
            self._edge_faces = built
        return self._edge_faces


def frame_points_outward(face: FaceLike) -> bool | None:
    """Does this face's own surface frame already point out of the solid?

    The material-side convention, in one place. An analytic surface carries a frame whose normal
    direction is a property of the *surface*, not of the face using it, so a face records
    separately whether it runs with that direction (``FORWARD``) or against it (``REVERSED``).
    Neither term alone answers the question: **mirroring a solid makes the frame left-handed and
    flips the orientations too**, so a test on orientation alone inverts on a mirrored part. The
    answer is the product, ``FORWARD == frame.Direct()``.

    Three sites derive *which side* from this: cylinders in ``_recess_core`` and
    ``_cylinder_substrate``, spheres in ``_hole_features``. A fourth used it to build a planar
    outward normal, which turned out to be redundant -- ``normal_at`` already returns one -- so
    what this is for is the whole-face question, asked where there is no single normal to read.

    The sites agreed, which is luck rather than design: no test could tell, because
    left-handed frames are 6 of 3,853 across all 72 corpus parts, and deleting the handedness term
    from any of them left the whole suite green. ``tests/test_material_side.py`` generates the
    geometry the corpus lacks and checks the convention against the solid classifier.

    Returns None for a surface this cannot read a frame from -- a torus, a spline -- which is a
    genuine "no answer" rather than a default. Coercing it to False would put material on one
    side of a blend and leave nothing downstream able to tell that from a real answer.

    Every caller today has already established the surface type before asking, so None is
    unreachable for them and ``bool(...)`` at those sites records that rather than dismissing
    it. A caller that has *not* filtered first must handle the third answer, and the honest
    shape is to return None onwards rather than pick a side.
    """

    surface = BRepAdaptor_Surface(face.wrapped)
    kind = surface.GetType()
    if kind == GeomAbs_Plane:
        position = surface.Plane().Position()
    elif kind == GeomAbs_Cylinder:
        position = surface.Cylinder().Position()
    elif kind == GeomAbs_Sphere:
        position = surface.Sphere().Position()
    else:
        return None
    forward = face.wrapped.Orientation() == TopAbs_Orientation.TopAbs_FORWARD
    return bool(forward == position.Direct())


def edge_face_map(faces: Iterable[FaceLike], *, face_edges: FaceEdges | None = None) -> dict:
    """Map every edge of *faces* to the faces that meet along it.

    One pass. The pairwise alternative — asking every face pair whether any of their edges
    match — is ``O(faces² × edges²)``; ``fillets`` measured that at 3.7M ``IsSame`` calls
    and about six seconds before replacing it.

    Takes the faces rather than the part because every caller already holds them, and
    walking ``part.faces()`` a second time here measured at a tenth of ``recognise_fillets``
    on the pinned corpus — the same reason :func:`~quiddity._geometry.part_scale`
    takes a bounding box rather than a solid.

    Pass *face_edges* to reuse a :class:`FaceEdges` memo across recognisers; omitted, the map
    is built from a private one, so a lone recogniser call behaves exactly as before.

    An edge normally maps to two faces. A seam edge on a closed surface maps to one, and
    a non-manifold edge to more, so callers must not assume the length.
    """

    memo = face_edges if face_edges is not None else FaceEdges()
    edge_faces: dict = {}
    for face in faces:
        for edge in memo.of(face):
            edge_faces.setdefault(edge, []).append(face)
    return edge_faces


def neighbours(face: FaceLike, edge_faces: dict, *, face_edges: FaceEdges | None = None) -> list:
    """The distinct faces sharing an edge with *face*, excluding *face* itself.

    Order follows *face*'s own edge order, so it inherits the part's traversal order and
    nothing more — a caller that needs a deterministic result must sort or reduce it, as
    :func:`quiddity.recognise_chamfers` does by keeping the nearest neighbour per
    axis rather than the first one seen.

    *face_edges* reuses a shared :class:`FaceEdges` memo; this is the hottest caller of it,
    since the blend recognisers ask for the neighbours of every face of the part.
    """

    seen = {face}
    out = []
    for edge in face_edges.of(face) if face_edges is not None else face.edges():
        for other in edge_faces.get(edge, ()):
            if other in seen:
                continue
            seen.add(other)
            out.append(other)
    return out


def axis_aligned_axis(face_wrapped) -> tuple[int, float] | None:
    """The axis a planar face's normal aligns with and that plane's fixed coordinate along
    it, or None if the face is not planar or not axis-aligned. Sign-agnostic (only alignment
    matters here); the coordinate locates the plane."""

    s = BRepAdaptor_Surface(face_wrapped)
    if s.GetType() != GeomAbs_Plane:
        return None
    d = s.Plane().Axis().Direction()
    comp = (abs(d.X()), abs(d.Y()), abs(d.Z()))
    if max(comp) <= AXIS_ALIGNED_COS:
        return None
    ax = max(range(3), key=lambda i: comp[i])
    loc = s.Plane().Location()
    return ax, (loc.X(), loc.Y(), loc.Z())[ax]


def connected_components(
    items: Iterable[_T], joined: Callable[[_T, _T], bool]
) -> list[tuple[_T, ...]]:
    """Group *items* into connected components under the *joined* predicate.

    An ordinary private utility with two consumers, and deliberately not more than that. It was
    private to `polygonal_bosses` until `passages` wanted the same walk -- finding from inside a
    void the ring `polygonal_bosses` finds from outside a prism -- and one consumer does not
    justify a shared primitive.

    It is **not** shared face-adjacency semantics, and an earlier version of this docstring
    called it "the other half of the answer-face-adjacency-once finding", which overstated it:
    every caller still supplies the entire relation, so what they share is the breadth-first
    mechanism and not a domain model. The face adjacency they genuinely share is
    :class:`FaceGraph`, which both of them read their nodes' attributes from.

    **`joined` must be symmetric.** The walk only ever asks `joined(current, other)` with
    *current* already in the component, so an asymmetric predicate makes the answer depend on
    set iteration order: over ``0..4``, ``j == i + 1`` yields one component and ``i == j + 1``
    yields five singletons for the same relation.

    **Component and member order are unspecified.** Reversing either passes the whole suite, so
    nothing pins them -- `polygonal_bosses` sorts its rings by heading downstream and the record
    emitters canonicalise. They are also only deterministic for items whose hash is stable
    across runs, which today means the small ints the one caller passes. Do not rely on either.

    *joined* is supplied by the caller rather than fixed here. Both consumers happen to want
    the same conjunction today -- a shared edge and a shared span -- but they compute the span
    differently, along a caller-chosen axis for passages and along Z for bosses, and neither
    could use the other's. The span half is not decoration: without it, two equal-height bosses
    standing on one plate merge into a single twelve-sided ring.
    """

    components: list[tuple[_T, ...]] = []
    unseen = set(items)
    while unseen:
        connected = {unseen.pop()}
        frontier = list(connected)
        while frontier:
            current = frontier.pop()
            attached = {other for other in unseen if joined(current, other)}
            unseen -= attached
            connected |= attached
            frontier.extend(attached)
        components.append(tuple(connected))
    return components


def nearest_axis_aligned_planes(
    face: FaceLike,
    edge_faces: dict,
    centre: dict[int, float],
    *,
    exclude_axis: int,
    refuse_equidistant: bool = False,
    face_edges: FaceEdges | None = None,
) -> dict[int, float]:
    """Per axis, the coordinate of *face*'s nearest axis-aligned neighbour plane.

    The shared "what does this blend bridge" query. ``recognise_chamfers`` and
    ``recognise_fillets`` both need a bevel's or round's two neighbour planes to rebuild the
    virtual sharp corner it replaces, and both previously carried their own copy of this
    filter and its supporting :func:`axis_aligned_axis` — identical code, with each file's
    comment pointing at the other. A caller reads the result twice: an axis missing from it
    means no such neighbour on that axis, which is itself a rejection.

    *exclude_axis* is the axis the feature runs **along**; a plane facing that way is an end
    cap, not one of the two walls the feature bridges.

    The nearest plane per axis is the one forming this local corner. Ties normally resolve to the
    lower coordinate so existing Chamfer and Angled Step behaviour remains stable and independent
    of kernel traversal order. A consumer whose acceptance cannot disambiguate the physical plane
    may set *refuse_equidistant*: two distinct planes at the same nearest distance then omit that
    axis, avoiding a choice that changes under an axis-sign inversion. Split coplanar neighbours
    remain one plane and retain their midpoint coordinate.
    """

    candidates: dict[int, list[float]] = {}
    for other in neighbours(face, edge_faces, face_edges=face_edges):
        aligned = axis_aligned_axis(other.wrapped)
        if aligned is None or aligned[0] == exclude_axis:
            continue
        ax, coord = aligned
        candidates.setdefault(ax, []).append(coord)

    selected: dict[int, float] = {}
    for axis, coordinates in candidates.items():
        if not refuse_equidistant:
            selected[axis] = min(
                coordinates,
                key=lambda coordinate: (abs(coordinate - centre[axis]), coordinate),
            )
            continue
        distances = [abs(coordinate - centre[axis]) for coordinate in coordinates]
        nearest = min(distances)
        tied = [
            coordinate
            for coordinate, distance in zip(coordinates, distances, strict=True)
            if abs(distance - nearest) <= length_tol(max(distance, nearest), rel=1e-9)
        ]
        plane_tol = length_tol(max(abs(coordinate - centre[axis]) for coordinate in tied), rel=1e-9)
        if max(tied) - min(tied) > plane_tol:
            continue
        selected[axis] = 0.5 * (min(tied) + max(tied))
    return selected
