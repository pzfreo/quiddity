# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Geometry-only evidence for a large void behind a body's openings.

The sampled air volume is an estimate, not a reconstructed cutting tool.  The
faces and openings are original B-rep faces; no casting or machining history is
inferred from them.
"""

from __future__ import annotations

import math
from dataclasses import dataclass

from build123d import Face
from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.BRepClass import BRepClass_FaceClassifier
from OCP.BRepClass3d import BRepClass3d_SolidClassifier
from OCP.GeomAbs import GeomAbs_Cylinder, GeomAbs_Sphere
from OCP.gp import gp_Dir, gp_Lin, gp_Pnt
from OCP.IntCurvesFace import IntCurvesFace_ShapeIntersector
from OCP.Standard import Standard_Failure, Standard_NullObject
from OCP.TopAbs import TopAbs_IN, TopAbs_OUT

from quiddity._adjacency import FaceGraph
from quiddity._body_identity import unambiguous_body_keys
from quiddity._candidates import CompletedInputs, FamilyId
from quiddity._claims import EvidenceWriter
from quiddity._definitions import (
    DiscoveryServices,
    FullyAttributed,
    ManifestEvidence,
    NotCounted,
    PhysicalDefinition,
    always,
)
from quiddity._geometry import COORD_FLOOR
from quiddity._interior_void_grid import Cell, VoidGrid, expanded_air_components, sample_void_grid
from quiddity._record import Record
from quiddity._solid_properties import solid_properties
from quiddity._typing import Part

_AXES = ((1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1))
_UV_PROBES = ((0.25, 0.25), (0.5, 0.5), (0.75, 0.75), (0.25, 0.75), (0.75, 0.25))


@dataclass(frozen=True, slots=True)
class InteriorVoid(Record):
    """One observed void region within one valid source solid.

    ``void_faces`` are the original faces bounding the sampled core region.
    Each ``openings`` group is a connected set of adjacent original faces with
    a proved unobstructed direction to exterior air.  Neither claims a
    manufacturing operation.  ``estimated_volume`` counts six-axis enclosed
    air cells, so it excludes unsampled detail and the open throat; it is not an
    exact cavity or subtraction volume.  The sample counts expose its basis.
    """

    body_index: int
    body_key: tuple[float, ...] | None
    void_faces: tuple[int, ...]
    openings: tuple[tuple[int, ...], ...]
    estimated_volume: float
    volume_method: str
    grid_pitch: float
    enclosed_samples: int
    air_samples: int

    def __post_init__(self) -> None:
        if self.body_index < 0 or not self.void_faces:
            raise ValueError("an interior void needs a body and source faces")
        if self.void_faces != tuple(sorted(set(self.void_faces))) or self.void_faces[0] < 0:
            raise ValueError("void faces must be unique nonnegative source indices")
        if self.volume_method != "six_axis_grid":
            raise ValueError("unknown interior-void volume method")
        if not math.isfinite(self.estimated_volume) or self.estimated_volume <= 0:
            raise ValueError("estimated void volume must be positive and finite")
        if not math.isfinite(self.grid_pitch) or self.grid_pitch <= 0:
            raise ValueError("grid pitch must be positive and finite")
        if not 0 < self.enclosed_samples <= self.air_samples:
            raise ValueError("enclosed samples must be part of the air sample set")
        if any(not group or any(face < 0 for face in group) for group in self.openings):
            raise ValueError("opening groups need nonnegative source indices")
        if self.openings != tuple(sorted(set(self.openings))) or any(
            group != tuple(sorted(set(group))) for group in self.openings
        ):
            raise ValueError("opening groups must be sorted and unique")
        if len({face for group in self.openings for face in group}) != sum(map(len, self.openings)):
            raise ValueError("opening groups cannot overlap")
        if any(set(group) & set(self.void_faces) for group in self.openings):
            raise ValueError("opening faces cannot also be void faces")


def _face_samples(face: Face) -> tuple[tuple[float, float, float], ...]:
    classifier = BRepClass_FaceClassifier()
    points = []
    for u, v in _UV_PROBES:
        try:
            point = face.position_at(u, v)
            xyz = (float(point.X), float(point.Y), float(point.Z))
            classifier.Perform(face.wrapped, gp_Pnt(*xyz), COORD_FLOOR)
        except (Standard_Failure, Standard_NullObject, RuntimeError, ValueError):
            continue
        if classifier.State() == TopAbs_IN:
            points.append(xyz)
    return tuple(points)


def _escape_directions(face: Face) -> tuple[tuple[float, float, float], ...]:
    directions: list[tuple[float, float, float]] = list(_AXES)
    surface = BRepAdaptor_Surface(face.wrapped)
    if surface.GetType() == GeomAbs_Cylinder:
        axis = surface.Cylinder().Axis().Direction()
        direction = (axis.X(), axis.Y(), axis.Z())
        directions.extend((direction, tuple(-value for value in direction)))
    return tuple(directions)


def _escapes(
    face: Face,
    material: BRepClass3d_SolidClassifier,
    intersector: IntCurvesFace_ShapeIntersector,
    *,
    pitch: float,
    max_distance: float,
) -> bool | None:
    """True for a witnessed straight air ray; None if no bounded probe succeeds."""

    bounded_sample = False
    directions = _escape_directions(face)
    offset = max(COORD_FLOOR * 10, min(pitch, math.sqrt(face.area)) * 1e-3)
    for point in _face_samples(face):
        try:
            normal = face.normal_at(point)
            normal_xyz = tuple(float(value) for value in normal)
        except (Standard_Failure, Standard_NullObject, RuntimeError, ValueError):
            continue
        outside = None
        for sign in (-1, 1):
            proposed = tuple(point[a] + sign * offset * normal_xyz[a] for a in range(3))
            try:
                material.Perform(gp_Pnt(*proposed), COORD_FLOOR)
            except (Standard_Failure, Standard_NullObject, RuntimeError, ValueError):
                continue
            if material.State() == TopAbs_OUT:
                outside = proposed
                break
        if outside is None:
            continue
        all_hit = True
        for direction in directions:
            try:
                intersector.Perform(gp_Lin(gp_Pnt(*outside), gp_Dir(*direction)), 0.0, max_distance)
                hit = any(
                    intersector.WParameter(index) > 2 * offset
                    for index in range(1, intersector.NbPnt() + 1)
                )
            except (Standard_Failure, Standard_NullObject, RuntimeError, ValueError):
                all_hit = False
                continue
            if not hit:
                return True
        bounded_sample |= all_hit
    return False if bounded_sample else None


def _contact_faces(
    component: frozenset[Cell],
    grid: VoidGrid,
    intersector: IntCurvesFace_ShapeIntersector,
    face_indices: dict[Face, int],
) -> set[int] | None:
    """Map each sampled core/material boundary to its first original face."""

    found: set[int] = set()
    for cell in component:
        source = grid.center(cell)
        for direction in _AXES:
            neighbour = tuple(cell[axis] + direction[axis] for axis in range(3))
            if neighbour not in grid.material:
                continue
            try:
                intersector.Perform(gp_Lin(gp_Pnt(*source), gp_Dir(*direction)), 0.0, grid.pitch)
                hits = sorted(
                    [
                        (
                            intersector.WParameter(hit),
                            face_indices.get(Face(intersector.Face(hit))),
                        )
                        for hit in range(1, intersector.NbPnt() + 1)
                        if COORD_FLOOR * 10 < intersector.WParameter(hit) < grid.pitch
                    ],
                    key=lambda item: item[0],
                )
            except (Standard_Failure, Standard_NullObject, RuntimeError, ValueError):
                return None
            if not hits or hits[0][1] is None:
                return None
            found.add(hits[0][1])
    return found or None


def _opening_regions(
    void_faces: set[int], neighbours: dict[int, set[int]]
) -> tuple[tuple[int, ...], ...]:
    remaining = {
        other for face in void_faces for other in neighbours[face] if other not in void_faces
    }
    regions = []
    while remaining:
        seed = min(remaining)
        remaining.remove(seed)
        region = {seed}
        pending = [seed]
        while pending:
            for other in neighbours[pending.pop()] & remaining:
                remaining.remove(other)
                region.add(other)
                pending.append(other)
        regions.append(tuple(sorted(region)))
    return tuple(sorted(regions))


def _region_faces(
    seeds: set[int],
    faces: tuple[Face, ...],
    neighbours: dict[int, set[int]],
    material: BRepClass3d_SolidClassifier,
    intersector: IntCurvesFace_ShapeIntersector,
    *,
    pitch: float,
    max_distance: float,
) -> tuple[tuple[int, ...], tuple[tuple[int, ...], ...]] | None:
    skin = seeds.copy()
    decisions: dict[int, bool | None] = {}
    while True:
        boundary = {other for face in skin for other in neighbours[face] if other not in skin}
        newly_enclosed = set()
        for index in sorted(boundary):
            if index not in decisions:
                decisions[index] = _escapes(
                    faces[index], material, intersector, pitch=pitch, max_distance=max_distance
                )
            if decisions[index] is None:
                return None
            if decisions[index] is False:
                newly_enclosed.add(index)
        if not newly_enclosed:
            break
        skin.update(newly_enclosed)
    openings = _opening_regions(skin, neighbours)
    return tuple(sorted(skin)), openings


def _discover_interior_voids(part: Part, *, graph: FaceGraph | None = None) -> list[InteriorVoid]:
    bodies = tuple(part.solids())
    properties = solid_properties(graph)
    keys = unambiguous_body_keys(bodies, require_valid_solid=True, properties=properties)
    faces = tuple(part.faces())
    face_indices = {face: index for index, face in enumerate(faces)}
    shared_graph = graph
    records: list[InteriorVoid] = []
    for body_index, (body, key) in enumerate(zip(bodies, keys, strict=True)):
        if not properties.is_valid(body):
            continue
        intersector = IntCurvesFace_ShapeIntersector()
        intersector.Load(body.wrapped, COORD_FLOOR)
        material = BRepClass3d_SolidClassifier(body.wrapped)
        bounds = properties.bounding_box(body)
        max_distance = math.sqrt(sum(size * size for size in bounds.size)) * 2
        # A cheap original-face witness gates the much larger volume sample.
        # No bounded air face means no core skin to report.
        if not any(
            _escapes(
                face, material, intersector, pitch=max_distance / 64, max_distance=max_distance
            )
            is False
            for face in body.faces()
        ):
            continue
        grid = sample_void_grid(body, bounds=bounds)
        if grid is None or not grid.components:
            continue
        if shared_graph is None:
            shared_graph = FaceGraph(part)
        body_faces = tuple(body.faces())
        body_indices = {face_indices[face] for face in body_faces}
        neighbours = {
            index: {
                face_indices[shared_graph.face(node)]
                for node in shared_graph.neighbours(shared_graph.require_node(faces[index]))
            }
            & body_indices
            for index in body_indices
        }
        expanded: frozenset[int] | None = None
        seen_regions: set[tuple[int, ...]] = set()
        for component_index, component in enumerate(grid.components):
            # Line parity is independently checked against the original solid.
            samples = sorted(component)
            if any(
                _material_state(material, grid.center(samples[index])) != TopAbs_OUT
                for index in {0, len(samples) // 2, len(samples) - 1}
            ):
                continue
            seeds = _contact_faces(component, grid, intersector, face_indices)
            if seeds is None or not seeds <= body_indices:
                continue
            region = _region_faces(
                seeds,
                faces,
                neighbours,
                material,
                intersector,
                pitch=grid.pitch,
                max_distance=max_distance,
            )
            if region is None:
                continue
            skin, openings = region
            if skin in seen_regions:
                continue
            # A genuine curved core can remain broad all the way to an
            # opening; otherwise require a sampled widening behind a throat.
            spherical_skin_count = sum(
                BRepAdaptor_Surface(faces[index].wrapped).GetType() == GeomAbs_Sphere
                for index in skin
            )
            if spherical_skin_count < 2 and openings:
                if expanded is None:
                    expanded = expanded_air_components(grid)
                if component_index not in expanded:
                    continue
            seen_regions.add(skin)
            records.append(
                InteriorVoid(
                    body_index=body_index,
                    body_key=key,
                    void_faces=skin,
                    openings=openings,
                    estimated_volume=len(component) * grid.pitch**3,
                    volume_method="six_axis_grid",
                    grid_pitch=grid.pitch,
                    enclosed_samples=len(component),
                    air_samples=grid.air_samples,
                )
            )
    return records


def _material_state(
    classifier: BRepClass3d_SolidClassifier, point: tuple[float, float, float]
) -> object:
    try:
        classifier.Perform(gp_Pnt(*point), COORD_FLOOR)
    except (Standard_Failure, Standard_NullObject, RuntimeError, ValueError):
        return None
    return classifier.State()


def recognise_interior_voids(part: Part) -> list[InteriorVoid]:
    """Report well-sampled body voids and their original skin/opening faces."""

    return _discover_interior_voids(part)


def _discover(services: DiscoveryServices, inputs: CompletedInputs) -> list[object]:
    del inputs
    records = _discover_interior_voids(services.context.part, graph=services.context.graph)
    return list(_claim_records(records, services.context.part, services.writer))


def _claim_records(
    records: list[InteriorVoid], part: Part, writer: EvidenceWriter
) -> list[InteriorVoid]:
    graph = writer.graph
    faces = tuple(part.faces())
    retained = []
    for record in records:
        defining = {graph.require_node(faces[index]) for index in record.void_faces}
        constituent = defining | {
            graph.require_node(faces[index]) for opening in record.openings for index in opening
        }
        if graph.common_valid_solid(defining) is None:
            continue
        writer.add_defining(
            record,
            sorted(defining, key=lambda node: node.index),
            family=FamilyId.INTERIOR_VOIDS,
            constituent=sorted(constituent, key=lambda node: node.index),
        )
        retained.append(record)
    return retained


DEFINITION = PhysicalDefinition(
    family=FamilyId.INTERIOR_VOIDS,
    record_types=(InteriorVoid,),
    result_field="interior_voids",
    public_entrypoint=recognise_interior_voids.__name__,
    dependencies=(),
    applicable=always,
    discover=_discover,
    census=NotCounted("whole-body void evidence, not a machined feature count"),
    attribution=FullyAttributed("every core-skin face is original and body-proven"),
    evidence=ManifestEvidence(
        tests=("tests/test_interior_voids.py",),
        golden_paths=("tests/interior_void_expected.json",),
        introduced="0.4.0",
    ),
)
