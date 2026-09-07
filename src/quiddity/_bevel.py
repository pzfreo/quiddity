# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""The single-face bevel read and the convex-corner probe, shared by three recognisers.

`recognise_chamfers`, `recognise_angled_steps` and `recognise_fillets` all begin by asking the
same two questions of a face: **is this an oblique planar bevel, and does it break a convex
corner?** Those two answers lived in `chamfers` and the other two families imported them from
it, which made a recogniser a substrate for its siblings -- something the module seam map does
not police, because it covers only the private modules.

Epic 0002 item 2 lifted the convexity probe out of `fillets`, where it had been a line-for-line
copy. That made three consumers of machinery living in a recogniser, and three is where "one
module happens to import another" stops being a convenience and starts being an undeclared
layer. Naming it is what lets the seam map have an opinion.

`BevelReject` and `classify_bevel` remain public through
:mod:`quiddity`; the package's export surface is unchanged by the move.
"""

from __future__ import annotations

from typing import Literal, TypeAlias

from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.BRepClass3d import BRepClass3d_SolidClassifier
from OCP.GeomAbs import GeomAbs_Plane
from OCP.gp import gp_Pnt
from OCP.TopAbs import TopAbs_IN

from quiddity._geometry import AXIS_ALIGNED_COS, INTERIOR_PROBE_FRAC
from quiddity._solid_properties import SolidProperties, SolidPropertyOwner, solid_properties
from quiddity._typing import FaceLike, Part, Vector3

#: The in-plane component below which a normal counts as running along that axis, so the face
#: is a single-axis bevel rather than a compound corner. Dimensionless (ADR 0008).
_RUN_AXIS_COS = 0.05

_BevelRejectReason: TypeAlias = Literal["nonplanar", "degenerate", "aligned", "compound"]


class BevelReject(ValueError):
    """*face* is not a single-axis oblique planar bevel; ``reason`` says why:
    ``"nonplanar"``, ``"degenerate"`` (no clean normal), ``"aligned"`` (axis-aligned or a
    shallow draft angle — a real face, not a chamfer), or ``"compound"`` (oblique on all
    three axes — a corner bevel, out of scope)."""

    reason: _BevelRejectReason

    def __init__(self, reason: _BevelRejectReason) -> None:
        self.reason = reason
        super().__init__(reason)


def classify_bevel(
    face: FaceLike,
) -> tuple[int, Vector3, dict[int, tuple[float, float]], float, float]:
    """The single-face oblique-bevel read, and the one home for the normal-to-axis
    classification thresholds and the leg geometry.

    Shared by ``recognise_chamfers``, ``recognise_angled_steps`` and the explicit-face reader.
    Its docstring named only the first of those until this module existed, which is how a helper
    three families depend on can look like one family's private business. Returns
    ``(edge_i, nv, span, leg_hi, leg_lo)``: the along-edge axis index, the unit normal,
    per-axis ``(lo, hi)`` bbox spans, and the two in-plane leg lengths (unrounded,
    ``leg_hi >= leg_lo``). Raises :class:`BevelReject` when the face is not one."""
    if BRepAdaptor_Surface(face.wrapped).GetType() != GeomAbs_Plane:
        raise BevelReject("nonplanar")
    try:
        nvec = face.normal_at()
    except Exception:  # noqa: BLE001 — a degenerate face has no clean normal
        raise BevelReject("degenerate") from None
    nv = (nvec.X, nvec.Y, nvec.Z)
    if max(abs(c) for c in nv) > AXIS_ALIGNED_COS:
        raise BevelReject("aligned")
    edge_i = next((i for i in (0, 1, 2) if abs(nv[i]) < _RUN_AXIS_COS), None)
    if edge_i is None:
        raise BevelReject("compound")
    oi = [j for j in (0, 1, 2) if j != edge_i]
    fb = face.bounding_box()
    span = {0: (fb.min.X, fb.max.X), 1: (fb.min.Y, fb.max.Y), 2: (fb.min.Z, fb.max.Z)}
    leg_u = span[oi[0]][1] - span[oi[0]][0]
    leg_v = span[oi[1]][1] - span[oi[1]][0]
    return edge_i, nv, span, max(leg_u, leg_v), min(leg_u, leg_v)


def convex_bevel(
    part: Part,
    centre: dict[int, float],
    edge_i: int,
    neigh_coord: dict[int, float],
    *,
    properties: SolidProperties | SolidPropertyOwner | None = None,
) -> bool:
    """Does the virtual sharp corner the bevel replaces lie outside the solid?

    *centre* is the bevel face's own centre per axis and *neigh_coord* the coordinates of
    the two neighbour planes it bridges, as :func:`nearest_axis_aligned_planes` returns them.

    The virtual corner sits where those two planes cross, at the bevel's own edge position.
    Nudged a little toward the bevel face it lands in the removed-wedge *vacuum* for a real
    (convex) bevel, but in filled *material* for a gusset/rib/web bevelling a concave
    re-entrant corner. The nudge clears the on-boundary knife-edge at the raw corner; this
    is the discriminator adjacency alone cannot make, since a gusset's hypotenuse is also
    edge-adjacent to two perpendicular walls.

    Shared with :func:`quiddity.recognise_angled_steps`, which admits larger slants
    than a chamfer and so needs this call for exactly the same reason: a gusset satisfies
    every other gate either recogniser applies.

    *properties* is the run's whole-solid cache, which is where the point classifier this
    probe needs lives -- see :func:`_material_at`. Omitting it answers the same question with
    a classifier of this call's own.
    """

    return not _material_at(
        part, _near_corner(centre, edge_i, neigh_coord, toward=1.0), properties=properties
    )


def material_beyond_corner(
    part: Part,
    centre: dict[int, float],
    edge_i: int,
    neigh_coord: dict[int, float],
    *,
    properties: SolidProperties | SolidPropertyOwner | None = None,
) -> bool:
    """Is there solid on the *far* side of the virtual sharp corner, away from the bevel?

    :func:`convex_bevel` asks what is between the corner and the bevel; this asks what is
    behind it, and the pair together say what kind of corner it is. A bevel on an **edge of
    the part** replaces a corner of the stock, so beyond it is free space in both directions.
    A bevel on a **wall of a recess** replaces the corner where two walls of that recess meet,
    and beyond that corner is the material the recess was cut out of.

    Both cases have vacuum between corner and bevel, which is why convexity alone passes them
    both. Found by the held-out corpus: a triangular pocket whose two other walls happen to be
    axis-aligned satisfies every gate ``recognise_angled_steps`` applies, and was reported as
    a step. The design corpus contained no such pocket.

    *properties* is the run's whole-solid cache, as in :func:`convex_bevel`.
    """

    return _material_at(
        part, _near_corner(centre, edge_i, neigh_coord, toward=-1.0), properties=properties
    )


def _near_corner(
    centre: dict[int, float], edge_i: int, neigh_coord: dict[int, float], *, toward: float
) -> Vector3:
    """A point just off the virtual sharp corner, on the bevel's side (*toward* 1) or the far
    side (-1). The offset clears the on-boundary knife-edge at the raw corner itself."""

    oi = [j for j in (0, 1, 2) if j != edge_i]
    corner = [0.0, 0.0, 0.0]
    corner[edge_i] = centre[edge_i]
    corner[oi[0]] = neigh_coord[oi[0]]
    corner[oi[1]] = neigh_coord[oi[1]]
    step = toward * INTERIOR_PROBE_FRAC
    return (
        corner[0] + step * (centre[0] - corner[0]),
        corner[1] + step * (centre[1] - corner[1]),
        corner[2] + step * (centre[2] - corner[2]),
    )


#: The name the run's whole-solid cache files this module's point classifier under.
_CLASSIFIER = "_bevel.solid_classifier"


def _classifier(shape: Part) -> BRepClass3d_SolidClassifier:
    """Load one shape into a point classifier -- the expensive half of a material probe."""

    return BRepClass3d_SolidClassifier(shape.wrapped)


def _material_at(
    part: Part,
    point: Vector3,
    *,
    properties: SolidProperties | SolidPropertyOwner | None = None,
) -> bool:
    """Is *point* inside the material of *part*?

    ``BRepClass3d_SolidClassifier`` is built for repeated queries: constructing it loads and
    indexes the shape, and ``Perform`` then classifies one point against that. This asked for
    a fresh one per point, so every probe paid the loading and none of the reuse -- 54 probes
    of the 664-face NIST part were 0.6 s, essentially all of it construction. One classifier
    per shape *asked about* is what the run's cache holds -- in production that is the whole
    part every caller passes, not one of its bodies -- and ``Perform`` answers exactly what a
    freshly built classifier answers, at the same tolerance.

    **The orientation half of the cache key is what makes that reuse safe**, and this is the
    strongest reason :mod:`quiddity._solid_properties` keys on it. ``IsSame`` -- and so the
    wrapper half of the key on its own -- ignores orientation, but a classifier does not: a
    reversed solid *inverts* ``IN`` and ``OUT`` rather than flipping a sign the way ``volume``
    does, so a shape and ``Solid(shape.wrapped.Reversed())`` would otherwise share one entry
    and one of them would get the other's answer back. Nothing reverses a solid today; the key
    is what keeps "exactly what a freshly built classifier answers" true anyway.
    """

    clsf = solid_properties(properties).derived(_CLASSIFIER, part, _classifier)
    clsf.Perform(gp_Pnt(*point), 1e-6)
    # `State()` is untyped in OCP, so the comparison is Any until it is narrowed here.
    return bool(clsf.State() == TopAbs_IN)
