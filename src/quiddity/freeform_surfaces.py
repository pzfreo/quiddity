# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Native B-spline support and bounded relationships for source faces.

The support is the untrimmed surface carried by the B-rep. Construction history
is not recovered from a generic B-spline. Face indices use the input part's
original face roster and remain local to one recognition run.
"""

from __future__ import annotations

import math
from dataclasses import dataclass

from build123d import Face
from OCP.BRep import BRep_Tool
from OCP.Geom import Geom_BSplineSurface

from quiddity._adjacency import FaceGraph, is_any_smooth
from quiddity._candidates import CompletedInputs, FamilyId
from quiddity._definitions import (
    DiscoveryServices,
    FullyAttributed,
    ManifestEvidence,
    NotCounted,
    PhysicalDefinition,
    always,
)
from quiddity._geometry import length_tol
from quiddity._record import Record
from quiddity._typing import Part
from quiddity.thin_walls import ThinWallBody, _discover_thin_wall_bodies


@dataclass(frozen=True, slots=True)
class BSplineSurfaceSupport(Record):
    """Full native tensor-product support, before the face's trimming wires."""

    u_degree: int
    v_degree: int
    poles: tuple[tuple[tuple[float, float, float], ...], ...]
    weights: tuple[tuple[float, ...], ...]
    u_knots: tuple[float, ...]
    v_knots: tuple[float, ...]
    u_multiplicities: tuple[int, ...]
    v_multiplicities: tuple[int, ...]
    u_periodic: bool
    v_periodic: bool


@dataclass(frozen=True, slots=True)
class SurfaceContinuityLink(Record):
    """A direct shared-edge relation to another native freeform face."""

    other_face: int
    kind: str  # same_support or G1; G2 is reserved until independently proved.


@dataclass(frozen=True, slots=True)
class FreeformSurface(Record):
    """One native B-spline face and only the relations its geometry establishes."""

    face: int
    support_kind: str
    support: BSplineSurfaceSupport
    construction_kind: str | None
    construction_axis: str | None
    construction_vector: tuple[float, float, float] | None
    continuity_group: tuple[int, ...]
    continuity_links: tuple[SurfaceContinuityLink, ...]
    offset_partner: int | None
    offset_distance: float | None
    offset_basis: str | None


def _support(surface: Geom_BSplineSurface) -> BSplineSurfaceSupport:
    return BSplineSurfaceSupport(
        u_degree=surface.UDegree(),
        v_degree=surface.VDegree(),
        poles=tuple(
            tuple(
                (
                    surface.Pole(u, v).X(),
                    surface.Pole(u, v).Y(),
                    surface.Pole(u, v).Z(),
                )
                for v in range(1, surface.NbVPoles() + 1)
            )
            for u in range(1, surface.NbUPoles() + 1)
        ),
        weights=tuple(
            tuple(surface.Weight(u, v) for v in range(1, surface.NbVPoles() + 1))
            for u in range(1, surface.NbUPoles() + 1)
        ),
        u_knots=tuple(surface.UKnot(i) for i in range(1, surface.NbUKnots() + 1)),
        v_knots=tuple(surface.VKnot(i) for i in range(1, surface.NbVKnots() + 1)),
        u_multiplicities=tuple(surface.UMultiplicity(i) for i in range(1, surface.NbUKnots() + 1)),
        v_multiplicities=tuple(surface.VMultiplicity(i) for i in range(1, surface.NbVKnots() + 1)),
        u_periodic=surface.IsUPeriodic(),
        v_periodic=surface.IsVPeriodic(),
    )


def _construction(
    support: BSplineSurfaceSupport,
) -> tuple[str | None, str | None, tuple[float, float, float] | None]:
    """Prove only a two-section ruled sweep or its constant-vector extrusion."""

    for axis in ("u", "v"):
        if axis == "u":
            if support.u_periodic or support.u_degree != 1 or len(support.poles) != 2:
                continue
            first, last = support.poles
            first_weights, last_weights = support.weights
        else:
            if support.v_periodic or support.v_degree != 1 or len(support.poles[0]) != 2:
                continue
            first = tuple(row[0] for row in support.poles)
            last = tuple(row[1] for row in support.poles)
            first_weights = tuple(row[0] for row in support.weights)
            last_weights = tuple(row[1] for row in support.weights)
        vector = (
            last[0][0] - first[0][0],
            last[0][1] - first[0][1],
            last[0][2] - first[0][2],
        )
        tolerance = length_tol(math.dist(first[0], last[0]), rel=1e-9)
        if (
            math.dist((0.0, 0.0, 0.0), vector) > tolerance
            and all(
                math.dist(vector, tuple(end[i] - start[i] for i in range(3))) <= tolerance
                for start, end in zip(first, last, strict=True)
            )
            and first_weights == last_weights
        ):
            return "linear_extrusion", axis, vector
        return "ruled", axis, None
    return None, None, None


def _records(
    faces: tuple[Face, ...], graph: FaceGraph, walls: tuple[ThinWallBody, ...]
) -> list[FreeformSurface]:
    surfaces = {
        index: surface
        for index, face in enumerate(faces)
        if isinstance(surface := BRep_Tool.Surface_s(face.wrapped), Geom_BSplineSurface)
        and graph.common_valid_solid((graph.require_node(face),)) is not None
    }
    indices = {face: index for index, face in enumerate(faces)}
    links: dict[int, list[SurfaceContinuityLink]] = {index: [] for index in surfaces}
    for index in surfaces:
        node = graph.require_node(faces[index])
        for neighbour in graph.neighbours(node):
            other = indices[graph.face(neighbour)]
            if other not in surfaces or other <= index:
                continue
            kind = (
                "same_support"
                if surfaces[index] == surfaces[other]
                else "G1"
                if is_any_smooth(graph.arc(node, neighbour))
                else None
            )
            if kind is not None:
                links[index].append(SurfaceContinuityLink(other, kind))
                links[other].append(SurfaceContinuityLink(index, kind))

    remaining = set(surfaces)
    groups: dict[int, tuple[int, ...]] = {}
    while remaining:
        component = {seed := min(remaining)}
        remaining.remove(seed)
        pending = [seed]
        while pending:
            for link in links[pending.pop()]:
                if link.other_face in remaining:
                    remaining.remove(link.other_face)
                    component.add(link.other_face)
                    pending.append(link.other_face)
        group = tuple(sorted(component))
        groups.update({index: group for index in component})

    offsets: dict[int, tuple[int, float]] = {}
    for wall in walls:
        for pair in wall.face_pairs:
            if pair.first_face in surfaces and pair.second_face in surfaces and pair.thickness:
                offsets[pair.first_face] = (pair.second_face, pair.thickness)
                offsets[pair.second_face] = (pair.first_face, pair.thickness)

    result = []
    for index, surface in sorted(surfaces.items()):
        support = _support(surface)
        construction_kind, construction_axis, construction_vector = _construction(support)
        result.append(
            FreeformSurface(
                face=index,
                support_kind="bspline",
                support=support,
                construction_kind=construction_kind,
                construction_axis=construction_axis,
                construction_vector=construction_vector,
                continuity_group=groups[index],
                continuity_links=tuple(sorted(links[index], key=lambda link: link.other_face)),
                offset_partner=offsets[index][0] if index in offsets else None,
                offset_distance=offsets[index][1] if index in offsets else None,
                offset_basis="reciprocal_material_rays" if index in offsets else None,
            )
        )
    return result


def recognise_freeform_surfaces(part: Part) -> list[FreeformSurface]:
    """Read native B-spline supports on faces of valid source solids."""

    faces = tuple(part.faces())
    if not any(
        isinstance(BRep_Tool.Surface_s(face.wrapped), Geom_BSplineSurface) for face in faces
    ):
        return []
    graph = FaceGraph(part)
    return _records(faces, graph, tuple(_discover_thin_wall_bodies(part, graph=graph)))


def _discover(services: DiscoveryServices, inputs: CompletedInputs) -> list[object]:
    graph = services.context.graph
    faces = tuple(services.context.part.faces())
    records = _records(faces, graph, inputs.records(FamilyId.THIN_WALL_BODIES, ThinWallBody))
    retained: list[object] = []
    for record in records:
        node = graph.require_node(faces[record.face])
        services.writer.add_defining(record, (node,), family=FamilyId.FREEFORM_SURFACES)
        retained.append(record)
    return retained


DEFINITION = PhysicalDefinition(
    family=FamilyId.FREEFORM_SURFACES,
    record_types=(FreeformSurface,),
    result_field="freeform_surfaces",
    public_entrypoint=recognise_freeform_surfaces.__name__,
    dependencies=(FamilyId.THIN_WALL_BODIES,),
    applicable=always,
    discover=_discover,
    census=NotCounted("source support geometry, not a manufactured feature count"),
    attribution=FullyAttributed("each support record claims its original source face"),
    evidence=ManifestEvidence(
        tests=("tests/test_freeform_surfaces.py",),
        golden_paths=("tests/freeform_surfaces_expected.json",),
        introduced="0.3.6",
        extra_records=(
            ("BSplineSurfaceSupport", "nested", ()),
            ("SurfaceContinuityLink", "nested", ()),
        ),
    ),
)
