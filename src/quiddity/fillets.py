# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Fillet (rounded-edge) radius recognition.

``recognise_fillets`` recovers the radius of each external edge fillet so it can be
called out (``R3`` / ``4× R3``) rather than left as a rendered-but-undimensioned round. A
fillet is the **arc analog** of a chamfer (:mod:`.chamfers`): where a chamfer
is a small *oblique planar* face bevelling a convex edge, a fillet is a small *partial
cylindrical* face rounding one. On turned stock that same round is a short external torus.
The gates mirror the chamfer's, swapping the plane test for a cylinder/torus test and the leg
geometry for the blend radius:

- **partial cylinder** — the face's angular span is less than a bore's, so a real hole /
  boss (a full or near-full turn) is excluded; a convex edge fillet is a quarter-turn;
- **axis-aligned run** — the cylinder axis is one principal direction (the edge the fillet
  runs along); a compound corner round (a sphere-like blend) is skipped;
- **bridges two axis-aligned faces** on distinct in-plane axes — a fillet rounds a sharp
  90° edge between two axis-aligned walls (this also excludes a milled-slot end cap, whose
  two neighbours are parallel walls on the *same* axis); and
- **convex** — the virtual sharp corner the round replaces (where the two neighbour planes
  cross) lies *outside* the solid. An **internal** round filling a concave re-entrant
  corner buries that corner *inside* the material — the discriminator that face type +
  adjacency alone cannot make, so an internal fillet / a slot-wall blend / a
  counterbore-floor round is excluded.

The radius is the cylinder radius, read from the geometry, not estimated from the view.
A too-small round (an edge-break / deburr, below ``min_radius``) is not a dimensioned
feature.

Depends on :mod:`.chamfers` for :func:`~quiddity.chamfers.convex_bevel` rather than
copying it, as :mod:`.angled_steps` does and for the same reason: a change to what counts as a
convex corner should reach every family that asks. That test was a line-for-line copy here until
epic 0002 item 2 — same construction, same probe fraction, same classifier tolerance — and two
copies of one decision can drift with nothing to notice.
"""

from __future__ import annotations

import math
from dataclasses import dataclass

from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.GeomAbs import GeomAbs_Cylinder, GeomAbs_Plane, GeomAbs_Sphere, GeomAbs_Torus

from quiddity._adjacency import (
    FaceEdges,
    edge_face_map,
    nearest_axis_aligned_planes,
    neighbours,
)
from quiddity._bevel import convex_bevel
from quiddity._candidates import FamilyId
from quiddity._claims import EvidenceWriter
from quiddity._features import analyse_cylinders
from quiddity._geometry import AXIS_ALIGNED_COS, _coaxial_axis_lines, length_tol
from quiddity._record import Record
from quiddity._solid_properties import run_solid_properties
from quiddity._typing import CylinderInventory, FaceLike, Part, SurfaceAdaptor

#: **A minimum-evidence threshold, not a tolerance — deliberately absolute (ADR 0008).**
#: Scaling it to the part makes a feature's existence depend on what surrounds it, so a small
#: feature on a large part disappears. Whether such a feature is worth dimensioning is consumer
#: policy, and ADR 0001 puts policy with the consumer; recognition reports it either way.
_MIN_RADIUS = 0.6

# A convex edge fillet is a quarter-turn (≈π/2); a real bore keeps more than half a turn.
# Gate below this (the same threshold that keeps partial cylinders out of hole/boss
# recognition, `_features._FULL_CYL_MIN_EXTENT`) so a bore is excluded; the convex test
# then drops the internal rounds / slot end caps (half turns) that also pass this gate.
_FILLET_MAX_EXTENT = math.pi * 1.05

# A turned edge fillet is a quarter-circle in the radial/axial section. Slightly loose for
# imported parameter noise, but below the half-round of a toroidal bead or rolled feature.
_TURNED_FILLET_MAX_EXTENT = math.pi / 2 * 1.05

# Analytic axes derived from adjacent STEP faces should coincide; this permits only their
# modelling noise. It is a tolerance, so it follows the cylinder diameter (ADR 0008).
_COAXIAL_FRAC = 1e-4


@dataclass(frozen=True)
class Fillet(Record):
    """A recognised external edge fillet. ``axis`` is the rounded edge's direction
    ("x"/"y"/"z"); ``radius`` is the fillet radius (the cylinder radius); ``at`` is the
    fillet face centre in part space (the ``R`` callout leader's tip). ``turned`` distinguishes
    a toroidal treatment swept around a shaft from a cylindrical prismatic blend; it defaults
    to ``False`` for constructor compatibility."""

    axis: str
    radius: float
    at: tuple[float, float, float]
    turned: bool = False


@dataclass(frozen=True, slots=True)
class _FilletProposal:
    record: Fillet
    face: FaceLike
    turned_context: tuple[FaceLike, ...]


def fillet_anchor(s: SurfaceAdaptor) -> tuple[float, float, float]:
    """A leader-tip point on the analytic surface at the middle of *s*'s trimmed
    U/V parameter bounds. An interior trimming hole can exclude that point from the retained
    topological face. Never use the bbox centre, which sits off the analytic round near the
    virtual sharp corner.
    The implementation is shared by :func:`recognise_fillets` and explicit-face readers,
    so both paths select the same leader anchor."""
    u_mid = 0.5 * (s.FirstUParameter() + s.LastUParameter())
    v_mid = 0.5 * (s.FirstVParameter() + s.LastVParameter())
    p = s.Value(u_mid, v_mid)
    return (p.X(), p.Y(), p.Z())


def recognise_fillets(
    part: Part,
    *,
    min_radius: float | None = None,
    max_radius_frac: float = 0.45,
    face_edges: FaceEdges | None = None,
    cyls: CylinderInventory | None = None,
    include_cylindrical: bool = True,
) -> list[Fillet]:
    """Recognise the external edge fillets of *part* (see module docstring). Returns one
    :class:`Fillet` per qualifying blend face, sorted deterministically. A prismatic blend
    is a partial cylinder; a turned blend is a short external torus. Empty when the part has
    no dimension-worthy fillet. Only single-axis fillets (running along one principal axis)
    are recovered; a compound corner round is skipped.

    Pass *cyls* — a precomputed :func:`analyse_cylinders` result — to avoid re-scanning a
    turned part. It is consulted only for toroidal faces, to prove that the blend is external
    rather than an internal bore round. Set *include_cylindrical* to ``False`` for a part
    already classified as rotational, retaining that aggregate's policy for incidental
    prismatic blends while recognising its toroidal fillets."""
    return _discover_fillets(
        part,
        min_radius=min_radius,
        max_radius_frac=max_radius_frac,
        face_edges=face_edges,
        cyls=cyls,
        include_cylindrical=include_cylindrical,
    )


def _discover_fillets(
    part: Part,
    *,
    min_radius: float | None,
    max_radius_frac: float,
    face_edges: FaceEdges | None,
    cyls: CylinderInventory | None,
    include_cylindrical: bool,
    writer: EvidenceWriter | None = None,
) -> list[Fillet]:
    """Discover Fillets and validate every defining-face binding before publication."""

    bb = run_solid_properties(writer).bounding_box(part)
    min_radius = _MIN_RADIUS if min_radius is None else min_radius
    max_ext = max(bb.max.X - bb.min.X, bb.max.Y - bb.min.Y, bb.max.Z - bb.min.Z)
    all_faces = list(part.faces())
    edge_faces = edge_face_map(all_faces, face_edges=face_edges)

    out: list[_FilletProposal] = []
    for f in all_faces if include_cylindrical else ():
        fw = f.wrapped
        s = BRepAdaptor_Surface(fw)
        if s.GetType() != GeomAbs_Cylinder:
            continue
        if s.LastUParameter() - s.FirstUParameter() >= _FILLET_MAX_EXTENT:
            continue  # a full/near-full turn — a real bore/boss, not an edge blend
        cyl = s.Cylinder()
        radius = cyl.Radius()
        if radius < min_radius or radius > max_radius_frac * max_ext:
            continue  # a deburr edge-break (too small) or a large cove (not an edge break)
        d = cyl.Axis().Direction()
        comp = (abs(d.X()), abs(d.Y()), abs(d.Z()))
        if max(comp) <= AXIS_ALIGNED_COS:
            continue  # a compound corner round (axis not a principal direction) — out of scope
        edge_i = max(range(3), key=lambda i: comp[i])  # the edge the fillet runs along
        oi = [j for j in (0, 1, 2) if j != edge_i]  # the two in-plane axes it bridges
        fb = f.bounding_box()
        span = {0: (fb.min.X, fb.max.X), 1: (fb.min.Y, fb.max.Y), 2: (fb.min.Z, fb.max.Z)}
        fc = {i: 0.5 * (span[i][0] + span[i][1]) for i in (0, 1, 2)}  # face centre

        # Must bridge two axis-aligned faces on distinct in-plane axes (rounds a 90° edge).
        # Each neighbour plane's coordinate lets the convex test rebuild the virtual corner.
        neigh_coord = nearest_axis_aligned_planes(
            f,
            edge_faces,
            fc,
            exclude_axis=edge_i,
            refuse_equidistant=True,
            face_edges=face_edges,
        )
        if oi[0] not in neigh_coord or oi[1] not in neigh_coord:
            continue

        # The convex-edge test, shared with the two bevel families rather than restated: the
        # virtual sharp corner this round replaces sits where the two neighbour planes cross,
        # and a nudge toward the blend lands in the removed *vacuum* for a real fillet but in
        # filled *material* for an internal round bevelling a re-entrant corner. This was a
        # line-for-line copy of `convex_bevel` down to the probe fraction and the classifier
        # tolerance, and the copies could have drifted with nothing to notice.
        if not convex_bevel(part, fc, edge_i, neigh_coord):
            continue  # concave corner — an internal round / slot-wall blend, not an edge fillet

        # Anchor the leader on the curved radius surface itself via the shared
        # fillet_anchor: one shared implementation for automatic and explicit-face reads.
        # The adaptor's bounds are the FACE's trimmed range (already gated below a full
        # turn above), so a periodic seam is handled by using those bounds directly rather
        # than the raw 0..2π surface range. On-face for the ordinary quarter-round edge
        # blend (a UV-rectangular patch); a fillet whose UV region is punched by an
        # interior hole through its centre could still land mid-UV in that void — rare,
        # and no worse than the bbox centre it replaces.
        p = fillet_anchor(s)
        record = Fillet(
            axis="xyz"[edge_i],
            radius=round(radius, 3),
            at=(round(p[0], 3), round(p[1], 3), round(p[2], 3)),
        )
        out.append(_FilletProposal(record, f, ()))

    # A lathe sweeps an ordinary edge fillet around its axis, producing a torus rather than a
    # partial cylinder. The minor radius is the callout radius. An adjacent external cylinder
    # supplies the material-side proof: internal toroidal bore blends have the same analytic
    # surface but meet an internal cylinder instead.
    tori = [f for f in all_faces if BRepAdaptor_Surface(f.wrapped).GetType() == GeomAbs_Torus]
    if tori:
        z_cyls, cross_cyls = cyls if cyls is not None else analyse_cylinders(part)
        external_cylinders = {c["face"]: c for c in (*z_cyls, *cross_cyls) if c["external"]}
        for f in tori:
            s = BRepAdaptor_Surface(f.wrapped)
            torus = s.Torus()
            radius = torus.MinorRadius()
            if radius < min_radius or radius > max_radius_frac * max_ext:
                continue
            if abs(s.LastVParameter() - s.FirstVParameter()) > _TURNED_FILLET_MAX_EXTENT:
                continue  # a half/full toroidal profile, not an edge round
            torus_axis = torus.Axis()
            d = torus_axis.Direction()
            direction = (d.X(), d.Y(), d.Z())
            comp = (abs(d.X()), abs(d.Y()), abs(d.Z()))
            if max(comp) <= AXIS_ALIGNED_COS:
                continue  # a compound corner round (axis not a principal direction)
            edge_i = max(range(3), key=lambda i: comp[i])
            location = torus_axis.Location()
            axis_point = (location.X(), location.Y(), location.Z())
            torus_neighbours = neighbours(f, edge_faces, face_edges=face_edges)
            coaxial_cylinders = [
                external_cylinders[n]
                for n in torus_neighbours
                if n in external_cylinders
                and _coaxial_axis_lines(
                    axis_point,
                    direction,
                    external_cylinders[n]["axis_xyz"],
                    external_cylinders[n]["dir_xyz"],
                    tol=length_tol(external_cylinders[n]["diameter"], rel=_COAXIAL_FRAC),
                )
            ]
            if not coaxial_cylinders:
                continue
            branch_context: list[FaceLike] = []
            transverse_planes: list[FaceLike] = []
            round_continuations: list[FaceLike] = []
            for neighbour in torus_neighbours:
                neighbour_surface = BRepAdaptor_Surface(neighbour.wrapped)
                if neighbour_surface.GetType() == GeomAbs_Sphere:
                    # A compound turned fillet can continue into its spherical corner cap.
                    # This is distinct from a rolled bead, whose two quarter-torus patches
                    # continue into one another without reaching a second stock face.
                    round_continuations.append(neighbour)
                    continue
                if neighbour_surface.GetType() != GeomAbs_Plane:
                    continue
                nd = neighbour_surface.Plane().Axis().Direction()
                normal_dot = nd.X() * direction[0] + nd.Y() * direction[1] + nd.Z() * direction[2]
                if abs(normal_dot) > AXIS_ALIGNED_COS:
                    transverse_planes.append(neighbour)
            bridges_two_bands = len({c["diameter"] for c in coaxial_cylinders}) >= 2
            if not transverse_planes and not bridges_two_bands and not round_continuations:
                continue  # a toroidal bead meeting one band, not a rounded edge
            branch_context.extend(c["face"] for c in coaxial_cylinders)
            branch_context.extend(transverse_planes)
            branch_context.extend(round_continuations)
            p = fillet_anchor(s)
            record = Fillet(
                axis="xyz"[edge_i],
                radius=round(radius, 3),
                at=(round(p[0], 3), round(p[1], 3), round(p[2], 3)),
                turned=True,
            )
            out.append(_FilletProposal(record, f, tuple(dict.fromkeys(branch_context))))
    out.sort(key=lambda proposal: (proposal.record.axis, proposal.record.at))
    if writer is not None:
        pending = tuple(
            (
                proposal.record,
                writer.graph.require_node(proposal.face),
                tuple(writer.graph.require_node(face) for face in proposal.turned_context),
            )
            for proposal in out
        )
        if any(
            writer.graph.common_valid_solid((node, *consulted)) is None
            for _record, node, consulted in pending
        ):
            raise ValueError("fillet defining face has no unambiguous valid solid")
        for record, node, _consulted in pending:
            writer.sink.propose(FamilyId.FILLETS, record, defining=(node,))
    return [proposal.record for proposal in out]
