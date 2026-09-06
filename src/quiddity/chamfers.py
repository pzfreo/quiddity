# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Chamfer (bevelled-edge) recognition.

``recognise_chamfers`` recovers the manufacturing size of each chamfer so it can be called
out (``C12`` / ``12 × 45°``) rather than left as a rendered-but-undimensioned bevel. A
chamfer is a small **oblique** planar face whose normal is not aligned
with any principal axis (a 45° equal-leg chamfer on a Z edge has normal ≈ (0.707, 0.707,
0)) — that breaks a **convex** edge where two mutually-perpendicular axis-aligned faces
meet. Four gates keep it to genuine chamfers and recover the right size:

- **oblique planar or conical** — a prismatic chamfer is an oblique plane; a turned chamfer is
  its rotational analogue, a short external cone. Both recover their axial/radial legs from
  the analytic face rather than from a rendered view;
- **bridges two axis-aligned faces** — the face is edge-adjacent to axis-aligned faces on
  two distinct in-plane axes, excluding a hex/polygon prism's real oblique sides (whose
  neighbours are themselves oblique);
- **convex** — the virtual sharp corner the bevel replaces (where the two neighbour planes
  cross, at the chamfer's own edge position) lies *outside* the solid. A gusset / rib /
  web fills a **concave** re-entrant corner, so that corner point is buried *inside* the
  material — the discriminator that face-normal + adjacency alone cannot make (a gusset's
  hypotenuse is also edge-adjacent to two perpendicular walls); and
- **not a spanning wedge** — a chamfer's cut is small relative to the part's *overall*
  size, so its larger leg stays under ``max_leg_frac`` of the part's largest dimension; a
  structural ramp/wedge/oversized bevel spans a large fraction and is excluded. Gating
  against the whole-part size (not each leg's own axis extent) keeps a legitimate plate
  edge-break — small in absolute terms — whose cut into a thin thickness axis would trip a
  per-axis fraction of that small extent, while still rejecting a long shallow ramp.

The two legs are the chamfer face's in-plane bbox extents, so an equal-leg and an
asymmetric chamfer are distinguished from the geometry, not estimated from the rendered
view. Depends on :mod:`._bevel` for the single-face read and the convex-corner probe, which
``recognise_angled_steps`` and ``recognise_fillets`` share -- they lived here until three
recognisers wanted them, which is one more than a recogniser should be a substrate for.

**A fifth gate used to live here, and it was not a chamfer test.** A bevel edge-adjacent to a
triangular flat is the slant of an angled blind step, and this recogniser declined it — by
consulting the *other* family's signature through a helper the two shared, so that neither
would report the face and both would agree about which. That is the hand-rolled ownership
device the run-local face graph and its claim ledger were built to remove. Nothing on the face
itself says a chamfer is not a step: the question is which of two families owns it, and ADR 0003
puts that question after discovery, not inside it. So this recogniser proposes every bevel that
clears its own four gates, and
:func:`quiddity._reconcile.chamfers_that_are_not_angled_steps` drops the ones
``recognise_angled_steps`` claimed.

**What that means for a caller.** Run through :func:`quiddity.build_recognition_result`
or :func:`quiddity.feature_census` and the answer is what it always was — the
reconciliation is inside both. Call this function *directly* and a blind step's slant now comes
back as a chamfer, as it did before ``recognise_angled_steps`` existed. That is the same
posture ``recognise_passages`` takes towards a slot's void and ``recognise_turned_steps``
towards a groove's rung: a recogniser reports what its own evidence supports, and a rule that
can see both families decides.
"""

from __future__ import annotations

import math
from dataclasses import dataclass

from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.GeomAbs import GeomAbs_Cone, GeomAbs_Plane

from quiddity._adjacency import (
    FaceEdges,
    edge_face_map,
    nearest_axis_aligned_planes,
    neighbours,
)
from quiddity._bevel import BevelReject, classify_bevel, convex_bevel
from quiddity._candidates import FamilyId
from quiddity._claims import ClaimLedger, EvidenceWriter
from quiddity._features import analyse_cylinders
from quiddity._geometry import (
    AXIS_ALIGNED_COS,
    COORD_FLOOR,
    _coaxial_axis_lines,
    length_tol,
)
from quiddity._record import Record
from quiddity._typing import CylinderInventory, FaceLike, Part
from quiddity.countersinks import cone_rims

#: A chamfer runs *along* one principal axis, so its normal has essentially no component on
#: that axis. Looser than the package's AXIS_ZERO_COS because a chamfer face carries more
#: angular noise than a plane that is nominally perpendicular to an axis.
_RUN_AXIS_COS = 0.05

# Analytic axes derived from adjacent STEP faces should coincide; this permits only their
# modelling noise. It is a tolerance, so it follows the cylinder diameter (ADR 0008).
_COAXIAL_FRAC = 1e-4


def _below_minimum_evidence(value: float, minimum: float) -> bool:
    """Whether a measured leg is materially below the declared evidence floor.

    Imported analytic geometry carries final-bit coordinate noise: GRM-03's nominal 0.3 mm
    chamfer measures infinitesimally below 0.3 before the public record is quantised. Equality
    belongs on the accepted side of a minimum, so use the package coordinate floor only to
    settle that boundary; it does not widen the feature-size policy in practice.
    """

    return value < minimum and not math.isclose(value, minimum, rel_tol=0.0, abs_tol=COORD_FLOOR)


@dataclass(frozen=True)
class Chamfer(Record):
    """A recognised chamfer. ``axis`` is the chamfered edge's direction ("x"/"y"/"z");
    ``leg1``/``leg2`` are the cut depths into the two adjacent faces (equal for a 45°
    chamfer, ``leg1`` the larger); ``angle`` is the chamfer angle in degrees (45 for
    equal-leg); ``at`` is the chamfer face centre in part space (the callout leader's
    tip). ``turned`` distinguishes a conical treatment swept around a shaft from a planar
    prismatic bevel; it defaults to ``False`` for constructor compatibility."""

    axis: str
    leg1: float
    leg2: float
    angle: float
    at: tuple[float, float, float]
    turned: bool = False


def recognise_chamfers(
    part: Part,
    *,
    tol: float | None = None,
    max_leg_frac: float = 0.45,
    face_edges: FaceEdges | None = None,
    ledger: ClaimLedger | EvidenceWriter | None = None,
    cyls: CylinderInventory | None = None,
    include_planar: bool = True,
) -> list[Chamfer]:
    """Recognise the chamfers of *part* (see module docstring). Returns one
    :class:`Chamfer` per qualifying bevel face, sorted deterministically. Empty when the
    part has no chamfer. A prismatic chamfer is an oblique plane and a turned chamfer is a
    short external cone; both must run along one principal axis. A compound corner bevel
    (oblique on all three axes) is skipped.

    Pass *cyls* — a precomputed :func:`analyse_cylinders` result — to avoid re-scanning a
    turned part. It is consulted only for conical faces, to prove that the cone is outside the
    material rather than a countersink or other internal transition.

    Set *include_planar* to ``False`` when a caller has already classified the part as
    rotational. That preserves the aggregate's existing policy of not treating incidental
    planar bevels on a turned part as its turned chamfer inventory, while retaining its
    principal-axis external cones.

    *ledger* records the face a chamfer was **established by**: the bevel, and only that. The
    two axis-aligned planes it bridges remain unclaimed: they locate the virtual sharp corner
    the convexity probe tests, and each belongs to whatever feature owns it.

    A blind step's slant clears every gate here, because on the face alone it is a bevel; pass
    the ledger ``recognise_angled_steps`` wrote into and
    :func:`quiddity._reconcile.chamfers_that_are_not_angled_steps` removes it."""
    bb = part.bounding_box()
    # Geometry, rather than callout significance, decides the default answer. A caller that
    # deliberately wants a minimum reportable leg may still supply it through ``tol``.
    tol = 0.0 if tol is None else tol
    ext = {0: bb.max.X - bb.min.X, 1: bb.max.Y - bb.min.Y, 2: bb.max.Z - bb.min.Z}
    all_faces = list(part.faces())
    edge_faces = edge_face_map(all_faces, face_edges=face_edges)

    out: list[tuple[Chamfer, FaceLike]] = []
    for f in all_faces if include_planar else ():
        # The shared single-face read (classification thresholds + legs = the face's OWN
        # in-plane bbox extents, not measured against a possibly distant outermost wall).
        # (The old extra skew gate on the in-plane normal components was unreachable — a
        # unit normal with two components < 0.05 forces the third > 0.99, which "aligned"
        # already rejects, so the redundant gate was removed when thresholds moved.)
        try:
            edge_i, _nv, span, leg_hi, leg_lo = classify_bevel(f)
        except BevelReject:
            continue
        oi = [j for j in (0, 1, 2) if j != edge_i]
        fc = {i: 0.5 * (span[i][0] + span[i][1]) for i in (0, 1, 2)}  # face centre
        if _below_minimum_evidence(leg_lo, tol):
            continue
        if leg_hi > max_leg_frac * max(ext.values()):
            continue  # a ramp/wedge spanning a large fraction of the part — not an edge break
        # Must bridge two axis-aligned faces on distinct in-plane axes (a chamfer replaces
        # a sharp 90° edge). A hex side abuts oblique faces. Each neighbour plane's
        # coordinate lets the convex-corner test below reconstruct the virtual corner.
        neigh_coord = nearest_axis_aligned_planes(
            f, edge_faces, fc, exclude_axis=edge_i, face_edges=face_edges
        )
        if oi[0] not in neigh_coord or oi[1] not in neigh_coord:
            continue
        if not convex_bevel(part, fc, edge_i, neigh_coord):
            continue  # concave corner — a gusset / rib / web, not a chamfer
        # Anchor the leader on the bevel FACE (its centroid), not the supporting plane's
        # parametric origin: that origin is arbitrary (OCC parameterisation) and can project to
        # a chamfer endpoint/corner rather than the middle of the diagonal. The centroid
        # of the convex planar bevel lies on the face; this also matches declare's
        # _read_chamfer_face, so detected and declared chamfers anchor identically.
        fctr = f.center()
        angle = math.degrees(math.atan2(leg_lo, leg_hi))
        out.append(
            (
                Chamfer(
                    axis="xyz"[edge_i],
                    leg1=round(leg_hi, 3),
                    leg2=round(leg_lo, 3),
                    angle=round(angle, 2),
                    at=(round(fctr.X, 3), round(fctr.Y, 3), round(fctr.Z, 3)),
                ),
                f,
            )
        )

    # A lathe turns the planar cut of an ordinary chamfer into a cone. Its two circular rims
    # give exactly the same legs as the planar reader: axial separation and *radial* (not
    # diameter) reduction. Requiring an adjacent external cylinder is the material-side proof
    # that distinguishes it from a countersink, which has the same analytic cone but neighbours
    # an internal bore.
    cones = [f for f in all_faces if BRepAdaptor_Surface(f.wrapped).GetType() == GeomAbs_Cone]
    if cones:
        z_cyls, cross_cyls = cyls if cyls is not None else analyse_cylinders(part)
        external_cylinders = {c["face"]: c for c in (*z_cyls, *cross_cyls) if c["external"]}
        for f in cones:
            rims = cone_rims(f)
            if rims is None:
                continue  # drill point / degenerate cone, not an edge treatment
            minor, major, _included = rims
            cone = BRepAdaptor_Surface(f.wrapped).Cone()
            cone_axis = cone.Axis()
            direction = cone_axis.Direction()
            dv = (direction.X(), direction.Y(), direction.Z())
            edge_i = max(range(3), key=lambda i: abs(dv[i]))
            if abs(dv[edge_i]) < AXIS_ALIGNED_COS:
                continue  # a slanted cone is not a principal-axis turned chamfer
            radial = major.radius - minor.radius
            minor_c, major_c = minor.arc_center, major.arc_center
            axial = abs(
                (major_c.X - minor_c.X) * dv[0]
                + (major_c.Y - minor_c.Y) * dv[1]
                + (major_c.Z - minor_c.Z) * dv[2]
            )
            leg_hi, leg_lo = max(axial, radial), min(axial, radial)
            if _below_minimum_evidence(leg_lo, tol) or leg_hi > max_leg_frac * max(ext.values()):
                continue
            cone_neighbours = neighbours(f, edge_faces, face_edges=face_edges)
            axis_location = cone_axis.Location()
            axis_point = (axis_location.X(), axis_location.Y(), axis_location.Z())
            coaxial_cylinders = [
                external_cylinders[n]
                for n in cone_neighbours
                if n in external_cylinders
                and _coaxial_axis_lines(
                    axis_point,
                    dv,
                    external_cylinders[n]["axis_xyz"],
                    external_cylinders[n]["dir_xyz"],
                    tol=length_tol(external_cylinders[n]["diameter"], rel=_COAXIAL_FRAC),
                )
            ]
            if not coaxial_cylinders:
                continue
            transverse_plane = False
            for neighbour in cone_neighbours:
                neighbour_surface = BRepAdaptor_Surface(neighbour.wrapped)
                if neighbour_surface.GetType() != GeomAbs_Plane:
                    continue
                nd = neighbour_surface.Plane().Axis().Direction()
                normal_dot = nd.X() * dv[0] + nd.Y() * dv[1] + nd.Z() * dv[2]
                if abs(normal_dot) > AXIS_ALIGNED_COS:
                    transverse_plane = True
                    break
            bridges_two_bands = len({c["diameter"] for c in coaxial_cylinders}) >= 2
            if not transverse_plane and not bridges_two_bands:
                continue  # a taper meeting one band, not a bevelled edge
            fctr = f.center()
            out.append(
                (
                    Chamfer(
                        axis="xyz"[edge_i],
                        leg1=round(leg_hi, 3),
                        leg2=round(leg_lo, 3),
                        angle=round(math.degrees(math.atan2(leg_lo, leg_hi)), 2),
                        at=(round(fctr.X, 3), round(fctr.Y, 3), round(fctr.Z, 3)),
                        turned=True,
                    ),
                    f,
                )
            )
    out.sort(key=lambda pair: (pair[0].axis, pair[0].at))
    if ledger is not None:
        for chamfer, face in out:
            ledger.add_defining(
                chamfer,
                [ledger.graph.require_node(face)],
                family=FamilyId.CHAMFERS,
            )
    return [chamfer for chamfer, _ in out]
