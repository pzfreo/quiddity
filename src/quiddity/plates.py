# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Thin-slab (plate/wall) recognition for multi-plate prismatic parts.

``recognise_plates`` returns the plate/wall thicknesses of a prismatic part — the thin
extent of each slab that makes up an L-/T-/U-bracket and kin. It is the
complement of the other prismatic recognisers: ``recognise_face_levels`` (levels.py)
finds a monotonic Z staircase and ``EnvelopeFeature`` gives the overall bbox, but
neither recovers a *plate thickness* that is (a) along X or Y, or (b) along Z yet
too thin to survive the step-ladder legibility gate. A single flat plate needs no
help — its thickness IS the envelope, already dimensioned by ``dim_height``.

A plate along axis *a* is a slab of solid material between two large parallel
planar faces perpendicular to *a*: an **outward-−a** face at the low coord and an
**outward-+a** face at the high coord (solid lies between them). The opposite
arrangement — +a at the low coord, −a at the high — is a *slot / channel* with air
between the faces, and is correctly rejected. Two gates keep it to genuine plates:

- **large area** — each bounding face must cover at least ``min_area_frac`` of the
  part's cross-section on that axis, so a small internal feature face (a
  counterbore floor, a boss end) is never read as a plate; and
- **thin** — the thickness must be under ``max_thick_frac`` of the part's overall
  extent on that axis, so the full-envelope span of a single flat plate (thickness
  == extent) is excluded (``dim_height``/envelope already own it). A slab thicker
  than that fraction of its axis reads as a block, not a plate, and is left to the
  step/envelope dims — the conservative side of the cut.

Only the low−a/high+a *adjacent* pair along an axis is a plate: a pairing that skips
an intervening face crosses an air gap (two stacked plates on a common post) and is
rejected, so a slab thickness never spans a void.

Bottom of the recognition DAG: depends only on build123d/OCP.
"""

from __future__ import annotations

import math
from collections.abc import Sequence
from dataclasses import dataclass, replace

from build123d import Axis, Face
from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.BRepGProp import BRepGProp
from OCP.GeomAbs import GeomAbs_Plane
from OCP.GProp import GProp_GProps

from quiddity._adjacency import FaceNode, SolidRef
from quiddity._body_identity import unambiguous_body_keys
from quiddity._candidates import FamilyId
from quiddity._claims import EvidenceWriter
from quiddity._geometry import (
    AXIS_ALIGNED_COS,
    clears_threshold,
    cluster_coordinates,
)
from quiddity._record import Record
from quiddity._typing import Part

#: **A minimum-evidence threshold, not a tolerance — deliberately absolute (ADR 0008).**
#: Scaling it to the part makes a feature's existence depend on what surrounds it, so a small
#: feature on a large part disappears. Whether such a feature is worth dimensioning is consumer
#: policy, and ADR 0001 puts policy with the consumer; recognition reports it either way.
#: Also the slab-thickness minimum, which is why it cannot follow the part.
_TOL = 0.5

# Imported coplanar faces can return normals that differ only in the final floating-point bits.
# Treating those as distinct directions repeats the same whole-shape rotation without adding a
# geometric candidate. Nine decimal places in degrees is about 1.7e-11 radians: far below the
# package's directional predicates, but above the observed kernel representation noise.
_ORIENTED_ANGLE_DIGITS = 9


@dataclass(frozen=True)
class Plate(Record):
    """A recognised thin slab. ``axis`` is the thin (thickness) axis ("x"/"y"/"z");
    ``lo``/``hi`` are the slab's two bounding coords along it (``hi - lo`` is the
    thickness); ``u``/``v`` are the slab centre on the other two axes (in axis order),
    a representative point the renderer places the thickness dim beside."""

    axis: str
    lo: float
    hi: float
    u: float
    v: float
    # See Channel.body_key. Appended for positional compatibility.
    body_key: tuple[float, ...] | None = ()

    @property
    def thickness(self) -> float:
        return self.hi - self.lo


class _PlateAttributionError(ValueError):
    """Complete Plate provenance cannot be published for this aggregate input."""


@dataclass(frozen=True, slots=True)
class _PlateProposal:
    record: Plate
    low_faces: tuple[Face, ...]
    high_faces: tuple[Face, ...]


@dataclass(frozen=True, slots=True)
class _PlateGroup:
    area: float
    u_sum: float
    v_sum: float
    faces: tuple[Face, ...]


def _oriented_cross_area(
    part: Part,
    faces: Sequence[Face],
    axis_index: int,
    extents: tuple[float, float, float],
) -> float:
    """Smallest body cross-envelope in directions established by its planar faces.

    A world-axis bounding rectangle changes area when the recognition frame rolls around the
    Plate normal.  For a prismatic body, the minimum enclosing rectangle is attained with one
    side parallel to a boundary direction.  Rotate each eligible intrinsic planar normal onto a
    transverse coordinate axis and retain the smallest exact bounding-box cross area. A body with
    no transverse planar direction retains the legacy coordinate envelope for compatibility; that
    case is outside the documented prismatic Plate domain and carries no roll-covariance claim.
    """

    other = [index for index in range(3) if index != axis_index]
    angles: set[float] = set()
    vertices = tuple(tuple(vertex) for vertex in part.vertices())
    support_eps = max(max(extents), 1.0) * 1e-9
    for face in faces:
        try:
            normal = tuple(face.normal_at())
        except Exception:  # noqa: BLE001 -- a degenerate plane establishes no direction
            continue
        if abs(normal[axis_index]) > 1.0 - AXIS_ALIGNED_COS:
            continue
        projected = math.hypot(normal[other[0]], normal[other[1]])
        if projected < AXIS_ALIGNED_COS:
            continue
        location = BRepAdaptor_Surface(face.wrapped).Plane().Location()
        plane = (location.X(), location.Y(), location.Z())
        plane_projection = sum(
            value * direction for value, direction in zip(plane, normal, strict=True)
        )
        if (
            vertices
            and max(
                sum(value * direction for value, direction in zip(vertex, normal, strict=True))
                for vertex in vertices
            )
            > plane_projection + support_eps
        ):
            # A concave/internal planar wall does not establish a body-envelope direction.
            continue
        angle = math.degrees(math.atan2(normal[other[1]], normal[other[0]])) % 90.0
        # Apply modulo again so a value numerically just below 90 canonicalises with 0.
        angles.add(round(angle, _ORIENTED_ANGLE_DIGITS) % 90.0)

    if not angles:
        return extents[other[0]] * extents[other[1]]

    rotation_axis = (Axis.X, Axis.Y, Axis.Z)[axis_index]
    sign = 1.0 if axis_index == 1 else -1.0
    cross_areas: list[float] = []
    for angle in angles:
        size = (
            extents
            if angle == 0.0
            else tuple(
                float(component)
                for component in part.rotate(rotation_axis, sign * angle).bounding_box().size
            )
        )
        cross_areas.append(size[other[0]] * size[other[1]])
    return min(cross_areas)


def has_multi_axis_plates(plates: Sequence[Plate]) -> bool:
    """Whether plate evidence proves a base/wall structure rather than one slab axis."""
    return len({plate.axis for plate in plates}) >= 2


def recognise_plates(
    part: Part,
    *,
    min_area_frac: float = 0.4,
    max_thick_frac: float = 0.5,
    tol: float | None = None,
) -> list[Plate]:
    """Recognise the plate/wall thicknesses of a prismatic *part* (see module docstring).

    Returns one :class:`Plate` per recognised body-local slab, deduplicated by
    (axis, lo, hi) only within one solid. Equal-valued slabs on separate solids retain
    their physical multiplicity. Deterministic: sorted by geometry. Empty for a single
    flat plate (its thickness is the envelope) or a part with no thin slabs.
    """
    return _discover_plates(
        part,
        min_area_frac=min_area_frac,
        max_thick_frac=max_thick_frac,
        tol=tol,
    )


def _plate_proposals(
    part: Part,
    *,
    min_area_frac: float,
    max_thick_frac: float,
    tol: float,
) -> list[_PlateProposal]:
    """Discover one body's Plate proposals without publishing evidence."""

    bb = part.bounding_box()
    extents = (bb.max.X - bb.min.X, bb.max.Y - bb.min.Y, bb.max.Z - bb.min.Z)
    ext = dict(zip("xyz", extents, strict=True))
    axidx = {"x": 0, "y": 1, "z": 2}
    faces = [f for f in part.faces() if BRepAdaptor_Surface(f.wrapped).GetType() == GeomAbs_Plane]

    out: list[_PlateProposal] = []
    for axis, i in axidx.items():
        sides: tuple[list[tuple[float, float, float, float, Face]], ...] = ([], [])
        oi = [j for j in (0, 1, 2) if j != i]
        for face in faces:
            surface = BRepAdaptor_Surface(face.wrapped)
            try:
                normal = face.normal_at()
            except Exception:  # noqa: BLE001 — a degenerate face has no clean normal
                continue
            component = (normal.X, normal.Y, normal.Z)[i]
            if abs(component) < AXIS_ALIGNED_COS:
                continue
            properties = GProp_GProps()
            BRepGProp.SurfaceProperties_s(face.wrapped, properties)
            area = properties.Mass()
            centre = properties.CentreOfMass()
            centre_point = (centre.X(), centre.Y(), centre.Z())
            plane_location = surface.Plane().Location()
            location = (plane_location.X(), plane_location.Y(), plane_location.Z())[i]
            sides[component > 0].append(
                (
                    location,
                    area,
                    centre_point[oi[0]] * area,
                    centre_point[oi[1]] * area,
                    face,
                )
            )

        grouped: list[dict[float, _PlateGroup]] = []
        for side in sides:
            groups: dict[float, _PlateGroup] = {}
            for cluster in cluster_coordinates([entry[0] for entry in side], tol=tol):
                groups[min(side[index][0] for index in cluster)] = _PlateGroup(
                    sum(side[index][1] for index in cluster),
                    sum(side[index][2] for index in cluster),
                    sum(side[index][3] for index in cluster),
                    tuple(side[index][4] for index in cluster),
                )
            grouped.append(groups)
        negative, positive = grouped

        maximum_thickness = max_thick_frac * ext[axis]
        # The area authority cannot make an axis eligible unless it already contains at least one
        # correctly ordered, geometrically thin opposed span. Avoid constructing oriented
        # envelopes for axes that are incapable of publishing a Plate under any area threshold.
        if not any(
            tol < high - low and clears_threshold(maximum_thickness, high - low)
            for low in negative
            for high in positive
        ):
            continue

        cross = _oriented_cross_area(part, faces, i, extents)
        if cross <= 0:
            continue
        threshold = min_area_frac * cross
        events = [
            (coordinate, -1, group)
            for coordinate, group in negative.items()
            if clears_threshold(group.area, threshold)
        ]
        events += [
            (coordinate, 1, group)
            for coordinate, group in positive.items()
            if clears_threshold(group.area, threshold)
        ]
        events.sort(key=lambda event: (event[0], event[1]))
        for (low, low_sign, low_group), (high, high_sign, high_group) in zip(
            events, events[1:], strict=False
        ):
            if low_sign != -1 or high_sign != 1:
                continue
            thickness = high - low
            if thickness <= tol or not clears_threshold(maximum_thickness, thickness):
                continue
            combined_area = low_group.area + high_group.area
            out.append(
                _PlateProposal(
                    Plate(
                        axis=axis,
                        lo=round(low, 3),
                        hi=round(high, 3),
                        u=(low_group.u_sum + high_group.u_sum) / combined_area,
                        v=(low_group.v_sum + high_group.v_sum) / combined_area,
                    ),
                    low_group.faces,
                    high_group.faces,
                )
            )
    return sorted(
        out,
        key=lambda proposal: (
            proposal.record.axis,
            proposal.record.lo,
            proposal.record.hi,
        ),
    )


def _plate_scopes(part: Part) -> list[Part]:
    """Return independent solid scopes, retaining record-only open-shell compatibility."""

    solids = list(part.solids())
    return solids if solids else [part]


def _discover_plates(
    part: Part,
    *,
    min_area_frac: float = 0.4,
    max_thick_frac: float = 0.5,
    tol: float | None = None,
    writer: EvidenceWriter | None = None,
    excluded_solids: frozenset[SolidRef] = frozenset(),
) -> list[Plate]:
    """Discover Plates and optionally issue complete low/high planar groups atomically."""

    tol = _TOL if tol is None else tol
    scopes = _plate_scopes(part)
    body_keys = unambiguous_body_keys(scopes, require_valid_solid=True)
    proposal_groups = [
        [
            replace(proposal, record=replace(proposal.record, body_key=body_key))
            for proposal in _plate_proposals(
                scope,
                min_area_frac=min_area_frac,
                max_thick_frac=max_thick_frac,
                tol=tol,
            )
        ]
        for scope, body_key in zip(scopes, body_keys, strict=True)
    ]
    ordered = sorted(
        (proposal for proposals in proposal_groups for proposal in proposals),
        key=lambda proposal: (
            proposal.record.axis,
            proposal.record.lo,
            proposal.record.hi,
            proposal.record.u,
            proposal.record.v,
        ),
    )
    if writer is None:
        uniq = []
        for proposals in proposal_groups:
            seen: set[tuple[str, float, float]] = set()
            for proposal in proposals:
                key = (proposal.record.axis, proposal.record.lo, proposal.record.hi)
                if key not in seen:
                    seen.add(key)
                    uniq.append(proposal)
        uniq.sort(
            key=lambda proposal: (
                proposal.record.axis,
                proposal.record.lo,
                proposal.record.hi,
                proposal.record.u,
                proposal.record.v,
            )
        )
    else:
        bound: dict[
            tuple[str, float, float, SolidRef],
            dict[tuple[frozenset[FaceNode], frozenset[FaceNode]], _PlateProposal],
        ] = {}
        used: set[FaceNode] = set()
        try:
            for proposal in ordered:
                low = frozenset(writer.graph.require_node(face) for face in proposal.low_faces)
                high = frozenset(writer.graph.require_node(face) for face in proposal.high_faces)
                if not low or not high or low & high:
                    raise _PlateAttributionError("Plate role groups are empty or overlap")
                low_by_solid: dict[SolidRef, set[FaceNode]] = {}
                high_by_solid: dict[SolidRef, set[FaceNode]] = {}
                for role, owner_groups in ((low, low_by_solid), (high, high_by_solid)):
                    for node in role:
                        solid = writer.graph.common_valid_solid((node,))
                        if solid is None:
                            raise _PlateAttributionError(
                                "Plate role face has no unambiguous valid solid"
                            )
                        owner_groups.setdefault(solid, set()).add(node)
                shared_solids = low_by_solid.keys() & high_by_solid.keys()
                if len(shared_solids) != 1:
                    raise _PlateAttributionError(
                        "Plate role groups do not identify one common solid"
                    )
                solid = next(iter(shared_solids))
                if solid in excluded_solids:
                    continue
                low = frozenset(low_by_solid[solid])
                high = frozenset(high_by_solid[solid])
                bound_key = (proposal.record.axis, proposal.record.lo, proposal.record.hi, solid)
                bound.setdefault(bound_key, {}).setdefault((low, high), proposal)
            if any(len(role_pairs) > 1 for role_pairs in bound.values()):
                raise _PlateAttributionError("Plate key has competing defining groups")

            pending: list[tuple[Plate, tuple[FaceNode, ...]]] = []
            uniq = []
            for role_pairs in bound.values():
                (low, high), proposal = next(iter(role_pairs.items()))
                resolved = low | high
                nodes = tuple(node for node in writer.graph.nodes if node in resolved)
                if used & resolved:
                    raise _PlateAttributionError("Plate occurrences reuse defining faces")
                if writer.graph.common_valid_solid(nodes) is None:
                    raise _PlateAttributionError(
                        "Plate defining groups do not prove one valid solid"
                    )
                used.update(resolved)
                pending.append((proposal.record, nodes))
                uniq.append(proposal)
        except _PlateAttributionError:
            raise
        except (KeyError, RuntimeError, ValueError) as exc:
            raise _PlateAttributionError("Plate face binding failed") from exc
        for record, nodes in pending:
            writer.add_defining(record, nodes, family=FamilyId.PLATES)
    return [proposal.record for proposal in uniq]
