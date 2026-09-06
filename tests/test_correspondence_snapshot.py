# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""F6a private body descriptors and accepted RRP snapshot authority."""

from __future__ import annotations

import ast
import copy
import dataclasses
import math
from collections.abc import Mapping
from itertools import permutations, product
from pathlib import Path

import pytest
from build123d import (
    Align,
    Box,
    Compound,
    Cylinder,
    Edge,
    Face,
    Plane,
    Polygon,
    Pos,
    Rot,
    Solid,
    Sphere,
    Vector,
    Wire,
    export_step,
    extrude,
    import_step,
)
from OCP.BRep import BRep_Tool
from OCP.BRepAdaptor import (
    BRepAdaptor_Curve,
    BRepAdaptor_Curve2d,
    BRepAdaptor_Surface,
)
from OCP.BRepBuilderAPI import BRepBuilderAPI_Transform
from OCP.BRepClass3d import BRepClass3d_SolidClassifier
from OCP.BRepGProp import BRepGProp
from OCP.gp import gp_Pnt, gp_Pnt2d, gp_Trsf
from OCP.GProp import GProp_GProps
from OCP.Standard import Standard_Failure
from OCP.TopAbs import (
    TopAbs_FORWARD,
    TopAbs_IN,
    TopAbs_Orientation,
    TopAbs_OUT,
    TopAbs_REVERSED,
)
from OCP.TopExp import TopExp
from OCP.TopoDS import TopoDS, TopoDS_Vertex

import quiddity
from quiddity import _body_geometry
from quiddity import _correspondence as correspondence_module
from quiddity._adjacency import BodyGeometryAuthorityError, FaceGraph
from quiddity._body_geometry import (
    FaceGeometry,
    UnsupportedBodyGeometry,
    matching_boundary_for_solid,
)
from quiddity._candidates import EvidenceIndex, FamilyId
from quiddity._correspondence import (
    CORRESPONDENCE_FAMILIES,
    CorrespondenceSnapshotError,
    MatchingBoundaryGraph,
    MatchingCurve,
    MatchingFace,
    MatchingHalfEdge,
    MatchingWire,
    MatchingWireVertex,
    correspondence_snapshot,
)
from quiddity.result import _take_inventory

ROOT = Path(__file__).parents[1]


def _proper_signed_permutations():
    matrices = []
    for axes in permutations(range(3)):
        for signs in product((-1, 1), repeat=3):
            matrix = tuple(
                tuple(signs[row] if axes[row] == column else 0 for column in range(3))
                for row in range(3)
            )
            determinant = round(
                matrix[0][0] * (matrix[1][1] * matrix[2][2] - matrix[1][2] * matrix[2][1])
                - matrix[0][1] * (matrix[1][0] * matrix[2][2] - matrix[1][2] * matrix[2][0])
                + matrix[0][2] * (matrix[1][0] * matrix[2][1] - matrix[1][1] * matrix[2][0])
            )
            if determinant == 1:
                matrices.append(matrix)
    return tuple(matrices)


def _proper_transform(part, matrix):
    transform = gp_Trsf()
    values = tuple(item for row in matrix for item in row)
    transform.SetValues(
        values[0],
        values[1],
        values[2],
        0.0,
        values[3],
        values[4],
        values[5],
        0.0,
        values[6],
        values[7],
        values[8],
        0.0,
    )
    return Solid(BRepBuilderAPI_Transform(part.wrapped, transform, True).Shape())


def _apply_rotation(matrix, value):
    return tuple(
        sum(matrix[row][column] * value[column] for column in range(3)) for row in range(3)
    )


def _raw_cylinder_cycle_oracle(part):
    """Derive material cylinder cycles from raw topology/pcurves before production reads."""

    volume = GProp_GProps()
    BRepGProp.VolumeProperties_s(part.wrapped, volume)
    centre = tuple(float(value) for value in volume.CentreOfMass().Coord())
    faces = tuple(part.faces())
    raw_edges: list[Edge] = []

    def edge_index(edge: Edge) -> int:
        found = next(
            (
                index
                for index, existing in enumerate(raw_edges)
                if existing.wrapped.IsSame(edge.wrapped)
            ),
            None,
        )
        if found is None:
            found = len(raw_edges)
            raw_edges.append(edge)
        return found

    result = {}
    classifier = BRepClass3d_SolidClassifier(part.wrapped)
    for face_index, face in enumerate(faces):
        surface = BRepAdaptor_Surface(face.wrapped)
        if surface.GetType().name != "GeomAbs_Cylinder":
            for edge in face.edges():
                edge_index(edge)
            continue
        cylinder = surface.Cylinder()
        raw_axis = tuple(float(value) for value in cylinder.Axis().Direction().Coord())
        first_significant = next(value for value in raw_axis if abs(value) > 1e-12)
        axis_sign = 1.0 if first_significant > 0.0 else -1.0
        axis = tuple(axis_sign * value for value in raw_axis)
        axes = ((1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0))
        reference = min(
            axes,
            key=lambda candidate: abs(
                sum(left * right for left, right in zip(candidate, axis, strict=True))
            ),
        )
        projection = tuple(
            value - sum(left * right for left, right in zip(reference, axis, strict=True)) * normal
            for value, normal in zip(reference, axis, strict=True)
        )
        norm = math.sqrt(sum(value * value for value in projection))
        u_axis = tuple(value / norm for value in projection)
        v_axis = (
            axis[1] * u_axis[2] - axis[2] * u_axis[1],
            axis[2] * u_axis[0] - axis[0] * u_axis[2],
            axis[0] * u_axis[1] - axis[1] * u_axis[0],
        )
        location = tuple(float(value) for value in cylinder.Location().Coord())
        delta = tuple(value - origin for value, origin in zip(location, centre, strict=True))
        along = sum(value * normal for value, normal in zip(delta, axis, strict=True))
        axis_point = tuple(
            value - along * normal for value, normal in zip(delta, axis, strict=True)
        )
        origin = surface.Value(0.0, 0.0).Coord()
        relative = tuple(
            value - body - offset
            for value, body, offset in zip(origin, centre, axis_point, strict=True)
        )
        radial_along = sum(value * normal for value, normal in zip(relative, axis, strict=True))
        radial = tuple(
            value - radial_along * normal for value, normal in zip(relative, axis, strict=True)
        )
        phase = math.atan2(
            sum(value * basis for value, basis in zip(radial, v_axis, strict=True)),
            sum(value * basis for value, basis in zip(radial, u_axis, strict=True)),
        )
        sample = tuple(float(value) for value in face.center())
        sample_delta = tuple(value - origin for value, origin in zip(sample, location, strict=True))
        sample_along = sum(
            value * direction for value, direction in zip(sample_delta, axis, strict=True)
        )
        radial = tuple(
            value - sample_along * direction
            for value, direction in zip(sample_delta, axis, strict=True)
        )
        radial_norm = math.sqrt(sum(value * value for value in radial))
        radial_unit = tuple(value / radial_norm for value in radial)
        probe_step = max(float(cylinder.Radius()) * 1e-6, 1e-6)
        states = []
        for sign in (1.0, -1.0):
            classifier.Perform(
                gp_Pnt(
                    *(
                        value + sign * probe_step * direction
                        for value, direction in zip(sample, radial_unit, strict=True)
                    )
                ),
                probe_step * 0.01,
            )
            states.append(classifier.State())
        if tuple(states) == (TopAbs_OUT, TopAbs_IN):
            material_side = 1
        elif tuple(states) == (TopAbs_IN, TopAbs_OUT):
            material_side = -1
        else:
            raise AssertionError("raw cylinder oracle could not prove material side")
        outer = face.outer_wire()
        for wire in face.wires():
            groups: list[tuple[Edge, list[Edge]]] = []
            for edge in wire.edges():
                group = next(
                    (
                        occurrences
                        for token, occurrences in groups
                        if token.wrapped.IsSame(edge.wrapped)
                    ),
                    None,
                )
                if group is None:
                    group = []
                    groups.append((edge, group))
                group.append(edge)
            occurrences = []
            for edge, edge_occurrences in groups:
                raw_index = edge_index(edge)
                curve = BRepAdaptor_Curve(edge.wrapped)
                full = (
                    abs(abs(curve.LastParameter() - curve.FirstParameter()) - 2.0 * math.pi) <= 1e-8
                    and curve.GetType().name == "GeomAbs_Circle"
                )
                variants = set()
                for orientation in (TopAbs_FORWARD, TopAbs_REVERSED):
                    oriented = Edge(TopoDS.Edge_s(edge.wrapped.Oriented(orientation)))
                    pcurve = BRepAdaptor_Curve2d(oriented.wrapped, face.wrapped)
                    first = pcurve.FirstParameter()
                    last = pcurve.LastParameter()
                    endpoints = []
                    for parameter in (first, last):
                        uv = pcurve.Value(parameter)
                        point = surface.Value(uv.X(), uv.Y()).Coord()
                        point_relative = tuple(
                            value - body - offset
                            for value, body, offset in zip(point, centre, axis_point, strict=True)
                        )
                        z = sum(
                            value * normal
                            for value, normal in zip(point_relative, axis, strict=True)
                        )
                        endpoints.append((axis_sign * uv.X() + phase, z))
                    variants.add(tuple(endpoints))
                assert len(variants) == len(edge_occurrences)
                first_vertex = TopExp.FirstVertex_s(edge.wrapped, False)
                last_vertex = TopExp.LastVertex_s(edge.wrapped, False)
                for start_parameter, end_parameter in sorted(variants):
                    occurrences.append(
                        (
                            raw_index,
                            None if full else first_vertex,
                            None if full else last_vertex,
                            start_parameter,
                            end_parameter,
                        )
                    )

            accepted = set()
            radius_value = float(cylinder.Radius())
            expected_positive_value = (wire == outer) == (material_side > 0)

            def join(left, right):
                _edge, _direction, _start_vertex, left_vertex, _start, left_end = left
                (
                    _right_edge,
                    _right_direction,
                    right_vertex,
                    _end_vertex,
                    right_start,
                    _end,
                ) = right
                if (
                    left_vertex is not None
                    and right_vertex is not None
                    and not left_vertex.IsSame(right_vertex)
                ):
                    return None
                if abs(left_end[1] - right_start[1]) > 1e-6:
                    return None
                turns = round((left_end[0] - right_start[0]) / (2.0 * math.pi))
                offset = turns * 2.0 * math.pi
                if abs(left_end[0] - right_start[0] - offset) > 1e-8:
                    return None
                return offset

            def retain(
                sequence,
                radius: float = radius_value,
                expected_positive: bool = expected_positive_value,
                accepted_cycles=accepted,
            ):
                if join(sequence[-1], sequence[0]) is None:
                    return
                points = tuple(
                    (start[0], start[1] / radius)
                    for _edge, _direction, _left, _right, start, _end in sequence
                )

                def cross(a, b, c):
                    return (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])

                def intersects(a0, a1, b0, b1, cross_product=cross):
                    values = (
                        cross_product(a0, a1, b0),
                        cross_product(a0, a1, b1),
                        cross_product(b0, b1, a0),
                        cross_product(b0, b1, a1),
                    )
                    if all(abs(value) > 1e-9 for value in values):
                        return (values[0] > 0.0) != (values[1] > 0.0) and (
                            (values[2] > 0.0) != (values[3] > 0.0)
                        )
                    return not (
                        max(a0[0], a1[0]) < min(b0[0], b1[0]) - 1e-9
                        or max(b0[0], b1[0]) < min(a0[0], a1[0]) - 1e-9
                        or max(a0[1], a1[1]) < min(b0[1], b1[1]) - 1e-9
                        or max(b0[1], b1[1]) < min(a0[1], a1[1]) - 1e-9
                    )

                for left_index, left_start in enumerate(points):
                    left_end = points[(left_index + 1) % len(points)]
                    if math.dist(left_start, left_end) <= 1e-9:
                        return
                    for right_index in range(left_index + 1, len(points)):
                        if right_index in {
                            left_index,
                            (left_index + 1) % len(points),
                        } or left_index == (right_index + 1) % len(points):
                            continue
                        if intersects(
                            left_start,
                            left_end,
                            points[right_index],
                            points[(right_index + 1) % len(points)],
                        ):
                            return
                area = 0.5 * sum(
                    start[0] * end[1] - end[0] * start[1]
                    for _edge, _direction, _left, _right, start, end in sequence
                )
                if abs(area) <= 1e-10 or ((area > 0.0) != expected_positive):
                    return
                theta_delta = sum(
                    end[0] - start[0] for _edge, _direction, _left, _right, start, end in sequence
                )
                winding = round(theta_delta / (2.0 * math.pi))
                if abs(theta_delta - winding * 2.0 * math.pi) > 1e-8 or abs(winding) > 1:
                    return
                cycle = tuple((edge, direction) for edge, direction, *_rest in sequence)
                accepted_cycles.add(
                    (
                        winding,
                        min(cycle[index:] + cycle[:index] for index in range(len(cycle))),
                    )
                )

            def extend(sequence, remaining):
                if not remaining:
                    retain(sequence)
                    return
                for index, item in enumerate(remaining):
                    rest = remaining[:index] + remaining[index + 1 :]
                    for reverse in (False, True):
                        edge, start_vertex, end_vertex, start, end = item
                        oriented = (
                            edge,
                            -1 if reverse else 1,
                            end_vertex if reverse else start_vertex,
                            start_vertex if reverse else end_vertex,
                            end if reverse else start,
                            start if reverse else end,
                        )
                        if sequence:
                            offset = join(sequence[-1], oriented)
                            if offset is None:
                                continue
                            edge_value, direction, left, right, start_value, end_value = oriented
                            oriented = (
                                edge_value,
                                direction,
                                left,
                                right,
                                (start_value[0] + offset, start_value[1]),
                                (end_value[0] + offset, end_value[1]),
                            )
                        extend((*sequence, oriented), rest)

            extend((), tuple(occurrences))
            assert len(accepted) == 1
            result[(face_index, "outer" if wire == outer else "inner")] = accepted.pop()
    return result, tuple(raw_edges)


def _raw_planar_cycle_oracle(part):
    """Derive material-oriented planar cycles before any production graph is read."""

    volume = GProp_GProps()
    BRepGProp.VolumeProperties_s(part.wrapped, volume)
    centre = tuple(float(value) for value in volume.CentreOfMass().Coord())
    faces = tuple(part.faces())
    classifier = BRepClass3d_SolidClassifier(part.wrapped)
    raw_edges: list[Edge] = []
    result = {}

    def edge_index(edge: Edge) -> int:
        found = next(
            (
                index
                for index, candidate in enumerate(raw_edges)
                if candidate.wrapped.IsSame(edge.wrapped)
            ),
            None,
        )
        if found is None:
            found = len(raw_edges)
            raw_edges.append(edge)
        return found

    for face_index, face in enumerate(faces):
        adaptor = BRepAdaptor_Surface(face.wrapped)
        if adaptor.GetType().name != "GeomAbs_Plane":
            for edge in face.edges():
                edge_index(edge)
            continue
        plane = adaptor.Plane()
        raw_normal = tuple(float(value) for value in plane.Axis().Direction().Coord())
        first = next(value for value in raw_normal if abs(value) > 1e-12)
        normal = tuple((1.0 if first > 0.0 else -1.0) * value for value in raw_normal)
        axes = ((1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0))
        reference = min(
            axes,
            key=lambda candidate: abs(
                sum(left * right for left, right in zip(candidate, normal, strict=True))
            ),
        )
        u_axis = tuple(
            value
            - sum(left * right for left, right in zip(reference, normal, strict=True)) * direction
            for value, direction in zip(reference, normal, strict=True)
        )
        u_norm = math.sqrt(sum(value * value for value in u_axis))
        u_axis = tuple(value / u_norm for value in u_axis)
        v_axis = (
            normal[1] * u_axis[2] - normal[2] * u_axis[1],
            normal[2] * u_axis[0] - normal[0] * u_axis[2],
            normal[0] * u_axis[1] - normal[1] * u_axis[0],
        )
        sample = tuple(float(value) for value in face.position_at(0.2, 0.5))
        probe_step = max(math.sqrt(float(face.area)) * 1e-6, 1e-6)
        states = []
        for sign in (1.0, -1.0):
            classifier.Perform(
                gp_Pnt(
                    *(
                        value + sign * probe_step * direction
                        for value, direction in zip(sample, normal, strict=True)
                    )
                ),
                probe_step * 0.01,
            )
            states.append(classifier.State())
        if tuple(states) == (TopAbs_OUT, TopAbs_IN):
            material_side = 1
        elif tuple(states) == (TopAbs_IN, TopAbs_OUT):
            material_side = -1
        else:
            raise AssertionError("raw plane oracle could not prove material side")

        def plane_point(point, u_axis=u_axis, v_axis=v_axis) -> tuple[float, float]:
            relative = tuple(
                float(value) - origin for value, origin in zip(point.Coord(), centre, strict=True)
            )
            return (
                sum(a * b for a, b in zip(relative, u_axis, strict=True)),
                sum(a * b for a, b in zip(relative, v_axis, strict=True)),
            )

        def integral(raw_index: int, direction: int, normal=normal) -> float:
            curve = BRepAdaptor_Curve(raw_edges[raw_index].wrapped)
            first_parameter = curve.FirstParameter()
            last_parameter = curve.LastParameter()
            start = plane_point(curve.Value(first_parameter))
            end = plane_point(curve.Value(last_parameter))
            if curve.GetType().name == "GeomAbs_Line":
                if direction < 0:
                    start, end = end, start
                return 0.5 * (start[0] * end[1] - end[0] * start[1])
            assert curve.GetType().name == "GeomAbs_Circle"
            circle = curve.Circle()
            circle_axis = tuple(float(value) for value in circle.Axis().Direction().Coord())
            axis_factor = sum(left * right for left, right in zip(circle_axis, normal, strict=True))
            assert abs(abs(axis_factor) - 1.0) <= 1e-9
            sweep = (last_parameter - first_parameter) * (1.0 if axis_factor > 0.0 else -1.0)
            if direction < 0:
                start, end = end, start
                sweep = -sweep
            circle_centre = plane_point(circle.Location())
            radius = float(circle.Radius())
            start_angle = math.atan2(start[1] - circle_centre[1], start[0] - circle_centre[0])
            end_angle = start_angle + sweep
            reconstructed = (
                circle_centre[0] + radius * math.cos(end_angle),
                circle_centre[1] + radius * math.sin(end_angle),
            )
            assert reconstructed == pytest.approx(end, abs=1e-7)
            return 0.5 * (
                radius * circle_centre[0] * (math.sin(end_angle) - math.sin(start_angle))
                + radius * circle_centre[1] * (-math.cos(end_angle) + math.cos(start_angle))
                + radius**2 * sweep
            )

        outer = face.outer_wire()
        for wire in face.wires():
            role = "outer" if wire == outer else "inner"
            indices = tuple(edge_index(edge) for edge in wire.edges())
            expected_positive = (role == "outer") == (material_side > 0)
            if len(indices) == 1:
                raw_index = indices[0]
                curve = BRepAdaptor_Curve(raw_edges[raw_index].wrapped)
                if (
                    curve.GetType().name == "GeomAbs_Circle"
                    and abs(abs(curve.LastParameter() - curve.FirstParameter()) - 2.0 * math.pi)
                    <= 1e-8
                ):
                    direction = 1 if (integral(raw_index, 1) > 0.0) == expected_positive else -1
                    result[(face_index, role)] = ((raw_index, direction),)
                    continue
            endpoints = {
                raw_index: (
                    TopExp.FirstVertex_s(raw_edges[raw_index].wrapped, False),
                    TopExp.LastVertex_s(raw_edges[raw_index].wrapped, False),
                )
                for raw_index in indices
            }
            accepted = set()

            def extend(
                sequence,
                remaining,
                *,
                accepted=accepted,
                endpoints=endpoints,
                expected_positive=expected_positive,
            ):
                if not remaining:
                    if not sequence or not sequence[-1][2].IsSame(sequence[0][1]):
                        return
                    signed_area = sum(
                        integral(index, direction) for index, _left, _right, direction in sequence
                    )
                    if (signed_area > 0.0) != expected_positive:
                        return
                    cycle = tuple(
                        (index, direction) for index, _left, _right, direction in sequence
                    )
                    accepted.add(
                        min(cycle[offset:] + cycle[:offset] for offset in range(len(cycle)))
                    )
                    return
                for offset, raw_index in enumerate(remaining):
                    rest = remaining[:offset] + remaining[offset + 1 :]
                    left, right = endpoints[raw_index]
                    for direction, start, end in ((1, left, right), (-1, right, left)):
                        if sequence and not sequence[-1][2].IsSame(start):
                            continue
                        extend((*sequence, (raw_index, start, end, direction)), rest)

            extend((), indices)
            assert len(accepted) == 1
            result[(face_index, role)] = accepted.pop()
    return result, tuple(raw_edges)


def _raw_schema_three_incidence_oracle(
    part,
    matching: MatchingBoundaryGraph,
    planar_cycles,
    planar_edge_roster,
    cylinder_cycles,
    cylinder_edge_roster,
) -> None:
    """Map raw-OCP edge/face identity to matching values without production helpers."""

    volume = GProp_GProps()
    BRepGProp.VolumeProperties_s(part.wrapped, volume)
    centre = tuple(float(value) for value in volume.CentreOfMass().Coord())
    faces = tuple(part.faces())
    face_map = {}
    for raw_index, face in enumerate(faces):
        adaptor = BRepAdaptor_Surface(face.wrapped)
        kind = adaptor.GetType().name.removeprefix("GeomAbs_").upper()
        relative = tuple(
            float(value) - origin
            for value, origin in zip(tuple(face.center()), centre, strict=True)
        )
        choices = tuple(
            index
            for index, candidate in enumerate(matching.faces)
            if candidate.kind == kind
            and candidate.area == pytest.approx(face.area, abs=1e-4)
            and candidate.centroid == pytest.approx(relative, abs=1e-5)
        )
        assert len(choices) == 1
        face_map[raw_index] = choices[0]

    raw_edges = []
    raw_incidence: dict[int, list[int]] = {}
    raw_wires: dict[int, list[tuple[str, tuple[int, ...], tuple[Edge, ...]]]] = {}
    for raw_face_index, face in enumerate(faces):
        outer = face.outer_wire()
        for wire in face.wires():
            wire_edges = tuple(wire.edges())
            wire_indices = []
            for edge in wire_edges:
                edge_index = next(
                    (
                        index
                        for index, existing in enumerate(raw_edges)
                        if existing.wrapped.IsSame(edge.wrapped)
                    ),
                    None,
                )
                if edge_index is None:
                    edge_index = len(raw_edges)
                    raw_edges.append(edge)
                raw_incidence.setdefault(edge_index, []).append(face_map[raw_face_index])
                wire_indices.append(edge_index)
            raw_wires.setdefault(raw_face_index, []).append(
                (
                    "outer" if wire == outer else "inner",
                    tuple(wire_indices),
                    wire_edges,
                )
            )
    assert all(len(owners) == 2 for owners in raw_incidence.values())
    assert len(raw_edges) == len(matching.curves)

    curve_map = {}
    curve_direction = {}
    for raw_index, edge in enumerate(raw_edges):
        adaptor = BRepAdaptor_Curve(edge.wrapped)
        kind = adaptor.GetType().name.removeprefix("GeomAbs_").upper()
        endpoints = tuple(
            tuple(
                float(value) - origin
                for value, origin in zip(tuple(edge.position_at(at)), centre, strict=True)
            )
            for at in (0.0, 1.0)
        )
        choices = []
        for index, candidate in enumerate(matching.curves):
            if candidate.kind != kind or candidate.length != pytest.approx(edge.length, abs=1e-5):
                continue
            if candidate.vertices is not None:
                candidate_points = tuple(matching.vertices[vertex] for vertex in candidate.vertices)
                if all(
                    any(point == pytest.approx(raw, abs=1e-5) for point in candidate_points)
                    for raw in endpoints
                ):
                    choices.append(index)
            elif kind == "CIRCLE" and candidate.centre is not None:
                raw_centre = tuple(
                    float(value) - origin
                    for value, origin in zip(
                        adaptor.Circle().Location().Coord(), centre, strict=True
                    )
                )
                if candidate.centre == pytest.approx(raw_centre, abs=1e-5):
                    choices.append(index)
        assert len(choices) == 1
        curve_map[raw_index] = choices[0]
        candidate = matching.curves[choices[0]]
        if candidate.vertices is None:
            curve_direction[raw_index] = 1
        else:
            first_vertex = TopExp.FirstVertex_s(edge.wrapped, False)
            first_point = tuple(
                float(value) - origin
                for value, origin in zip(BRep_Tool.Pnt_s(first_vertex).Coord(), centre, strict=True)
            )
            candidate_first = matching.vertices[candidate.vertices[0]]
            curve_direction[raw_index] = (
                1 if candidate_first == pytest.approx(first_point, abs=1e-5) else -1
            )
    expected = {curve_map[edge]: tuple(sorted(owners)) for edge, owners in raw_incidence.items()}
    actual = {
        curve: tuple(sorted(face for face, _wire, _occurrence in occurrences))
        for curve, occurrences in matching.incidence
    }
    assert actual == expected

    for raw_face_index, wire_roster in raw_wires.items():
        matching_face = matching.faces[face_map[raw_face_index]]
        for role, raw_curve_indices, wire_edges in wire_roster:
            expected_curves = tuple(sorted(curve_map[index] for index in raw_curve_indices))
            choices = tuple(
                wire
                for wire in matching_face.wires
                if wire.role == role
                and tuple(sorted(item.curve for item in wire.cycle)) == expected_curves
            )
            assert len(choices) == 1
            matching_wire = choices[0]
            assert len(matching_wire.cycle) == len(wire_edges)
            if matching_face.kind != "CYLINDER":
                raw_expected = planar_cycles[(raw_face_index, role)]
                expected_cycle = tuple(
                    (
                        curve_map[
                            next(
                                index
                                for index, candidate in enumerate(raw_edges)
                                if candidate.wrapped.IsSame(planar_edge_roster[raw_edge].wrapped)
                            )
                        ],
                        direction
                        * curve_direction[
                            next(
                                index
                                for index, candidate in enumerate(raw_edges)
                                if candidate.wrapped.IsSame(planar_edge_roster[raw_edge].wrapped)
                            )
                        ],
                    )
                    for raw_edge, direction in raw_expected
                )
                expected_cycle = min(
                    expected_cycle[index:] + expected_cycle[:index]
                    for index in range(len(expected_cycle))
                )
                assert matching_wire.theta_winding == 0
                assert (
                    tuple((item.curve, item.direction) for item in matching_wire.cycle)
                    == expected_cycle
                )
                continue
            expected_winding, expected_cycle = cylinder_cycles[(raw_face_index, role)]
            mapped_cycle = tuple(
                (
                    curve_map[
                        next(
                            index
                            for index, candidate in enumerate(raw_edges)
                            if candidate.wrapped.IsSame(cylinder_edge_roster[raw_edge].wrapped)
                        )
                    ],
                    direction
                    * curve_direction[
                        next(
                            index
                            for index, candidate in enumerate(raw_edges)
                            if candidate.wrapped.IsSame(cylinder_edge_roster[raw_edge].wrapped)
                        )
                    ],
                )
                for raw_edge, direction in expected_cycle
            )
            mapped_cycle = min(
                mapped_cycle[index:] + mapped_cycle[:index] for index in range(len(mapped_cycle))
            )
            assert matching_wire.theta_winding == expected_winding
            assert (
                tuple((item.curve, item.direction) for item in matching_wire.cycle) == mapped_cycle
            )
            axis = matching_face.parameters[:3]
            axis_point = matching_face.parameters[3:6]
            reference_axes = ((1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0))
            reference = min(
                reference_axes,
                key=lambda candidate: abs(
                    sum(left * right for left, right in zip(candidate, axis, strict=True))
                ),
            )
            projection = tuple(
                value
                - sum(left * right for left, right in zip(reference, axis, strict=True)) * normal
                for value, normal in zip(reference, axis, strict=True)
            )
            norm = math.sqrt(sum(value * value for value in projection))
            u_axis = tuple(value / norm for value in projection)
            v_axis = (
                axis[1] * u_axis[2] - axis[2] * u_axis[1],
                axis[2] * u_axis[0] - axis[0] * u_axis[2],
                axis[0] * u_axis[1] - axis[1] * u_axis[0],
            )
            surface = BRepAdaptor_Surface(faces[raw_face_index].wrapped)
            raw_parameters = []
            for edge in wire_edges:
                pcurve = BRepAdaptor_Curve2d(edge.wrapped, faces[raw_face_index].wrapped)
                for parameter in (pcurve.FirstParameter(), pcurve.LastParameter()):
                    uv = pcurve.Value(parameter)
                    point = surface.Value(uv.X(), uv.Y()).Coord()
                    relative = tuple(
                        value - origin - offset
                        for value, origin, offset in zip(point, centre, axis_point, strict=True)
                    )
                    z = sum(value * normal for value, normal in zip(relative, axis, strict=True))
                    radial = tuple(
                        value - z * normal for value, normal in zip(relative, axis, strict=True)
                    )
                    theta = math.atan2(
                        sum(value * basis for value, basis in zip(radial, v_axis, strict=True)),
                        sum(value * basis for value, basis in zip(radial, u_axis, strict=True)),
                    ) % (2.0 * math.pi)
                    raw_parameters.append((theta, z))
            stored_parameters = [
                (parameter[0] % (2.0 * math.pi), parameter[1])
                for item in matching_wire.cycle
                for parameter in (
                    () if item.start is None else (item.start.parameter, item.end.parameter)
                )
            ]
            assert len(stored_parameters) <= len(raw_parameters)
            for stored in stored_parameters:
                assert any(
                    abs(math.remainder(stored[0] - raw[0], 2.0 * math.pi)) <= 1e-8
                    and abs(stored[1] - raw[1]) <= 1e-5
                    for raw in raw_parameters
                )


def test_schema_three_matching_values_freeze_global_reference_shape() -> None:
    line = MatchingCurve("LINE", (0, 1), 1.0, None, None, None, None, False)
    circle = MatchingCurve(
        "CIRCLE",
        None,
        2.0 * math.pi,
        (0.0, 0.0, 0.0),
        (0.0, 0.0, 1.0),
        1.0,
        2.0 * math.pi,
        True,
    )
    start = MatchingWireVertex(0, (0.0, 0.0))
    end = MatchingWireVertex(1, (1.0, 0.0))
    line_use = MatchingHalfEdge(0, 1, start, end)
    full_use = MatchingHalfEdge(1, -1, None, None)
    wire = MatchingWire("outer", 0, (line_use, full_use))
    face = MatchingFace(
        "PLANE",
        (0.0, 0.0, 1.0, 0.0),
        1.0,
        (0.0, 0.0, 0.0),
        1,
        (wire,),
    )
    graph = MatchingBoundaryGraph(
        ((0.0, 0.0, 0.0), (1.0, 0.0, 0.0)),
        (line, circle),
        (face,),
        ((0, ((0, 0, 0),)), (1, ((0, 0, 1),))),
        1,
        1,
        2,
        False,
    )
    assert graph.curves[0].vertices == (0, 1)
    assert graph.curves[1].vertices is None
    assert graph.faces[0].wires[0].cycle[1].start is None


def test_line_plane_matching_graph_erases_face_and_edge_traversal_order(monkeypatch) -> None:
    part = Box(10, 20, 30)
    graph = FaceGraph(part)
    solid = graph.common_valid_solid(graph.nodes)
    assert solid is not None
    descriptor = graph.body_geometry(solid).descriptor
    source = matching_boundary_for_solid(part, descriptor)
    solid_faces = Solid.faces
    wire_edges = Wire.edges
    monkeypatch.setattr(Solid, "faces", lambda self: list(reversed(solid_faces(self))))
    monkeypatch.setattr(Wire, "edges", lambda self: list(reversed(wire_edges(self))))
    assert matching_boundary_for_solid(part, descriptor) == source
    assert source.face_count == 6
    assert source.wire_count == 6
    assert source.edge_occurrence_count == 24


def test_schema_three_box_half_edges_covary_under_all_24_proper_rotations() -> None:
    source_part = Box(10, 20, 30)
    source_graph = FaceGraph(source_part)
    source_solid = source_graph.common_valid_solid(source_graph.nodes)
    assert source_solid is not None
    source = source_graph.matching_boundary(source_solid)

    for rotation in _proper_signed_permutations():
        target_part = _proper_transform(source_part, rotation)
        target_graph = FaceGraph(target_part)
        target_solid = target_graph.common_valid_solid(target_graph.nodes)
        assert target_solid is not None
        target = target_graph.matching_boundary(target_solid)
        vertex_map = {
            index: min(
                range(len(target.vertices)),
                key=lambda other: math.dist(
                    _apply_rotation(rotation, vertex), target.vertices[other]
                ),
            )
            for index, vertex in enumerate(source.vertices)
        }
        assert len(set(vertex_map.values())) == len(source.vertices)
        face_map = {
            index: min(
                range(len(target.faces)),
                key=lambda other: math.dist(
                    _apply_rotation(rotation, face.centroid), target.faces[other].centroid
                ),
            )
            for index, face in enumerate(source.faces)
        }
        assert len(set(face_map.values())) == len(source.faces)
        curve_map = {}
        presentation = {}
        for index, curve in enumerate(source.curves):
            assert curve.kind == "LINE" and curve.vertices is not None
            transformed = tuple(vertex_map[item] for item in curve.vertices)
            matches = tuple(
                other
                for other, candidate in enumerate(target.curves)
                if candidate.kind == "LINE"
                and candidate.vertices is not None
                and set(candidate.vertices) == set(transformed)
            )
            assert len(matches) == 1
            curve_map[index] = matches[0]
            presentation[index] = 1 if target.curves[matches[0]].vertices == transformed else -1
        mapped = sorted(
            (
                face_map[face_index],
                curve_map[half_edge.curve],
                half_edge.direction * presentation[half_edge.curve],
            )
            for face_index, face in enumerate(source.faces)
            for wire in face.wires
            for half_edge in wire.cycle
        )
        expected = sorted(
            (face_index, half_edge.curve, half_edge.direction)
            for face_index, face in enumerate(target.faces)
            for wire in face.wires
            for half_edge in wire.cycle
        )
        assert mapped == expected


def test_planar_full_circle_cycle_has_no_serialized_seam() -> None:
    face = FaceGeometry(
        "PLANE",
        (0.0, 0.0, 1.0, 0.0),
        math.pi,
        (0.0, 0.0, 0.0),
        1,
        (),
    )
    curve = MatchingCurve(
        "CIRCLE",
        None,
        2.0 * math.pi,
        (0.0, 0.0, 0.0),
        (0.0, 0.0, 1.0),
        1.0,
        2.0 * math.pi,
        True,
    )

    wire = _body_geometry._planar_cycle(
        (0,),
        (curve,),
        "outer",
        face,
        1e-9,
        (),
        _body_geometry._MatchingConstructionBudget(),
    )

    assert wire == MatchingWire("outer", 0, (MatchingHalfEdge(0, 1, None, None),))

    reversed_material = dataclasses.replace(face, material_side=-1)
    reversed_wire = _body_geometry._planar_cycle(
        (0,),
        (curve,),
        "outer",
        reversed_material,
        1e-9,
        (),
        _body_geometry._MatchingConstructionBudget(),
    )
    assert reversed_wire.cycle[0].direction == -1


def test_planar_trimmed_circle_integral_reconstructs_the_arc() -> None:
    face = FaceGeometry(
        "PLANE",
        (0.0, 0.0, 1.0, 0.0),
        1.0,
        (0.0, 0.0, 0.0),
        1,
        (),
    )
    curve = MatchingCurve(
        "CIRCLE",
        (0, 1),
        0.5 * math.pi,
        (0.0, 0.0, 0.0),
        (0.0, 0.0, 1.0),
        1.0,
        0.5 * math.pi,
        False,
    )
    half_edge = MatchingHalfEdge(
        0,
        1,
        MatchingWireVertex(0, (1.0, 0.0)),
        MatchingWireVertex(1, (0.0, 1.0)),
    )

    assert _body_geometry._half_edge_integral(half_edge, (curve,), face, 1e-9) == pytest.approx(
        0.25 * math.pi
    )


def test_cylindrical_seam_matching_graph_erases_wire_presentation(monkeypatch) -> None:
    part = Cylinder(10, 20)
    graph = FaceGraph(part)
    solid = graph.common_valid_solid(graph.nodes)
    assert solid is not None
    descriptor = graph.body_geometry(solid).descriptor
    source = matching_boundary_for_solid(part, descriptor)

    wire_edges = Wire.edges
    monkeypatch.setattr(Wire, "edges", lambda self: list(reversed(wire_edges(self))))

    assert matching_boundary_for_solid(part, descriptor) == source
    cylinder = next(face for face in source.faces if face.kind == "CYLINDER")
    assert cylinder.wires[0].theta_winding == 0
    seam_uses = tuple(
        item for item in cylinder.wires[0].cycle if source.curves[item.curve].kind == "LINE"
    )
    assert len(seam_uses) == 2
    assert seam_uses[0].curve == seam_uses[1].curve
    seam_thetas = sorted(item.start.parameter[0] for item in seam_uses if item.start is not None)
    assert seam_thetas == pytest.approx((0.0, 2.0 * math.pi), abs=_body_geometry.ANGLE_TOL)


def test_cylindrical_seam_matching_graph_erases_occurrence_orientation(
    monkeypatch,
) -> None:
    part = Cylinder(10, 20)
    graph = FaceGraph(part)
    solid = graph.common_valid_solid(graph.nodes)
    assert solid is not None
    descriptor = graph.body_geometry(solid).descriptor
    source = matching_boundary_for_solid(part, descriptor)

    wire_edges = Wire.edges
    monkeypatch.setattr(
        Wire,
        "edges",
        lambda self: [edge.reversed() for edge in reversed(wire_edges(self))],
    )

    assert matching_boundary_for_solid(part, descriptor) == source


def test_schema_three_matching_incidence_mutation_refuses() -> None:
    snapshot = correspondence_snapshot(_take_inventory(_rrp()))
    occurrence = snapshot.occurrences[0]
    matching = occurrence.matching_boundary
    malformed = dataclasses.replace(matching, incidence=())
    changed_occurrence = dataclasses.replace(occurrence, matching_boundary=malformed)
    changed = dataclasses.replace(snapshot, occurrences=(changed_occurrence,))

    with pytest.raises(CorrespondenceSnapshotError, match="matching boundary"):
        correspondence_module._validate_snapshot(changed)


def test_schema_three_coherent_parameter_mutation_refuses() -> None:
    occurrence = correspondence_snapshot(_take_inventory(_rrp())).occurrences[0]
    graph = occurrence.matching_boundary
    face_index, wire_index, half_edge_index = next(
        (face_index, wire_index, half_edge_index)
        for face_index, face in enumerate(graph.faces)
        for wire_index, wire in enumerate(face.wires)
        for half_edge_index, half_edge in enumerate(wire.cycle)
        if half_edge.start is not None
    )
    face = graph.faces[face_index]
    wire = face.wires[wire_index]
    half_edge = wire.cycle[half_edge_index]
    assert half_edge.start is not None
    changed_half_edge = dataclasses.replace(
        half_edge,
        start=dataclasses.replace(half_edge.start, parameter=(999.0, 999.0)),
    )
    changed_wire = dataclasses.replace(
        wire,
        cycle=tuple(
            changed_half_edge if index == half_edge_index else item
            for index, item in enumerate(wire.cycle)
        ),
    )
    changed_face = dataclasses.replace(
        face,
        wires=tuple(
            changed_wire if index == wire_index else item for index, item in enumerate(face.wires)
        ),
    )
    changed = dataclasses.replace(
        graph,
        faces=tuple(
            changed_face if index == face_index else item for index, item in enumerate(graph.faces)
        ),
    )

    with pytest.raises(UnsupportedBodyGeometry, match="matching"):
        _body_geometry.validate_matching_boundary_graph(changed, occurrence.body.quantization)


@pytest.mark.parametrize(
    "mutation",
    [
        "container",
        "vertex",
        "line_shape",
        "line_reconstruction",
        "circle_shape",
        "circle_gauge",
        "circle_reconstruction",
        "face_count",
        "face_gauge",
        "cylinder_radius",
        "wire_schema",
        "canonical_start",
        "half_edge_schema",
        "full_endpoint",
        "traversal",
        "topology_join",
        "plane_orientation",
        "cylinder_orientation",
        "theta_winding",
        "incidence",
    ],
)
def test_schema_three_semantic_validator_refuses_complete_mutation_roster(
    mutation: str,
) -> None:
    part = (
        Cylinder(10, 20)
        if mutation.startswith(("circle", "cylinder", "theta")) or mutation == "full_endpoint"
        else Box(4, 5, 6)
    )
    graph_query = FaceGraph(part)
    solid = graph_query.common_valid_solid(graph_query.nodes)
    assert solid is not None
    quantization = graph_query.body_geometry(solid).descriptor.quantization
    graph = graph_query.matching_boundary(solid)

    if mutation == "container":
        changed = dataclasses.replace(graph, vertices=list(graph.vertices))
    elif mutation == "vertex":
        changed = dataclasses.replace(graph, vertices=((math.nan, 0.0, 0.0), *graph.vertices[1:]))
    elif mutation.startswith("line"):
        curve_index = next(
            index for index, curve in enumerate(graph.curves) if curve.kind == "LINE"
        )
        curve = graph.curves[curve_index]
        changed_curve = dataclasses.replace(
            curve,
            full=True if mutation == "line_shape" else curve.full,
            length=curve.length + (1.0 if mutation == "line_reconstruction" else 0.0),
        )
        changed = dataclasses.replace(
            graph,
            curves=tuple(
                changed_curve if index == curve_index else item
                for index, item in enumerate(graph.curves)
            ),
        )
    elif mutation.startswith("circle"):
        curve_index = next(
            index for index, curve in enumerate(graph.curves) if curve.kind == "CIRCLE"
        )
        curve = graph.curves[curve_index]
        changed_curve = dataclasses.replace(
            curve,
            sweep=math.nan if mutation == "circle_shape" else curve.sweep,
            axis=(0.5, 0.0, 0.0) if mutation == "circle_gauge" else curve.axis,
            radius=(curve.radius + 1.0)
            if mutation == "circle_reconstruction" and curve.radius is not None
            else curve.radius,
        )
        changed = dataclasses.replace(
            graph,
            curves=tuple(
                changed_curve if index == curve_index else item
                for index, item in enumerate(graph.curves)
            ),
        )
    elif mutation == "face_count":
        changed = dataclasses.replace(graph, face_count=graph.face_count + 1)
    elif mutation in {"face_gauge", "cylinder_radius"}:
        face_index = next(
            index
            for index, face in enumerate(graph.faces)
            if mutation == "face_gauge" or face.kind == "CYLINDER"
        )
        face = graph.faces[face_index]
        parameters = list(face.parameters)
        parameters[0] = 0.5 if mutation == "face_gauge" else parameters[0]
        if mutation == "cylinder_radius":
            parameters[6] = 0.0
        changed_face = dataclasses.replace(face, parameters=tuple(parameters))
        changed = dataclasses.replace(
            graph,
            faces=tuple(
                changed_face if index == face_index else item
                for index, item in enumerate(graph.faces)
            ),
        )
    elif mutation in {
        "wire_schema",
        "canonical_start",
        "half_edge_schema",
        "full_endpoint",
        "traversal",
        "topology_join",
        "plane_orientation",
        "cylinder_orientation",
        "theta_winding",
    }:
        face_index, wire_index = next(
            (face_index, wire_index)
            for face_index, face in enumerate(graph.faces)
            for wire_index, wire in enumerate(face.wires)
            if (
                (mutation in {"cylinder_orientation", "theta_winding", "full_endpoint"})
                == (face.kind == "CYLINDER")
            )
            and len(wire.cycle) > 1
        )
        face = graph.faces[face_index]
        wire = face.wires[wire_index]
        changed_face = face
        changed_wire = wire
        if mutation == "wire_schema":
            changed_wire = dataclasses.replace(wire, role="bad")
        elif mutation == "canonical_start":
            changed_wire = dataclasses.replace(wire, cycle=wire.cycle[1:] + wire.cycle[:1])
        elif mutation == "half_edge_schema":
            changed_wire = dataclasses.replace(
                wire, cycle=(dataclasses.replace(wire.cycle[0], direction=0), *wire.cycle[1:])
            )
        elif mutation == "full_endpoint":
            full_index = next(
                index for index, item in enumerate(wire.cycle) if graph.curves[item.curve].full
            )
            item = wire.cycle[full_index]
            vertex = MatchingWireVertex(0, (0.0, 0.0))
            changed_item = dataclasses.replace(item, start=vertex, end=vertex)
            changed_wire = dataclasses.replace(
                wire,
                cycle=tuple(
                    changed_item if index == full_index else candidate
                    for index, candidate in enumerate(wire.cycle)
                ),
            )
        elif mutation == "traversal":
            item = next(item for item in wire.cycle if item.start is not None)
            item_index = wire.cycle.index(item)
            changed_item = dataclasses.replace(item, direction=-item.direction)
            changed_wire = dataclasses.replace(
                wire,
                cycle=tuple(
                    changed_item if index == item_index else candidate
                    for index, candidate in enumerate(wire.cycle)
                ),
            )
        elif mutation == "topology_join":
            item = next(item for item in wire.cycle if item.start is not None)
            item_index = wire.cycle.index(item)
            assert item.start is not None
            changed_start = dataclasses.replace(
                item.start, vertex=(item.start.vertex + 1) % len(graph.vertices)
            )
            changed_item = dataclasses.replace(item, start=changed_start)
            changed_wire = dataclasses.replace(
                wire,
                cycle=tuple(
                    changed_item if index == item_index else candidate
                    for index, candidate in enumerate(wire.cycle)
                ),
            )
        elif mutation in {"plane_orientation", "cylinder_orientation"}:
            changed_face = dataclasses.replace(face, material_side=-face.material_side)
        else:
            changed_wire = dataclasses.replace(wire, theta_winding=wire.theta_winding + 1)
        if changed_wire is not wire:
            changed_face = dataclasses.replace(
                changed_face,
                wires=tuple(
                    changed_wire if index == wire_index else item
                    for index, item in enumerate(face.wires)
                ),
            )
        changed = dataclasses.replace(
            graph,
            faces=tuple(
                changed_face if index == face_index else item
                for index, item in enumerate(graph.faces)
            ),
        )
    else:
        changed = dataclasses.replace(graph, incidence=())

    with pytest.raises(UnsupportedBodyGeometry):
        _body_geometry.validate_matching_boundary_graph(changed, quantization)


@pytest.mark.parametrize("mutation", ["curve", "parameter", "material", "role"])
def test_schema_three_nested_value_mutation_refuses(mutation: str) -> None:
    occurrence = correspondence_snapshot(_take_inventory(_rrp())).occurrences[0]
    graph = occurrence.matching_boundary
    if mutation == "curve":
        curve = graph.curves[0]
        changed = dataclasses.replace(
            graph, curves=(dataclasses.replace(curve, length=math.nan), *graph.curves[1:])
        )
    else:
        face = graph.faces[0]
        if mutation == "parameter":
            face_index, wire_index, half_edge_index = next(
                (face_index, wire_index, half_edge_index)
                for face_index, candidate_face in enumerate(graph.faces)
                for wire_index, candidate_wire in enumerate(candidate_face.wires)
                for half_edge_index, candidate in enumerate(candidate_wire.cycle)
                if candidate.start is not None
            )
            face = graph.faces[face_index]
            wire = face.wires[wire_index]
            half_edge = wire.cycle[half_edge_index]
            assert half_edge.start is not None
            start = dataclasses.replace(half_edge.start, parameter=(math.nan, 0.0))
            changed_half_edge = dataclasses.replace(half_edge, start=start)
            changed_wire = dataclasses.replace(
                wire,
                cycle=tuple(
                    changed_half_edge if index == half_edge_index else item
                    for index, item in enumerate(wire.cycle)
                ),
            )
            changed_face = dataclasses.replace(
                face,
                wires=tuple(
                    changed_wire if index == wire_index else item
                    for index, item in enumerate(face.wires)
                ),
            )
        elif mutation == "material":
            changed_face = dataclasses.replace(face, material_side=0)
        else:
            changed_face = dataclasses.replace(
                face,
                wires=tuple(
                    dataclasses.replace(wire, role="inner") if wire.role == "outer" else wire
                    for wire in face.wires
                ),
            )
        changed = dataclasses.replace(
            graph,
            faces=tuple(changed_face if item is face else item for item in graph.faces),
        )
    with pytest.raises(UnsupportedBodyGeometry, match="matching"):
        _body_geometry.validate_matching_boundary_graph(changed, occurrence.body.quantization)


def test_schema_three_pcurve_reconstruction_refuses_displaced_surface_values(
    monkeypatch,
) -> None:
    face = Box(2, 3, 4).faces()[0]
    edge = face.edges()[0]
    original = _body_geometry.BRepAdaptor_Curve2d(edge.wrapped, face.wrapped)
    first = original.FirstParameter()
    last = original.LastParameter()

    class DisplacedPcurve:
        @staticmethod
        def FirstParameter():
            return first

        @staticmethod
        def LastParameter():
            return last

        @staticmethod
        def Value(_parameter):
            return gp_Pnt2d(1_000.0, 1_000.0)

        @staticmethod
        def D1(_parameter, point, tangent):
            point.SetCoord(1_000.0, 1_000.0)
            tangent.SetCoord(1.0, 0.0)

    monkeypatch.setattr(
        _body_geometry, "BRepAdaptor_Curve2d", lambda _edge, _face: DisplacedPcurve()
    )
    with pytest.raises(UnsupportedBodyGeometry, match="does not reconstruct"):
        _body_geometry._validate_matching_pcurve(
            edge,
            face,
            BRepAdaptor_Curve(edge.wrapped),
            BRepAdaptor_Surface(face.wrapped),
            1e-7,
            "LINE",
            False,
        )


@pytest.mark.parametrize("outside", [False, True])
def test_schema_three_pcurve_reconstruction_bound_is_inclusive(monkeypatch, outside: bool) -> None:
    quantum = 1e-7
    displacement = 2.0 * quantum
    if outside:
        displacement = math.nextafter(displacement, math.inf)

    class Curve:
        @staticmethod
        def FirstParameter():
            return 0.0

        @staticmethod
        def LastParameter():
            return 1.0

        @staticmethod
        def Value(_parameter):
            return gp_Pnt(0.0, 0.0, 0.0)

        @staticmethod
        def D1(_parameter, point, tangent):
            point.SetCoord(0.0, 0.0, 0.0)
            tangent.SetCoord(1.0, 0.0, 0.0)

    class Pcurve(Curve):
        @staticmethod
        def Value(_parameter):
            return gp_Pnt2d(0.0, 0.0)

        @staticmethod
        def D1(_parameter, point, tangent):
            point.SetCoord(0.0, 0.0)
            tangent.SetCoord(1.0, 0.0)

    class Surface:
        @staticmethod
        def Value(_u, _v):
            return gp_Pnt(displacement, 0.0, 0.0)

        @staticmethod
        def D1(_u, _v, point, tangent_u, tangent_v):
            point.SetCoord(displacement, 0.0, 0.0)
            tangent_u.SetCoord(1.0, 0.0, 0.0)
            tangent_v.SetCoord(0.0, 1.0, 0.0)

    monkeypatch.setattr(_body_geometry, "BRepAdaptor_Curve2d", lambda _edge, _face: Pcurve())
    edge = Box(1, 1, 1).edges()[0]
    face = Box(1, 1, 1).faces()[0]
    if outside:
        with pytest.raises(UnsupportedBodyGeometry, match="does not reconstruct"):
            _body_geometry._validate_matching_pcurve(
                edge, face, Curve(), Surface(), quantum, "LINE", False
            )
    else:
        _body_geometry._validate_matching_pcurve(
            edge, face, Curve(), Surface(), quantum, "LINE", False
        )


@pytest.mark.parametrize(
    ("failure", "message"),
    [
        ("unavailable", "unavailable"),
        ("nonfinite", "non-finite"),
        ("unsupported", "unsupported"),
        ("kernel", "reconstruction failed"),
        ("degenerate", "tangent is degenerate"),
        ("angular", "angular bound"),
    ],
)
def test_schema_three_pcurve_closed_refusal_roster(monkeypatch, failure: str, message: str) -> None:
    class Curve:
        @staticmethod
        def FirstParameter():
            return math.nan if failure == "nonfinite" else 0.0

        @staticmethod
        def LastParameter():
            return 1.0

        @staticmethod
        def D1(_parameter, point, tangent):
            if failure == "kernel":
                raise Standard_Failure("closed kernel failure")
            point.SetCoord(0.0, 0.0, 0.0)
            tangent.SetCoord(0.0, 0.0, 0.0) if failure == "degenerate" else tangent.SetCoord(
                1.0, 0.0, 0.0
            )

    class Pcurve:
        @staticmethod
        def FirstParameter():
            return 0.0

        @staticmethod
        def LastParameter():
            return 1.0

        @staticmethod
        def D1(_parameter, point, tangent):
            point.SetCoord(0.0, 0.0)
            tangent.SetCoord(1.0, 0.0)

    class Surface:
        @staticmethod
        def D1(_u, _v, point, tangent_u, tangent_v):
            point.SetCoord(0.0, 0.0, 0.0)
            if failure == "angular":
                tangent_u.SetCoord(0.0, 1.0, 0.0)
            else:
                tangent_u.SetCoord(1.0, 0.0, 0.0)
            tangent_v.SetCoord(0.0, 1.0, 0.0)

    if failure == "unavailable":

        def unavailable(_edge, _face):
            raise Standard_Failure("closed adaptor failure")

        monkeypatch.setattr(_body_geometry, "BRepAdaptor_Curve2d", unavailable)
    else:
        monkeypatch.setattr(_body_geometry, "BRepAdaptor_Curve2d", lambda _edge, _face: Pcurve())
    edge = Box(1, 1, 1).edges()[0]
    face = Box(1, 1, 1).faces()[0]
    with pytest.raises(UnsupportedBodyGeometry, match=message):
        _body_geometry._validate_matching_pcurve(
            edge,
            face,
            Curve(),
            Surface(),
            1e-7,
            "BSPLINE" if failure == "unsupported" else "LINE",
            False,
        )


def test_schema_three_construction_budget_is_inclusive() -> None:
    budget = _body_geometry._MatchingConstructionBudget(
        _body_geometry.CANONICAL_SERIALIZATION_BUDGET - 1
    )
    budget.charge()
    assert budget.attempts == _body_geometry.CANONICAL_SERIALIZATION_BUDGET
    with pytest.raises(UnsupportedBodyGeometry, match="construction budget"):
        budget.charge()


def test_schema_three_planar_cycle_uses_the_global_inclusive_budget() -> None:
    curve = MatchingCurve(
        "CIRCLE",
        None,
        2.0 * math.pi,
        (0.0, 0.0, 0.0),
        (0.0, 0.0, 1.0),
        1.0,
        2.0 * math.pi,
        True,
    )
    face = FaceGeometry(
        "PLANE",
        (0.0, 0.0, 1.0, 0.0),
        math.pi,
        (0.0, 0.0, 0.0),
        1,
        (),
    )
    accepted = _body_geometry._MatchingConstructionBudget(
        _body_geometry.CANONICAL_SERIALIZATION_BUDGET - 2
    )
    _body_geometry._planar_cycle((0,), (curve,), "outer", face, 1e-7, (), accepted)
    assert accepted.attempts == _body_geometry.CANONICAL_SERIALIZATION_BUDGET

    refused = _body_geometry._MatchingConstructionBudget(
        _body_geometry.CANONICAL_SERIALIZATION_BUDGET - 1
    )
    with pytest.raises(UnsupportedBodyGeometry, match="construction budget"):
        _body_geometry._planar_cycle((0,), (curve,), "outer", face, 1e-7, (), refused)


def test_schema_three_matching_leaf_refusal_roster(monkeypatch) -> None:
    quantum = 1e-7
    vertices = (
        (0.0, 0.0, 0.0),
        (1.0, 0.0, 0.0),
        (2.0, 0.0, 0.0),
    )
    line = MatchingCurve("LINE", (0, 1), 1.0, None, None, None, None, False)
    plane = FaceGeometry("PLANE", (0.0, 0.0, 1.0, 0.0), 1.0, (0.0, 0.0, 0.0), 1, ())
    budget = _body_geometry._MatchingConstructionBudget()

    with pytest.raises(UnsupportedBodyGeometry, match="normal"):
        _body_geometry._plane_parameter(
            (0.0, 0.0, 0.0), dataclasses.replace(plane, parameters=()), quantum
        )
    with pytest.raises(UnsupportedBodyGeometry, match="no endpoints"):
        _body_geometry._half_edge_integral(
            MatchingHalfEdge(0, 1, None, None), (line,), plane, quantum
        )
    malformed_circle = MatchingCurve("CIRCLE", (0, 1), 1.0, None, None, 1.0, 1.0, False)
    with pytest.raises(UnsupportedBodyGeometry, match="malformed"):
        _body_geometry._half_edge_integral(
            MatchingHalfEdge(
                0,
                1,
                MatchingWireVertex(0, (0.0, 0.0)),
                MatchingWireVertex(1, (1.0, 0.0)),
            ),
            (malformed_circle,),
            plane,
            quantum,
        )
    off_axis_circle = dataclasses.replace(
        malformed_circle, centre=(0.0, 0.0, 0.0), axis=(1.0, 0.0, 0.0)
    )
    with pytest.raises(UnsupportedBodyGeometry, match="face-normal"):
        _body_geometry._half_edge_integral(
            MatchingHalfEdge(
                0,
                1,
                MatchingWireVertex(0, (0.0, 0.0)),
                MatchingWireVertex(1, (1.0, 0.0)),
            ),
            (off_axis_circle,),
            plane,
            quantum,
        )
    circle = dataclasses.replace(malformed_circle, centre=(0.0, 0.0, 0.0), axis=(0.0, 0.0, 1.0))
    with pytest.raises(UnsupportedBodyGeometry, match="no endpoints"):
        _body_geometry._half_edge_integral(
            MatchingHalfEdge(0, 1, None, None), (circle,), plane, quantum
        )
    with pytest.raises(UnsupportedBodyGeometry, match="reconstruct"):
        _body_geometry._half_edge_integral(
            MatchingHalfEdge(
                0,
                1,
                MatchingWireVertex(0, (1.0, 0.0)),
                MatchingWireVertex(1, (-10.0, 0.0)),
            ),
            (circle,),
            plane,
            quantum,
        )
    with pytest.raises(UnsupportedBodyGeometry, match="complete wire"):
        _body_geometry._planar_cycle(
            (0, 1),
            (
                dataclasses.replace(circle, full=True, vertices=None, sweep=2.0 * math.pi),
                line,
            ),
            "outer",
            plane,
            quantum,
            vertices,
            budget,
        )
    ambiguous_full = dataclasses.replace(circle, full=True, vertices=None, sweep=0.0)
    with pytest.raises(UnsupportedBodyGeometry, match="orientation is ambiguous"):
        _body_geometry._planar_cycle(
            (0,),
            (ambiguous_full,),
            "outer",
            plane,
            quantum,
            vertices,
            budget,
        )
    with pytest.raises(UnsupportedBodyGeometry, match="malformed"):
        _body_geometry._planar_cycle(
            (0,),
            (dataclasses.replace(line, vertices=None),),
            "outer",
            plane,
            quantum,
            vertices,
            budget,
        )
    with pytest.raises(UnsupportedBodyGeometry, match="degree-two"):
        _body_geometry._planar_cycle((0,), (line,), "outer", plane, quantum, vertices, budget)
    collinear = (
        line,
        MatchingCurve("LINE", (1, 2), 1.0, None, None, None, None, False),
        MatchingCurve("LINE", (2, 0), 2.0, None, None, None, None, False),
    )
    with pytest.raises(UnsupportedBodyGeometry, match="area"):
        _body_geometry._planar_cycle(
            (0, 1, 2), collinear, "outer", plane, quantum, vertices, budget
        )

    first = Edge.make_line((0, 0, 0), (1, 0, 0))
    second = first.reversed()
    items = [first]
    assert _body_geometry._same_shape(first, second)
    assert _body_geometry._identity_index(items, second) == 0
    third = Edge.make_line((0, 1, 0), (1, 1, 0))
    assert _body_geometry._identity_index(items, third) == 1

    disconnected_vertices = (
        (0.0, 0.0, 0.0),
        (1.0, 0.0, 0.0),
        (0.0, 1.0, 0.0),
        (3.0, 0.0, 0.0),
        (4.0, 0.0, 0.0),
        (3.0, 1.0, 0.0),
    )
    disconnected = tuple(
        MatchingCurve("LINE", edge, 1.0, None, None, None, None, False)
        for edge in ((0, 1), (1, 2), (2, 0), (3, 4), (4, 5), (5, 3))
    )
    with pytest.raises(UnsupportedBodyGeometry, match="does not close"):
        _body_geometry._planar_cycle(
            tuple(range(6)),
            disconnected,
            "outer",
            plane,
            quantum,
            disconnected_vertices,
            budget,
        )

    triangle_vertices = disconnected_vertices[:3]
    triangle = disconnected[:3]
    monkeypatch.setattr(_body_geometry, "_half_edge_integral", lambda *_args: 1.0)
    with pytest.raises(UnsupportedBodyGeometry, match="material-oriented"):
        _body_geometry._planar_cycle(
            (0, 1, 2),
            triangle,
            "outer",
            dataclasses.replace(plane, material_side=-1),
            quantum,
            triangle_vertices,
            budget,
        )


def test_schema_three_cylinder_cycle_refusal_roster() -> None:
    quantum = 1e-7
    curves = tuple(
        MatchingCurve("LINE", (index, (index + 1) % 4), 1.0, None, None, None, None, False)
        for index in range(4)
    )
    parameters = ((0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0))
    occurrences = tuple(
        _body_geometry._CylinderPcurveOccurrence(
            index,
            index,
            (index + 1) % 4,
            parameters[index],
            parameters[(index + 1) % 4],
        )
        for index in range(4)
    )
    face = FaceGeometry(
        "CYLINDER",
        (0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0),
        1.0,
        (0.0, 0.0, 0.0),
        1,
        (),
    )
    assert _body_geometry._cylinder_cycle(
        (occurrences,),
        curves,
        "outer",
        face,
        quantum,
        _body_geometry._MatchingConstructionBudget(),
    ).cycle

    with pytest.raises(UnsupportedBodyGeometry, match="empty"):
        _body_geometry._cylinder_cycle(
            (), curves, "outer", face, quantum, _body_geometry._MatchingConstructionBudget()
        )
    with pytest.raises(UnsupportedBodyGeometry, match="wire is empty"):
        _body_geometry._cylinder_cycle_assignment(
            (),
            curves,
            "outer",
            face,
            quantum,
            _body_geometry._MatchingConstructionBudget(),
        )

    full = MatchingCurve(
        "CIRCLE",
        None,
        2.0 * math.pi,
        (0.0, 0.0, 0.0),
        (0.0, 0.0, 1.0),
        1.0,
        2.0 * math.pi,
        True,
    )
    with pytest.raises(UnsupportedBodyGeometry, match="seam vertex"):
        _body_geometry._cylinder_cycle(
            (
                (
                    _body_geometry._CylinderPcurveOccurrence(
                        0, 0, 0, (0.0, 0.0), (2.0 * math.pi, 0.0)
                    ),
                ),
            ),
            (full,),
            "outer",
            face,
            quantum,
            _body_geometry._MatchingConstructionBudget(),
        )

    with pytest.raises(UnsupportedBodyGeometry, match="lost a topology vertex"):
        _body_geometry._cylinder_cycle(
            ((_body_geometry._CylinderPcurveOccurrence(0, None, None, (0.0, 0.0), (1.0, 0.0)),),),
            (curves[0],),
            "outer",
            face,
            quantum,
            _body_geometry._MatchingConstructionBudget(),
        )

    with pytest.raises(UnsupportedBodyGeometry, match="global curve"):
        _body_geometry._cylinder_cycle(
            ((_body_geometry._CylinderPcurveOccurrence(0, 2, 3, (0.0, 0.0), (1.0, 0.0)),),),
            (curves[0],),
            "outer",
            face,
            quantum,
            _body_geometry._MatchingConstructionBudget(),
        )

    broken = dataclasses.replace(
        occurrences[1],
        start_parameter=(occurrences[1].start_parameter[0], 10.0),
        end_parameter=(occurrences[1].end_parameter[0], 10.0),
    )
    with pytest.raises(UnsupportedBodyGeometry, match="unique material-oriented"):
        _body_geometry._cylinder_cycle(
            ((occurrences[0], broken, *occurrences[2:]),),
            curves,
            "outer",
            face,
            quantum,
            _body_geometry._MatchingConstructionBudget(),
        )

    flat = tuple(
        dataclasses.replace(
            occurrence,
            start_parameter=(occurrence.start_parameter[0], 0.0),
            end_parameter=(occurrence.end_parameter[0], 0.0),
        )
        for occurrence in occurrences
    )
    with pytest.raises(UnsupportedBodyGeometry, match="unique material-oriented"):
        _body_geometry._cylinder_cycle(
            (flat,),
            curves,
            "outer",
            face,
            quantum,
            _body_geometry._MatchingConstructionBudget(),
        )


def test_schema_three_cylinder_pcurve_roster_is_complete_and_order_neutral(
    monkeypatch,
) -> None:
    part = Cylinder(10, 20)
    baseline_query = FaceGraph(part)
    baseline_solid = baseline_query.common_valid_solid(baseline_query.nodes)
    assert baseline_solid is not None
    baseline = baseline_query.matching_boundary(baseline_solid)
    original = _body_geometry._cylinder_pcurve_variants

    def reversed_variants(*args, **kwargs):
        return tuple(reversed(original(*args, **kwargs)))

    monkeypatch.setattr(_body_geometry, "_cylinder_pcurve_variants", reversed_variants)
    reversed_query = FaceGraph(part)
    reversed_solid = reversed_query.common_valid_solid(reversed_query.nodes)
    assert reversed_solid is not None
    assert reversed_query.matching_boundary(reversed_solid) == baseline

    monkeypatch.setattr(_body_geometry, "_cylinder_pcurve_variants", lambda *_args, **_kwargs: ())
    refused_query = FaceGraph(part)
    refused_solid = refused_query.common_valid_solid(refused_query.nodes)
    assert refused_solid is not None
    with pytest.raises(UnsupportedBodyGeometry, match="roster cardinality"):
        refused_query.matching_boundary(refused_solid)
    monkeypatch.setattr(_body_geometry, "_cylinder_pcurve_variants", original)
    assert refused_query.matching_boundary(refused_solid) == baseline


def test_schema_three_competing_cylinder_homologies_refuse(monkeypatch) -> None:
    occurrence = _body_geometry._CylinderPcurveOccurrence(0, 0, 1, (0.0, 0.0), (1.0, 0.0))
    first_assignment = (occurrence,)
    second_assignment = (dataclasses.replace(occurrence, curve=1),)
    vertex = MatchingWireVertex(0, (0.0, 0.0))
    zero = MatchingWire("outer", 0, (MatchingHalfEdge(0, 1, vertex, vertex),))
    primitive = MatchingWire("outer", 1, (MatchingHalfEdge(1, 1, vertex, vertex),))

    def competing(assignment, *_args):
        return {zero if assignment == first_assignment else primitive}

    monkeypatch.setattr(_body_geometry, "_cylinder_cycle_assignment", competing)
    face = FaceGeometry(
        "CYLINDER",
        (0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0),
        1.0,
        (0.0, 0.0, 0.0),
        1,
        (),
    )
    with pytest.raises(UnsupportedBodyGeometry, match="unique material-oriented"):
        _body_geometry._cylinder_cycle(
            (first_assignment, second_assignment),
            (),
            "outer",
            face,
            1e-7,
            _body_geometry._MatchingConstructionBudget(),
        )


@pytest.mark.parametrize("outside", [False, True])
def test_schema_three_cylinder_join_bound_is_inclusive(outside: bool) -> None:
    quantum = 1e-7
    curves = tuple(
        MatchingCurve("LINE", (index, (index + 1) % 4), 1.0, None, None, None, None, False)
        for index in range(4)
    )
    parameters = ((0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0))
    occurrences = tuple(
        _body_geometry._CylinderPcurveOccurrence(
            index,
            index,
            (index + 1) % 4,
            parameters[index],
            parameters[(index + 1) % 4],
        )
        for index in range(4)
    )
    residual = 4.0 * quantum
    if outside:
        residual = math.nextafter(residual, math.inf)
    changed = dataclasses.replace(occurrences[1], start_parameter=(1.0, residual))
    face = FaceGeometry(
        "CYLINDER",
        (0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0),
        1.0,
        (0.0, 0.0, 0.0),
        1,
        (),
    )
    if outside:
        with pytest.raises(UnsupportedBodyGeometry, match="unique material-oriented"):
            _body_geometry._cylinder_cycle(
                ((occurrences[0], changed, *occurrences[2:]),),
                curves,
                "outer",
                face,
                quantum,
                _body_geometry._MatchingConstructionBudget(),
            )
    else:
        assert _body_geometry._cylinder_cycle(
            ((occurrences[0], changed, *occurrences[2:]),),
            curves,
            "outer",
            face,
            quantum,
            _body_geometry._MatchingConstructionBudget(),
        ).cycle


@pytest.mark.parametrize(
    ("shape", "mutation", "message"),
    [
        ("cylinder", "circle_gauge", "circle gauge"),
        ("box", "face_gauge", "analytic gauge"),
        ("box", "empty_wire", "wire schema"),
        ("box", "planar_join", "cycle no longer joins"),
        ("box", "planar_parameter", "parameter changed"),
        ("cylinder", "circle_length", "reconstructs its length"),
        ("notched", "circle_vertex", "reconstructs its vertices"),
        ("cylinder", "cylinder_parameter", "parameter changed"),
    ],
)
def test_schema_three_semantic_validator_specific_refusals(
    shape: str, mutation: str, message: str
) -> None:
    part = Box(4, 5, 6) if shape == "box" else Cylinder(10, 20) if shape == "cylinder" else _rrp(7)
    query = FaceGraph(part)
    solid = query.common_valid_solid(query.nodes)
    assert solid is not None
    fact = query.body_geometry(solid)
    graph = query.matching_boundary(solid)

    if mutation.startswith("circle"):
        curve_index = next(
            index for index, curve in enumerate(graph.curves) if curve.kind == "CIRCLE"
        )
        curve = graph.curves[curve_index]
        if mutation == "circle_length":
            changed_curve = dataclasses.replace(curve, length=curve.length + 1.0)
        elif mutation == "circle_gauge":
            changed_curve = dataclasses.replace(curve, radius=1e-12)
        else:
            assert curve.centre is not None
            changed_curve = dataclasses.replace(
                curve, centre=(curve.centre[0] + 1.0, *curve.centre[1:])
            )
        changed = dataclasses.replace(
            graph,
            curves=tuple(
                changed_curve if index == curve_index else item
                for index, item in enumerate(graph.curves)
            ),
        )
    else:
        face_index, wire_index = next(
            (face_index, wire_index)
            for face_index, face in enumerate(graph.faces)
            for wire_index, wire in enumerate(face.wires)
            if face.kind == ("CYLINDER" if mutation == "cylinder_parameter" else "PLANE")
            and wire.cycle
            and any(item.start is not None for item in wire.cycle)
        )
        face = graph.faces[face_index]
        wire = face.wires[wire_index]
        changed_face = face
        if mutation == "face_gauge":
            changed_face = dataclasses.replace(
                face, parameters=(1.0, 0.5e-10, 0.0, face.parameters[3])
            )
        elif mutation == "empty_wire":
            changed_face = dataclasses.replace(
                face,
                wires=tuple(
                    dataclasses.replace(item, cycle=()) if index == wire_index else item
                    for index, item in enumerate(face.wires)
                ),
            )
        else:
            target = next(item for item in wire.cycle if item.start is not None)
            assert target.start is not None
            target_vertex = target.start.vertex

            def alter(item: MatchingHalfEdge) -> MatchingHalfEdge:
                start = item.start
                end = item.end
                if start is not None and start.vertex == target_vertex:
                    start = dataclasses.replace(
                        start, parameter=(start.parameter[0] + 1.0, start.parameter[1])
                    )
                if mutation != "planar_join" and end is not None and end.vertex == target_vertex:
                    end = dataclasses.replace(
                        end, parameter=(end.parameter[0] + 1.0, end.parameter[1])
                    )
                return dataclasses.replace(item, start=start, end=end)

            changed_wire = dataclasses.replace(wire, cycle=tuple(map(alter, wire.cycle)))
            changed_face = dataclasses.replace(
                face,
                wires=tuple(
                    changed_wire if index == wire_index else item
                    for index, item in enumerate(face.wires)
                ),
            )
        changed = dataclasses.replace(
            graph,
            faces=tuple(
                changed_face if index == face_index else item
                for index, item in enumerate(graph.faces)
            ),
        )

    with pytest.raises(UnsupportedBodyGeometry, match=message):
        _body_geometry.validate_matching_boundary_graph(changed, fact.descriptor.quantization)


def test_schema_three_cached_face_authority_refusal_roster() -> None:
    part = Box(4, 5, 6).solids()[0]
    described = _body_geometry.describe_solid(part)
    with pytest.raises(UnsupportedBodyGeometry, match="cached face authority"):
        matching_boundary_for_solid(
            part, described.descriptor, (*described.face_builds[:-1], object())
        )

    groups: list[tuple[object, list[tuple[int, int, int]]]] = []
    for face_index, face_build in enumerate(described.face_builds):
        for wire_index, wire_build in enumerate(face_build.wires):
            for edge_index, (token, _direction) in enumerate(wire_build.occurrences[0]):
                group = next(
                    (
                        positions
                        for representative, positions in groups
                        if _body_geometry._same_shape(representative, token)
                    ),
                    None,
                )
                if group is None:
                    group = []
                    groups.append((token, group))
                group.append((face_index, wire_index, edge_index))
    shared = next(positions for _token, positions in groups if len(positions) == 2)
    face_index, wire_index, edge_index = shared[1]
    face_build = described.face_builds[face_index]
    wire_build = face_build.wires[wire_index]
    geometry_edges = list(wire_build.geometry.edges)
    geometry, direction = geometry_edges[edge_index]
    geometry_edges[edge_index] = (
        dataclasses.replace(geometry, length=geometry.length + 1.0),
        direction,
    )
    changed_wire = dataclasses.replace(
        wire_build,
        geometry=dataclasses.replace(wire_build.geometry, edges=tuple(geometry_edges)),
    )
    changed_face = dataclasses.replace(
        face_build,
        wires=tuple(
            changed_wire if index == wire_index else item
            for index, item in enumerate(face_build.wires)
        ),
    )
    changed_builds = tuple(
        changed_face if index == face_index else item
        for index, item in enumerate(described.face_builds)
    )
    with pytest.raises(UnsupportedBodyGeometry, match="curve authority disagrees"):
        matching_boundary_for_solid(part, described.descriptor, changed_builds)

    altered_builds = list(described.face_builds)
    first_face, first_wire, first_edge = shared[0]
    source_geometry = (
        described.face_builds[first_face].wires[first_wire].geometry.edges[first_edge][0]
    )
    assert source_geometry.start is not None
    altered_geometry = dataclasses.replace(
        source_geometry,
        start=(source_geometry.start[0] + 1.0, *source_geometry.start[1:]),
    )
    for target_face, target_wire, target_edge in shared:
        target_build = altered_builds[target_face]
        target_wire_build = target_build.wires[target_wire]
        target_edges = list(target_wire_build.geometry.edges)
        _old_geometry, semantic = target_edges[target_edge]
        target_edges[target_edge] = (altered_geometry, semantic)
        replacement_wire = dataclasses.replace(
            target_wire_build,
            geometry=dataclasses.replace(target_wire_build.geometry, edges=tuple(target_edges)),
        )
        altered_builds[target_face] = dataclasses.replace(
            target_build,
            wires=tuple(
                replacement_wire if index == target_wire else item
                for index, item in enumerate(target_build.wires)
            ),
        )
    with pytest.raises(UnsupportedBodyGeometry, match="endpoints disagree"):
        matching_boundary_for_solid(part, described.descriptor, tuple(altered_builds))

    empty_wire = dataclasses.replace(wire_build, occurrences=())
    empty_face = dataclasses.replace(
        face_build,
        wires=tuple(
            empty_wire if index == wire_index else item
            for index, item in enumerate(face_build.wires)
        ),
    )
    empty_builds = tuple(
        empty_face if index == face_index else item
        for index, item in enumerate(described.face_builds)
    )
    with pytest.raises(UnsupportedBodyGeometry, match="wire authority is empty"):
        matching_boundary_for_solid(part, described.descriptor, empty_builds)

    graph = matching_boundary_for_solid(part, described.descriptor, described.face_builds)
    with pytest.raises(UnsupportedBodyGeometry, match="closed-shell pair"):
        _body_geometry._matching_graph_canonical(
            graph.vertices,
            graph.curves,
            graph.faces[:-1],
            _body_geometry._MatchingConstructionBudget(),
        )


def test_schema_three_cached_matching_roster_refuses_deep_drift(monkeypatch) -> None:
    part = Box(4, 5, 6).solids()[0]
    described = _body_geometry.describe_solid(part)

    first_build = described.face_builds[0]
    first_wire = first_build.wires[0]
    alignment = first_wire.occurrences[0]
    conflicting_alignment = ((object(), alignment[0][1]), *alignment[1:])
    conflicting_wire = dataclasses.replace(
        first_wire, occurrences=(alignment, conflicting_alignment)
    )
    conflicting_build = dataclasses.replace(
        first_build,
        wires=(conflicting_wire, *first_build.wires[1:]),
    )
    with pytest.raises(UnsupportedBodyGeometry, match="token roster disagrees"):
        matching_boundary_for_solid(
            part,
            described.descriptor,
            (conflicting_build, *described.face_builds[1:]),
        )

    original_faces = Solid.faces
    monkeypatch.setattr(Solid, "faces", lambda self: original_faces(self)[:-1])
    with pytest.raises(UnsupportedBodyGeometry, match="closed-shell pair"):
        matching_boundary_for_solid(part, described.descriptor, described.face_builds[:-1])


def test_schema_three_validator_refuses_a_reordered_disconnected_wire() -> None:
    graph, solid, fact = _body_descriptor(Box(4, 5, 6))
    matching = graph.matching_boundary(solid)
    face_index = next(
        index
        for index, face in enumerate(matching.faces)
        if face.kind == "PLANE" and len(face.wires[0].cycle) >= 4
    )
    face = matching.faces[face_index]
    wire = face.wires[0]
    cycle = wire.cycle
    reordered = dataclasses.replace(
        wire,
        cycle=(cycle[0], cycle[2], cycle[1], *cycle[3:]),
    )
    changed_face = dataclasses.replace(
        face,
        wires=(reordered, *face.wires[1:]),
    )
    changed = dataclasses.replace(
        matching,
        faces=tuple(
            changed_face if index == face_index else item
            for index, item in enumerate(matching.faces)
        ),
    )
    with pytest.raises(UnsupportedBodyGeometry, match="topology no longer joins"):
        _body_geometry.validate_matching_boundary_graph(changed, fact.descriptor.quantization)


def test_schema_three_validator_refuses_a_disconnected_planar_pcurve() -> None:
    graph, solid, fact = _body_descriptor(Box(4, 5, 6))
    matching = graph.matching_boundary(solid)
    face_index = next(
        index
        for index, face in enumerate(matching.faces)
        if face.kind == "PLANE" and len(face.wires[0].cycle) >= 4
    )
    face = matching.faces[face_index]
    wire = face.wires[0]
    half_edge = wire.cycle[1]
    assert half_edge.start is not None
    changed_start = dataclasses.replace(
        half_edge.start,
        parameter=(
            half_edge.start.parameter[0] + 5.0 * fact.descriptor.quantization.metric_quantum,
            half_edge.start.parameter[1],
        ),
    )
    changed_half_edge = dataclasses.replace(half_edge, start=changed_start)
    changed_wire = dataclasses.replace(
        wire,
        cycle=(wire.cycle[0], changed_half_edge, *wire.cycle[2:]),
    )
    changed_face = dataclasses.replace(
        face,
        wires=(changed_wire, *face.wires[1:]),
    )
    changed = dataclasses.replace(
        matching,
        faces=tuple(
            changed_face if index == face_index else item
            for index, item in enumerate(matching.faces)
        ),
    )
    with pytest.raises(UnsupportedBodyGeometry, match="pcurve cycle no longer joins"):
        _body_geometry.validate_matching_boundary_graph(changed, fact.descriptor.quantization)


def test_schema_three_cylinder_pcurve_roster_kernel_and_closure_refusals(
    monkeypatch,
) -> None:
    part = Cylinder(10, 20).solids()[0]
    described = _body_geometry.describe_solid(part)
    raw_faces = tuple(part.faces())
    at = next(
        index
        for index, face in enumerate(raw_faces)
        if BRepAdaptor_Surface(face.wrapped).GetType().name == "GeomAbs_Cylinder"
    )
    face = raw_faces[at]
    face_build = described.face_builds[at]
    adaptor = BRepAdaptor_Surface(face.wrapped)
    gauge = _body_geometry._cylinder_gauge(
        adaptor, face_build.geometry, described.descriptor.placement.centre_of_mass
    )
    edge = next(edge for edge in face.edges() if BRep_Tool.IsClosed_s(edge.wrapped, face.wrapped))
    label = next(
        geometry
        for wire in face_build.wires
        for geometry, _direction in wire.geometry.edges
        if geometry.kind == "LINE"
    )
    monkeypatch.setattr(_body_geometry, "_validate_matching_pcurve", lambda *_args: None)

    def unavailable(_edge, _face):
        raise Standard_Failure("closed pcurve failure")

    monkeypatch.setattr(_body_geometry, "BRepAdaptor_Curve2d", unavailable)
    with pytest.raises(UnsupportedBodyGeometry, match="roster is unavailable"):
        _body_geometry._cylinder_pcurve_variants(
            edge,
            face,
            adaptor,
            described.descriptor.placement.centre_of_mass,
            described.descriptor.quantization.metric_quantum,
            gauge,
            label.kind,
            label.full,
            _body_geometry._MatchingConstructionBudget(),
        )

    class OnePcurve:
        @staticmethod
        def FirstParameter():
            return 0.0

        @staticmethod
        def LastParameter():
            return 1.0

        @staticmethod
        def Value(parameter):
            return gp_Pnt2d(parameter, 0.0)

    monkeypatch.setattr(_body_geometry, "BRepAdaptor_Curve2d", lambda _edge, _face: OnePcurve())
    with pytest.raises(UnsupportedBodyGeometry, match="closure roster is incomplete"):
        _body_geometry._cylinder_pcurve_variants(
            edge,
            face,
            adaptor,
            described.descriptor.placement.centre_of_mass,
            described.descriptor.quantization.metric_quantum,
            gauge,
            label.kind,
            label.full,
            _body_geometry._MatchingConstructionBudget(),
        )


def test_schema_three_kernel_failure_mapping_is_narrow(monkeypatch) -> None:
    solid = Box(1, 1, 1).solids()[0]

    def mass_failure(*_args):
        raise Standard_Failure("mass failure")

    monkeypatch.setattr(_body_geometry.BRepGProp, "VolumeProperties_s", mass_failure)
    with pytest.raises(UnsupportedBodyGeometry, match="mass properties"):
        _body_geometry.describe_solid(solid)

    monkeypatch.undo()

    def boundary_failure(*_args):
        raise Standard_Failure("boundary failure")

    monkeypatch.setattr(_body_geometry, "_face_geometry", boundary_failure)
    with pytest.raises(UnsupportedBodyGeometry, match="body boundary"):
        _body_geometry.describe_solid(solid)


def test_graph_matching_boundary_revalidates_every_authority_layer(monkeypatch) -> None:
    graph, solid, fact = _body_descriptor(Box(4, 5, 6))
    copied = copy.copy(solid)
    assert copied is not solid
    with pytest.raises(BodyGeometryAuthorityError, match="graph-authorized"):
        graph.matching_boundary(copied)

    object.__setattr__(fact, "_solid", copied)
    with pytest.raises(BodyGeometryAuthorityError, match="lost its graph-issued solid"):
        graph.matching_boundary(solid)
    object.__setattr__(fact, "_solid", solid)

    monkeypatch.setattr(FaceGraph, "node_of", lambda _self, _face: None)
    with pytest.raises(BodyGeometryAuthorityError, match="face is not graph-owned"):
        graph.matching_boundary(solid)


def test_correspondence_closed_authority_failure_roster(monkeypatch) -> None:
    monkeypatch.setattr(
        correspondence_module.pickle,
        "dumps",
        lambda *_args, **_kwargs: (_ for _ in ()).throw(TypeError("not picklable")),
    )
    with pytest.raises(CorrespondenceSnapshotError, match="cannot be frozen"):
        correspondence_module._authority_value(object())

    monkeypatch.undo()
    product = _take_inventory(_rrp())
    candidate = product.physical.candidate_set(FamilyId.REPEATING_RADIAL_PROFILES).candidates[0]
    object.__setattr__(candidate, "record", object())
    with pytest.raises(CorrespondenceSnapshotError, match="wrong record type"):
        correspondence_module._occurrence(product.context.graph, product.evidence, candidate)


def test_cached_correspondence_pickle_failure_is_closed(monkeypatch) -> None:
    product = _take_inventory(_rrp())
    correspondence_snapshot(product)

    def fail(*_args, **_kwargs):
        raise TypeError("not picklable")

    monkeypatch.setattr(correspondence_module.pickle, "dumps", fail)
    with pytest.raises(CorrespondenceSnapshotError, match="changed after issue"):
        correspondence_snapshot(product)


def test_schema_three_complete_topology_budget_preflight_refuses(monkeypatch) -> None:
    edge = _body_geometry.EdgeGeometry("LINE", (0.0, 0.0, 0.0), (1.0, 0.0, 0.0), 1.0)
    wire = _body_geometry.WireGeometry("outer", 1, ((edge, 1), (edge, -1)))

    def build(area: float):
        alignments = tuple(((object(), 1), (object(), -1)) for _index in range(2))
        wire_build = _body_geometry._WireBuild(wire, alignments)
        face = FaceGeometry("PLANE", (), area, (0.0, 0.0, 0.0), 1, (wire,))
        return _body_geometry._FaceBuild(face, (wire_build,))

    monkeypatch.setattr(_body_geometry, "CANONICAL_SERIALIZATION_BUDGET", 3)
    with pytest.raises(UnsupportedBodyGeometry, match="budget"):
        _body_geometry._canonical_topology((build(1.0), build(2.0)))


def test_schema_three_empty_canonical_search_refuses(monkeypatch) -> None:
    monkeypatch.setattr(_body_geometry, "_matching_label_orders", lambda *_args: ())
    with pytest.raises(UnsupportedBodyGeometry, match="no canonical serialization"):
        _body_geometry._matching_graph_canonical(
            (), (), (), _body_geometry._MatchingConstructionBudget()
        )


def test_schema_three_cylinder_empty_wire_refuses(monkeypatch) -> None:
    part = Cylinder(10, 20).solids()[0]
    described = _body_geometry.describe_solid(part)
    monkeypatch.setattr(Wire, "edges", lambda _self: [])
    with pytest.raises(UnsupportedBodyGeometry, match="cylinder wire is empty"):
        matching_boundary_for_solid(part, described.descriptor, described.face_builds)


@pytest.mark.parametrize("failure", ["null", "foreign"])
def test_schema_three_cylinder_vertex_authority_refuses(monkeypatch, failure: str) -> None:
    part = Cylinder(10, 20).solids()[0]
    described = _body_geometry.describe_solid(part)
    cylinder = next(
        face
        for face in part.faces()
        if BRepAdaptor_Surface(face.wrapped).GetType().name == "GeomAbs_Cylinder"
    )
    seam = next(
        edge
        for edge in cylinder.edges()
        if BRepAdaptor_Curve(edge.wrapped).GetType().name == "GeomAbs_Line"
    )
    original = TopExp.FirstVertex_s
    foreign = original(Box(1, 1, 1).edges()[0].wrapped, False)
    calls = 0

    def first(edge, oriented):
        nonlocal calls
        if edge.IsSame(seam.wrapped):
            calls += 1
            if calls == 2:
                return TopoDS_Vertex() if failure == "null" else foreign
        return original(edge, oriented)

    monkeypatch.setattr(_body_geometry.TopExp, "FirstVertex_s", first)
    message = "lost a topology vertex" if failure == "null" else "outside its global curve roster"
    with pytest.raises(UnsupportedBodyGeometry, match=message):
        matching_boundary_for_solid(part, described.descriptor, described.face_builds)


def test_schema_three_curve_grammar_and_vertex_cardinality_refuse(monkeypatch) -> None:
    part = Box(4, 5, 6).solids()[0]
    described = _body_geometry.describe_solid(part)
    target = described.face_builds[0].wires[0].occurrences[0][0][0]

    changed_builds = []
    for build in described.face_builds:
        changed_wires = []
        for wire in build.wires:
            changed_edges = tuple(
                (
                    dataclasses.replace(geometry, kind="BSPLINE")
                    if _body_geometry._same_shape(token, target)
                    else geometry,
                    semantic,
                )
                for (geometry, semantic), (token, _direction) in zip(
                    wire.geometry.edges, wire.occurrences[0], strict=True
                )
            )
            changed_wires.append(
                dataclasses.replace(
                    wire,
                    geometry=dataclasses.replace(wire.geometry, edges=changed_edges),
                )
            )
        changed_builds.append(dataclasses.replace(build, wires=tuple(changed_wires)))
    monkeypatch.setattr(_body_geometry, "_validate_matching_pcurve", lambda *_args: None)
    with pytest.raises(UnsupportedBodyGeometry, match="curve grammar"):
        matching_boundary_for_solid(part, described.descriptor, tuple(changed_builds))

    monkeypatch.undo()
    original_last = TopExp.LastVertex_s

    def last(edge, oriented):
        if edge.IsSame(target.wrapped):
            return TopoDS_Vertex()
        return original_last(edge, oriented)

    monkeypatch.setattr(_body_geometry.TopExp, "LastVertex_s", last)
    with pytest.raises(UnsupportedBodyGeometry, match="two vertices"):
        matching_boundary_for_solid(part, described.descriptor, described.face_builds)


def test_correspondence_binding_refuses_filtered_and_rejected_rosters(monkeypatch) -> None:
    product = _take_inventory(_rrp())
    candidate = product.physical.candidate_set(FamilyId.REPEATING_RADIAL_PROFILES).candidates[0]
    object.__setattr__(candidate, "record", object())
    monkeypatch.setattr(EvidenceIndex, "validate_candidate_set", lambda *_args: None)
    with pytest.raises(CorrespondenceSnapshotError, match="wrong record type"):
        correspondence_module._CorrespondenceSnapshotAuthority().bind(product)

    monkeypatch.undo()
    product = _take_inventory(_rrp())

    def empty(_self, source):
        result = object.__new__(type(source))
        object.__setattr__(result, "family", source.family)
        object.__setattr__(result, "candidates", ())
        object.__setattr__(result, "_issuer", source._issuer)
        return result

    monkeypatch.setattr(type(product.reconciliation), "accepted_set", empty)
    with pytest.raises(CorrespondenceSnapshotError, match="accepted reconciliation"):
        correspondence_module._CorrespondenceSnapshotAuthority().bind(product)


def test_schema_three_joint_canonicalization_preserves_equal_topology_tokens() -> None:
    vertices = ((0.0, 0.0, 0.0), (0.0, 0.0, 0.0))
    curves = (
        MatchingCurve("LINE", (0, 1), 1.0, None, None, None, None, False),
        MatchingCurve("LINE", (0, 1), 1.0, None, None, None, None, False),
    )
    parameter = (0.0, 0.0)
    faces = tuple(
        MatchingFace(
            "PLANE",
            (0.0, 0.0, 1.0, 0.0),
            1.0,
            (0.0, 0.0, 0.0),
            1,
            (
                MatchingWire(
                    "outer",
                    0,
                    (
                        MatchingHalfEdge(
                            curve,
                            1,
                            MatchingWireVertex(0, parameter),
                            MatchingWireVertex(1, parameter),
                        ),
                        MatchingHalfEdge(
                            curve,
                            -1,
                            MatchingWireVertex(1, parameter),
                            MatchingWireVertex(0, parameter),
                        ),
                    ),
                ),
            ),
        )
        for curve in range(2)
    )

    graph = _body_geometry._matching_graph_canonical(
        vertices, curves, faces, _body_geometry._MatchingConstructionBudget()
    )

    assert len(graph.vertices) == 2
    assert len(graph.curves) == 2
    assert graph.symmetric


def _rrp(repeats: int = 5):
    part = Cylinder(20, 10)
    for index in range(repeats):
        part -= Rot(0, 0, 360 * index / repeats) * Pos(18, 0, 0) * Box(8, 3, 10)
    return part


@pytest.mark.parametrize("repeats", [5, 7])
def test_schema_three_rrp_half_edges_covary_under_all_24_proper_rotations(
    repeats: int,
) -> None:
    source_part = _rrp(repeats)
    source_graph = FaceGraph(source_part)
    source_solid = source_graph.common_valid_solid(source_graph.nodes)
    assert source_solid is not None
    source = source_graph.matching_boundary(source_solid)
    for rotation in _proper_signed_permutations():
        target_part = _proper_transform(source_part, rotation)
        oracle = None
        if repeats == 5:
            planar_cycles, planar_edges = _raw_planar_cycle_oracle(target_part)
            cylinder_cycles, cylinder_edges = _raw_cylinder_cycle_oracle(target_part)
            oracle = (planar_cycles, planar_edges, cylinder_cycles, cylinder_edges)
        target_graph = FaceGraph(target_part)
        target_solid = target_graph.common_valid_solid(target_graph.nodes)
        assert target_solid is not None
        target = target_graph.matching_boundary(target_solid)
        if oracle is not None:
            _raw_schema_three_incidence_oracle(target_part, target, *oracle)
        vertex_map = {
            index: min(
                range(len(target.vertices)),
                key=lambda other: math.dist(
                    _apply_rotation(rotation, vertex), target.vertices[other]
                ),
            )
            for index, vertex in enumerate(source.vertices)
        }
        assert len(set(vertex_map.values())) == len(source.vertices)
        curve_map = {}
        presentation = {}
        for index, curve in enumerate(source.curves):
            transformed_vertices = (
                None
                if curve.vertices is None
                else tuple(vertex_map[item] for item in curve.vertices)
            )
            transformed_centre = (
                None if curve.centre is None else _apply_rotation(rotation, curve.centre)
            )
            matches = tuple(
                other
                for other, candidate in enumerate(target.curves)
                if candidate.kind == curve.kind
                and abs(candidate.length - curve.length) < 1e-5
                and (
                    (
                        transformed_vertices is not None
                        and candidate.vertices is not None
                        and set(candidate.vertices) == set(transformed_vertices)
                    )
                    or (
                        transformed_vertices is None
                        and candidate.vertices is None
                        and candidate.centre is not None
                        and transformed_centre is not None
                        and math.dist(candidate.centre, transformed_centre) < 1e-5
                        and candidate.radius == pytest.approx(curve.radius, abs=1e-5)
                    )
                )
            )
            assert len(matches) == 1
            target_index = matches[0]
            curve_map[index] = target_index
            if transformed_vertices is not None:
                presentation[index] = (
                    1 if target.curves[target_index].vertices == transformed_vertices else -1
                )
            else:
                assert curve.axis is not None
                transformed_axis = _apply_rotation(rotation, curve.axis)
                gauge = -1 if next(item for item in transformed_axis if item != 0.0) < 0 else 1
                presentation[index] = gauge
        face_map = {}
        for index, face in enumerate(source.faces):
            transformed_centre = _apply_rotation(rotation, face.centroid)
            matches = tuple(
                other
                for other, candidate in enumerate(target.faces)
                if candidate.kind == face.kind
                and abs(candidate.area - face.area) < 1e-4
                and math.dist(candidate.centroid, transformed_centre) < 1e-4
            )
            assert len(matches) == 1
            face_map[index] = matches[0]
        mapped = sorted(
            (
                face_map[face_index],
                curve_map[half_edge.curve],
                half_edge.direction * presentation[half_edge.curve],
            )
            for face_index, face in enumerate(source.faces)
            for wire in face.wires
            for half_edge in wire.cycle
        )
        expected = sorted(
            (face_index, half_edge.curve, half_edge.direction)
            for face_index, face in enumerate(target.faces)
            for wire in face.wires
            for half_edge in wire.cycle
        )
        assert mapped == expected


def _line_rrp(repeats: int):
    points = []
    for sector in range(repeats):
        for offset, radius in enumerate((20.0, 16.0, 20.0, 18.0)):
            angle = 2 * math.pi * (sector / repeats + offset / (4 * repeats))
            points.append((radius * math.cos(angle), radius * math.sin(angle)))
    return extrude(Polygon(*points), 10)


def _two_rrp_one_solid():
    left = Pos(-35, 0, 0) * _line_rrp(5)
    right = Pos(35, 0, 0) * _line_rrp(7)
    bridge = Pos(0, 0, 5) * Box(40, 4, 2, align=(Align.CENTER, Align.CENTER, Align.CENTER))
    part = left + right + bridge
    assert len(part.solids()) == 1
    return part


@pytest.mark.parametrize(
    "part",
    [
        Box(10, 20, 30),
        Cylinder(10, 20),
        Cylinder(10, 5) - Cylinder(3, 5),
        _rrp(5),
        _rrp(7),
        _line_rrp(8),
    ],
)
def test_raw_ocp_schema_three_oracle_proves_complete_labelled_incidence(part) -> None:
    planar_cycles, planar_edge_roster = _raw_planar_cycle_oracle(part)
    cylinder_cycles, cylinder_edge_roster = _raw_cylinder_cycle_oracle(part)
    graph = FaceGraph(part)
    solid = graph.common_valid_solid(graph.nodes)
    assert solid is not None
    matching = graph.matching_boundary(solid)
    _raw_schema_three_incidence_oracle(
        part,
        matching,
        planar_cycles,
        planar_edge_roster,
        cylinder_cycles,
        cylinder_edge_roster,
    )
    _body_geometry.validate_matching_boundary_graph(
        matching, graph.body_geometry(solid).descriptor.quantization
    )


@pytest.mark.parametrize("native", [Cylinder(10, 5) - Cylinder(3, 5), _rrp(7)])
def test_raw_ocp_oracle_proves_step_and_reversed_presentation(
    native, tmp_path: Path, monkeypatch
) -> None:
    path = tmp_path / "oracle.step"
    export_step(native, path)
    presentations = (native, import_step(path))
    solid_faces = Solid.faces
    wire_edges = Wire.edges

    for part in presentations:
        planar_cycles, planar_roster = _raw_planar_cycle_oracle(part)
        cylinder_cycles, cylinder_roster = _raw_cylinder_cycle_oracle(part)
        graph = FaceGraph(part)
        solid = graph.common_valid_solid(graph.nodes)
        assert solid is not None
        matching = graph.matching_boundary(solid)
        _raw_schema_three_incidence_oracle(
            part,
            matching,
            planar_cycles,
            planar_roster,
            cylinder_cycles,
            cylinder_roster,
        )

    monkeypatch.setattr(Solid, "faces", lambda self: list(reversed(solid_faces(self))))
    monkeypatch.setattr(Wire, "edges", lambda self: list(reversed(wire_edges(self))))
    planar_cycles, planar_roster = _raw_planar_cycle_oracle(native)
    cylinder_cycles, cylinder_roster = _raw_cylinder_cycle_oracle(native)
    graph = FaceGraph(native)
    solid = graph.common_valid_solid(graph.nodes)
    assert solid is not None
    _raw_schema_three_incidence_oracle(
        native,
        graph.matching_boundary(solid),
        planar_cycles,
        planar_roster,
        cylinder_cycles,
        cylinder_roster,
    )


def test_schema_three_changed_incidence_with_identical_labels_refuses() -> None:
    graph, solid, fact = _body_descriptor(Box(10, 20, 30))
    matching = graph.matching_boundary(solid)
    incidence = list(matching.incidence)
    left_curve, left_occurrences = incidence[0]
    right_curve, right_occurrences = incidence[1]
    changed = list(incidence)
    changed[0] = (left_curve, (right_occurrences[0], *left_occurrences[1:]))
    changed[1] = (right_curve, (left_occurrences[0], *right_occurrences[1:]))
    malformed = dataclasses.replace(matching, incidence=tuple(changed))

    assert malformed.faces == matching.faces
    assert malformed.curves == matching.curves
    with pytest.raises(UnsupportedBodyGeometry):
        _body_geometry.validate_matching_boundary_graph(malformed, fact.descriptor.quantization)


def test_schema_three_seam_multiplicity_change_refuses() -> None:
    graph, solid, fact = _body_descriptor(Cylinder(10, 20))
    matching = graph.matching_boundary(solid)
    incidence = list(matching.incidence)
    at = next(
        index
        for index, (_curve, occurrences) in enumerate(incidence)
        if len(occurrences) == 2 and occurrences[0][0] == occurrences[1][0]
    )
    curve, occurrences = incidence[at]
    incidence[at] = (curve, occurrences[:1])
    malformed = dataclasses.replace(matching, incidence=tuple(incidence))

    with pytest.raises(UnsupportedBodyGeometry):
        _body_geometry.validate_matching_boundary_graph(malformed, fact.descriptor.quantization)


def test_schema_three_outer_inner_swap_refuses() -> None:
    graph, solid, fact = _body_descriptor(Cylinder(10, 5) - Cylinder(3, 5))
    matching = graph.matching_boundary(solid)
    changed_faces = []
    for face in matching.faces:
        if face.kind == "CYLINDER":
            changed_faces.append(
                dataclasses.replace(
                    face,
                    wires=tuple(
                        dataclasses.replace(wire, role="inner" if wire.role == "outer" else "outer")
                        for wire in face.wires
                    ),
                )
            )
        else:
            changed_faces.append(face)
    malformed = dataclasses.replace(matching, faces=tuple(changed_faces))

    with pytest.raises(UnsupportedBodyGeometry):
        _body_geometry.validate_matching_boundary_graph(malformed, fact.descriptor.quantization)


def _chiral_planar_body():
    return (
        Box(4, 4, 4)
        + Pos(4, 0, 0) * Box(8, 2, 2)
        + Pos(0, 3, 0) * Box(2, 6, 1.5)
        + Pos(0, 0, 2.5) * Box(1, 1, 5)
    )


def test_schema_three_chiral_mirror_has_no_proper_rotation_witness() -> None:
    source_part = _chiral_planar_body()
    source_graph, source_solid, _source_fact = _body_descriptor(source_part)
    source = source_graph.matching_boundary(source_solid)
    mirrored = source_part.mirror(Plane.YZ)

    candidates = []
    for rotation in _proper_signed_permutations():
        candidate_part = _proper_transform(mirrored, rotation)
        graph, solid, _fact = _body_descriptor(candidate_part)
        candidates.append(graph.matching_boundary(solid))

    assert all(candidate != source for candidate in candidates)


def _body_descriptor(part):
    graph = FaceGraph(part)
    solid = graph.common_valid_solid(graph.nodes)
    assert solid is not None
    return graph, solid, graph.body_geometry(solid)


def _raw_body_oracle(part):
    """Fresh raw-kernel facts collected before any production descriptor is read."""

    solids = tuple(part.solids())
    assert len(solids) == 1
    solid = solids[0]
    volume = GProp_GProps()
    surface_props = GProp_GProps()
    BRepGProp.VolumeProperties_s(solid.wrapped, volume)
    BRepGProp.SurfaceProperties_s(solid.wrapped, surface_props)
    faces = tuple(solid.faces())
    centre = tuple(float(value) for value in volume.CentreOfMass().Coord())
    face_geometry = []
    occurrence_tokens = []
    for face in faces:
        surface_adaptor = BRepAdaptor_Surface(face.wrapped)
        surface_kind = surface_adaptor.GetType().name.removeprefix("GeomAbs_").upper()
        if surface_kind == "PLANE":
            surface = surface_adaptor.Plane()
            raw_axis = _oracle_axis_raw(surface.Axis().Direction().Coord())
            axis = tuple(map(_rounded, raw_axis))
            location = tuple(float(value) for value in surface.Location().Coord())
            parameters = (
                *axis,
                _rounded(
                    sum(
                        direction * (value - origin)
                        for direction, value, origin in zip(raw_axis, location, centre, strict=True)
                    )
                ),
            )
        elif surface_kind == "CYLINDER":
            surface = surface_adaptor.Cylinder()
            raw_axis = _oracle_axis_raw(surface.Axis().Direction().Coord())
            axis = tuple(map(_rounded, raw_axis))
            location = tuple(float(value) for value in surface.Location().Coord())
            delta = tuple(value - origin for value, origin in zip(location, centre, strict=True))
            along = sum(value * direction for value, direction in zip(delta, raw_axis, strict=True))
            closest = tuple(
                value - along * direction for value, direction in zip(delta, raw_axis, strict=True)
            )
            parameters = (*axis, *map(_rounded, closest), _rounded(surface.Radius()))
        else:
            parameters = ()
        face_centre = face.center()
        normal = face.normal_at(face_centre)
        if surface_kind == "PLANE":
            gauge = parameters[:3]
            material_side = 1 if sum(a * b for a, b in zip(gauge, normal, strict=True)) >= 0 else -1
        else:
            gauge = parameters[:3]
            location = tuple(
                float(value) for value in surface_adaptor.Cylinder().Location().Coord()
            )
            sample_delta = tuple(
                value - origin for value, origin in zip(face_centre, location, strict=True)
            )
            along = sum(
                value * direction for value, direction in zip(sample_delta, gauge, strict=True)
            )
            radial = tuple(
                value - along * direction
                for value, direction in zip(sample_delta, gauge, strict=True)
            )
            material_side = (
                1 if sum(a * b for a, b in zip(radial, normal, strict=True)) >= 0 else -1
            )

        outer = face.outer_wire()
        wire_entries = []
        for wire in face.wires():
            edges = []
            for edge in wire.edges():
                curve = BRepAdaptor_Curve(edge.wrapped)
                kind = curve.GetType().name.removeprefix("GeomAbs_").upper()
                start = tuple(
                    _rounded(value - origin)
                    for value, origin in zip(tuple(edge.position_at(0)), centre, strict=True)
                )
                end = tuple(
                    _rounded(value - origin)
                    for value, origin in zip(tuple(edge.position_at(1)), centre, strict=True)
                )
                centre_label: tuple[float, ...] = ()
                axis_label: tuple[float, ...] = ()
                radius = 0.0
                sweep = 0.0
                full = False
                if kind == "CIRCLE":
                    raw = curve.Circle()
                    centre_label = tuple(
                        _rounded(value - origin)
                        for value, origin in zip(raw.Location().Coord(), centre, strict=True)
                    )
                    raw_axis = tuple(float(value) for value in raw.Axis().Direction().Coord())
                    canonical_axis = _oracle_axis_raw(raw_axis)
                    axis_sign = 1 if canonical_axis == raw_axis else -1
                    axis_label = tuple(map(_rounded, canonical_axis))
                    radius = _rounded(raw.Radius())
                    magnitude = float(edge.length) / float(raw.Radius())
                    midpoint = tuple(edge.position_at(0.5))
                    raw_centre = tuple(float(value) for value in raw.Location().Coord())
                    first_vector = tuple(
                        value - origin
                        for value, origin in zip(
                            tuple(edge.position_at(0)), raw_centre, strict=True
                        )
                    )
                    middle_vector = tuple(
                        value - origin for value, origin in zip(midpoint, raw_centre, strict=True)
                    )
                    cross = (
                        first_vector[1] * middle_vector[2] - first_vector[2] * middle_vector[1],
                        first_vector[2] * middle_vector[0] - first_vector[0] * middle_vector[2],
                        first_vector[0] * middle_vector[1] - first_vector[1] * middle_vector[0],
                    )
                    raw_sweep = (
                        magnitude
                        if sum(a * b for a, b in zip(cross, raw_axis, strict=True)) >= 0
                        else -magnitude
                    )
                    sweep = _rounded(axis_sign * raw_sweep)
                    full = abs(abs(raw_sweep) - 2 * math.pi) <= _body_geometry.ANGLE_TOL
                first = (start, end, sweep)
                second = (end, start, -sweep)
                canonical_start, canonical_end, canonical_sweep = min(first, second)
                label = (
                    kind,
                    canonical_start,
                    canonical_end,
                    _rounded(edge.length),
                    centre_label,
                    axis_label,
                    radius,
                    canonical_sweep,
                    full,
                )
                direction = (
                    -1 if edge.wrapped.Orientation() == TopAbs_Orientation.TopAbs_REVERSED else 1
                )
                edges.append((label, direction, edge))
            raw_wire_orientation = (
                -1 if wire.wrapped.Orientation() == TopAbs_Orientation.TopAbs_REVERSED else 1
            )
            canonical, semantic, alignments = _oracle_cycle(
                tuple(edges), raw_wire_orientation * material_side
            )
            wire_entries.append(
                (("outer" if wire == outer else "inner", semantic, canonical), alignments)
            )
        wire_entries.sort(key=lambda item: item[0])
        wires = tuple(item[0] for item in wire_entries)
        occurrence_tokens.extend(item[1] for item in wire_entries)
        face_label = (
            surface_kind,
            tuple(_rounded(value) for value in parameters),
            _rounded(face.area),
            tuple(
                _rounded(value - origin)
                for value, origin in zip(tuple(face_centre), centre, strict=True)
            ),
            material_side,
            wires,
        )
        face_geometry.append(face_label)

    ordered_faces = tuple(sorted(face_geometry))
    assert len(set(ordered_faces)) == len(ordered_faces), "oracle fixture needs unique face labels"
    face_indices = {label: index for index, label in enumerate(ordered_faces)}
    token_occurrences: dict[object, list[tuple[int, int, int, int]]] = {}
    token_labels: dict[object, object] = {}
    alignment_candidates = []
    for chosen_alignments in product(*occurrence_tokens):
        token_occurrences = {}
        token_labels = {}
        token_index = 0
        valid = True
        for face_label in face_geometry:
            canonical_face_index = face_indices[face_label]
            for wire_index, wire in enumerate(face_label[-1]):
                tokens = chosen_alignments[token_index]
                token_index += 1
                for edge_index, ((edge_label, direction), token) in enumerate(
                    zip(wire[2], tokens, strict=True)
                ):
                    prior = token_labels.setdefault(token, edge_label)
                    if prior != edge_label:
                        valid = False
                        break
                    token_occurrences.setdefault(token, []).append(
                        (canonical_face_index, wire_index, edge_index, direction)
                    )
                if not valid:
                    break
            if not valid:
                break
        if valid:
            alignment_candidates.append(
                tuple(
                    sorted(
                        (token_labels[token], tuple(sorted(items)))
                        for token, items in token_occurrences.items()
                    )
                )
            )
    assert alignment_candidates, "oracle found no consistent physical alignment"
    canonical_incidence = min(alignment_candidates)
    return {
        "volume": float(volume.Mass()),
        "surface_area": float(surface_props.Mass()),
        "centre": centre,
        "moments": tuple(sorted(float(value) for value in volume.PrincipalProperties().Moments())),
        "face_count": len(faces),
        "wire_count": sum(len(tuple(face.wires())) for face in faces),
        "edge_occurrence_count": sum(
            len(tuple(wire.edges())) for face in faces for wire in face.wires()
        ),
        "faces": ordered_faces,
        "incidence": canonical_incidence,
    }


def _rounded(value: float) -> float:
    result = round(float(value), 4)
    return 0.0 if result == 0.0 else result


def _oracle_axis(values) -> tuple[float, float, float]:
    return tuple(map(_rounded, _oracle_axis_raw(values)))  # type: ignore[return-value]


def _oracle_axis_raw(values) -> tuple[float, float, float]:
    axis = tuple(float(value) for value in values)
    sign = next((1 if value > 0 else -1 for value in axis if abs(value) >= 1e-10), 1)
    return tuple(sign * value for value in axis)  # type: ignore[return-value]


def _oracle_cycle(items, raw_orientation: int):
    candidates = []
    for reversed_presentation, source in (
        (False, items),
        (True, tuple((label, -direction, token) for label, direction, token in reversed(items))),
    ):
        for index in range(len(source)):
            rotated = source[index:] + source[:index]
            label = tuple((edge, direction) for edge, direction, _token in rotated)
            tokens = tuple(token for _edge, _direction, token in rotated)
            semantic = raw_orientation * (-1 if reversed_presentation else 1)
            candidates.append((label, semantic, tokens))
    canonical = min(label for label, _semantic, _tokens in candidates)
    matching = tuple(item for item in candidates if item[0] == canonical)
    semantics = {semantic for _label, semantic, _tokens in matching}
    assert len(semantics) == 1
    alignments = tuple({tokens for _label, semantic, tokens in matching if semantic in semantics})
    return canonical, semantics.pop(), alignments


def _descriptor_face_payload(face: FaceGeometry):
    wires = []
    for wire in face.wires:
        edges = []
        for edge, _direction in wire.edges:
            edges.append(_descriptor_edge_payload(edge))
        wires.append(
            (
                wire.role,
                wire.semantic_winding,
                tuple(zip(edges, (direction for _edge, direction in wire.edges), strict=True)),
            )
        )
    return (
        face.kind,
        tuple(map(_rounded, face.parameters)),
        _rounded(face.area),
        tuple(map(_rounded, face.centroid)),
        face.material_side,
        tuple(sorted(wires)),
    )


def _descriptor_edge_payload(edge):
    return (
        edge.kind,
        tuple(map(_rounded, edge.start)),
        tuple(map(_rounded, edge.end)),
        _rounded(edge.length),
        tuple(map(_rounded, edge.centre or ())),
        tuple(map(_rounded, edge.axis or ())),
        _rounded(edge.radius or 0.0),
        _rounded(edge.sweep or 0.0),
        edge.full,
    )


def _structure(value):
    if dataclasses.is_dataclass(value):
        fields = tuple(_structure(getattr(value, item.name)) for item in dataclasses.fields(value))
        return type(value).__name__, fields
    if isinstance(value, tuple):
        return tuple(_structure(item) for item in value)
    return "float" if isinstance(value, float) else value


def _numbers(value) -> tuple[float, ...]:
    if dataclasses.is_dataclass(value):
        return tuple(
            number
            for item in dataclasses.fields(value)
            for number in _numbers(getattr(value, item.name))
        )
    if isinstance(value, (tuple, list)):
        return tuple(number for item in value for number in _numbers(item))
    if isinstance(value, Mapping):
        return tuple(number for item in value.values() for number in _numbers(item))
    return (value,) if isinstance(value, float) else ()


def _alias_aware_calls(tree: ast.AST, target: str) -> tuple[ast.Call, ...]:
    """Find direct, qualified, imported, rebound and re-exported calls by leaf identity."""

    aliases = {target}
    changed = True
    while changed:
        changed = False
        for node in ast.walk(tree):
            introduced: str | None = None
            source: str | None = None
            if isinstance(node, ast.ImportFrom):
                for item in node.names:
                    if item.name in aliases:
                        introduced = item.asname or item.name
                        source = item.name
                        break
            elif isinstance(node, (ast.Assign, ast.AnnAssign)) and isinstance(
                node.value, (ast.Name, ast.Attribute)
            ):
                source = node.value.id if isinstance(node.value, ast.Name) else node.value.attr
                targets = node.targets if isinstance(node, ast.Assign) else (node.target,)
                introduced = next((item.id for item in targets if isinstance(item, ast.Name)), None)
            if source in aliases and introduced is not None and introduced not in aliases:
                aliases.add(introduced)
                changed = True
    return tuple(
        node
        for node in ast.walk(tree)
        if isinstance(node, ast.Call)
        and (
            (isinstance(node.func, ast.Name) and node.func.id in aliases)
            or (isinstance(node.func, ast.Attribute) and node.func.attr in aliases)
        )
    )


def test_body_geometry_is_translation_normalized_and_cached() -> None:
    graph, solid, source = _body_descriptor(_rrp())
    translated_graph, translated_solid, translated = _body_descriptor(Pos(7, 8, 9) * _rrp())

    assert graph.body_geometry(solid) is source
    assert source.descriptor.intrinsic == translated.descriptor.intrinsic
    assert source.descriptor.boundary == translated.descriptor.boundary
    assert graph.matching_boundary(solid) == translated_graph.matching_boundary(translated_solid)
    assert translated.descriptor.placement.centre_of_mass == pytest.approx((7.0, 8.0, 9.0))
    assert source.descriptor.placement != translated.descriptor.placement
    assert translated_graph is not graph


@pytest.mark.parametrize(
    ("transform", "expected_axis"),
    [
        (Pos(0, 0, 0), (0.0, 0.0, 1.0)),
        (Rot(0, 90, 0), (1.0, 0.0, 0.0)),
        (Rot(90, 0, 0), (0.0, 1.0, 0.0)),
    ],
)
def test_raw_ocp_oracle_independently_reconstructs_mass_and_topology(
    transform, expected_axis
) -> None:
    part = transform * _rrp(7)
    oracle = _raw_body_oracle(part)
    _graph, _solid, fact = _body_descriptor(part)

    scale = max(oracle["volume"] ** (1 / 3), math.sqrt(oracle["surface_area"]))
    metric = _body_geometry._metric_tolerance(scale)
    area_quantum = (scale + metric) ** 2 - scale**2
    volume_quantum = (scale + metric) ** 3 - scale**3
    moment_quantum = (scale + metric) ** 5 - scale**5
    assert abs(fact.descriptor.intrinsic.volume - oracle["volume"]) <= 2 * volume_quantum
    assert abs(fact.descriptor.intrinsic.surface_area - oracle["surface_area"]) <= 2 * area_quantum
    assert all(
        abs(actual - expected) <= 2 * moment_quantum
        for actual, expected in zip(
            fact.descriptor.intrinsic.principal_moments, oracle["moments"], strict=True
        )
    )
    assert fact.descriptor.placement.centre_of_mass == pytest.approx(oracle["centre"])
    assert fact.descriptor.boundary.face_count == oracle["face_count"]
    assert fact.descriptor.boundary.wire_count == oracle["wire_count"]
    assert fact.descriptor.boundary.edge_occurrence_count == oracle["edge_occurrence_count"]
    assert (
        tuple(sorted(map(_descriptor_face_payload, fact.descriptor.boundary.faces)))
        == oracle["faces"]
    )
    actual_incidence = tuple(
        sorted(
            (_descriptor_edge_payload(edge), occurrences)
            for edge, occurrences in fact.descriptor.boundary.incidence
        )
    )
    assert oracle["incidence"] == actual_incidence
    assert all(len(occurrences) == 2 for _edge, occurrences in oracle["incidence"])

    occurrence = correspondence_snapshot(_take_inventory(part)).occurrences[0]
    oracle_caps = tuple(
        face for face in oracle["faces"] if face[0] == "PLANE" and face[1][:3] == expected_axis
    )
    assert len(oracle_caps) == 2
    assert tuple(sorted(map(_descriptor_face_payload, occurrence.summary.defining))) == tuple(
        sorted(oracle_caps)
    )


def test_raw_oracle_enumerates_tied_outer_inner_and_seam_alignments() -> None:
    part = Cylinder(10, 5) - Cylinder(3, 5)
    oracle = _raw_body_oracle(part)
    descriptor = _body_descriptor(part)[2].descriptor
    assert oracle["faces"] == tuple(
        sorted(map(_descriptor_face_payload, descriptor.boundary.faces))
    )
    assert oracle["incidence"] == tuple(
        sorted(
            (_descriptor_edge_payload(edge), occurrences)
            for edge, occurrences in descriptor.boundary.incidence
        )
    )


def test_scalar_intrinsic_is_rigid_motion_invariant_but_boundary_is_world_oriented() -> None:
    _source_graph, _source_solid, source = _body_descriptor(_rrp())
    _turned_graph, _turned_solid, turned = _body_descriptor(Rot(13, 27, 9) * _rrp())

    assert source.descriptor.intrinsic == turned.descriptor.intrinsic
    assert source.descriptor.boundary != turned.descriptor.boundary

    _thin_graph, _thin_solid, thin = _body_descriptor(Box(100, 2, 0.5))
    _thin_rotated_graph, _thin_rotated_solid, thin_rotated = _body_descriptor(
        Rot(31, 17, 23) * Box(100, 2, 0.5)
    )
    assert thin.descriptor.intrinsic == thin_rotated.descriptor.intrinsic


def test_uniform_scale_obeys_mass_property_powers() -> None:
    _source_graph, _source_solid, source = _body_descriptor(_rrp())
    _scaled_graph, _scaled_solid, scaled = _body_descriptor(_rrp().scale(2))

    assert scaled.descriptor.intrinsic.volume == pytest.approx(
        8 * source.descriptor.intrinsic.volume, rel=1e-6
    )
    assert scaled.descriptor.intrinsic.surface_area == pytest.approx(
        4 * source.descriptor.intrinsic.surface_area, rel=1e-6
    )
    assert scaled.descriptor.intrinsic.principal_moments == pytest.approx(
        tuple(32 * value for value in source.descriptor.intrinsic.principal_moments),
        rel=5e-6,
    )


def test_mirror_and_translation_snapshots_preserve_intrinsic_multiplicity() -> None:
    source = correspondence_snapshot(_take_inventory(_line_rrp(8))).occurrences[0]
    mirrored = correspondence_snapshot(_take_inventory(_line_rrp(8).mirror(Plane.YZ))).occurrences[
        0
    ]
    translated = correspondence_snapshot(
        _take_inventory(Pos(17, -13, 29) * _line_rrp(8))
    ).occurrences[0]

    assert mirrored.body.intrinsic == source.body.intrinsic
    assert translated.body.intrinsic == source.body.intrinsic
    assert translated.body.boundary == source.body.boundary
    assert translated.matching_boundary == source.matching_boundary
    assert mirrored.matching_boundary != source.matching_boundary
    assert translated.body.placement.centre_of_mass == pytest.approx((17.0, -13.0, 34.0))


def test_representation_preserving_step_round_trip_has_the_same_descriptor(tmp_path) -> None:
    source = _rrp()
    target = tmp_path / "rrp.step"
    assert export_step(source, target)
    imported = import_step(target)

    _native_graph, _native_solid, native = _body_descriptor(source)
    _step_graph, _step_solid, stepped = _body_descriptor(imported)
    assert _structure(stepped.descriptor) == _structure(native.descriptor)
    assert _numbers(stepped.descriptor) == pytest.approx(
        _numbers(native.descriptor), rel=1e-8, abs=1e-7
    )
    assert stepped.descriptor.placement.centre_of_mass == pytest.approx(
        native.descriptor.placement.centre_of_mass, abs=1e-9
    )
    native_occurrence = correspondence_snapshot(_take_inventory(source)).occurrences[0]
    stepped_occurrence = correspondence_snapshot(_take_inventory(imported)).occurrences[0]
    assert native_occurrence.family == stepped_occurrence.family
    assert native_occurrence.record_type == stepped_occurrence.record_type
    assert native_occurrence.summary.repeat_count == stepped_occurrence.summary.repeat_count
    assert native_occurrence.summary.axis == stepped_occurrence.summary.axis
    assert _structure(native_occurrence.summary) == _structure(stepped_occurrence.summary)
    assert _numbers(native_occurrence.summary) == pytest.approx(
        _numbers(stepped_occurrence.summary), rel=1e-8, abs=1e-7
    )
    assert _structure(native_occurrence.matching_boundary) == _structure(
        stepped_occurrence.matching_boundary
    )
    assert _numbers(native_occurrence.matching_boundary) == pytest.approx(
        _numbers(stepped_occurrence.matching_boundary), rel=1e-8, abs=1e-7
    )


def test_uniform_scale_snapshot_preserves_occurrence_and_named_powers() -> None:
    source = correspondence_snapshot(_take_inventory(_line_rrp(5))).occurrences[0]
    scaled = correspondence_snapshot(_take_inventory(_line_rrp(5).scale(2))).occurrences[0]
    assert scaled.summary.repeat_count == source.summary.repeat_count
    assert scaled.summary.axis == source.summary.axis
    assert scaled.body.intrinsic.volume == pytest.approx(8 * source.body.intrinsic.volume, rel=1e-6)
    assert scaled.body.intrinsic.surface_area == pytest.approx(
        4 * source.body.intrinsic.surface_area, rel=1e-6
    )
    assert scaled.summary.span == pytest.approx(
        tuple(2 * value for value in source.summary.span), rel=1e-6
    )


def test_face_and_cyclic_wire_traversal_permutations_are_descriptor_neutral(
    monkeypatch,
) -> None:
    part = _line_rrp(8)
    source = _body_descriptor(part)[2].descriptor
    solid_faces = Solid.faces
    wire_edges = Wire.edges

    monkeypatch.setattr(Solid, "faces", lambda self: list(reversed(solid_faces(self))))

    def shifted(self):
        edges = list(wire_edges(self))
        return edges[1:] + edges[:1] if edges else edges

    monkeypatch.setattr(Wire, "edges", shifted)
    permuted = _body_descriptor(part)[2].descriptor
    assert permuted == source


def test_whole_wire_reversal_with_reversed_half_edges_is_descriptor_neutral(
    monkeypatch,
) -> None:
    part = _line_rrp(5)
    source = _body_descriptor(part)[2].descriptor
    wire_edges = Wire.edges
    wire_orientation = _body_geometry._wire_orientation

    def reversed_wrapper(self):
        return [edge.reversed() for edge in reversed(wire_edges(self))]

    monkeypatch.setattr(Wire, "edges", reversed_wrapper)
    monkeypatch.setattr(_body_geometry, "_wire_orientation", lambda wire: -wire_orientation(wire))
    assert _body_descriptor(part)[2].descriptor == source


def test_controlled_material_face_reversal_changes_physical_orientation(monkeypatch) -> None:
    part = _line_rrp(5)
    source = _body_descriptor(part)[2].descriptor
    graph = FaceGraph(part)
    solid = graph.common_valid_solid(graph.nodes)
    assert solid is not None
    source_matching = graph.matching_boundary(solid)
    solid_faces = Solid.faces

    monkeypatch.setattr(
        Solid,
        "faces",
        lambda self: [Face.cast(face.wrapped.Reversed()) for face in solid_faces(self)],
    )
    reversed_graph = FaceGraph(part)
    reversed_solid = reversed_graph.common_valid_solid(reversed_graph.nodes)
    assert reversed_solid is not None
    reversed_descriptor = reversed_graph.body_geometry(reversed_solid).descriptor
    reversed_matching = reversed_graph.matching_boundary(reversed_solid)
    assert reversed_descriptor != source
    assert tuple(face.material_side for face in reversed_descriptor.boundary.faces) != tuple(
        face.material_side for face in source.boundary.faces
    )
    assert reversed_matching != source_matching
    assert tuple(face.material_side for face in reversed_matching.faces) != tuple(
        face.material_side for face in source_matching.faces
    )


def test_body_geometry_refuses_foreign_and_copied_solid_refs() -> None:
    graph, solid, _fact = _body_descriptor(_rrp())
    foreign, foreign_solid, _foreign_fact = _body_descriptor(_rrp())

    with pytest.raises(BodyGeometryAuthorityError, match="not issued"):
        graph.body_geometry(copy.copy(solid))
    with pytest.raises(BodyGeometryAuthorityError, match="not issued"):
        graph.body_geometry(foreign_solid)
    assert foreign is not graph

    mutated_graph, mutated, _mutated_fact = _body_descriptor(_rrp())
    object.__setattr__(mutated, "ordinal", 99)
    with pytest.raises(BodyGeometryAuthorityError, match="not issued"):
        mutated_graph.body_geometry(mutated)


def test_body_geometry_refuses_unsupported_surface_without_caching() -> None:
    graph = FaceGraph(Sphere(5))
    solid = graph.common_valid_solid(graph.nodes)
    assert solid is not None

    with pytest.raises(UnsupportedBodyGeometry, match="unsupported body face surface"):
        graph.body_geometry(solid)


def test_body_geometry_refuses_supported_surface_with_freeform_curve() -> None:
    spline = Edge.make_spline([Vector(0, 0), Vector(2, 1), Vector(4, 0)])
    wire = Wire(
        [
            spline,
            Edge.make_line((4, 0), (4, 4)),
            Edge.make_line((4, 4), (0, 4)),
            Edge.make_line((0, 4), (0, 0)),
        ]
    )
    part = extrude(Face(wire), 5)
    graph = FaceGraph(part)
    solid = graph.common_valid_solid(graph.nodes)
    assert solid is not None

    with pytest.raises(UnsupportedBodyGeometry, match="unsupported body face surface"):
        graph.body_geometry(solid)
    with pytest.raises(UnsupportedBodyGeometry, match="unsupported body edge curve"):
        _body_geometry._edge_geometry(spline, (0.0, 0.0, 0.0), 1e-7)


def test_invalid_open_geometry_and_unexpected_programmer_errors_do_not_cache(
    monkeypatch,
) -> None:
    shell = _rrp().shells()[0]
    with pytest.raises(UnsupportedBodyGeometry, match="valid closed solid"):
        _body_geometry.describe_solid(shell)

    graph = FaceGraph(_rrp())
    solid = graph.common_valid_solid(graph.nodes)
    assert solid is not None

    def programmer_error(*_args):
        raise KeyError("controlled programmer error")

    monkeypatch.setattr(BRepGProp, "VolumeProperties_s", programmer_error)
    with pytest.raises(KeyError, match="programmer error"):
        graph.body_geometry(solid)
    with pytest.raises(KeyError, match="programmer error"):
        graph.body_geometry(solid)


@pytest.mark.parametrize("mass", [float("nan"), 0.0, -1.0])
def test_nonfinite_zero_and_negative_mass_refuse(mass: float, monkeypatch) -> None:
    graph = FaceGraph(_rrp())
    solid = graph.common_valid_solid(graph.nodes)
    assert solid is not None
    monkeypatch.setattr(GProp_GProps, "Mass", lambda _self: mass)
    with pytest.raises(UnsupportedBodyGeometry, match="mass properties"):
        graph.body_geometry(solid)


@pytest.mark.parametrize("surface", [float("nan"), 0.0, -1.0])
def test_nonfinite_zero_and_negative_surface_area_refuse(surface: float, monkeypatch) -> None:
    graph = FaceGraph(_rrp())
    solid = graph.common_valid_solid(graph.nodes)
    assert solid is not None
    original = GProp_GProps.Mass
    calls = 0

    def mass(props):
        nonlocal calls
        calls += 1
        return surface if calls == 2 else original(props)

    monkeypatch.setattr(GProp_GProps, "Mass", mass)
    with pytest.raises(UnsupportedBodyGeometry, match="mass properties"):
        graph.body_geometry(solid)


@pytest.mark.parametrize("moment", [float("nan"), -1.0])
def test_nonfinite_and_negative_principal_moment_refuse(moment: float, monkeypatch) -> None:
    graph = FaceGraph(_rrp())
    solid = graph.common_valid_solid(graph.nodes)
    assert solid is not None
    original = GProp_GProps.PrincipalProperties

    class BrokenPrincipal:
        def Moments(self):
            return (moment, 1.0, 1.0)

    monkeypatch.setattr(GProp_GProps, "PrincipalProperties", lambda _self: BrokenPrincipal())
    with pytest.raises(UnsupportedBodyGeometry, match="mass properties"):
        graph.body_geometry(solid)
    monkeypatch.setattr(GProp_GProps, "PrincipalProperties", original)


@pytest.mark.parametrize("length", [float("nan"), 0.0, -1.0])
def test_nonfinite_zero_and_negative_curve_length_refuse(length: float, monkeypatch) -> None:
    edge = Edge.make_line((0, 0, 0), (1, 0, 0))
    monkeypatch.setattr(Edge, "length", property(lambda _self: length))
    with pytest.raises(UnsupportedBodyGeometry, match="edge length"):
        _body_geometry._edge_geometry(edge, (0.0, 0.0, 0.0), 1e-7)


def test_internal_runtime_boundary_failure_propagates_and_does_not_cache(
    monkeypatch,
) -> None:
    graph = FaceGraph(_rrp())
    solid = graph.common_valid_solid(graph.nodes)
    assert solid is not None

    monkeypatch.setattr(
        _body_geometry,
        "_face_geometry",
        lambda *_args: (_ for _ in ()).throw(RuntimeError("controlled programmer failure")),
    )
    with pytest.raises(RuntimeError, match="programmer failure"):
        graph.body_geometry(solid)
    with pytest.raises(RuntimeError, match="programmer failure"):
        graph.body_geometry(solid)


def test_solid_face_enumeration_runtime_failure_is_closed_and_does_not_cache(monkeypatch) -> None:
    graph = FaceGraph(_rrp())
    solid_ref = graph.common_valid_solid(graph.nodes)
    assert solid_ref is not None

    monkeypatch.setattr(
        Solid,
        "faces",
        lambda _self: (_ for _ in ()).throw(RuntimeError("controlled wrapper failure")),
    )
    with pytest.raises(UnsupportedBodyGeometry, match="body boundary"):
        graph.body_geometry(solid_ref)
    with pytest.raises(UnsupportedBodyGeometry, match="body boundary"):
        graph.body_geometry(solid_ref)


@pytest.mark.parametrize("end_angle", [180, 270])
def test_trimmed_circle_geometry_is_direction_and_semicircle_safe(end_angle: float) -> None:
    edge = Edge.make_circle(5, Plane.XY, start_angle=0, end_angle=end_angle)
    direct = _body_geometry._edge_geometry(edge, (0.0, 0.0, 0.0), 1e-7)
    reversed_geometry = _body_geometry._edge_geometry(edge.reversed(), (0.0, 0.0, 0.0), 1e-7)

    assert direct == reversed_geometry
    assert direct.start != direct.end
    assert abs(direct.sweep or 0.0) == pytest.approx(
        end_angle * math.pi / 180, abs=_body_geometry.ANGLE_TOL
    )


def test_real_outer_inner_and_seam_wire_orientation_is_step_stable(tmp_path) -> None:
    tube = Cylinder(10, 5) - Cylinder(3, 5)
    target = tmp_path / "tube.step"
    assert export_step(tube, target)
    native = _body_descriptor(tube)[2].descriptor
    stepped = _body_descriptor(import_step(target))[2].descriptor

    native_roles = sorted(
        (wire.role, wire.semantic_winding) for face in native.boundary.faces for wire in face.wires
    )
    stepped_roles = sorted(
        (wire.role, wire.semantic_winding) for face in stepped.boundary.faces for wire in face.wires
    )
    assert native_roles == stepped_roles
    assert {role for role, _winding in native_roles} == {"inner", "outer"}
    assert all(len(incidence) == 2 for _edge, incidence in native.boundary.incidence)


def test_canonicalization_budget_is_inclusive(monkeypatch) -> None:
    label = FaceGeometry("PLANE", (), 1.0, (0.0, 0.0, 0.0), 1, ())
    builds = tuple(_body_geometry._FaceBuild(label, ()) for _ in range(8))

    monkeypatch.setattr(_body_geometry, "CANONICAL_SERIALIZATION_BUDGET", 40_320)
    _ordered, _incidence, symmetric = _body_geometry._canonical_topology(builds)
    assert symmetric

    monkeypatch.setattr(_body_geometry, "CANONICAL_SERIALIZATION_BUDGET", 40_319)
    with pytest.raises(UnsupportedBodyGeometry, match="budget"):
        _body_geometry._canonical_topology(builds)


def test_equal_wire_and_mixed_budget_counts_only_complete_serializations(monkeypatch) -> None:
    edge = _body_geometry.EdgeGeometry("LINE", (0.0, 0.0, 0.0), (1.0, 0.0, 0.0), 1.0)
    wire = _body_geometry.WireGeometry("outer", 1, ((edge, 1), (edge, -1)))
    tokens = (object(), object())
    wire_builds = tuple(
        _body_geometry._WireBuild(wire, (((token, 1), (token, -1)),)) for token in tokens
    )
    face = FaceGeometry("PLANE", (), 1.0, (0.0, 0.0, 0.0), 1, (wire, wire))
    build = _body_geometry._FaceBuild(face, wire_builds)

    monkeypatch.setattr(_body_geometry, "CANONICAL_SERIALIZATION_BUDGET", 2)
    _body_geometry._canonical_topology((build,))
    monkeypatch.setattr(_body_geometry, "CANONICAL_SERIALIZATION_BUDGET", 1)
    with pytest.raises(UnsupportedBodyGeometry, match="budget"):
        _body_geometry._canonical_topology((build,))


@pytest.mark.parametrize("occurrence_count", [1, 3])
def test_invalid_edge_incidence_cardinality_refuses(occurrence_count: int) -> None:
    edge = _body_geometry.EdgeGeometry("LINE", (0.0, 0.0, 0.0), (1.0, 0.0, 0.0), 1.0)
    wire = _body_geometry.WireGeometry("outer", 1, ((edge, 1),) * occurrence_count)
    token = object()
    build = _body_geometry._FaceBuild(
        FaceGeometry("PLANE", (), 1.0, (0.0, 0.0, 0.0), 1, (wire,)),
        (_body_geometry._WireBuild(wire, (tuple((token, 1) for _ in range(occurrence_count)),)),),
    )
    with pytest.raises(UnsupportedBodyGeometry, match="closed-shell pair"):
        _body_geometry._canonical_topology((build,))


def test_seam_pair_is_supported_but_conflicting_edge_labels_refuse() -> None:
    line = _body_geometry.EdgeGeometry("LINE", (0.0, 0.0, 0.0), (1.0, 0.0, 0.0), 1.0)
    changed = dataclasses.replace(line, length=2.0)
    token = object()

    def build(labels):
        wire = _body_geometry.WireGeometry(
            "outer", 1, tuple((label, direction) for label, direction in labels)
        )
        return _body_geometry._FaceBuild(
            FaceGeometry("PLANE", (), 1.0, (0.0, 0.0, 0.0), 1, (wire,)),
            (
                _body_geometry._WireBuild(
                    wire,
                    (tuple((token, direction) for _label, direction in labels),),
                ),
            ),
        )

    _body_geometry._canonical_topology((build(((line, 1), (line, -1))),))
    with pytest.raises(UnsupportedBodyGeometry, match="conflicting analytic labels"):
        _body_geometry._canonical_topology((build(((line, 1), (changed, -1))),))


def test_numeric_degeneracy_and_reconstruction_boundaries_are_inclusive(monkeypatch) -> None:
    quantum = 0.25
    assert _body_geometry._positive_fact(quantum, quantum, name="fact") == quantum
    with pytest.raises(UnsupportedBodyGeometry, match="degenerate"):
        _body_geometry._positive_fact(math.nextafter(quantum, 0.0), quantum, name="fact")

    monkeypatch.setattr(_body_geometry, "_snap", lambda value, _quantum: value + 2.0 * quantum)
    assert _body_geometry._snap_checked(1.0, quantum, name="fact") == 1.5
    monkeypatch.setattr(
        _body_geometry,
        "_snap",
        lambda value, _quantum: math.nextafter(value + 2.0 * quantum, math.inf),
    )
    with pytest.raises(UnsupportedBodyGeometry, match="reconstruction"):
        _body_geometry._snap_checked(1.0, quantum, name="fact")


def test_vector_reconstruction_uses_combined_world_distance(monkeypatch) -> None:
    quantum = 0.25
    component = 2.0 * quantum / math.sqrt(3.0)
    monkeypatch.setattr(_body_geometry, "_snap", lambda value, _quantum: value + component)
    _body_geometry._relative_point((0.0, 0.0, 0.0), quantum, name="axis point")

    outside = math.nextafter(component, math.inf)
    monkeypatch.setattr(_body_geometry, "_snap", lambda value, _quantum: value + outside)
    with pytest.raises(UnsupportedBodyGeometry, match="axis point"):
        _body_geometry._relative_point((0.0, 0.0, 0.0), quantum, name="axis point")


def test_every_descriptor_numeric_field_routes_through_closed_validators(monkeypatch) -> None:
    part = _rrp(7)
    oracle = _raw_body_oracle(part)
    scale = max(oracle["volume"] ** (1 / 3), math.sqrt(oracle["surface_area"]))
    metric = _body_geometry._metric_tolerance(scale)
    area = (scale + metric) ** 2 - scale**2
    volume = (scale + metric) ** 3 - scale**3
    moment = (scale + metric) ** 5 - scale**5
    scalar_calls: list[tuple[str, float]] = []
    vector_calls: list[tuple[str, float]] = []
    axis_calls = 0
    original_scalar = _body_geometry._snap_checked
    original_vector = _body_geometry._relative_point
    original_axis = _body_geometry._qaxis

    def scalar(value, quantum, *, name):
        scalar_calls.append((name, quantum))
        return original_scalar(value, quantum, name=name)

    def vector(raw, quantum, *, name):
        vector_calls.append((name, quantum))
        return original_vector(raw, quantum, name=name)

    def axis(raw):
        nonlocal axis_calls
        axis_calls += 1
        return original_axis(raw)

    monkeypatch.setattr(_body_geometry, "_snap_checked", scalar)
    monkeypatch.setattr(_body_geometry, "_relative_point", vector)
    monkeypatch.setattr(_body_geometry, "_qaxis", axis)
    graph, solid, _fact = _body_descriptor(part)
    graph.matching_boundary(solid)

    expected_scalar_quantum = {
        "plane offset": metric,
        "pcurve u": metric,
        "pcurve v": metric,
        "edge length": metric,
        "circle radius": metric,
        "circle sweep": _body_geometry.ANGLE_TOL,
        "cylinder radius": metric,
        "cylinder theta": _body_geometry.ANGLE_TOL,
        "cylinder z": metric,
        "face area": area,
        "body volume": volume,
        "body surface area": area,
        "principal moment": moment,
    }
    assert set(expected_scalar_quantum) <= {name for name, _quantum in scalar_calls}
    for name, quantum in scalar_calls:
        assert quantum == pytest.approx(expected_scalar_quantum[name])
    expected_vector_names = {
        "edge endpoint",
        "circle centre",
        "face centroid",
        "cylinder axis point",
    }
    assert expected_vector_names <= {name for name, _quantum in vector_calls}
    assert all(quantum == pytest.approx(metric) for _name, quantum in vector_calls)
    assert axis_calls > 0

    # Every production scalar quantum uses the same inclusive closed validator. Exercise the
    # exact equality and nextafter-outside reconstruction rule for each caller-supplied quantum.
    for name, quantum in expected_scalar_quantum.items():
        with monkeypatch.context() as boundary:
            boundary.setattr(
                _body_geometry,
                "_snap",
                lambda value, _q, quantum=quantum: value + 2.0 * quantum,
            )
            assert _body_geometry._snap_checked(0.0, quantum, name=name) == 2.0 * quantum
        with monkeypatch.context() as boundary:
            boundary.setattr(
                _body_geometry,
                "_snap",
                lambda value, _q, quantum=quantum: math.nextafter(value + 2.0 * quantum, math.inf),
            )
            with pytest.raises(UnsupportedBodyGeometry, match="reconstruction"):
                _body_geometry._snap_checked(0.0, quantum, name=name)


def test_direction_quantization_boundaries_are_inclusive(monkeypatch) -> None:
    tolerance = _body_geometry.DIRECTION_TOL
    component = 2.0 * tolerance / math.sqrt(2.0)
    raw = (1.0, 0.0, 0.0)
    monkeypatch.setattr(
        _body_geometry,
        "_snap",
        lambda value, _quantum: value + (component if value == 0.0 else 0.0),
    )
    _body_geometry._qaxis(raw)
    outside = math.nextafter(component, math.inf)
    monkeypatch.setattr(
        _body_geometry,
        "_snap",
        lambda value, _quantum: value + (outside if value == 0.0 else 0.0),
    )
    with pytest.raises(UnsupportedBodyGeometry, match="direction"):
        _body_geometry._qaxis(raw)


@pytest.mark.parametrize("scale", [1e-3, 1e3])
def test_characteristic_quanta_remain_finite_at_supported_scale_extremes(scale: float) -> None:
    descriptor = _body_descriptor(_line_rrp(5).scale(scale))[2].descriptor
    assert descriptor.intrinsic.volume > 0.0
    assert descriptor.intrinsic.surface_area > 0.0
    assert all(math.isfinite(value) for value in _numbers(descriptor))


def test_plane_axis_parameterization_flip_is_identical_at_nonzero_offset() -> None:
    positive = _body_geometry._plane_parameters(
        (1.0, 0.0, 0.0), (7.0, 2.0, 3.0), (2.0, 2.0, 3.0), 1e-7
    )
    negative = _body_geometry._plane_parameters(
        (-1.0, 0.0, 0.0), (7.0, 2.0, 3.0), (2.0, 2.0, 3.0), 1e-7
    )
    assert positive == negative == (1.0, 0.0, 0.0, 5.0)


def test_complete_incidence_distinguishes_equal_labelled_nonisomorphic_graphs() -> None:
    edge = _body_geometry.EdgeGeometry("LINE", (0.0, 0.0, 0.0), (1.0, 0.0, 0.0), 1.0)
    wire = _body_geometry.WireGeometry("outer", 1, ((edge, 1), (edge, 1)))
    face = FaceGeometry("PLANE", (), 1.0, (0.0, 0.0, 0.0), 1, (wire,))

    def builds(pairs):
        occurrences = [[] for _ in range(4)]
        for token, (left, right) in enumerate(pairs):
            occurrences[left].append((token, 1))
            occurrences[right].append((token, 1))
        return tuple(
            _body_geometry._FaceBuild(
                face,
                (_body_geometry._WireBuild(wire, (tuple(items),)),),
            )
            for items in occurrences
        )

    cycle = builds(((0, 1), (1, 2), (2, 3), (3, 0)))
    doubled = builds(((0, 1), (0, 1), (2, 3), (2, 3)))
    assert _body_geometry._canonical_topology(cycle) != _body_geometry._canonical_topology(doubled)


def test_wire_wrapper_reversal_normalizes_but_material_orientation_survives() -> None:
    first = _body_geometry.EdgeGeometry("LINE", (0.0, 0.0, 0.0), (1.0, 0.0, 0.0), 1.0)
    second = _body_geometry.EdgeGeometry("LINE", (0.0, 1.0, 0.0), (2.0, 1.0, 0.0), 2.0)
    items = ((first, 1, "a"), (second, 1, "b"))
    direct = _body_geometry._canonical_cycle_with_tokens(items, 1)
    shallow_reversal = _body_geometry._canonical_cycle_with_tokens(
        tuple((edge, -direction, token) for edge, direction, token in reversed(items)), -1
    )
    material_reversal = _body_geometry._canonical_cycle_with_tokens(items, -1)

    assert direct[0] == shallow_reversal[0]
    assert direct[2] == shallow_reversal[2]
    assert direct[2] != material_reversal[2]


def test_snapshot_contains_only_exact_accepted_rrp_occurrences() -> None:
    product = _take_inventory(_rrp(7))
    physical = product.physical.candidate_set(FamilyId.REPEATING_RADIAL_PROFILES)
    accepted = product.reconciliation.accepted_set(physical)

    snapshot = correspondence_snapshot(product)

    assert CORRESPONDENCE_FAMILIES == (FamilyId.REPEATING_RADIAL_PROFILES,)
    assert len(snapshot.occurrences) == len(physical.candidates) == len(accepted.candidates) == 1
    occurrence = snapshot.occurrences[0]
    assert occurrence.family == FamilyId.REPEATING_RADIAL_PROFILES.value
    assert occurrence.record_type == "RepeatingRadialProfile"
    assert occurrence.summary.repeat_count == 7
    assert len(occurrence.summary.defining) == 2
    assert correspondence_snapshot(product) is snapshot


@pytest.mark.parametrize(
    "part, expected_axis, repeats",
    [
        (_line_rrp(5), "z", 5),
        (Rot(90, 0, 0) * _rrp(7), "y", 7),
        (Rot(0, 90, 0) * _line_rrp(8), "x", 8),
    ],
)
def test_accepted_snapshot_roster_covers_principal_axes_and_mixed_curves(
    part, expected_axis: str, repeats: int
) -> None:
    snapshot = correspondence_snapshot(_take_inventory(part))
    assert len(snapshot.occurrences) == 1
    summary = snapshot.occurrences[0].summary
    assert summary.axis == expected_axis
    assert summary.repeat_count == repeats
    kinds = {
        edge.kind
        for face in summary.defining
        for wire in face.wires
        for edge, _direction in wire.edges
    }
    assert kinds == {sector[0] for sector in summary.sector_signature}


def test_equal_coincident_bodies_retain_two_indistinguishable_occurrences() -> None:
    product = _take_inventory(Compound([_rrp(), _rrp()]))
    snapshot = correspondence_snapshot(product)

    assert len(snapshot.occurrences) == 2
    assert snapshot.occurrences[0] == snapshot.occurrences[1]
    assert snapshot.schema_version == 3
    assert snapshot.body_groups == ((0,), (1,))


def test_two_unequal_occurrences_on_one_valid_solid_retain_one_body_authority() -> None:
    part = _two_rrp_one_solid()
    snapshot = correspondence_snapshot(_take_inventory(part))
    assert len(snapshot.occurrences) == 2
    assert [item.summary.repeat_count for item in snapshot.occurrences] == [5, 7]
    assert snapshot.occurrences[0].body is snapshot.occurrences[1].body
    assert snapshot.occurrences[0].summary.centre != snapshot.occurrences[1].summary.centre
    assert snapshot.body_groups == ((0, 1),)


def test_arbitrary_rotation_changes_no_recognition_and_has_no_snapshot_entry() -> None:
    product = _take_inventory(Rot(13, 27, 9) * _rrp())
    assert not product.result.repeating_radial_profiles
    assert correspondence_snapshot(product).occurrences == ()
    assert correspondence_snapshot(product).body_groups == ()


def test_snapshot_revalidates_raw_derived_quantization_authority() -> None:
    product = _take_inventory(_rrp())
    snapshot = correspondence_snapshot(product)
    quantization = snapshot.occurrences[0].body.quantization

    assert quantization.metric_quantum == pytest.approx(
        _body_geometry.DESCRIPTOR_REL * quantization.characteristic_scale
        + _body_geometry.DESCRIPTOR_FLOOR
    )
    object.__setattr__(
        quantization,
        "metric_quantum",
        math.nextafter(quantization.metric_quantum, math.inf),
    )
    with pytest.raises(CorrespondenceSnapshotError, match="occurrence values changed"):
        correspondence_snapshot(product)


def test_snapshot_refuses_consistently_reforged_quantization_and_occurrence() -> None:
    product = _take_inventory(_rrp())
    snapshot = correspondence_snapshot(product)
    occurrence = snapshot.occurrences[0]
    quantization = occurrence.body.quantization
    scale = quantization.characteristic_scale * 2.0
    metric = _body_geometry.DESCRIPTOR_REL * scale + _body_geometry.DESCRIPTOR_FLOOR
    object.__setattr__(quantization, "characteristic_scale", scale)
    object.__setattr__(quantization, "metric_quantum", metric)
    object.__setattr__(quantization, "area_quantum", (scale + metric) ** 2 - scale**2)
    object.__setattr__(quantization, "volume_quantum", (scale + metric) ** 3 - scale**3)
    object.__setattr__(quantization, "moment_quantum", (scale + metric) ** 5 - scale**5)
    with pytest.raises(CorrespondenceSnapshotError, match="occurrence values changed"):
        correspondence_snapshot(product)

    other = _take_inventory(_rrp())
    other_snapshot = correspondence_snapshot(other)
    object.__setattr__(other_snapshot.occurrences[0], "family", "forged")
    with pytest.raises(CorrespondenceSnapshotError, match="occurrence values changed"):
        correspondence_snapshot(other)


def test_first_read_invalid_quantization_maps_to_snapshot_error(monkeypatch) -> None:
    product = _take_inventory(_rrp())
    original = FaceGraph.body_geometry

    def invalid(self, solid):
        fact = original(self, solid)
        object.__setattr__(fact.descriptor.quantization, "metric_quantum", 0.0)
        return fact

    monkeypatch.setattr(FaceGraph, "body_geometry", invalid)
    with pytest.raises(CorrespondenceSnapshotError, match="quantization is unavailable"):
        correspondence_snapshot(product)


def test_snapshot_revalidates_body_group_partition() -> None:
    product = _take_inventory(_two_rrp_one_solid())
    snapshot = correspondence_snapshot(product)
    object.__setattr__(snapshot, "body_groups", ((0,), (1,)))

    # Splitting one issuer-proved body into two groups is not made valid merely because both
    # occurrences carry equal descriptor values.
    with pytest.raises(CorrespondenceSnapshotError, match="body groups"):
        correspondence_snapshot(product)


@pytest.mark.parametrize("scale", [0.0, math.nan, math.inf])
def test_descriptor_quantization_refuses_invalid_characteristic_scale(scale: float) -> None:
    product = _take_inventory(_rrp())
    quantization = correspondence_snapshot(product).occurrences[0].body.quantization
    changed = dataclasses.replace(quantization, characteristic_scale=scale)
    with pytest.raises(UnsupportedBodyGeometry, match="characteristic scale"):
        _body_geometry.validate_descriptor_quantization(changed)


@pytest.mark.parametrize("value", [None, True, (1.0,), "invalid"])
def test_descriptor_quantization_refuses_wrong_runtime_types(value: object) -> None:
    if value is None or type(value) is not _body_geometry.DescriptorQuantization:
        with pytest.raises(UnsupportedBodyGeometry, match="runtime types"):
            _body_geometry.validate_descriptor_quantization(value)  # type: ignore[arg-type]

    product = _take_inventory(_rrp())
    snapshot = correspondence_snapshot(product)
    object.__setattr__(snapshot.occurrences[0].body, "quantization", value)
    with pytest.raises(CorrespondenceSnapshotError, match="occurrence values changed"):
        correspondence_snapshot(product)


def test_schema2_snapshot_validator_closes_schema_partition_and_group_geometry() -> None:
    one = correspondence_snapshot(_take_inventory(_rrp()))
    with pytest.raises(CorrespondenceSnapshotError, match="schema is unsupported"):
        correspondence_module._validate_snapshot(dataclasses.replace(one, schema_version=1))
    with pytest.raises(CorrespondenceSnapshotError, match="schema is malformed"):
        correspondence_module._validate_snapshot(dataclasses.replace(one, schema_version=True))
    for malformed in (
        dataclasses.replace(one, occurrences=list(one.occurrences)),
        dataclasses.replace(one, body_groups=list(one.body_groups)),
    ):
        with pytest.raises(CorrespondenceSnapshotError, match="body groups are malformed"):
            correspondence_module._validate_snapshot(malformed)
    with pytest.raises(CorrespondenceSnapshotError, match="occurrence schema is malformed"):
        correspondence_module._validate_snapshot(dataclasses.replace(one, occurrences=(object(),)))
    malformed_body = dataclasses.replace(one.occurrences[0], body=object())
    with pytest.raises(CorrespondenceSnapshotError, match="occurrence schema is malformed"):
        correspondence_module._validate_snapshot(
            dataclasses.replace(one, occurrences=(malformed_body,))
        )
    for groups in (((False,),), ((0.0,),), ([0],)):
        with pytest.raises(CorrespondenceSnapshotError, match="body groups are malformed"):
            correspondence_module._validate_snapshot(dataclasses.replace(one, body_groups=groups))
    for groups in (((0,), ()), ((1,),), ((0, 0),)):
        with pytest.raises(CorrespondenceSnapshotError, match="complete partition"):
            correspondence_module._validate_snapshot(dataclasses.replace(one, body_groups=groups))

    two = correspondence_snapshot(
        _take_inventory(Compound([Pos(-50, 0, 0) * _rrp(5), Pos(50, 0, 0) * _rrp(7)]))
    )
    assert two.body_groups == ((0,), (1,))
    with pytest.raises(CorrespondenceSnapshotError, match="unequal geometry"):
        correspondence_module._validate_snapshot(dataclasses.replace(two, body_groups=((0, 1),)))

    invalid_quantization = dataclasses.replace(
        one.occurrences[0].body.quantization,
        metric_quantum=0.0,
    )
    invalid_body = dataclasses.replace(
        one.occurrences[0].body,
        quantization=invalid_quantization,
    )
    invalid_occurrence = dataclasses.replace(one.occurrences[0], body=invalid_body)
    with pytest.raises(CorrespondenceSnapshotError, match="quantization is invalid"):
        correspondence_module._validate_snapshot(
            dataclasses.replace(one, occurrences=(invalid_occurrence,))
        )


def test_cached_snapshot_malformed_occurrence_refuses_before_dereference() -> None:
    product = _take_inventory(_rrp())
    snapshot = correspondence_snapshot(product)
    object.__setattr__(snapshot, "occurrences", (object(),))
    with pytest.raises(CorrespondenceSnapshotError, match="occurrence values changed"):
        correspondence_snapshot(product)


def test_snapshot_is_lazy_and_body_descriptor_runs_once(monkeypatch) -> None:
    calls = 0
    original = FaceGraph.body_geometry

    def counted(self, solid):
        nonlocal calls
        calls += 1
        return original(self, solid)

    monkeypatch.setattr(FaceGraph, "body_geometry", counted)
    product = _take_inventory(_rrp())
    assert calls == 0

    first = correspondence_snapshot(product)
    second = correspondence_snapshot(product)
    assert first is second
    assert calls == 1


def test_late_second_body_failure_returns_no_snapshot_and_can_retry(monkeypatch) -> None:
    part = Compound([Pos(-50, 0, 0) * _rrp(5), Pos(50, 0, 0) * _rrp(7)])
    product = _take_inventory(part)
    result = product.result
    candidate_snapshot = product.physical.candidate_set(
        FamilyId.REPEATING_RADIAL_PROFILES
    ).candidates
    evidence_snapshot = tuple(
        (candidate, product.evidence.defining_of(candidate)) for candidate in candidate_snapshot
    )
    original = FaceGraph.body_geometry
    calls = 0

    def fail_second(self, solid):
        nonlocal calls
        calls += 1
        if calls == 2:
            raise UnsupportedBodyGeometry("controlled late body failure")
        return original(self, solid)

    monkeypatch.setattr(FaceGraph, "body_geometry", fail_second)
    with pytest.raises(CorrespondenceSnapshotError, match="body geometry is unavailable"):
        correspondence_snapshot(product)
    assert product.result is result
    assert (
        tuple(
            (candidate, product.evidence.defining_of(candidate)) for candidate in candidate_snapshot
        )
        == evidence_snapshot
    )
    assert (
        product.physical.candidate_set(FamilyId.REPEATING_RADIAL_PROFILES).candidates
        == candidate_snapshot
    )

    monkeypatch.setattr(FaceGraph, "body_geometry", original)
    snapshot = correspondence_snapshot(product)
    assert len(snapshot.occurrences) == 2


def test_late_second_matching_graph_failure_returns_no_snapshot_and_can_retry(
    monkeypatch,
) -> None:
    product = _take_inventory(Compound([Pos(-50, 0, 0) * _rrp(5), Pos(50, 0, 0) * _rrp(7)]))
    authority = product._correspondence_authority
    assert authority is not None
    original = FaceGraph.matching_boundary
    calls = 0

    def fail_second(self, solid):
        nonlocal calls
        calls += 1
        if calls == 2:
            raise UnsupportedBodyGeometry("controlled late matching failure")
        return original(self, solid)

    monkeypatch.setattr(FaceGraph, "matching_boundary", fail_second)
    with pytest.raises(CorrespondenceSnapshotError, match="matching boundary is unavailable"):
        correspondence_snapshot(product)
    assert authority._snapshot is None
    assert authority._bound_occurrences is None
    assert authority._bound_body_groups is None

    monkeypatch.setattr(FaceGraph, "matching_boundary", original)
    assert len(correspondence_snapshot(product).occurrences) == 2


def test_cross_solid_defining_evidence_refuses_atomically(monkeypatch) -> None:
    part = Compound([Pos(-50, 0, 0) * _rrp(5), Pos(50, 0, 0) * _rrp(7)])
    product = _take_inventory(part)
    graph = product.context.graph
    selected = []
    owners = []
    for node in graph.nodes:
        owner = graph.common_valid_solid((node,))
        if owner is not None and all(owner is not previous for previous in owners):
            owners.append(owner)
            selected.append(node)
    assert len(selected) == 2

    original = EvidenceIndex.defining_of

    def mixed(self, subject):
        if getattr(subject, "family", None) is FamilyId.REPEATING_RADIAL_PROFILES:
            return frozenset(selected)
        return original(self, subject)

    monkeypatch.setattr(EvidenceIndex, "defining_of", mixed)
    with pytest.raises(CorrespondenceSnapshotError, match="one valid solid"):
        correspondence_snapshot(product)


def test_foreign_defining_nodes_refuse_before_body_projection(monkeypatch) -> None:
    product = _take_inventory(_rrp())
    foreign = FaceGraph(Pos(3, 4, 5) * _rrp())
    nodes = foreign.nodes[:2]
    original = EvidenceIndex.defining_of

    def stale(self, subject):
        if getattr(subject, "family", None) is FamilyId.REPEATING_RADIAL_PROFILES:
            return frozenset(nodes)
        return original(self, subject)

    monkeypatch.setattr(EvidenceIndex, "defining_of", stale)
    with pytest.raises(CorrespondenceSnapshotError, match="exactly two original faces"):
        correspondence_snapshot(product)


def test_deep_copied_defining_nodes_refuse_before_body_projection(monkeypatch) -> None:
    product = _take_inventory(_rrp())
    nodes = tuple(copy.deepcopy(node) for node in product.context.graph.nodes[:2])
    original = EvidenceIndex.defining_of

    def stale(self, subject):
        if getattr(subject, "family", None) is FamilyId.REPEATING_RADIAL_PROFILES:
            return frozenset(nodes)
        return original(self, subject)

    monkeypatch.setattr(EvidenceIndex, "defining_of", stale)
    with pytest.raises(CorrespondenceSnapshotError, match="exactly two original faces"):
        correspondence_snapshot(product)


def test_reused_defining_face_refuses_before_body_projection(monkeypatch) -> None:
    product = _take_inventory(_rrp())
    node = product.context.graph.nodes[0]
    original = EvidenceIndex.defining_of

    def reused(self, subject):
        if getattr(subject, "family", None) is FamilyId.REPEATING_RADIAL_PROFILES:
            return (node, node)
        return original(self, subject)

    monkeypatch.setattr(EvidenceIndex, "defining_of", reused)
    with pytest.raises(CorrespondenceSnapshotError, match="exactly two original faces"):
        correspondence_snapshot(product)


def test_copied_or_constructed_inventory_product_cannot_reuse_authority() -> None:
    product = _take_inventory(_rrp())
    copied = dataclasses.replace(product)
    unissued = dataclasses.replace(product, _correspondence_authority=None)

    with pytest.raises(CorrespondenceSnapshotError, match="not this authority"):
        correspondence_snapshot(copied)
    with pytest.raises(CorrespondenceSnapshotError, match="no snapshot authority"):
        correspondence_snapshot(unissued)
    assert correspondence_snapshot(product).occurrences


def test_record_mutation_after_inventory_binding_refuses() -> None:
    product = _take_inventory(_rrp())
    assert correspondence_snapshot(product).occurrences
    candidate = product.physical.candidate_set(FamilyId.REPEATING_RADIAL_PROFILES).candidates[0]
    object.__setattr__(candidate.record, "repeat_count", candidate.record.repeat_count + 1)

    with pytest.raises(CorrespondenceSnapshotError, match="identity or value changed"):
        correspondence_snapshot(product)


def test_bound_product_component_mutation_refuses() -> None:
    product = _take_inventory(_rrp())
    foreign = _take_inventory(_rrp(7))
    object.__setattr__(product, "evidence", foreign.evidence)

    with pytest.raises(CorrespondenceSnapshotError, match="not this authority"):
        correspondence_snapshot(product)


def test_forged_reconciliation_membership_refuses() -> None:
    product = _take_inventory(_rrp())
    object.__setattr__(product.reconciliation, "_membership", frozenset())
    with pytest.raises(CorrespondenceSnapshotError, match="stale or mixed"):
        correspondence_snapshot(product)


def test_wrong_record_type_refuses_authority_binding() -> None:
    product = _take_inventory(_rrp())
    candidate = product.physical.candidate_set(FamilyId.REPEATING_RADIAL_PROFILES).candidates[0]
    object.__setattr__(candidate, "record", object())
    authority = correspondence_module._CorrespondenceSnapshotAuthority()
    with pytest.raises(CorrespondenceSnapshotError, match="stale or mixed"):
        authority.bind(product)


@pytest.mark.parametrize("count", [0, 1, 3])
def test_wrong_defining_face_cardinality_refuses(count: int, monkeypatch) -> None:
    product = _take_inventory(_rrp())
    nodes = product.context.graph.nodes[:count]
    original = EvidenceIndex.defining_of

    def wrong(self, subject):
        if getattr(subject, "family", None) is FamilyId.REPEATING_RADIAL_PROFILES:
            return frozenset(nodes)
        return original(self, subject)

    monkeypatch.setattr(EvidenceIndex, "defining_of", wrong)
    with pytest.raises(CorrespondenceSnapshotError, match="exactly two"):
        correspondence_snapshot(product)


def test_nonplanar_defining_faces_refuse(monkeypatch) -> None:
    product = _take_inventory(_rrp())
    graph = product.context.graph
    nonplanar = tuple(node for node in graph.nodes if not graph.is_planar(node))[:2]
    assert len(nonplanar) == 2
    original = EvidenceIndex.defining_of

    def wrong(self, subject):
        if getattr(subject, "family", None) is FamilyId.REPEATING_RADIAL_PROFILES:
            return frozenset(nonplanar)
        return original(self, subject)

    monkeypatch.setattr(EvidenceIndex, "defining_of", wrong)
    with pytest.raises(CorrespondenceSnapshotError, match="non-planar"):
        correspondence_snapshot(product)


def test_snapshot_is_private_and_changes_no_public_result() -> None:
    before = _take_inventory(_rrp())
    result_before = before.result
    snapshot = correspondence_snapshot(before)

    assert snapshot.occurrences
    assert before.result is result_before
    assert "correspondence_snapshot" not in quiddity.__all__
    assert not hasattr(quiddity, "CorrespondenceSnapshot")


def test_private_correspondence_layering_and_handle_guards_are_closed() -> None:
    lower_path = ROOT / "src/quiddity/_body_geometry.py"
    upper_path = ROOT / "src/quiddity/_correspondence.py"
    lower = ast.parse(lower_path.read_text())
    upper = ast.parse(upper_path.read_text())

    forbidden_lower = {
        "_candidates",
        "_claims",
        "_registry",
        "_reconcile",
        "_dispositions",
        "result",
    }
    lower_imports = {
        node.module.rsplit(".", 1)[-1]
        for node in ast.walk(lower)
        if isinstance(node, ast.ImportFrom) and node.module
    }
    assert lower_imports.isdisjoint(forbidden_lower)

    forbidden_attributes = {"ordinal", "index"}
    for tree in (lower, upper):
        assert (
            not {node.attr for node in ast.walk(tree) if isinstance(node, ast.Attribute)}
            & forbidden_attributes
        )

    body_callers = {
        node.name
        for node in ast.walk(upper)
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))
        and any(
            isinstance(item, ast.Call)
            and isinstance(item.func, ast.Attribute)
            and item.func.attr == "body_geometry"
            for item in ast.walk(node)
        )
    }
    assert body_callers == {"_occurrence"}

    source_paths = tuple((ROOT / "src/quiddity").glob("*.py"))
    all_body_callers = []
    correspondence_importers = []
    for path in source_paths:
        tree = ast.parse(path.read_text())
        body_call_nodes = {id(node) for node in _alias_aware_calls(tree, "body_geometry")}
        for node in ast.walk(tree):
            if isinstance(node, ast.ImportFrom) and node.module == "quiddity._correspondence":
                correspondence_importers.append(path.name)
            if id(node) in body_call_nodes:
                owner = next(
                    (
                        parent.name
                        for parent in ast.walk(tree)
                        if isinstance(parent, (ast.FunctionDef, ast.AsyncFunctionDef))
                        and node in tuple(ast.walk(parent))
                    ),
                    None,
                )
                all_body_callers.append((path.name, owner))
    assert set(all_body_callers) == {
        ("_correspondence.py", "_occurrence"),
        ("_adjacency.py", "matching_boundary"),
    }
    matching_callers = set()
    for path in source_paths:
        tree = ast.parse(path.read_text())
        for node in _alias_aware_calls(tree, "matching_boundary"):
            owner = next(
                (
                    parent.name
                    for parent in ast.walk(tree)
                    if isinstance(parent, (ast.FunctionDef, ast.AsyncFunctionDef))
                    and node in tuple(ast.walk(parent))
                ),
                None,
            )
            matching_callers.add((path.name, owner))
    assert matching_callers == {("_correspondence.py", "_occurrence")}
    assert set(correspondence_importers) == {"_correspondence_match.py", "result.py"}

    lower_calls = {
        target: tuple(ast.unparse(node.func) for node in _alias_aware_calls(lower, target))
        for target in {
            "VolumeProperties_s",
            "SurfaceProperties_s",
            "Plane",
            "Cylinder",
            "Circle",
        }
    }
    assert lower_calls == {
        "VolumeProperties_s": ("BRepGProp.VolumeProperties_s",),
        "SurfaceProperties_s": ("BRepGProp.SurfaceProperties_s",),
        "Plane": ("adaptor.Plane",),
        "Cylinder": ("adaptor.Cylinder", "surface.Cylinder"),
        "Circle": ("curve.Circle",),
    }
    upper_names = {node.id for node in ast.walk(upper) if isinstance(node, ast.Name)}
    assert {"CORRESPONDENCE_FAMILIES", "RepeatingRadialProfile", "accepted"} <= upper_names
    assert not (
        {"digest", "hash", "unchanged", "moved", "resized", "split", "merged"} & upper_names
    )
    assert "correspondence_snapshot" not in quiddity.__all__
    assert not hasattr(quiddity, "CorrespondenceSnapshot")


@pytest.mark.parametrize(
    "source",
    [
        "def f(graph, solid):\n    return graph.body_geometry(solid)\n",
        "def f(graph, solid):\n    query = graph.body_geometry\n    return query(solid)\n",
        "from x import body_geometry as query\ndef f(solid):\n    return query(solid)\n",
        "import package\ndef f(graph, solid):\n    return package.graph.body_geometry(solid)\n",
    ],
)
def test_alias_aware_body_query_guard_detects_every_supported_call_form(source: str) -> None:
    assert len(_alias_aware_calls(ast.parse(source), "body_geometry")) == 1


def test_snapshot_values_contain_no_run_or_kernel_handles() -> None:
    snapshot = correspondence_snapshot(_take_inventory(_rrp()))

    def visit(value):
        if dataclasses.is_dataclass(value):
            for item in dataclasses.fields(value):
                yield from visit(getattr(value, item.name))
        elif isinstance(value, tuple):
            for item in value:
                yield from visit(item)
        else:
            yield value

    leaves = tuple(visit(snapshot))
    assert all(value is None or isinstance(value, (bool, int, float, str)) for value in leaves)


@pytest.mark.parametrize(
    ("value", "message"),
    [
        (float("nan"), "non-finite"),
        ({1: "value"}, "non-string key"),
        (object(), "unsupported object state"),
    ],
)
def test_snapshot_value_freezer_refuses_unstable_state(value, message: str) -> None:
    with pytest.raises(CorrespondenceSnapshotError, match=message):
        correspondence_module._freeze(value)


def test_snapshot_value_freezer_normalizes_nested_and_negative_zero() -> None:
    assert correspondence_module._freeze({"b": [-0.0], "a": None}) == (
        ("a", None),
        ("b", (0.0,)),
    )


@pytest.mark.parametrize(
    "call",
    [
        lambda: _body_geometry._finite(float("inf")),
        lambda: _body_geometry._snap(1.0, 0.0),
        lambda: _body_geometry._vector(Vector(2.0, 0.0, 0.0).wrapped),
        lambda: _body_geometry._canonical_cycle(()),
        lambda: _body_geometry._canonical_cycle_with_tokens((), 1),
    ],
)
def test_low_level_descriptor_refusals_are_named(call) -> None:
    with pytest.raises(UnsupportedBodyGeometry):
        call()


def test_positive_fact_refuses_quantization_collapse(monkeypatch) -> None:
    monkeypatch.setattr(_body_geometry, "_snap_checked", lambda *_args, **_kwargs: 0.0)
    with pytest.raises(UnsupportedBodyGeometry, match="collapses"):
        _body_geometry._positive_fact(1.0, 0.1, name="controlled fact")


def test_quantized_axis_refuses_nonunit_serialization(monkeypatch) -> None:
    monkeypatch.setattr(_body_geometry, "_snap", lambda _value, _quantum: 0.0)
    with pytest.raises(UnsupportedBodyGeometry, match="unit length"):
        _body_geometry._qaxis((1.0, 0.0, 0.0))


def test_ambiguous_wire_semantic_winding_refuses() -> None:
    edge = _body_geometry.EdgeGeometry("LINE", (0.0, 0.0, 0.0), (1.0, 0.0, 0.0), 1.0)
    items = ((edge, 1, "a"), (edge, -1, "b"))
    with pytest.raises(UnsupportedBodyGeometry, match="semantic winding is ambiguous"):
        _body_geometry._canonical_cycle_with_tokens(items, 1)


def test_degenerate_circle_radius_refuses(monkeypatch) -> None:
    edge = Edge.make_circle(1.0)
    monkeypatch.setattr(type(edge), "radius", property(lambda _self: 0.0))
    with pytest.raises(UnsupportedBodyGeometry, match="circle radius"):
        _body_geometry._arc_sweep(edge, (0.0, 0.0, 1.0))


def test_degenerate_circle_sweep_refuses(monkeypatch) -> None:
    edge = Edge.make_circle(1.0)
    monkeypatch.setattr(type(edge), "length", property(lambda _self: 1e-12))
    with pytest.raises(UnsupportedBodyGeometry, match="circle sweep"):
        _body_geometry._arc_sweep(edge, (0.0, 0.0, 1.0))


def test_degenerate_face_area_refuses(monkeypatch) -> None:
    face = Box(1, 1, 1).faces()[0]
    monkeypatch.setattr(type(face), "area", property(lambda _self: 0.0))
    with pytest.raises(UnsupportedBodyGeometry, match="face area"):
        _body_geometry._face_geometry(face, (0.0, 0.0, 0.0), 1.0)


def test_snapshot_authority_cannot_bind_twice() -> None:
    product = _take_inventory(_rrp())
    authority = correspondence_module._CorrespondenceSnapshotAuthority()
    authority.bind(product)
    with pytest.raises(CorrespondenceSnapshotError, match="already bound"):
        authority.bind(product)


def test_body_fact_solid_identity_is_revalidated(monkeypatch) -> None:
    product = _take_inventory(_rrp())
    original = FaceGraph.body_geometry

    def wrong_solid(self, solid):
        fact = original(self, solid)
        return dataclasses.replace(fact, _solid=copy.copy(solid))

    monkeypatch.setattr(FaceGraph, "body_geometry", wrong_solid)
    with pytest.raises(CorrespondenceSnapshotError, match="lost its graph-issued solid"):
        correspondence_snapshot(product)


def test_defining_face_authority_failure_is_wrapped(monkeypatch) -> None:
    product = _take_inventory(_rrp())

    def refuse(_self, _node):
        raise BodyGeometryAuthorityError("controlled missing face")

    monkeypatch.setattr(
        "quiddity._adjacency.BodyGeometryFact._defining_face",
        refuse,
    )
    with pytest.raises(CorrespondenceSnapshotError, match="defining face geometry"):
        correspondence_snapshot(product)


def test_body_fact_rejects_a_nonmember_face_node() -> None:
    graph, solid, fact = _body_descriptor(_rrp())
    foreign = FaceGraph(Pos(3, 4, 5) * _rrp()).nodes[0]
    assert graph.owns(fact._faces[0][0])
    with pytest.raises(BodyGeometryAuthorityError, match="not part"):
        fact._defining_face(foreign)


def test_body_geometry_revalidates_reference_identity_and_closed_membership() -> None:
    graph, solid, _fact = _body_descriptor(_rrp())
    copied = copy.copy(solid)
    graph._issued_solid_refs[copied] = copied.ordinal
    with pytest.raises(BodyGeometryAuthorityError, match="identity changed"):
        graph.body_geometry(copied)

    graph._body_geometry.clear()
    assert graph._closed_solids is not None
    graph._closed_solids = frozenset()
    with pytest.raises(BodyGeometryAuthorityError, match="valid closed solid"):
        graph.body_geometry(solid)


def test_body_geometry_refuses_unowned_described_face(monkeypatch) -> None:
    graph = FaceGraph(_rrp())
    solid = graph.common_valid_solid(graph.nodes)
    assert solid is not None
    monkeypatch.setattr(FaceGraph, "node_of", lambda _self, _face: None)
    with pytest.raises(BodyGeometryAuthorityError, match="face is not owned"):
        graph.body_geometry(solid)


def test_evidence_index_rejects_a_different_graph_run() -> None:
    product = _take_inventory(_rrp())
    foreign = FaceGraph(_rrp())
    with pytest.raises(ValueError, match="another graph run"):
        product.evidence._validate_graph(foreign)


def test_plain_cycle_canonicalization_normalizes_reversal() -> None:
    first = _body_geometry.EdgeGeometry("LINE", (0.0, 0.0, 0.0), (1.0, 0.0, 0.0), 1.0)
    second = _body_geometry.EdgeGeometry("LINE", (1.0, 0.0, 0.0), (1.0, 1.0, 0.0), 1.0)
    direct = ((first, 1), (second, 1))
    reversed_items = tuple((edge, -direction) for edge, direction in reversed(direct))
    assert _body_geometry._canonical_cycle(direct) == _body_geometry._canonical_cycle(
        reversed_items
    )
