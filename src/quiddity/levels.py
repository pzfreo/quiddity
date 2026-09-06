# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Horizontal face-level and step-riser recognition.

``recognise_face_levels`` returns the Z-coords of a part's horizontal planar faces —
the step levels of a *prismatic* part. It is the complement of ``recognise_turned_steps``
(turned.py): a box-stepped part has no cylinders, so the OD-silhouette recogniser
cannot see its steps, while a turned shaft's shoulders are better filtered by the OD
silhouette than by a raw face scan. These are complementary geometry classes, not duplicate
answers: a consumer chooses the applicable ladder from its part classification. Bottom of the
recognition DAG: depends only on build123d/OCP.
"""

from __future__ import annotations

from collections.abc import Mapping, Sequence
from dataclasses import dataclass, replace
from functools import total_ordering

from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.BRepGProp import BRepGProp
from OCP.GeomAbs import GeomAbs_Plane
from OCP.GProp import GProp_GProps

from quiddity._adjacency import FaceNode
from quiddity._body_identity import BodyKey, unambiguous_body_keys
from quiddity._candidates import FamilyId
from quiddity._claims import EvidenceWriter
from quiddity._geometry import (
    AXIS_ALIGNED_COS,
    AXIS_ZERO_COS,
    clears_threshold,
    cluster_coordinates,
)
from quiddity._record import Record
from quiddity._typing import FaceLike, Part


@total_ordering
@dataclass(frozen=True)
class FaceLevel(Record):
    """A recognised horizontal face level and its in-plane supporting extent.

    ``x_span`` / ``y_span`` are the union bounds of the horizontal faces at ``z``. They let
    downstream dimensions retain a real witness station instead of inventing the part's
    envelope edge after the face correspondence has been discarded. ``None`` keeps
    construction of legacy value-only records compatible. ``order=True`` makes recognition
    deterministic.
    """

    z: float
    x_span: tuple[float, float] | None = None
    y_span: tuple[float, float] | None = None
    # Comparable source-body membership. Empty retains hand-built legacy values; ``None``
    # refuses an unavailable or colliding physical-body signature.
    body_key: BodyKey | None = ()

    def __lt__(self, other: object) -> bool:
        if not isinstance(other, FaceLevel):
            return NotImplemented
        return self._order_key() < other._order_key()

    def _order_key(self) -> tuple[object, ...]:
        return (
            self.z,
            self.x_span is not None,
            self.x_span or (),
            self.y_span is not None,
            self.y_span or (),
            self.body_key is not None,
            self.body_key or (),
        )


@dataclass(frozen=True, slots=True)
class _FaceLevelProposal:
    record: FaceLevel
    faces: tuple[FaceLike, ...]


@dataclass(frozen=True, order=True)
class StepShoulder(Record):
    """A recognised step/rebate shoulder. ``axis`` is the riser's normal axis
    ("x"/"y"); ``position`` is the world coord of the shoulder along it. ``order=True``
    so recognisers can return a deterministically sorted list."""

    axis: str
    position: float


@total_ordering
@dataclass(frozen=True)
class RiserEvidence(Record):
    """One candidate step riser, recognised WITHOUT reference to any level set.

    :func:`recognise_risers` scans the solid once and emits these; each consumer then calls
    :func:`project_step_shoulders` with the level set *it* cares about. The split exists
    because the scan is the cost and the levels are only a filter: model construction
    projects over levels filtered by plate and pocket ownership, while critique must project
    over the unfiltered ones (the independent-evidence rule forbids lint taking its inventory
    from the model). One scan, two answers — rather than one scan per asker.

    ``z_lo``/``z_hi`` are the riser face's vertical extent, kept raw so the level test stays
    in the projection. ``lo_at_envelope``/``hi_at_envelope`` pre-answer the part of the
    oblique tie-test that does NOT depend on levels — whether that end sits on the part's top
    or bottom — so the projection needs the levels and nothing else about the solid.

    The explicit total ordering keeps pre-0.4.9 hand-built records (whose body authority is
    ``None``) sortable alongside recogniser-produced records while retaining that authority in
    record equality.  Dataclass-generated ordering cannot compare ``None`` with a tuple.
    """

    vertical: bool
    axis: str
    positions: tuple[float, ...]
    other_axis: str
    other_positions: tuple[float, ...]
    z_lo: float
    z_hi: float
    lo_at_envelope: bool
    hi_at_envelope: bool
    #: The tolerance this evidence was scanned with, so the projection matches it by default.
    #: Without this the split silently broke a non-default ``tol``: the old one-stage call used
    #: one value for the geometric gates AND the level ties, whereas ``recognise_risers(part,
    #: tol=0.1)`` followed by a bare ``project_step_shoulders(...)`` mixed 0.1 with the
    #: projection's own default. Carried on the record rather than passed
    #: separately because a caller who has the evidence should not have to remember how it was
    #: produced.
    #:
    #: The default is a constructor convenience only. A recogniser never uses it —
    #: :func:`recognise_risers` always passes the value it resolved for that part — and a record
    #: built by hand was never scanned, so no value would be more truthful than another.
    tol: float = 0.5
    #: Interior FaceLevel occurrences proved on the same valid solid as this riser occurrence.
    #: ``None`` is retained only for compatibility with hand-built pre-0.4.9 records; every
    #: recogniser-produced record carries a tuple (possibly empty), so projection cannot borrow a
    #: matching occurrence from another solid in a compound. Keeping the full record, rather than
    #: only Z, lets a caller select one of two equal-Z levels by its body-local support evidence.
    body_levels: tuple[FaceLevel, ...] | None = None
    #: Public same-solid join shared with other structural records in this result.
    body_key: BodyKey | None = ()

    def __lt__(self, other: object) -> bool:
        if not isinstance(other, RiserEvidence):
            return NotImplemented
        return self._order_key() < other._order_key()

    def _order_key(self) -> tuple[object, ...]:
        return (
            self.vertical,
            self.axis,
            self.positions,
            self.other_axis,
            self.other_positions,
            self.z_lo,
            self.z_hi,
            self.lo_at_envelope,
            self.hi_at_envelope,
            self.tol,
            self.body_levels is not None,
            self.body_levels or (),
            self.body_key is not None,
            self.body_key or (),
        )


@dataclass(frozen=True, slots=True)
class _RiserProposal:
    record: RiserEvidence
    faces: tuple[FaceLike, ...]


def recognise_face_levels(
    part: Part, *, tol: float | None = None, min_area_frac: float = 0.0
) -> list[FaceLevel]:
    """Return the sorted unique horizontal (normal≈±Z) face levels as :class:`FaceLevel`
    records — one per distinct Z of a horizontal planar face.

    Faces whose Z values lie within *tol* of each other are one level, grouped by distance
    apart rather than by which ``tol``-wide grid cell they round into — see
    :func:`~quiddity._geometry.cluster_coordinates`. The level reports the lowest
    actual face Z in its group, not a rounded bucket centre, so dimension labels match the
    true geometry, and the choice is independent of the order faces were traversed in.

    When *min_area_frac* > 0, a Z level is kept only if the total area of its
    horizontal faces is at least ``min_area_frac × (x_size × y_size)`` (the
    part's plan footprint). This drops sub-feature faces — e.g. fragments of
    engraved text/numbers — that are not real steps and would otherwise be
    dimensioned as phantom shoulders.
    """
    tol = _TOL if tol is None else tol
    scopes = list(part.solids()) or [part]
    body_keys = unambiguous_body_keys(scopes, require_valid_solid=True)
    return sorted(
        replace(proposal.record, body_key=body_key)
        for scope, body_key in zip(scopes, body_keys, strict=True)
        for proposal in _face_level_proposals_one(scope, tol=tol, min_area_frac=min_area_frac)
    )


def _face_level_proposals_one(
    part: Part, *, tol: float, min_area_frac: float
) -> list[_FaceLevelProposal]:
    """Recognise levels within one valid body so support never bridges another solid."""

    zs: list[float] = []
    face_bounds: list[tuple[float, float, float, float]] = []
    faces: list[FaceLike] = []
    face_areas: list[float] = []
    for face in part.faces():
        surf = BRepAdaptor_Surface(face.wrapped)
        if surf.GetType() == GeomAbs_Plane:
            ax = surf.Plane().Axis().Direction()
            if abs(ax.Z()) > AXIS_ALIGNED_COS:
                zs.append(surf.Plane().Location().Z())
                faces.append(face)
                bb = face.bounding_box()
                face_bounds.append((bb.min.X, bb.min.Y, bb.max.X, bb.max.Y))
                if min_area_frac > 0.0:
                    props = GProp_GProps()
                    BRepGProp.SurfaceProperties_s(face.wrapped, props)
                    face_areas.append(props.Mass())

    threshold = 0.0
    if min_area_frac > 0.0:
        bb = part.bounding_box()
        threshold = min_area_frac * (bb.max.X - bb.min.X) * (bb.max.Y - bb.min.Y)

    levels: list[_FaceLevelProposal] = []
    for cluster in cluster_coordinates(zs, tol=tol):
        if min_area_frac > 0.0 and not clears_threshold(
            sum(face_areas[i] for i in cluster), threshold
        ):
            continue
        spans = [face_bounds[i] for i in cluster]
        levels.append(
            _FaceLevelProposal(
                FaceLevel(
                    min(zs[i] for i in cluster),
                    (min(s[0] for s in spans), max(s[2] for s in spans)),
                    (min(s[1] for s in spans), max(s[3] for s in spans)),
                ),
                tuple(faces[i] for i in cluster),
            )
        )
    return levels


#: Band within which two horizontal faces are one level. **Absolute, per ADR 0008.** Scaling it
#: to the part made a large plate merge levels a small one kept, and also served as a minimum
#: riser height — so it lost records in both directions at once.
_TOL = 0.5

# Minimum horizontal-face area (as a fraction of the plan footprint) for a Z level to count
# as a genuine prismatic step — drops an incidental tiny face (a blind-pocket floor, a small
# pad top) that would otherwise read as a phantom step rung.
_STEP_MIN_AREA_FRAC = 0.01

#: Default end exclusion for both prismatic level capture and Z-turned ladder projection, in
#: model length units (normally mm). Equality at either inset boundary is excluded.
#:
#: Retained for callers that pass an explicit margin, and as the documented ADR 0006 value.
#: The *default* is now derived per part from the fraction below, per ADR 0008.
STEP_LADDER_BOUNDARY_MARGIN: float = 0.6

#: The largest fraction of a span the end exclusion may consume. The margin above is absolute
#: because it excludes an end treatment, which does not grow with the part — but an unbounded
#: absolute constant swallows a short span whole, so ADR 0008 requires the cap.
_END_MARGIN_MAX_FRAC: float = 0.25


def bounded_end_margin(span: float) -> float:
    """The end exclusion for a span: absolute, but never more than a quarter of it.

    Deliberately not proportional. The inset exists to drop a shoulder produced by a chamfer or
    edge break just inside an end face, and a deburr is the same size on a 20 mm dowel and a 2 m
    shaft — the ADR 0006 regression pins exactly that, a 0.6 mm end step on a 10 mm part. The cap
    is what keeps a legitimately absolute constant safe when the part is modelled small, the same
    bound ``turned._OD_SPAN_PAD`` uses against its band width.
    """

    return min(STEP_LADDER_BOUNDARY_MARGIN, max(span, 0.0) * _END_MARGIN_MAX_FRAC)


def step_level_records(part: Part, *, tol: float | None = None) -> list[FaceLevel]:
    """Area-filtered interior face-level records, retaining their support bounds."""
    records: list[FaceLevel] = []
    scopes = list(part.solids()) or [part]
    body_keys = unambiguous_body_keys(scopes, require_valid_solid=True)
    for scope, body_key in zip(scopes, body_keys, strict=True):
        bb = scope.bounding_box()
        scope_tol = bounded_end_margin(bb.max.Z - bb.min.Z) if tol is None else tol
        records.extend(
            replace(proposal.record, body_key=body_key)
            for proposal in _face_level_proposals_one(
                scope, tol=_TOL, min_area_frac=_STEP_MIN_AREA_FRAC
            )
            if bb.min.Z + scope_tol < proposal.record.z < bb.max.Z - scope_tol
        )
    return sorted(records)


def _discover_step_levels(part: Part, *, writer: EvidenceWriter) -> list[FaceLevel]:
    """Return aggregate levels and publish their exact body-local horizontal faces."""

    accepted: list[_FaceLevelProposal] = []
    scopes = list(part.solids()) or [part]
    body_keys = unambiguous_body_keys(scopes, require_valid_solid=True)
    for scope, body_key in zip(scopes, body_keys, strict=True):
        bb = scope.bounding_box()
        scope_tol = bounded_end_margin(bb.max.Z - bb.min.Z)
        accepted.extend(
            replace(proposal, record=replace(proposal.record, body_key=body_key))
            for proposal in _face_level_proposals_one(
                scope, tol=_TOL, min_area_frac=_STEP_MIN_AREA_FRAC
            )
            if bb.min.Z + scope_tol < proposal.record.z < bb.max.Z - scope_tol
        )
    accepted.sort(key=lambda proposal: proposal.record)
    pending = tuple(
        (
            proposal.record,
            tuple(writer.graph.require_node(face) for face in proposal.faces),
        )
        for proposal in accepted
    )
    if any(writer.graph.common_valid_solid(nodes) is None for _record, nodes in pending):
        raise ValueError("FaceLevel defining faces have no unambiguous valid solid")
    for record, nodes in pending:
        writer.sink.propose(FamilyId.STEP_LEVELS, record, defining=nodes)
    return [proposal.record for proposal in accepted]


def step_level_zs(part: Part, *, tol: float | None = None) -> list[float]:
    """The interior prismatic step Z-levels: the area-filtered horizontal face levels strictly
    inside the part height (``base + tol < z < top - tol``). The single source of truth for the
    step-height ladder for every consumer. Using raw, unfiltered
    :func:`recognise_face_levels` in one path and this gate in another would let a tiny
    incidental face leak in as a phantom level and make the answers diverge."""
    return [fl.z for fl in step_level_records(part, tol=tol)]


#: A bounded slanted face is a structural ramp only if it is this fraction of the part on every
#: axis it spans. Below it the face is an edge-break chamfer, which is not a transition between
#: levels and must not contribute shoulder stations.
_STRUCTURAL_RAMP_MIN_FRAC = 0.1

#: A bounded riser is judged against its own footprint rather than the part cross-section, and
#: must fill this fraction of it. A full-span riser uses the caller's ``min_area_frac`` instead,
#: because there the part cross-section is the meaningful denominator.
_BOUNDED_RISER_AREA_FRAC = 0.5


def _ramp_positions(
    fb,
    axis: str,
    other: str,
    ext: dict[str, float],
    *,
    full_span: bool,
    flo: float,
    fhi: float,
) -> tuple[tuple[float, ...], tuple[float, ...]] | None:
    """Transition stations for an oblique riser, or ``None`` if it is not a structural ramp.

    A full-span slanted face contributes its two stations along the riser axis. A *bounded* one
    also contributes its extrusion endpoints on the perpendicular axis, which is what defines
    the ramp's width — but only once it is large enough on all three axes to be a deliberate
    transition rather than an edge break, since a chamfer is bounded in exactly the same way.
    """

    axis_positions = (
        fb.min.X if axis == "x" else fb.min.Y,
        fb.max.X if axis == "x" else fb.max.Y,
    )
    if full_span:
        return axis_positions, ()
    if (
        fhi - flo < _STRUCTURAL_RAMP_MIN_FRAC * ext[other]
        or axis_positions[1] - axis_positions[0] < _STRUCTURAL_RAMP_MIN_FRAC * ext[axis]
        or _STRUCTURAL_RAMP_MIN_FRAC * ext["z"] > fb.max.Z - fb.min.Z
    ):
        return None  # ordinary edge-break chamfer, not a structural ramp
    return axis_positions, (flo, fhi)


def _riser_orientation(normal) -> tuple[bool, str] | None:
    """Classify a face normal as a riser candidate: ``(vertical, axis)``, or ``None``.

    A riser faces along one in-plane axis and not the other — a normal with a foot in both is a
    corner treatment, not a step. ``vertical`` distinguishes a square riser from an oblique
    ramp, which contribute different transition stations further down.
    """

    on_x = abs(normal.X) > AXIS_ZERO_COS and abs(normal.Y) <= AXIS_ZERO_COS
    on_y = abs(normal.Y) > AXIS_ZERO_COS and abs(normal.X) <= AXIS_ZERO_COS
    if not (on_x or on_y):
        return None
    return abs(normal.Z) <= AXIS_ZERO_COS, "x" if on_x else "y"


def recognise_risers(
    part: Part, *, min_area_frac: float = 0.15, tol: float | None = None
) -> list[RiserEvidence]:
    """Scan *part* once for candidate step risers, independent of any level set.

    This is the expensive half of the old ``recognise_step_shoulders``: the full
    ``part.faces()`` walk and every geometric gate that does not need levels — planarity,
    in-plane axis, the full-span test that separates a step from a pad or blind pocket, the
    interior-position test, the structural-ramp size floor and the area floor.

    What it deliberately does NOT do is decide which candidates rise from a *recognised*
    level; that is :func:`project_step_shoulders`, because the answer differs per consumer
    and re-scanning per consumer is the cost the aggregate single-scan design exists to remove.

    See :class:`RiserEvidence`. Values are deduplicated within one valid solid, then occurrences
    are globally sorted without collapsing equal records from separate solids.

    ``recognise_face_levels`` recovers the step *heights* (Z); the projection over this
    evidence recovers *where along the part* each shoulder sits, so a stepped block is fully
    constrained (two different shoulder positions no longer draw the same sheet). A shoulder
    is either a vertical riser or an endpoint of a full-span slanted transition. The latter's
    two stations, together with the adjacent height levels, define the ramp without
    a redundant angle dimension.

    The riser must also span its WHOLE valid solid edge-to-edge on its perpendicular in-plane
    axis (reach both envelope edges within *tol*); this is what separates a step/rebate
    from a raised pad/island or a blind pocket, whose walls rise from a level but are
    bounded. The conservative side of that cut: a partial *corner notch* (a step reaching
    only one edge) or a step whose riser is inset from the edges by end fillets/chamfers
    larger than *tol* is not recognised — the alternative, loosening the span test,
    re-admits pads/pockets, so the full-span sharp-edged step is the recognised class
    (partial/filleted-end steps are a future refinement).
    """
    tol = _TOL if tol is None else tol
    scopes = list(part.solids()) or [part]
    body_keys = unambiguous_body_keys(scopes, require_valid_solid=True)
    proposals: list[_RiserProposal] = []
    for scope, body_key in zip(scopes, body_keys, strict=True):
        bb = scope.bounding_box()
        body_levels = tuple(
            replace(proposal.record, body_key=body_key)
            for proposal in _face_level_proposals_one(
                scope, tol=_TOL, min_area_frac=_STEP_MIN_AREA_FRAC
            )
            if bb.min.Z + tol < proposal.record.z < bb.max.Z - tol
        )
        proposals.extend(
            replace(proposal, record=replace(proposal.record, body_key=body_key))
            for proposal in _riser_proposals_one(
                scope,
                min_area_frac=min_area_frac,
                tol=tol,
                body_levels=body_levels,
            )
        )
    return sorted(proposal.record for proposal in proposals)


def _riser_proposals_one(
    part: Part,
    *,
    min_area_frac: float,
    tol: float,
    body_levels: tuple[FaceLevel, ...],
) -> list[_RiserProposal]:
    """Discover and reduce riser occurrences within one valid-solid authority."""

    bb = part.bounding_box()
    ext = {"x": bb.max.X - bb.min.X, "y": bb.max.Y - bb.min.Y, "z": bb.max.Z - bb.min.Z}
    lo = {"x": bb.min.X, "y": bb.min.Y}
    hi = {"x": bb.max.X, "y": bb.max.Y}
    found: dict[RiserEvidence, list[FaceLike]] = {}
    for f in part.faces():
        s = BRepAdaptor_Surface(f.wrapped)
        if s.GetType() != GeomAbs_Plane:
            continue
        try:
            nv = f.normal_at()
        except Exception:  # pragma: no cover - OCCT defensive boundary
            continue
        classified = _riser_orientation(nv)
        if classified is None:
            continue
        vertical, axis = classified
        fb = f.bounding_box()
        other = "y" if axis == "x" else "x"
        # A step/rebate shoulder crosses the WHOLE part edge-to-edge on the
        # perpendicular in-plane axis — its riser reaches both envelope edges. A raised
        # pad / island or a blind pocket has bounded walls that do NOT span the part, so
        # this excludes them (they rise from a level and can clear the area gate, but
        # they are not steps — the level tie alone doesn't separate a blind pocket from a
        # through slot). Without this, a central pad or blind pocket is mis-located as a
        # shoulder.
        flo = fb.min.X if other == "x" else fb.min.Y
        fhi = fb.max.X if other == "x" else fb.max.Y
        full_span = flo <= lo[other] + tol and fhi >= hi[other] - tol
        positions: tuple[float, ...]
        other_positions: tuple[float, ...] = ()
        if vertical:
            if not full_span:
                continue  # a bounded vertical wall belongs to a pad/pocket
            loc = s.Plane().Location()
            pos = loc.X() if axis == "x" else loc.Y()
            if not (lo[axis] + tol < pos < hi[axis] - tol):
                continue  # interior only — an envelope face is not a shoulder
            # The "rises from a step level" test lives in project_step_shoulders — it is the
            # one gate whose answer depends on which level set the asker holds.
            positions = (pos,)
        else:
            # A genuine oblique profile face contributes both transition
            # stations. A bounded slanted interruption also contributes its
            # extrusion endpoints on the perpendicular in-plane axis, defining
            # both the ramp and its width.
            # `vertical` is False here, so nv.Z is already outside AXIS_ZERO_COS; the height
            # test is the only live half of what used to be a two-part guard.
            if tol >= fb.max.Z - fb.min.Z:
                continue
            ramp = _ramp_positions(fb, axis, other, ext, full_span=full_span, flo=flo, fhi=fhi)
            if ramp is None:
                continue
            positions, other_positions = ramp
        cross = ext[other] * ext["z"]
        props = GProp_GProps()
        BRepGProp.SurfaceProperties_s(f.wrapped, props)
        area_floor = (
            min_area_frac * cross
            if full_span
            else _BOUNDED_RISER_AREA_FRAC * ((fhi - flo) * (fb.max.Z - fb.min.Z))
        )
        if cross <= 0 or props.Mass() < area_floor:
            continue  # a large riser, not an incidental feature face
        record = RiserEvidence(
            vertical=vertical,
            axis=axis,
            positions=tuple(
                round(pos, 3) for pos in positions if lo[axis] + tol < pos < hi[axis] - tol
            ),
            other_axis=other,
            other_positions=tuple(
                round(pos, 3) for pos in other_positions if lo[other] + tol < pos < hi[other] - tol
            ),
            z_lo=fb.min.Z,
            z_hi=fb.max.Z,
            # The level-independent half of the oblique tie-test: an end sitting on the
            # part's top or bottom is structural whatever the level set says.
            lo_at_envelope=abs(fb.min.Z - bb.min.Z) < tol or abs(fb.min.Z - bb.max.Z) < tol,
            hi_at_envelope=abs(fb.max.Z - bb.min.Z) < tol or abs(fb.max.Z - bb.max.Z) < tol,
            tol=tol,
            body_levels=body_levels,
        )
        found.setdefault(record, []).append(f)
    return [
        _RiserProposal(record, tuple(faces))
        for record, faces in sorted(found.items(), key=lambda item: item[0])
    ]


def _discover_risers(
    part: Part,
    *,
    writer: EvidenceWriter,
    body_levels: Mapping[object, Sequence[FaceLevel]],
) -> list[RiserEvidence]:
    """Return body-local risers using completed FaceLevel authority from this run."""

    pending: list[tuple[RiserEvidence, tuple[FaceNode, ...]]] = []
    scopes = list(part.solids()) or [part]
    body_keys = unambiguous_body_keys(scopes, require_valid_solid=True)
    for scope, body_key in zip(scopes, body_keys, strict=True):
        for proposal in _riser_proposals_one(scope, min_area_frac=0.15, tol=_TOL, body_levels=()):
            nodes = tuple(writer.graph.require_node(face) for face in proposal.faces)
            solid = writer.graph.common_valid_solid(nodes)
            if solid is None:  # pragma: no cover - graph-bound nodes retain one valid owner
                raise ValueError("Riser defining faces have no unambiguous valid solid")
            record = replace(
                proposal.record,
                body_levels=tuple(sorted(body_levels.get(solid, ()))),
                body_key=body_key,
            )
            pending.append((record, nodes))
    pending.sort(key=lambda item: item[0])
    for record, nodes in pending:
        writer.sink.propose(FamilyId.RISERS, record, defining=nodes)
    return [record for record, _nodes in pending]


def project_step_shoulders(
    risers: Sequence[RiserEvidence],
    *,
    levels: Sequence[float | FaceLevel] = (),
    levels_by_riser: Sequence[Sequence[float | FaceLevel]] | None = None,
    tol: float | None = None,
) -> list[StepShoulder]:
    """Project :func:`recognise_risers` evidence onto *levels* — the pure half.

    A candidate riser counts as a step shoulder only if it rises from a level the caller
    recognises: a vertical riser's foot must sit on one, and an oblique ramp's two ends must
    each sit on one or on the part envelope. Pass full :class:`FaceLevel` occurrences when their
    body-local supports distinguish equal-Z levels. For value-identical body occurrences, pass
    ``levels_by_riser`` aligned with the supplied riser occurrence roster; this is an explicit
    serializable occurrence association without a public topology/body handle. Numeric values
    retain the historical value-selection behavior. That is the whole level dependency, and it
    is the whole reason the old ``recognise_step_shoulders`` could not be hoisted into the shared
    aggregate — its answer depends on who is asking.

    Model construction passes levels filtered by plate and pocket ownership; critique passes
    the unfiltered geometry ladder, because the independent-evidence rule forbids lint taking
    its inventory from the model. Both project the same evidence; neither rescans the solid.

    No *part* argument, by construction: this cannot look at geometry, so it cannot become a
    second recognition site. Returns a sorted occurrence list, retaining equal projections from
    separate body-local risers; empty when *levels* is empty (a part with no recognised step has no
    shoulders to locate).

    *tol* defaults to the tolerance the evidence was scanned with, so a two-stage call is
    equivalent to the old single-stage one at ANY tolerance, not just the default. Pass it
    explicitly only to project more or less tightly than the scan deliberately.
    """
    risers = list(risers)
    if not risers:
        return []
    if levels_by_riser is not None:
        if levels:
            raise ValueError("pass levels or levels_by_riser, not both")
        if len(levels_by_riser) != len(risers):
            raise ValueError("levels_by_riser must align one-for-one with risers")
    elif not levels:
        return []
    if tol is None:
        tol = risers[0].tol

    def selected_z(selection: float | FaceLevel) -> float:
        return selection.z if isinstance(selection, FaceLevel) else selection

    def tied(z: float, at_envelope: bool, candidate_levels: Sequence[FaceLevel]) -> bool:
        return at_envelope or any(abs(z - level.z) < tol for level in candidate_levels)

    out: list[StepShoulder] = []
    for ordinal, r in enumerate(risers):
        selections = levels if levels_by_riser is None else levels_by_riser[ordinal]
        if r.body_levels is None:
            body_levels = tuple(FaceLevel(selected_z(level)) for level in selections)
        else:
            body_levels = tuple(
                owned
                for owned in r.body_levels
                if any(
                    (
                        abs(selection.z - owned.z) < tol
                        and selection.x_span == owned.x_span
                        and selection.y_span == owned.y_span
                    )
                    if isinstance(selection, FaceLevel)
                    else abs(selection - owned.z) < tol
                    for selection in selections
                )
            )

        if r.vertical:
            if not any(abs(r.z_lo - level.z) < tol for level in body_levels):
                continue  # rises from a step level (not a through slot's wall)
        elif not (
            tied(r.z_lo, r.lo_at_envelope, body_levels)
            and tied(r.z_hi, r.hi_at_envelope, body_levels)
        ):
            continue  # drafted/incidental face not tied to recognised profile levels
        out.extend(StepShoulder(r.axis, pos) for pos in r.positions)
        out.extend(StepShoulder(r.other_axis, pos) for pos in r.other_positions)
    return sorted(out)
