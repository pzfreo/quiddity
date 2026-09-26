# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Whole-body wall evidence shared by shell and sheet-metal recognition.

The wall evidence describes the finished B-rep. A separate history hint labels
possible shell direction and operation order as heuristics because neither is
uniquely recoverable from the final solid. Face indices refer to the input
part's face roster, as in the recognition document, and are run-local.
"""

from __future__ import annotations

import math
from dataclasses import dataclass

from build123d import Face, Vector
from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.BRepClass import BRepClass_FaceClassifier
from OCP.BRepClass3d import BRepClass3d_SolidClassifier
from OCP.GeomAbs import GeomAbs_BSplineSurface, GeomAbs_Cylinder, GeomAbs_Plane
from OCP.gp import gp_Dir, gp_Lin, gp_Pnt
from OCP.IntCurvesFace import IntCurvesFace_ShapeIntersector
from OCP.TopAbs import TopAbs_IN

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
from quiddity._geometry import COORD_FLOOR, length_tol
from quiddity._record import Record
from quiddity._solid_properties import SolidProperties, solid_properties
from quiddity._typing import Part

# Five separated interior probes reject a coincidental nearest hit at a single point
# without making the cost proportional to face tessellation density.
_UV_PROBES = ((0.25, 0.25), (0.5, 0.5), (0.75, 0.75), (0.25, 0.75), (0.75, 0.25))
# Imported offset B-splines on cgb241 differ by about 0.00056 mm over a 3 mm
# nominal wall; this admits that approximation while remaining below 0.1%.
_PAIR_REL_TOL = 3e-4
# cgb207's small imported blend splines depart from a nominal 3 mm offset by
# at most 0.013 mm. Reciprocal material rays and opposed normals remain required.
_SPLINE_BLEND_REL_TOL = 5e-3
_OPPOSED_NORMAL_COS = -0.9999
_MIN_PAIRED_AREA_FRAC = 0.85
_MIN_CLASS_AREA_FRAC = 0.05


@dataclass(frozen=True, slots=True)
class WallFacePair(Record):
    """Two original faces separated by one measured local wall thickness."""

    first_face: int
    second_face: int
    thickness: float | None = None

    def __post_init__(self) -> None:
        if self.thickness is not None and (
            not math.isfinite(self.thickness) or self.thickness <= 0
        ):
            raise ValueError("wall pair thickness must be positive and finite")


@dataclass(frozen=True, slots=True)
class UnpairedWallFace(Record):
    """A bounded interpretation of one face outside the proven wall-pair set.

    ``non_wall_feature`` is the residual class: the current wall proof cannot
    safely bridge or repeat it, so a consumer must model it separately.
    """

    face: int
    kind: str

    def __post_init__(self) -> None:
        if self.face < 0 or self.kind not in {
            "cut_edge",
            "joint_blend",
            "non_wall_feature",
        }:
            raise ValueError("unpaired wall face needs a nonnegative index and a closed kind")


@dataclass(frozen=True, slots=True)
class ShellHistoryHint(Record):
    """Explicitly heuristic construction reading, separate from proven wall evidence.

    ``inward`` means the larger envelope is assumed to be the source body and
    material was removed toward the smaller one. The finished B-rep alone cannot
    distinguish that from an outward shell of a different source body. Faces not
    listed on either side remain unoriented rather than acquiring a guessed role.
    """

    basis: str
    direction: str | None
    outer_faces: tuple[int, ...]
    inner_faces: tuple[int, ...]
    opening_rims: tuple[tuple[int, ...], ...]
    before_shell_collar_pairs: tuple[WallFacePair, ...]
    after_shell_cut_faces: tuple[int, ...]


@dataclass(frozen=True, slots=True)
class ThinWallBody(Record):
    """One solid with proved locally constant wall thicknesses.

    ``thickness`` remains the dominant class for older consumers. Each
    recogniser-produced ``face_pairs`` entry gives its own measured offset.
    ``unpaired_faces`` contains cut edges, mouths and unsupported local features.
    ``unpaired_face_classes`` gives each one a bounded topology-based reading;
    the residual non-wall class is not permission to fill that face as a wall.
    ``paired_area_fraction`` counts each face only once.
    """

    body_index: int
    body_key: tuple[float, ...] | None
    thickness: float
    face_pairs: tuple[WallFacePair, ...]
    unpaired_faces: tuple[int, ...]
    rim_regions: tuple[tuple[int, ...], ...]
    paired_area_fraction: float
    history_hint: ShellHistoryHint
    unpaired_face_classes: tuple[UnpairedWallFace, ...] = ()


@dataclass(frozen=True, slots=True)
class _Hit:
    target: int
    distance: float


def _matches_offset(
    source: int, hit: _Hit, thickness: float, surface_types: tuple[object, ...]
) -> bool:
    rel = (
        _SPLINE_BLEND_REL_TOL
        if surface_types[source] == surface_types[hit.target] == GeomAbs_BSplineSurface
        else _PAIR_REL_TOL
    )
    return abs(hit.distance - thickness) <= length_tol(thickness, rel=rel)


def _native_surface(face: Face) -> BRepAdaptor_Surface:
    """Read the original analytic type used by the wall and history heuristics."""

    return BRepAdaptor_Surface(face.wrapped)


def _samples(face: Face) -> tuple[tuple[float, float, float], ...]:
    classifier = BRepClass_FaceClassifier()
    points = []
    for u, v in _UV_PROBES:
        try:
            point = face.position_at(u, v)
            classifier.Perform(face.wrapped, gp_Pnt(point.X, point.Y, point.Z), COORD_FLOOR)
        except (RuntimeError, ValueError):
            continue
        if classifier.State() == TopAbs_IN:
            points.append((point.X, point.Y, point.Z))
    if len(points) < 2:
        # Narrow imported blend patches and long concave trims can miss every
        # point in the surface's rectangular UV range. Triangle barycentres
        # lie inside the actual trimmed face, including curved joint rounds.
        vertices, triangles = face.tessellate(0.1)
        ranked = sorted(
            triangles,
            key=lambda tri: (
                -(vertices[tri[1]] - vertices[tri[0]])
                .cross(vertices[tri[2]] - vertices[tri[0]])
                .length,
                tri,
            ),
        )
        for a, b, c in ranked[:5]:
            barycentre = tuple(
                (tuple(vertices[a])[axis] + tuple(vertices[b])[axis] + tuple(vertices[c])[axis]) / 3
                for axis in range(3)
            )
            # The triangle lies slightly inside a convex analytic surface; use
            # the exact nearest point before measuring an opposing wall gap.
            try:
                projected, _ = face.closest_points(Vector(*barycentre))
            except (RuntimeError, ValueError):
                continue
            points.append(tuple(projected))
    return tuple(points)


def _first_material_hit(
    face: Face,
    point: tuple[float, float, float],
    faces: tuple[Face, ...],
    face_indices: dict[Face, int],
    intersector: IntCurvesFace_ShapeIntersector,
    material: BRepClass3d_SolidClassifier,
    max_distance: float,
) -> _Hit | None:
    normal = face.normal_at(point)
    for sign in (-1.0, 1.0):
        direction = (sign * normal.X, sign * normal.Y, sign * normal.Z)
        ray = gp_Lin(gp_Pnt(*point), gp_Dir(*direction))
        intersector.Perform(ray, 0.0, max_distance)
        hits = sorted(
            (
                (intersector.WParameter(index), intersector.Face(index))
                for index in range(1, intersector.NbPnt() + 1)
                if intersector.WParameter(index) > COORD_FLOOR * 10
            ),
            key=lambda hit: hit[0],
        )
        if not hits:
            continue
        distance, target_shape = hits[0]
        midpoint = gp_Pnt(*(p + d * distance / 2 for p, d in zip(point, direction, strict=True)))
        material.Perform(midpoint, COORD_FLOOR)
        if material.State() != TopAbs_IN:
            continue
        target = face_indices.get(Face(target_shape))
        if target is None:
            continue
        end = tuple(p + d * distance for p, d in zip(point, direction, strict=True))
        opposite = faces[target].normal_at(end)
        if normal.dot(opposite) > _OPPOSED_NORMAL_COS:
            continue
        return _Hit(target, distance)
    return None


def _body_pairs(
    body: Part, properties: SolidProperties
) -> tuple[float, tuple[tuple[int, int, float], ...], float] | None:
    faces = tuple(body.faces())
    if len(faces) < 4:
        return None
    bounds = properties.bounding_box(body)
    span = max(float(bounds.size.X), float(bounds.size.Y), float(bounds.size.Z))
    total_area = math.fsum(face.area for face in faces)
    if total_area <= 0 or 2 * properties.volume(body) / total_area > 0.2 * span:
        # A coarse stock body has no useful wall-thickness candidate. This is
        # only a rejection prefilter; the ray and coverage proof decides yes.
        return None
    intersector = IntCurvesFace_ShapeIntersector()
    intersector.Load(body.wrapped, COORD_FLOOR)
    material = BRepClass3d_SolidClassifier(body.wrapped)
    face_indices = {face: index for index, face in enumerate(faces)}
    surface_types = tuple(_native_surface(face).GetType() for face in faces)
    hits: dict[int, tuple[_Hit, ...]] = {}
    for index, face in enumerate(faces):
        found = []
        for point in _samples(face):
            try:
                hit = _first_material_hit(
                    face, point, faces, face_indices, intersector, material, span
                )
            except (RuntimeError, ValueError):
                continue
            if hit is not None and hit.target != index:
                found.append(hit)
        if found:
            hits[index] = tuple(found)

    # A physical wall needs support from more than one source face. This also
    # makes the largest isolated bore or pocket distance unable to set thickness.
    distances = sorted(hit.distance for samples in hits.values() for hit in samples)
    if len(distances) < 4:
        return None
    clusters: list[list[float]] = []
    for distance in distances:
        if not clusters or abs(distance - clusters[-1][0]) > length_tol(
            clusters[-1][0], rel=_PAIR_REL_TOL
        ):
            clusters.append([distance])
        else:
            clusters[-1].append(distance)
    pair_thickness: dict[tuple[int, int], float] = {}
    class_areas: list[tuple[float, float]] = []
    for cluster in sorted(clusters, key=lambda values: (-len(values), values[0])):
        if len(cluster) < 4:
            continue
        thickness = math.fsum(cluster) / len(cluster)

        local_pairs: set[tuple[int, int]] = set()
        for source, samples in hits.items():
            matching = [
                hit for hit in samples if _matches_offset(source, hit, thickness, surface_types)
            ]
            if len(matching) < 2 or len(matching) * 5 < len(samples) * 4:
                continue
            for hit in matching:
                target_samples = hits.get(hit.target, ())
                if any(
                    back.target == source
                    and _matches_offset(hit.target, back, thickness, surface_types)
                    for back in target_samples
                ):
                    local_pairs.add((min(source, hit.target), max(source, hit.target)))
        owned_faces = {index for pair in pair_thickness for index in pair}
        new_pairs = {
            pair
            for pair in local_pairs - pair_thickness.keys()
            if not owned_faces.intersection(pair)
        }
        if not new_pairs:
            continue
        class_faces = {index for pair in new_pairs for index in pair}
        class_area = math.fsum(faces[index].area for index in sorted(class_faces))
        largest_skin_area = max(faces[index].area for index in class_faces)
        if thickness > 0.3 * math.sqrt(largest_skin_area):
            continue
        # Each thickness must describe a material wall region, not a few
        # incidental bore or joint hits at another distance.
        if class_area < _MIN_CLASS_AREA_FRAC * total_area:
            continue
        class_areas.append((class_area, thickness))
        for a, b in sorted(new_pairs):
            distances = [
                hit.distance
                for source, target in ((a, b), (b, a))
                for hit in hits.get(source, ())
                if hit.target == target and _matches_offset(source, hit, thickness, surface_types)
            ]
            pair_thickness[(a, b)] = math.fsum(distances) / len(distances)
    if not pair_thickness:
        return None
    thickness = max(class_areas, key=lambda item: (item[0], -item[1]))[1]
    candidate_pairs = set(pair_thickness)
    paired = {index for pair in candidate_pairs for index in pair}
    paired_fraction = math.fsum(faces[index].area for index in sorted(paired)) / total_area
    if paired_fraction < _MIN_PAIRED_AREA_FRAC:
        return None
    # One opposed planar pair is a thin slab, not a shelled body. Require
    # substantial curved skin or walls in a second planar direction. Small
    # bores through a slab must not turn its stock faces into a shell.
    weighted_pairs = sorted(
        ((faces[a].area + faces[b].area, a, b) for a, b in candidate_pairs),
        reverse=True,
    )
    curved_area = math.fsum(
        area
        for area, a, b in weighted_pairs
        if any(_native_surface(faces[index]).GetType() != GeomAbs_Plane for index in (a, b))
    )
    paired_area = math.fsum(area for area, _, _ in weighted_pairs)
    primary = _native_surface(faces[weighted_pairs[0][1]])
    nonparallel_area = (
        math.fsum(
            area
            for area, a, _ in weighted_pairs
            if _native_surface(faces[a]).GetType() == GeomAbs_Plane
            and abs(
                primary.Plane()
                .Axis()
                .Direction()
                .Dot(_native_surface(faces[a]).Plane().Axis().Direction())
            )
            < 0.95
        )
        if primary.GetType() == GeomAbs_Plane
        else 0.0
    )
    if max(curved_area, nonparallel_area) < 0.1 * paired_area:
        return None
    # 2V/A is a whole-body sanity check, not the thickness measurement. Edges
    # make it somewhat smaller than t, as on the 4 mm hanger (3.647 mm).
    ratio = 2 * properties.volume(body) / total_area
    minimum = min(pair_thickness.values())
    maximum = max(pair_thickness.values())
    if not 0.5 * minimum <= ratio <= 1.1 * maximum:
        return None
    return (
        thickness,
        tuple((a, b, pair_thickness[a, b]) for a, b in sorted(candidate_pairs)),
        paired_fraction,
    )


def recognise_thin_wall_bodies(part: Part) -> list[ThinWallBody]:
    """Report solids whose paired skins have proved local wall offsets."""

    return _discover_thin_wall_bodies(part)


def _rim_regions(
    graph: FaceGraph,
    faces: tuple[Face, ...],
    unpaired: tuple[int, ...],
    pairs: tuple[WallFacePair, ...],
) -> tuple[tuple[int, ...], ...]:
    indices = {face: index for index, face in enumerate(faces)}
    rims = set()
    for index in unpaired:
        neighbours = {
            indices[graph.face(node)] for node in graph.neighbours(graph.require_node(faces[index]))
        }
        if any({pair.first_face, pair.second_face} <= neighbours for pair in pairs):
            rims.add(index)
    regions = []
    while rims:
        seed = min(rims)
        rims.remove(seed)
        region = {seed}
        pending = [seed]
        while pending:
            for neighbour in graph.neighbours(graph.require_node(faces[pending.pop()])):
                index = indices[graph.face(neighbour)]
                if index in rims:
                    rims.remove(index)
                    region.add(index)
                    pending.append(index)
        regions.append(tuple(sorted(region)))
    return tuple(regions)


def _classify_unpaired(
    graph: FaceGraph,
    faces: tuple[Face, ...],
    unpaired: tuple[int, ...],
    pairs: tuple[WallFacePair, ...],
    rims: tuple[tuple[int, ...], ...],
    history_hint: ShellHistoryHint,
) -> tuple[UnpairedWallFace, ...]:
    indices = {face: index for index, face in enumerate(faces)}
    paired = {index for pair in pairs for index in (pair.first_face, pair.second_face)}
    rim_faces = {index for region in rims for index in region} | set(
        history_hint.after_shell_cut_faces
    )
    result = []
    for index in unpaired:
        neighbours = {
            indices[graph.face(node)] for node in graph.neighbours(graph.require_node(faces[index]))
        }
        if index in rim_faces:
            kind = "cut_edge"
        elif (
            _native_surface(faces[index]).GetType() != GeomAbs_Plane
            and len(neighbours & paired) >= 2
        ):
            kind = "joint_blend"
        else:
            kind = "non_wall_feature"
        result.append(UnpairedWallFace(index, kind))
    return tuple(result)


def _skin_components(graph: FaceGraph, faces: tuple[Face, ...], paired: set[int]) -> list[set[int]]:
    indices = {face: index for index, face in enumerate(faces)}
    remaining = set(paired)
    components = []
    while remaining:
        seed = min(remaining)
        remaining.remove(seed)
        component = {seed}
        pending = [seed]
        while pending:
            current = pending.pop()
            for node in graph.neighbours(graph.require_node(faces[current])):
                neighbour = indices[graph.face(node)]
                if neighbour in remaining:
                    remaining.remove(neighbour)
                    component.add(neighbour)
                    pending.append(neighbour)
        components.append(component)
    return sorted(components, key=lambda group: -math.fsum(faces[i].area for i in group))


def _encloses(outer: set[int], inner: set[int], faces: tuple[Face, ...], tolerance: float) -> bool:
    outer_bounds = [faces[index].bounding_box() for index in outer]
    inner_bounds = [faces[index].bounding_box() for index in inner]
    wider = 0
    for axis in "XYZ":
        outer_low = min(getattr(bounds.min, axis) for bounds in outer_bounds)
        outer_high = max(getattr(bounds.max, axis) for bounds in outer_bounds)
        inner_low = min(getattr(bounds.min, axis) for bounds in inner_bounds)
        inner_high = max(getattr(bounds.max, axis) for bounds in inner_bounds)
        if outer_low > inner_low + tolerance or outer_high < inner_high - tolerance:
            return False
        wider += int(outer_low < inner_low - tolerance or outer_high > inner_high + tolerance)
    return wider >= 2


def _history_hint(
    graph: FaceGraph,
    faces: tuple[Face, ...],
    pairs: tuple[WallFacePair, ...],
    unpaired: tuple[int, ...],
    rims: tuple[tuple[int, ...], ...],
) -> ShellHistoryHint:
    paired = {index for pair in pairs for index in (pair.first_face, pair.second_face)}
    components = _skin_components(graph, faces, paired)
    outer: set[int] = set()
    inner: set[int] = set()
    if len(components) >= 2:
        first, second = components[:2]
        cross = sum(
            (pair.first_face in first and pair.second_face in second)
            or (pair.second_face in first and pair.first_face in second)
            for pair in pairs
        )
        if cross * 2 >= min(len(first), len(second)):
            tolerance = length_tol(max(face.area for face in faces) ** 0.5, rel=1e-6)
            if _encloses(first, second, faces, tolerance):
                outer, inner = first, second
            elif _encloses(second, first, faces, tolerance):
                outer, inner = second, first

    total_paired_area = math.fsum(faces[index].area for index in paired)
    collars = []
    for pair in pairs:
        a, b = (_native_surface(faces[index]) for index in (pair.first_face, pair.second_face))
        if (
            a.GetType() == b.GetType() == GeomAbs_Cylinder
            and a.LastUParameter() - a.FirstUParameter() >= math.pi - 1e-3
            and b.LastUParameter() - b.FirstUParameter() >= math.pi - 1e-3
            and faces[pair.first_face].area + faces[pair.second_face].area < 0.2 * total_paired_area
        ):
            collars.append(pair)

    indices = {face: index for index, face in enumerate(faces)}
    after = []
    for index in unpaired:
        if not outer or not inner:
            continue
        if _native_surface(faces[index]).GetType() != GeomAbs_Cylinder:
            continue
        neighbours = {
            indices[graph.face(node)] for node in graph.neighbours(graph.require_node(faces[index]))
        }
        if neighbours & outer and neighbours & inner:
            after.append(index)
    opening_rims = (
        tuple(region for region in rims if not any(index in after for index in region))
        if outer and inner
        else ()
    )
    return ShellHistoryHint(
        basis="heuristic",
        direction="inward" if outer and inner and opening_rims else None,
        outer_faces=tuple(sorted(outer)),
        inner_faces=tuple(sorted(inner)),
        opening_rims=opening_rims,
        before_shell_collar_pairs=tuple(collars),
        after_shell_cut_faces=tuple(after),
    )


def _discover_thin_wall_bodies(part: Part, *, graph: FaceGraph | None = None) -> list[ThinWallBody]:
    bodies = tuple(part.solids())
    properties = solid_properties(graph)
    keys = unambiguous_body_keys(bodies, require_valid_solid=True, properties=properties)
    all_faces = tuple(part.faces())
    all_indices = {face: index for index, face in enumerate(all_faces)}
    shared_graph = graph
    records = []
    for body_index, (body, key) in enumerate(zip(bodies, keys, strict=True)):
        if not properties.is_valid(body):
            continue
        found = _body_pairs(body, properties)
        if found is None:
            continue
        thickness, pairs, fraction = found
        body_faces = tuple(body.faces())
        translated = tuple(
            WallFacePair(all_indices[body_faces[a]], all_indices[body_faces[b]], distance)
            for a, b, distance in pairs
        )
        paired = {index for pair in translated for index in (pair.first_face, pair.second_face)}
        unpaired = tuple(
            all_indices[face] for face in body_faces if all_indices[face] not in paired
        )
        if shared_graph is None:
            shared_graph = FaceGraph(part)
        components = _skin_components(shared_graph, all_faces, paired)
        component_of = {index: order for order, group in enumerate(components) for index in group}
        # Opposite wall skins must stay separated by material. A pair whose
        # faces are connected through other paired faces describes a folded
        # plate assembly, not the two sides of one wall.
        if any(
            component_of[pair.first_face] == component_of[pair.second_face] for pair in translated
        ):
            continue
        rims = _rim_regions(shared_graph, all_faces, unpaired, translated)
        history_hint = _history_hint(shared_graph, all_faces, translated, unpaired, rims)
        records.append(
            ThinWallBody(
                body_index=body_index,
                body_key=key,
                thickness=thickness,
                face_pairs=translated,
                unpaired_faces=unpaired,
                rim_regions=rims,
                paired_area_fraction=fraction,
                history_hint=history_hint,
                unpaired_face_classes=_classify_unpaired(
                    shared_graph, all_faces, unpaired, translated, rims, history_hint
                ),
            )
        )
    return records


def _discover(services: DiscoveryServices, inputs: CompletedInputs) -> list[object]:
    del inputs
    records = _discover_thin_wall_bodies(services.context.part, graph=services.context.graph)
    return list(_claim_records(records, services.context.part, services.writer))


def _claim_records(
    records: list[ThinWallBody], part: Part, writer: EvidenceWriter
) -> list[ThinWallBody]:
    graph = writer.graph
    faces = tuple(part.faces())
    retained = []
    for record in records:
        paired = {
            index for pair in record.face_pairs for index in (pair.first_face, pair.second_face)
        }
        constituent = paired | {index for region in record.rim_regions for index in region}
        if (
            graph.local_degradation
            and graph.common_valid_solid(
                graph.require_node(faces[index]) for index in sorted(constituent)
            )
            is None
        ):
            continue
        writer.add_defining(
            record,
            (graph.require_node(faces[index]) for index in sorted(paired)),
            family=FamilyId.THIN_WALL_BODIES,
            constituent=(graph.require_node(faces[index]) for index in sorted(constituent)),
        )
        retained.append(record)
    return retained


DEFINITION = PhysicalDefinition(
    family=FamilyId.THIN_WALL_BODIES,
    record_types=(ThinWallBody,),
    result_field="thin_wall_bodies",
    public_entrypoint=recognise_thin_wall_bodies.__name__,
    dependencies=(),
    applicable=always,
    discover=_discover,
    census=NotCounted("whole-body construction evidence, not a machined feature count"),
    attribution=FullyAttributed(
        "every record defines both faces of each proven pair; rim faces are constituents"
    ),
    evidence=ManifestEvidence(
        tests=("tests/test_thin_walls.py",),
        golden_paths=("tests/thin_wall_expected.json",),
        introduced="0.3.5",
        extra_records=(
            ("WallFacePair", "nested", ()),
            ("ShellHistoryHint", "nested", ()),
            ("UnpairedWallFace", "nested", ()),
        ),
    ),
)
