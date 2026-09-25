# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Developable, two-sided sheet bodies with flange and bend evidence.

The neutral-axis allowance uses an explicit k-factor. The material and its
forming history are absent from a STEP solid, so neither is inferred here.
Face indices refer to the supplied part's face roster for this one run.
"""

from __future__ import annotations

import math
from dataclasses import dataclass

from build123d import Face
from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.GeomAbs import GeomAbs_Cylinder, GeomAbs_Plane

from quiddity._adjacency import FaceGraph
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
from quiddity.thin_walls import ThinWallBody, WallFacePair, _discover_thin_wall_bodies

_MIN_SKIN_COVERAGE = 0.98
_MIN_CUT_BRIDGE_AREA = 0.8
_DEFAULT_K_FACTOR = 0.5


@dataclass(frozen=True, slots=True)
class SheetFlange(Record):
    """One planar region on the reference side and its opposite skin patches."""

    index: int
    reference_face: int
    mate_faces: tuple[int, ...]
    origin: tuple[float, float, float]
    normal: tuple[float, float, float]
    area: float


@dataclass(frozen=True, slots=True)
class SheetBend(Record):
    """A cylindrical bend pair between two flanges."""

    index: int
    face_pair: WallFacePair
    first_flange: int
    second_flange: int
    axis_origin: tuple[float, float, float]
    axis_direction: tuple[float, float, float]
    angle_degrees: float
    inner_radius: float
    fold_direction: str
    neutral_radius: float
    bend_allowance: float


@dataclass(frozen=True, slots=True)
class FlatPatternPlan(Record):
    """A rooted bend traversal sufficient to flatten the source B-rep flanges.

    Flange outlines and cutouts remain in the referenced source faces. ``tree_bends``
    visits each other flange once; ``cycle_bends`` are additional constraints that
    require a cut or consistency check before publishing a manufacturing blank.
    This plan does not claim an overlap-free flat outline.
    """

    k_factor: float
    base_flange: int
    tree_bends: tuple[int, ...]
    cycle_bends: tuple[int, ...]


@dataclass(frozen=True, slots=True)
class SheetMetalBody(Record):
    """A dominant developable sheet with explicit flange and bend geometry."""

    body_index: int
    body_key: tuple[float, ...] | None
    thickness: float
    first_side_faces: tuple[int, ...]
    second_side_faces: tuple[int, ...]
    cut_edge_faces: tuple[int, ...]
    unresolved_faces: tuple[int, ...]
    flanges: tuple[SheetFlange, ...]
    bends: tuple[SheetBend, ...]
    flat_pattern: FlatPatternPlan
    paired_area_fraction: float


def _surface(face: Face) -> BRepAdaptor_Surface:
    return BRepAdaptor_Surface(face.wrapped)


def _coordinates(point: object) -> tuple[float, float, float]:
    return (float(point.X()), float(point.Y()), float(point.Z()))  # type: ignore[attr-defined]


def _sides(
    wall: ThinWallBody, graph: FaceGraph, faces: tuple[Face, ...]
) -> tuple[set[int], set[int]] | None:
    plane_pairs = [
        pair
        for pair in wall.face_pairs
        if _surface(faces[pair.first_face]).GetType() == GeomAbs_Plane
    ]
    if len(plane_pairs) < 2:
        return None
    seed = max(
        plane_pairs,
        key=lambda pair: faces[pair.first_face].area + faces[pair.second_face].area,
    )
    indices = {face: index for index, face in enumerate(faces)}
    first = {
        indices[graph.face(node)]
        for node in graph.smooth_region(graph.require_node(faces[seed.first_face]))
    }
    second = {
        indices[graph.face(node)]
        for node in graph.smooth_region(graph.require_node(faces[seed.second_face]))
    }
    if first & second:
        return None
    paired = {index for pair in wall.face_pairs for index in (pair.first_face, pair.second_face)}
    coverage = math.fsum(faces[index].area for index in (first | second) & paired) / math.fsum(
        faces[index].area for index in paired
    )
    return (first, second) if coverage >= _MIN_SKIN_COVERAGE else None


def _cut_faces(
    wall: ThinWallBody,
    graph: FaceGraph,
    faces: tuple[Face, ...],
    first: set[int],
    second: set[int],
) -> tuple[tuple[int, ...], tuple[int, ...]] | None:
    body_faces = {
        index for pair in wall.face_pairs for index in (pair.first_face, pair.second_face)
    } | set(wall.unpaired_faces)
    remaining = body_faces - first - second
    indices = {face: index for index, face in enumerate(faces)}
    cut = []
    for index in sorted(remaining):
        neighbours = {
            indices[graph.face(node)] for node in graph.neighbours(graph.require_node(faces[index]))
        }
        if neighbours & first and neighbours & second:
            cut.append(index)
    total = math.fsum(faces[index].area for index in remaining)
    bridged = math.fsum(faces[index].area for index in cut)
    if total <= 0 or bridged / total < _MIN_CUT_BRIDGE_AREA:
        return None
    return tuple(cut), tuple(sorted(remaining - set(cut)))


def _flanges(
    wall: ThinWallBody, faces: tuple[Face, ...], first: set[int], second: set[int]
) -> tuple[tuple[SheetFlange, ...], dict[int, int]] | None:
    mates: dict[int, set[int]] = {}
    for pair in wall.face_pairs:
        if _surface(faces[pair.first_face]).GetType() != GeomAbs_Plane:
            continue
        a, b = pair.first_face, pair.second_face
        if a in first and b not in first:
            reference, mate = a, b
        elif (b in first and a not in first) or (a in second and b not in second):
            reference, mate = b, a
        elif b in second and a not in second:
            reference, mate = a, b
        else:
            return None
        mates.setdefault(reference, set()).add(mate)
    flanges = []
    face_to_flange = {}
    for index, reference in enumerate(sorted(mates)):
        surface = _surface(faces[reference]).Plane()
        normal = faces[reference].normal_at()
        flanges.append(
            SheetFlange(
                index=index,
                reference_face=reference,
                mate_faces=tuple(sorted(mates[reference])),
                origin=_coordinates(surface.Location()),
                normal=(normal.X, normal.Y, normal.Z),
                area=faces[reference].area,
            )
        )
        face_to_flange[reference] = index
    return tuple(flanges), face_to_flange


def _bends(
    wall: ThinWallBody,
    graph: FaceGraph,
    faces: tuple[Face, ...],
    first: set[int],
    face_to_flange: dict[int, int],
    k_factor: float,
) -> tuple[SheetBend, ...] | None:
    indices = {face: index for index, face in enumerate(faces)}
    result: list[SheetBend] = []
    for pair in wall.face_pairs:
        surface_a = _surface(faces[pair.first_face])
        if surface_a.GetType() != GeomAbs_Cylinder:
            continue
        surface_b = _surface(faces[pair.second_face])
        if surface_b.GetType() != GeomAbs_Cylinder:
            return None
        radius_a = float(surface_a.Cylinder().Radius())
        radius_b = float(surface_b.Cylinder().Radius())
        if abs(abs(radius_a - radius_b) - wall.thickness) > length_tol(wall.thickness, rel=0.001):
            return None
        reference = pair.first_face if pair.first_face in first else pair.second_face
        if reference not in first:
            return None
        reference_surface = surface_a if reference == pair.first_face else surface_b
        # Relief cuts can trim the opposite skin to a shorter angular span.
        # The continuous reference-side patch retains the bend's full sweep.
        angle = abs(reference_surface.LastUParameter() - reference_surface.FirstUParameter())
        if not 0 < angle < 2 * math.pi - 1e-3:
            return None
        neighbours = {
            indices[graph.face(node)]
            for node in graph.neighbours(graph.require_node(faces[reference]))
        }
        adjacent = sorted({face_to_flange[i] for i in neighbours if i in face_to_flange})
        if len(adjacent) != 2:
            return None
        cylinder = surface_a.Cylinder()
        axis = cylinder.Axis()
        neutral_radius = min(radius_a, radius_b) + k_factor * wall.thickness
        result.append(
            SheetBend(
                index=len(result),
                face_pair=pair,
                first_flange=adjacent[0],
                second_flange=adjacent[1],
                axis_origin=_coordinates(axis.Location()),
                axis_direction=_coordinates(axis.Direction()),
                angle_degrees=math.degrees(angle),
                inner_radius=min(radius_a, radius_b),
                fold_direction=(
                    "up"
                    if (radius_a if reference == pair.first_face else radius_b)
                    == min(radius_a, radius_b)
                    else "down"
                ),
                neutral_radius=neutral_radius,
                bend_allowance=neutral_radius * angle,
            )
        )
    return tuple(result) if result else None


def _flat_pattern_plan(
    flanges: tuple[SheetFlange, ...], bends: tuple[SheetBend, ...], k_factor: float
) -> FlatPatternPlan | None:
    if not flanges:
        return None
    base = max(flanges, key=lambda flange: flange.area).index
    visited = {base}
    tree = []
    pending = [base]
    while pending:
        current = pending.pop(0)
        for bend in bends:
            other = (
                bend.second_flange
                if bend.first_flange == current
                else bend.first_flange
                if bend.second_flange == current
                else None
            )
            if other is not None and other not in visited:
                visited.add(other)
                tree.append(bend.index)
                pending.append(other)
    if len(visited) != len(flanges):
        return None
    return FlatPatternPlan(
        k_factor=k_factor,
        base_flange=base,
        tree_bends=tuple(tree),
        cycle_bends=tuple(bend.index for bend in bends if bend.index not in tree),
    )


def _body(
    wall: ThinWallBody,
    graph: FaceGraph,
    faces: tuple[Face, ...],
    k_factor: float,
) -> SheetMetalBody | None:
    skin_types = {
        _surface(faces[index]).GetType()
        for pair in wall.face_pairs
        for index in (pair.first_face, pair.second_face)
    }
    if not skin_types <= {GeomAbs_Plane, GeomAbs_Cylinder} or GeomAbs_Cylinder not in skin_types:
        return None
    if any(
        _surface(faces[index]).GetType() not in {GeomAbs_Plane, GeomAbs_Cylinder}
        for index in wall.unpaired_faces
    ):
        return None
    sides = _sides(wall, graph, faces)
    if sides is None:
        return None
    first, second = sides
    cut = _cut_faces(wall, graph, faces, first, second)
    if cut is None:
        return None
    cut_faces, unresolved = cut
    flange_result = _flanges(wall, faces, first, second)
    if flange_result is None:
        return None
    flanges, face_to_flange = flange_result
    bends = _bends(wall, graph, faces, first, face_to_flange, k_factor)
    if bends is None:
        return None
    plan = _flat_pattern_plan(flanges, bends, k_factor)
    if plan is None:
        return None
    return SheetMetalBody(
        body_index=wall.body_index,
        body_key=wall.body_key,
        thickness=wall.thickness,
        first_side_faces=tuple(sorted(first)),
        second_side_faces=tuple(sorted(second)),
        cut_edge_faces=cut_faces,
        unresolved_faces=unresolved,
        flanges=flanges,
        bends=bends,
        flat_pattern=plan,
        paired_area_fraction=wall.paired_area_fraction,
    )


def recognise_sheet_metal_bodies(
    part: Part, *, k_factor: float = _DEFAULT_K_FACTOR
) -> list[SheetMetalBody]:
    """Recognise developable folded sheets; use *k_factor* for bend allowances."""

    if not math.isfinite(k_factor) or not 0 <= k_factor <= 1:
        raise ValueError("k_factor must be finite and between zero and one")
    graph = FaceGraph(part)
    faces = tuple(part.faces())
    return _build_sheet_metal_records(
        _discover_thin_wall_bodies(part, graph=graph), graph, faces, k_factor
    )


def _build_sheet_metal_records(
    walls: list[ThinWallBody] | tuple[ThinWallBody, ...],
    graph: FaceGraph,
    faces: tuple[Face, ...],
    k_factor: float,
) -> list[SheetMetalBody]:
    return [record for wall in walls if (record := _body(wall, graph, faces, k_factor)) is not None]


def _discover(services: DiscoveryServices, inputs: CompletedInputs) -> list[object]:
    faces = tuple(services.context.part.faces())
    graph = services.context.graph
    records = _build_sheet_metal_records(
        inputs.records(FamilyId.THIN_WALL_BODIES, ThinWallBody),
        graph,
        faces,
        _DEFAULT_K_FACTOR,
    )
    for record in records:
        indices = set(record.first_side_faces) | set(record.second_side_faces)
        indices.update(record.cut_edge_faces)
        services.writer.add_defining(
            record,
            (graph.require_node(faces[index]) for index in sorted(indices)),
            family=FamilyId.SHEET_METAL_BODIES,
        )
    return list(records)


DEFINITION = PhysicalDefinition(
    family=FamilyId.SHEET_METAL_BODIES,
    record_types=(SheetMetalBody,),
    result_field="sheet_metal_bodies",
    public_entrypoint=recognise_sheet_metal_bodies.__name__,
    dependencies=(FamilyId.THIN_WALL_BODIES,),
    applicable=always,
    discover=_discover,
    census=NotCounted("whole-body fabrication interpretation, not a machined feature count"),
    attribution=FullyAttributed("accepted sheets claim both propagated skins and cut-edge faces"),
    evidence=ManifestEvidence(
        tests=("tests/test_sheet_metal.py",),
        golden_paths=("tests/sheet_metal_expected.json",),
        introduced="0.3.4",
        extra_records=(
            ("SheetFlange", "nested", ()),
            ("SheetBend", "nested", ()),
            ("FlatPatternPlan", "nested", ()),
        ),
    ),
)
