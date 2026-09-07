# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""turned — step/shoulder recognition for turned (rotational) parts.

``recognise_turned_steps`` extracts the axial steps of a stepped shaft — the
contiguous segments between shoulders — so the engine can dimension each step
*length* (the drive-screw gap: every diameter dimensioned, but no shoulder
locatable). It builds on this package's ``analyse_cylinders`` primitive.

Why not ``recognise_bosses``: a boss's ``.height`` is its *cylindrical-face* length,
shortened by the chamfers at each shoulder, so boss spans neither tile the axis
nor sum to the overall length — wrong for axial dims. ``analyse_cylinders``
instead gives each cylinder's true axial span (``s_lo``/``s_hi``) and an
``external`` flag.

Algorithm:

1. Take the **external** cylinders on the dominant turning axis (≥2 distinct
   diameters, else the part is not a stepped turned part → ``None``). Internal
   bores are excluded by the ``external`` flag, so a bored shaft is handled and
   a blind bore's flat bottom never reads as a shoulder.
2. The shoulders/end faces are the part's transverse planar faces (normal along
   the axis). Keep a face only when its outer radius **reaches the local OD
   silhouette** (the max external-band radius spanning that axial position,
   within a chamfer allowance). This separates true shoulders/ends — which reach
   the OD — from internal feature faces (a bore bottom sits well inside the OD).
   The allowance tolerates the chamfers that shrink a real shoulder face below
   the nominal OD.
3. Sorted shoulder positions delimit the steps; each step carries the local OD.
"""

from __future__ import annotations

from collections import Counter
from collections.abc import Iterable
from dataclasses import dataclass
from functools import total_ordering

from quiddity._body_identity import BodyKey, unambiguous_body_keys
from quiddity._candidates import FamilyId
from quiddity._claims import ClaimLedger, EvidenceWriter
from quiddity._features import analyse_cylinders
from quiddity._record import Record
from quiddity._solid_properties import SolidProperties, solid_properties
from quiddity._typing import CylinderEvidence, CylinderInventory, Part

# A face's axial position counts as on a band edge / its normal counts as
# axis-aligned within these tolerances (mm / unit-vector component).
_AXIS_NORMAL_TOL = 0.05
# Pad a band's [s_lo, s_hi] when asking "what is the OD here", so a shoulder face sitting
# exactly at a (chamfer-shortened) band edge still sees that band's OD.
#
# DELIBERATELY ABSOLUTE, per ADR 0008. The pad spans an edge break, the same physical constant
# as _CHAMFER_ALLOWANCE_ABS below, and a deburr does not grow with the shaft. Making it a
# fraction of the band diameter was tried and reverted: at 8.75% it reaches 2.6 mm on a 30 mm
# band, bridges the 5 mm groove in the turned-step golden, and reports the groove's step at its
# neighbour's OD.
#
# It is also capped at half the band's own width. That cap used to carry the whole safety
# argument -- "it can never bridge the band it pads, which is what keeps it safe on a part
# modelled small" -- which is true and is not the property that matters: what bridges a groove
# is the pad on the wide band *beside* it, which its own half-width does not restrain at all.
# What keeps this safe is that the pad is now consulted only when no band contains the position,
# so it can never override a band that does. See `bands_over`.
_OD_SPAN_PAD = 0.7
# A transverse face is a shoulder/end when its outer radius is within this of the
# local OD. Constant + proportional terms cover both a fixed edge-break and a
# chamfer that scales with the feature — enough to keep a chamfered shoulder, far
# less than the gap between an internal bore radius and the OD.
#
# The sum is capped at half the local radius, and that cap is not decoration: without it the
# absolute term alone exceeds the whole radius on a small part and the threshold goes
# *negative*, so every transverse face qualifies as a shoulder. Measured on the GRM-03 screw at
# 0.05x, where the radius is 0.125 and the allowance 0.515: a 0.04-radius internal face was
# admitted and split a rung into two of equal diameter. A face that does not reach even halfway
# to the OD is not a shoulder, however generous the deburr allowance.
_CHAMFER_ALLOWANCE_ABS = 0.5
_CHAMFER_ALLOWANCE_FRAC = 0.12
# A genuine turned body is round about its axis: the perpendicular cross-section is
# roughly square and the OD silhouette fills it. Looser than the rotational
# classifier's gate (chamfers/features perturb the bbox), but firmly rejects an
# incidental cylinder on a prismatic part (a tiny OD in a large oblong bbox).
_SQUARENESS_TOL = 0.15
_OD_FILL_MIN = 0.6


@total_ordering
@dataclass(frozen=True)
class TurnedProfileKey(Record):
    """Serializable body-local membership for one turned profile.

    ``axis_origin`` is the canonical closest point on the principal axis line with its axial
    component set to zero. ``body_bounds`` distinguishes coaxial-disjoint bodies without exposing
    graph or topology identity.
    """

    axis: str
    axis_origin: tuple[float, float, float]
    body_bounds: tuple[float, float, float, float, float, float]
    # Exact public join to other records discovered on this physical body.
    body_key: BodyKey | None = ()

    def __lt__(self, other: object) -> bool:
        if not isinstance(other, TurnedProfileKey):
            return NotImplemented
        return self._order_key() < other._order_key()

    def _order_key(self) -> tuple[object, ...]:
        return (
            self.axis,
            self.axis_origin,
            self.body_bounds,
            self.body_key is not None,
            self.body_key or (),
        )


@dataclass(frozen=True)
class TurnedStep(Record):
    """One axial segment of a stepped shaft, between two shoulders (or ends). ``axis`` is
    the turning axis ("x"/"y"/"z") the segment is coaxial about — carried on the step so
    it is a **self-contained record**: ``lo``/``hi``/``diameter`` are only interpretable
    given the axis they are measured along."""

    axis: str  # "x" / "y" / "z"
    lo: float
    hi: float
    diameter: float  # the external OD over this segment
    profile: TurnedProfileKey | None = None

    @property
    def length(self) -> float:
        return self.hi - self.lo


@dataclass(frozen=True)
class TurnedProfile(Record):
    """The turned-shaft **aggregate** for the annotation pipeline — the coaxial ``steps``
    plus their shared ``axis`` and derived shoulders. **Not a recogniser return**
    (:func:`recognise_turned_steps` returns ``list[TurnedStep]`` per package ADR 0002); built
    from that list via :meth:`from_steps` for consumers that want axis + shoulders as a
    unit."""

    axis: str  # "x" / "y" / "z"
    steps: tuple[TurnedStep, ...]
    profile: TurnedProfileKey | None = None

    @classmethod
    def from_steps(cls, steps: Iterable[TurnedStep]) -> TurnedProfile | None:
        """Aggregate a recogniser's ``list[TurnedStep]`` into a profile, or ``None`` if
        empty (a non-turned part). The steps must be **coaxial** — a mixed-axis input is a
        programming error and raises (it would otherwise silently pick one axis and
        misrepresent the rest). Steps are sorted by ``lo`` so ``shoulders`` is correct
        regardless of input order; contiguity / non-overlap is a caller precondition (the
        recogniser guarantees it — this aggregate does not re-validate spans)."""
        steps = tuple(steps)
        if not steps:
            return None
        axes = {s.axis for s in steps}
        if len(axes) != 1:
            raise ValueError(f"turned steps are not coaxial: got axes {sorted(axes)}")
        memberships = {step.profile for step in steps}
        if len(memberships) != 1:
            raise ValueError("turned steps belong to multiple physical profiles")
        return cls(
            axis=next(iter(axes)),
            steps=tuple(sorted(steps, key=lambda s: s.lo)),
            profile=next(iter(memberships)),
        )

    @classmethod
    def grouped_from_steps(cls, steps: Iterable[TurnedStep]) -> tuple[TurnedProfile, ...]:
        """Group a deterministic occurrence roster into physical turned profiles.

        Recogniser-produced records group by their serializable :class:`TurnedProfileKey`.
        Hand-built legacy records have no membership key and retain their historical one-profile-
        per-axis interpretation.
        """

        groups: dict[tuple[str, TurnedProfileKey | None], list[TurnedStep]] = {}
        for step in steps:
            groups.setdefault((step.axis, step.profile), []).append(step)
        profiles = [
            cls.from_steps(group)
            for _key, group in sorted(
                groups.items(),
                key=lambda item: (
                    item[0][0],
                    item[0][1] is not None,
                    item[0][1],
                ),
            )
        ]
        return tuple(profile for profile in profiles if profile is not None)

    @property
    def shoulders(self) -> tuple[float, ...]:
        """Sorted axial positions of every step's faces (shoulders + the two ends).

        The union of every step's ``lo`` AND ``hi`` — identical to ``lo``s + the
        last ``hi`` for a contiguous chain (each ``hi`` is the next ``lo``), but
        also correct for a NON-contiguous profile (e.g. two coaxial discs with an
        axial gap), where an interior ``hi`` is a real end face, not a shared
        shoulder — so it is not silently dropped."""
        if not self.steps:
            return ()
        return tuple(sorted({p for s in self.steps for p in (s.lo, s.hi)}))


def profile_key_from_bands(
    part: Part,
    axis: str,
    bands: list[CylinderEvidence],
    *,
    body_key: BodyKey | None = (),
    properties: SolidProperties | None = None,
) -> TurnedProfileKey:
    """Return the public profile membership proved by one body's cylinder bands.

    This is shared by the step ladder and other turned features that must publish the exact
    same body/profile join.  Callers must pass bands already partitioned to *part*.

    *properties* is the run's whole-solid cache. ``recognise_grooves`` calls this once per
    shaft, and every shaft of one body wants that body's *same* bounding box: on the 664-face
    ``nist_ctc_02`` those 57 calls were 10.2 s of a 26 s run, all of it one box computed 57
    times. The key is unchanged -- the same box, asked once.
    """
    idx = "xyz".index(axis)

    def axis_origin(band: CylinderEvidence) -> tuple[float, float, float]:
        point = band["axis_xyz"]
        values = tuple(
            0.0 if coordinate_idx == idx else round(float(value), 8)
            for coordinate_idx, value in enumerate(point)
        )
        return values[0], values[1], values[2]

    origins = Counter(axis_origin(band) for band in bands)
    origin = min(origins, key=lambda value: (-origins[value], value))
    bounds = solid_properties(properties).bounding_box(part)
    body_bounds = (
        round(float(bounds.min.X), 8),
        round(float(bounds.max.X), 8),
        round(float(bounds.min.Y), 8),
        round(float(bounds.max.Y), 8),
        round(float(bounds.min.Z), 8),
        round(float(bounds.max.Z), 8),
    )
    return TurnedProfileKey(axis, origin, body_bounds, body_key)


def recognise_turned_steps(
    part: Part,
    *,
    cyls: CylinderInventory | None = None,
    ledger: ClaimLedger | EvidenceWriter | None = None,
) -> list[TurnedStep]:
    """Recognise the axial steps of a stepped turned ``part``.

    Returns ``[]`` for a non-turned part, a plain (single-diameter) cylinder, or
    anything with fewer than two steps — nothing to dimension axially. Each
    :class:`TurnedStep` carries the turning ``axis``; aggregate them with
    :meth:`TurnedProfile.from_steps` if the axis/shoulders unit is wanted.

    Pass *cyls* — a precomputed ``analyse_cylinders(part)`` result — to avoid
    re-scanning the solid, matching :func:`recognise_holes`'s dependency-injection contract.

    *ledger* records the bands a step was **established by**: the widest external bands lying
    over its span, which are what set its diameter. The shoulder planes that set ``lo`` and
    ``hi`` are read from transverse faces belonging to the neighbouring steps, and claiming
    those would have every rung of the ladder contest the ones either side of it.

    A rung whose band is also a groove is not dropped -- the ladder is a profile, and a profile
    with a hole in it describes a different shaft. See :mod:`quiddity._reconcile`.
    """
    inventory = cyls if cyls is not None else analyse_cylinders(part)
    properties = solid_properties(None if ledger is None else ledger.graph)
    scopes = list(part.solids()) or [part]
    body_keys = unambiguous_body_keys(scopes, require_valid_solid=True, properties=properties)
    proposals: list[tuple[TurnedStep, list[CylinderEvidence]]] = []
    for solid_idx, (scope, body_key) in enumerate(zip(scopes, body_keys, strict=True)):
        scoped = (
            [item for item in inventory[0] if len(scopes) == 1 or item["solid_idx"] == solid_idx],
            [item for item in inventory[1] if len(scopes) == 1 or item["solid_idx"] == solid_idx],
        )
        proposals.extend(
            _turned_step_proposals_one(scope, cyls=scoped, body_key=body_key, properties=properties)
        )
    if ledger is not None:
        # Bind and validate the complete family before publishing any occurrence. A malformed
        # cylinder inventory must not leave a partial candidate prefix in the aggregate run.
        pending = [
            (
                step,
                tuple(ledger.graph.require_node(c["face"]) for c in over),
            )
            for step, over in proposals
        ]
        profile_owners: dict[TurnedProfileKey, object] = {}
        for step, nodes in pending:
            owner = ledger.graph.common_valid_solid(nodes)
            if owner is None:
                raise ValueError("turned step evidence has no common valid solid")
            assert step.profile is not None
            previous = profile_owners.setdefault(step.profile, owner)
            if previous != owner:
                raise ValueError("turned profile key identifies multiple valid solids")
        for step, nodes in pending:
            ledger.add_defining(step, nodes, family=FamilyId.TURNED_STEPS)
    found = [step for step, _over in proposals]
    return sorted(
        found,
        key=lambda step: (
            step.profile is not None,
            step.profile,
            step.axis,
            step.lo,
            step.hi,
            step.diameter,
        ),
    )


def _turned_step_proposals_one(
    part: Part,
    *,
    cyls: CylinderInventory,
    body_key: BodyKey | None,
    properties: SolidProperties | None = None,
) -> list[tuple[TurnedStep, list[CylinderEvidence]]]:
    """Propose one valid-solid turned profile from prepartitioned cylinder evidence."""

    z_cyls, cross_cyls = cyls
    ext = [c for c in (*z_cyls, *cross_cyls) if c.get("external")]
    if not ext:
        return []
    axis, _ = Counter(c["axis"] for c in ext).most_common(1)[0]
    bands = [c for c in ext if c["axis"] == axis]
    if len({round(c["diameter"], 2) for c in bands}) < 2:
        return []  # one OD → not a stepped turned part
    idx = "xyz".index(axis)

    # A genuine turned shaft is a body of revolution about *axis*: its OD silhouette
    # (largest external band) fills a roughly-square cross-section perpendicular to the
    # axis. Reject incidental small cylinders on a prismatic part — e.g. a case shell's
    # side screw-holes — whose unrelated feature faces would otherwise be read as a
    # spurious multi-step profile.
    memo = solid_properties(properties)
    pbb = memo.bounding_box(part)
    perp = [s for i, s in enumerate((pbb.size.X, pbb.size.Y, pbb.size.Z)) if i != idx]
    cross = max(perp)
    max_od = max(c["diameter"] for c in bands)
    if (
        cross <= 0
        or max_od < _OD_FILL_MIN * cross
        or abs(perp[0] - perp[1]) > _SQUARENESS_TOL * cross
    ):
        return []

    profile = profile_key_from_bands(part, axis, bands, body_key=body_key, properties=memo)

    def bands_over(pos: float) -> list[CylinderEvidence]:
        """The widest external bands covering *pos* -- what sets the OD there, and therefore
        what a step at *pos* is established by. Split out of `local_od` so the diameter and the
        faces behind it come from one selection rather than two that could drift apart."""

        # A band that actually contains *pos* wins outright, and the pad is only consulted when
        # none does. That is what the pad was introduced for -- a position landing in the edge
        # break between two bands -- and letting it also override a band that genuinely covers
        # the point is what made this scale-dependent: on the pinned turned-step fixture at
        # 0.05x, the 2.525-wide neighbour padded 0.7 straight across the 0.25-wide groove, so
        # every rung reported the shaft OD and the profile described a plain shaft.
        #
        # Capping the pad at half the narrowest band was tried instead and ties exactly on that
        # band's midpoint, which is the position a step's OD is read at -- so it fixes nothing
        # and depends on a float comparison landing the right way. Containment has no such
        # edge, and needs no constant.
        over: list[CylinderEvidence] = [c for c in bands if c["s_lo"] <= pos <= c["s_hi"]]
        if not over:
            for c in bands:
                pad = min(_OD_SPAN_PAD, (c["s_hi"] - c["s_lo"]) / 2)
                if c["s_lo"] - pad <= pos <= c["s_hi"] + pad:
                    over.append(c)
        if not over:
            return []
        widest = max(c["diameter"] for c in over)
        return [c for c in over if c["diameter"] == widest]

    def local_od(pos: float) -> float:
        over = bands_over(pos)
        return over[0]["diameter"] / 2 if over else 0.0

    shoulders: set[float] = set()
    for face in part.faces():
        try:
            nrm = face.normal_at(face.center())
        except Exception:  # noqa: BLE001 — a face whose normal won't evaluate isn't a shoulder
            continue
        nv = (nrm.X, nrm.Y, nrm.Z)
        if abs(abs(nv[idx]) - 1) > _AXIS_NORMAL_TOL or any(
            abs(nv[j]) > _AXIS_NORMAL_TOL for j in range(3) if j != idx
        ):
            continue  # not transverse to the axis
        bb = face.bounding_box()
        pos = (face.center().X, face.center().Y, face.center().Z)[idx]
        spans = ((bb.min.X, bb.max.X), (bb.min.Y, bb.max.Y), (bb.min.Z, bb.max.Z))
        outer = max(
            max(
                abs(spans[j][0] - profile.axis_origin[j]),
                abs(spans[j][1] - profile.axis_origin[j]),
            )
            for j in range(3)
            if j != idx
        )
        od = local_od(pos)
        allowance = min(_CHAMFER_ALLOWANCE_ABS + _CHAMFER_ALLOWANCE_FRAC * od, od / 2)
        if outer >= od - allowance:
            shoulders.add(round(pos, 3))

    planes = sorted(shoulders)
    if len(planes) < 3:  # fewer than two steps
        return []
    # A segment whose midpoint has no external band over it (`local_od` → 0) is a
    # gap between disconnected bands, not a real step — drop it so it never renders
    # as a phantom ø0 diameter.
    found = []
    for i in range(len(planes) - 1):
        # One selection, read twice: the diameter and the faces behind it must be the same
        # bands, and calling `bands_over` again would only invite them to stop being.
        over = bands_over((planes[i] + planes[i + 1]) / 2)
        step = TurnedStep(
            axis=axis,
            lo=planes[i],
            hi=planes[i + 1],
            diameter=over[0]["diameter"] if over else 0.0,
            profile=profile,
        )
        if step.diameter > 0:
            found.append((step, over))
    if len(found) < 2:  # fewer than two real steps → nothing to dimension axially
        return []
    return found
