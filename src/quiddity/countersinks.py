# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Countersink recognition for drilled holes.

``recognise_countersinks`` recovers the countersinks a plain hole recogniser reports as
mere openings: an internal **cone** that flares from a drilled bore (the minor circle)
out to a larger opening (the major circle), coaxial with a **cylinder** of the drill
radius. That is exactly a countersink for a flat-head screw. Keying on it — a cone with
two distinct-radius circular edges whose smaller radius matches a coaxial drilled
cylinder — excludes drill-point cones (a single circle + apex, not flared) and external
edge chamfers (no coaxial bore).

Bottom of the recognition DAG: depends only on build123d/OCP.

Heuristic limits (``recognised`` tier): an edge-break / deburr / lead-in chamfer at a hole
mouth is geometrically a shallow countersink, so a **flare-ratio floor** (``_MIN_MAJOR_RATIO``)
excludes it — a screw seat flares to roughly twice the bore, an edge break barely widens
it; a near-flat cone above ``_MAX_INCLUDED_ANGLE`` (a draft / relief) is excluded; a
  countersink whose trimmed rims cease to be circular is missed. Material-side orientation
  rejects external cones even when trimming leaves circular rim arcs.

Known limitations (edge geometries; the common one-face countersink is exact):

- a **DIN 332 lathe centre-drill** (a 60° cone flaring from a small pilot bore) is
  geometrically near-indistinguishable from a small countersink and can register;
- a through hole countersunk on **both** faces yields two coaxial countersinks, but the
  single ``HoleRecord.csink`` slot records only one — so it under-reports, and (since
  ``HoleSpec`` keys the csink on size only) a both-face hole can group with a one-face
  hole of the same size. A two-slot csink model would be needed to draw both seats.
"""

from __future__ import annotations

import math
from collections.abc import Sequence
from dataclasses import dataclass
from typing import Protocol

from build123d import GeomType
from OCP.gp import gp_Cone

from quiddity._candidates import FamilyId
from quiddity._claims import EvidenceWriter
from quiddity._geometry import length_tol
from quiddity._record import Record
from quiddity._typing import EdgeLike, FaceLike, Part

# Every length gate here is a fraction of the bore it judges, per ADR 0008: an M3 countersink
# and an M30 one are the same feature, and a fixed millimetre gate matches neither well. Each
# fraction is the millimetre constant it replaces over the corpus's 6 mm reference bore
# diameter (3 mm radius).
_MINOR_MATCH_FRAC = 0.0167  # of the drill radius — does this cone sit on that bore?
_COAXIAL_FRAC = 0.0167  # of the drill diameter — how far the opening may sit off its axis
# Real countersinks are ≤120° included (60/82/90/100/120 standards); a near-flat cone is
# a draft/relief/washer face, not a countersink. 160° keeps every real countersink with
# margin while excluding drafts (~176–178° included).
_MAX_INCLUDED_ANGLE = 160.0
# A screw seat flares to roughly twice the bore (a flat-head sits in it); an edge-break /
# deburr / lead-in chamfer on a hole mouth is the same *shape* but barely wider than the
# bore. Require the major to reach this multiple of the drill radius to exclude those —
# else every chamfered hole mouth would be called out as a countersink.
_MIN_MAJOR_RATIO = 1.5
# Fractions of the hole diameter, for associating a recognised cone with a recognised hole.
_HOLE_DIA_FRAC = 0.0333
_HOLE_AXIS_FRAC = 0.0333
_HOLE_MOUTH_FRAC = 0.0833


@dataclass(frozen=True)
class CounterSink(Record):
    """A recognised countersink. ``axis`` points from the wide opening into the part;
    ``location`` is the opening (major-circle) centre; ``major_diameter`` the outer rim,
    ``drill_diameter`` the bore it sits on; ``included_angle`` the full cone angle
    (degrees); ``depth`` the axial cone depth. Fields mirror the shared-package record."""

    axis: tuple[float, float, float]
    location: tuple[float, float, float]
    major_diameter: float
    drill_diameter: float
    included_angle: float
    depth: float


@dataclass(frozen=True, slots=True)
class _CounterSinkProposal:
    record: CounterSink
    face: FaceLike


class _HoleLike(Protocol):
    @property
    def axis(self) -> tuple[float, float, float]: ...

    @property
    def location(self) -> tuple[float, float, float]: ...

    @property
    def diameter(self) -> float: ...

    @property
    def depth(self) -> float: ...

    @property
    def bottom(self) -> str: ...


def countersink_matches_hole(countersink: CounterSink, hole: _HoleLike) -> bool:
    """Return whether *countersink* is seated at a recognised bore mouth.

    CounterSink discovery already proves that the cone opens into a void. This stricter
    association establishes which recognised bore mouth it belongs to. Keep that geometry
    predicate recognition-owned so feature construction and downstream completeness cannot
    drift.
    """
    minor = tuple(
        countersink.location[index] + countersink.depth * countersink.axis[index]
        for index in range(3)
    )
    offset = tuple(minor[index] - hole.location[index] for index in range(3))
    axial = sum(offset[index] * hole.axis[index] for index in range(3))
    perpendicular = math.hypot(*(offset[index] - axial * hole.axis[index] for index in range(3)))
    if perpendicular > length_tol(hole.diameter, rel=_HOLE_AXIS_FRAC) or abs(
        countersink.drill_diameter - hole.diameter
    ) > length_tol(hole.diameter, rel=_HOLE_DIA_FRAC):
        return False
    mouth_tol = length_tol(hole.diameter, rel=_HOLE_MOUTH_FRAC)
    return bool(
        abs(axial) <= mouth_tol
        or (hole.bottom == "through" and abs(axial - hole.depth) <= mouth_tol)
    )


def cone_rims(face: FaceLike) -> tuple[EdgeLike, EdgeLike, float] | None:
    """``(minor_edge, major_edge, included_angle°)`` of a conical *face* — its two circular
    rims (smallest- and largest-radius) and the full cone angle (2 dp) — or ``None`` when
    the cone has fewer than two circular rims (a drill-point cone / degenerate). The
    single-face cone read shared by :func:`recognise_countersinks` and the declared
    explicit-face reader, so both paths use identical rim and angle semantics."""
    from OCP.BRepAdaptor import BRepAdaptor_Surface

    circles = sorted(face.edges().filter_by(GeomType.CIRCLE), key=lambda e: e.radius)
    if len(circles) < 2:
        return None
    cone = BRepAdaptor_Surface(face.wrapped).Cone()
    included = round(2 * abs(math.degrees(cone.SemiAngle())), 2)
    return circles[0], circles[-1], included


def _parallel(a: Sequence[float], b: Sequence[float]) -> bool:
    return bool(abs(a[0] * b[0] + a[1] * b[1] + a[2] * b[2]) > 1 - 1e-3)


def _dist_to_line(
    pt: Sequence[float], line_pt: Sequence[float], line_dir: Sequence[float]
) -> float:
    v = (pt[0] - line_pt[0], pt[1] - line_pt[1], pt[2] - line_pt[2])
    t = v[0] * line_dir[0] + v[1] * line_dir[1] + v[2] * line_dir[2]
    perp = (v[0] - t * line_dir[0], v[1] - t * line_dir[1], v[2] - t * line_dir[2])
    return math.sqrt(perp[0] ** 2 + perp[1] ** 2 + perp[2] ** 2)


def _opens_into_void(face: FaceLike, cone: gp_Cone) -> bool:
    """Whether the conical material-side normal points radially into its axis.

    A hole-mouth seat bounds a void, so its outward-from-material normal points toward the
    cone axis. An external stepped-shaft transition has the same analytic cone and coaxial
    cylinder evidence, but its normal points away from the axis. The sign is invariant under
    rigid transforms and uniform scale; the included-angle gate keeps a valid cone's radial
    component bounded away from zero.
    """
    axis = cone.Axis()
    origin = axis.Location()
    direction = axis.Direction()
    sample = face.center()
    offset = (
        sample.X - origin.X(),
        sample.Y - origin.Y(),
        sample.Z - origin.Z(),
    )
    along = offset[0] * direction.X() + offset[1] * direction.Y() + offset[2] * direction.Z()
    radial = (
        offset[0] - along * direction.X(),
        offset[1] - along * direction.Y(),
        offset[2] - along * direction.Z(),
    )
    normal = face.normal_at(sample)
    return bool(radial[0] * normal.X + radial[1] * normal.Y + radial[2] * normal.Z < 0.0)


def recognise_countersinks(part: Part) -> list[CounterSink]:
    """Recognise the countersinks of *part* (see module docstring). One
    :class:`CounterSink` per qualifying cone, sorted deterministically; empty when the
    part has none."""
    return _discover_countersinks(part)


def _discover_countersinks(
    part: Part, *, writer: EvidenceWriter | None = None
) -> list[CounterSink]:
    """Discover Countersinks and validate every owner before publishing evidence."""
    from OCP.BRepAdaptor import BRepAdaptor_Surface

    cones = list(part.faces().filter_by(GeomType.CONE))
    if not cones:
        return []  # no cones → no countersinks; skip the cylinder scan

    cyls = []
    for cy in part.faces().filter_by(GeomType.CYLINDER):
        # Radius from the same adaptor as the axis, not ``Face.radius``: that reports ``None``
        # for a trimmed cylindrical surface, which a STEP import can carry in quantity — 30 of
        # 58 cylinders on one NIST conformance part — and the ``None`` then reached the bore
        # arithmetic below. The adaptor answers for every one of them, and agrees exactly
        # wherever both are available.
        cylinder = BRepAdaptor_Surface(cy.wrapped).Cylinder()
        ax = cylinder.Axis()
        p, d = ax.Location(), ax.Direction()
        cyls.append((cylinder.Radius(), (p.X(), p.Y(), p.Z()), (d.X(), d.Y(), d.Z())))

    out: list[_CounterSinkProposal] = []
    for f in cones:
        cone = BRepAdaptor_Surface(f.wrapped).Cone()
        rims = cone_rims(f)  # the shared single-face cone read
        if rims is None:
            continue  # drill-point cone (one circle + apex) or degenerate
        minor_e, major_e, included_angle = rims
        minor_r, major_r = minor_e.radius, major_e.radius
        if major_r < _MIN_MAJOR_RATIO * minor_r:
            continue  # too little flare — an edge break / deburr, not a screw seat
        if included_angle > _MAX_INCLUDED_ANGLE:
            continue  # a near-flat cone is a draft/relief/washer face, not a countersink
        if not _opens_into_void(f, cone):
            continue  # an outward-opening cone is an external shaft transition
        opening = major_e.arc_center
        opening_pt = (opening.X, opening.Y, opening.Z)
        mc = minor_e.arc_center
        minor_pt = (mc.X, mc.Y, mc.Z)
        # Axis points INTO the part: from the wide opening toward the drilled bore.
        # (Deterministic — don't trust OCP's cone-axis sign across constructions.)
        av = (
            minor_pt[0] - opening_pt[0],
            minor_pt[1] - opening_pt[1],
            minor_pt[2] - opening_pt[2],
        )
        alen = math.sqrt(av[0] ** 2 + av[1] ** 2 + av[2] ** 2) or 1.0
        axis = (av[0] / alen, av[1] / alen, av[2] / alen)
        # A countersink sits on a drilled bore: a coaxial cylinder of the minor radius.
        if not any(
            abs(r - minor_r) <= length_tol(minor_r, rel=_MINOR_MATCH_FRAC)
            and _parallel(axis, ld)
            and _dist_to_line(opening_pt, lp, ld) <= length_tol(2 * minor_r, rel=_COAXIAL_FRAC)
            for r, lp, ld in cyls
        ):
            continue
        out.append(
            _CounterSinkProposal(
                CounterSink(
                    axis=(round(axis[0], 4), round(axis[1], 4), round(axis[2], 4)),
                    location=(
                        round(opening_pt[0], 4),
                        round(opening_pt[1], 4),
                        round(opening_pt[2], 4),
                    ),
                    major_diameter=round(2 * major_r, 4),
                    drill_diameter=round(2 * minor_r, 4),
                    included_angle=included_angle,
                    depth=round(alen, 4),
                ),
                f,
            )
        )
    out.sort(key=lambda proposal: (proposal.record.location, proposal.record.major_diameter))
    if writer is not None:
        pending = tuple(
            (proposal.record, writer.graph.require_node(proposal.face)) for proposal in out
        )
        if any(writer.graph.common_valid_solid((node,)) is None for _record, node in pending):
            raise ValueError("countersink defining face has no unambiguous valid solid")
        for record, node in pending:
            writer.sink.propose(FamilyId.COUNTERSINKS, record, defining=(node,))
    return [proposal.record for proposal in out]
