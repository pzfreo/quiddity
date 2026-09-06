# SPDX-License-Identifier: Apache-2.0
"""Occurrence-safe defining evidence for principal-axis Double-D bores."""

from __future__ import annotations

import ast
import copy
from math import asin, inf, nan, sqrt
from pathlib import Path
from typing import cast

import pytest
from build123d import (
    Align,
    Box,
    Circle,
    Compound,
    Cylinder,
    Face,
    GeomType,
    Pos,
    Rectangle,
    Rot,
    Shell,
    Solid,
    Sphere,
    Vector,
    export_step,
    import_step,
    loft,
)
from OCP.BRepAdaptor import BRepAdaptor_Surface

from quiddity import build_recognition_report, recognise_holes
from quiddity._adjacency import FaceEdges, FaceGraph, edge_face_map
from quiddity._candidates import FamilyId
from quiddity._claims import ClaimLedger
from quiddity._dispositions import Outcome, ReasonCode
from quiddity.profiled_bores import (
    _discover_double_d_bores,
    _valid_wall_chain_facts,
    recognise_double_d_bores,
)
from quiddity.result import _take_inventory

_CENTRE = (Align.CENTER, Align.CENTER, Align.CENTER)


def _chain_facts(
    first: tuple[str, ...] = ("a",),
    *,
    first_intervals: dict[str, tuple[float, float]] | None = None,
    first_edges: tuple[tuple[str, str], ...] = (),
    high: tuple[int, ...] = (0, 1, 2, 3),
) -> bool:
    chains = (first, ("b",), ("c",), ("d",))
    intervals: dict[str, tuple[float, float]] = {
        "b": (-5.0, 5.0),
        "c": (-5.0, 5.0),
        "d": (-5.0, 5.0),
    }
    intervals.update(first_intervals or {"a": (-5, 5)})
    ids = {name: at for at, name in enumerate(intervals)}
    return _valid_wall_chain_facts(
        tuple(tuple(ids[name] for name in chain) for chain in chains),
        high,
        {ids[name]: interval for name, interval in intervals.items()},
        tuple((ids[left], ids[right]) for left, right in first_edges),
        lo=-5,
        hi=5,
        tol=1e-6,
    )


def test_pure_wall_chain_reducer_accepts_full_and_consecutive_patch_roles() -> None:
    assert _chain_facts()
    assert _chain_facts(
        ("a0", "a1", "a2"),
        first_intervals={"a0": (-5, -1), "a1": (-1, 2), "a2": (2, 5)},
        first_edges=(("a0", "a1"), ("a1", "a2")),
    )


@pytest.mark.parametrize(
    ("first", "intervals", "edges", "high"),
    [
        (("a0", "a1"), {"a0": (-5, -1), "a1": (0, 5)}, (("a0", "a1"),), (0, 1, 2, 3)),
        (("a0", "a1"), {"a0": (-5, 1), "a1": (0, 5)}, (("a0", "a1"),), (0, 1, 2, 3)),
        (
            ("a0", "a1", "a2"),
            {"a0": (-5, -1), "a1": (-1, 2), "a2": (2, 5)},
            (("a0", "a1"), ("a1", "a2"), ("a0", "a2")),
            (0, 1, 2, 3),
        ),
        (
            ("a0", "a1", "a2", "a3"),
            {"a0": (-5, -2), "a1": (-2, 0), "a2": (0, 2), "a3": (2, 5)},
            (("a0", "a1"), ("a1", "a2"), ("a1", "a3")),
            (0, 1, 2, 3),
        ),
        (
            ("a0", "a1"),
            {"a0": (-5, 0), "a1": (0, 5)},
            (("a0", "a1"), ("a0", "a1")),
            (0, 1, 2, 3),
        ),
        (("a",), {"a": (-5, 5)}, (), (0, 0, 2, 3)),
        (("a", "b"), {"a": (-5, 0)}, (("a", "b"),), (0, 1, 2, 3)),
    ],
)
def test_pure_wall_chain_reducer_refuses_gap_overlap_cycle_and_role_ambiguity(
    first, intervals, edges, high
) -> None:
    assert not _chain_facts(first, first_intervals=intervals, first_edges=edges, high=high)


@pytest.mark.parametrize(
    ("intervals", "edges", "lo", "hi", "tol"),
    [
        ({0: (-5, 5), 1: (-5, 5), 2: (-5, 5)}, (), -5, 5, 1e-6),
        ({0: (-5, 5), 1: (-5, 5), 2: (-5, 5), 3: (-5, 5), 4: (0, 1)}, (), -5, 5, 1e-6),
        ({0: (-5, nan), 1: (-5, 5), 2: (-5, 5), 3: (-5, 5)}, (), -5, 5, 1e-6),
        ({0: (-5, inf), 1: (-5, 5), 2: (-5, 5), 3: (-5, 5)}, (), -5, 5, 1e-6),
        ({0: (5, -5), 1: (-5, 5), 2: (-5, 5), 3: (-5, 5)}, (), -5, 5, 1e-6),
        ({0: (-5, 5), 1: (-5, 5), 2: (-5, 5), 3: (-5, 5)}, ((0, 9),), -5, 5, 1e-6),
        ({0: (-5, 5), 1: (-5, 5), 2: (-5, 5), 3: (-5, 5)}, (), -5, 5, nan),
    ],
)
def test_wall_chain_fact_boundary_fails_closed_on_malformed_snapshots(
    intervals, edges, lo, hi, tol
) -> None:
    assert not _valid_wall_chain_facts(
        ((0,), (1,), (2,), (3,)),
        (0, 1, 2, 3),
        intervals,
        edges,
        lo=lo,
        hi=hi,
        tol=tol,
    )


def test_wall_chain_fact_boundary_refuses_structural_shape_errors() -> None:
    base_intervals = {0: (-5.0, 5.0), 1: (-5.0, 5.0), 2: (-5.0, 5.0), 3: (-5.0, 5.0)}
    cases: list[
        tuple[
            tuple[tuple[int, ...], ...],
            tuple[int, ...],
            dict[int, tuple[float, float]],
            tuple[tuple[int, int], ...],
        ]
    ] = [
        (((0,), (1,), (2,)), (0, 1, 2), base_intervals, ()),
        (((0,), (0,), (2,), (3,)), (0, 1, 2, 3), {0: (-5, 5), 2: (-5, 5), 3: (-5, 5)}, ()),
        (((0,), (1,), (2,), (3,)), (0, 1, 2, 3), {**base_intervals, 0: (-5, 4)}, ()),
        (((0,), (1,), (2,), (3,)), (0, 1, 2, 3), base_intervals, ((0, 0),)),
        (
            ((0, 4), (1,), (2,), (3,)),
            (0, 1, 2, 3),
            {**base_intervals, 0: (-5, 0), 4: (0, 5)},
            ((0, 0),),
        ),
    ]
    for chains, high, intervals, edges in cases:
        assert not _valid_wall_chain_facts(
            chains,
            high,
            intervals,
            edges,
            lo=-5,
            hi=5,
            tol=1e-6,
        )


def test_real_face_adapter_retains_three_consecutive_wall_patches_and_refuses_issuance(
    monkeypatch,
) -> None:
    """Exercise geometry-to-facts adaptation where OCCT normally heals the wall."""
    import quiddity.profiled_bores as module

    part = _plate()
    bbox = part.bounding_box()
    boundary = [
        face
        for face in part.faces()
        if face.geom_type == GeomType.PLANE
        and abs(face.bounding_box().min.Z - face.bounding_box().max.Z) < 1e-9
    ]
    low_face = next(face for face in boundary if face.bounding_box().min.Z < 0)
    high_face = next(face for face in boundary if face.bounding_box().min.Z > 0)
    low_wire = low_face.inner_wires()[0]
    high_wire = high_face.inner_wires()[0]
    profile = module.double_d_profile(low_wire, ("x", "y"), tol=1e-5)
    assert profile is not None

    native_incidence = edge_face_map(part.faces(), face_edges=FaceEdges())
    low_owners = [
        next(face for face in native_incidence[edge] if not face.wrapped.IsSame(low_face.wrapped))
        for edge in low_wire.edges()
    ]
    high_owners = [
        next(face for face in native_incidence[edge] if not face.wrapped.IsSame(high_face.wrapped))
        for edge in high_wire.edges()
    ]
    split_at = next(at for at, face in enumerate(low_owners) if face.geom_type == GeomType.PLANE)
    original = low_owners[split_at]
    cutters = [
        Pos(0, 0, -3.5) * Box(20, 20, 3, align=_CENTRE),
        Box(20, 20, 4, align=_CENTRE),
        Pos(0, 0, 3.5) * Box(20, 20, 3, align=_CENTRE),
    ]
    patches = tuple(original & cutter for cutter in cutters)
    assert [(face.bounding_box().min.Z, face.bounding_box().max.Z) for face in patches] == [
        (-5.0, -2.0),
        (-2.0, 2.0),
        (2.0, 5.0),
    ]
    seam_a = next(
        edge
        for edge in patches[0].edges()
        if abs(edge.bounding_box().min.Z + 2.0) < 1e-9
        and abs(edge.bounding_box().max.Z + 2.0) < 1e-9
    )
    seam_b = next(
        edge
        for edge in patches[1].edges()
        if abs(edge.bounding_box().min.Z - 2.0) < 1e-9
        and abs(edge.bounding_box().max.Z - 2.0) < 1e-9
    )
    low_edges = low_wire.edges()
    high_edges = high_wire.edges()
    controlled: dict = {}
    for at, (low_edge, low_owner) in enumerate(zip(low_edges, low_owners, strict=True)):
        high_at = next(
            index
            for index, high_owner in enumerate(high_owners)
            if low_owner.wrapped.IsSame(high_owner.wrapped)
        )
        high_edge = high_edges[high_at]
        if at == split_at:
            controlled[low_edge] = (low_face, patches[0])
            controlled[high_edge] = (high_face, patches[2])
        else:
            controlled[low_edge] = (low_face, low_owner)
            controlled[high_edge] = (high_face, low_owner)
    controlled[seam_a] = (patches[0], patches[1])
    controlled[seam_b] = (patches[1], patches[2])

    class ControlledEdges:
        def of(self, face):
            return tuple(
                edge
                for edge, owners in controlled.items()
                if any(owner.wrapped.IsSame(face.wrapped) for owner in owners)
            )

    class ControlledPart:
        def faces(self):
            ordinary = [face for face in part.faces() if not face.wrapped.IsSame(original.wrapped)]
            return [*ordinary, *patches]

    monkeypatch.setattr(module, "edge_face_map", lambda *_args, **_kwargs: controlled)
    walls = module._complete_wall_component(
        ControlledPart(),
        low_wire,
        high_wire,
        low_face,
        high_face,
        "z",
        profile,
        bbox.min.Z,
        bbox.max.Z,
        1e-5,
        face_edges=cast(FaceEdges, ControlledEdges()),
    )
    expected_walls = [
        *(owner for owner in low_owners if not owner.wrapped.IsSame(original.wrapped)),
        *patches,
    ]
    assert len(expected_walls) == 6
    assert all(
        any(face.wrapped.IsSame(expected.wrapped) for face in walls) for expected in expected_walls
    )
    assert all(
        any(face.wrapped.IsSame(expected.wrapped) for expected in expected_walls) for face in walls
    )

    outward = Face(patches[1].wrapped.Reversed())
    outward_incidence = {
        edge: tuple(
            outward if owner.wrapped.IsSame(patches[1].wrapped) else owner for owner in owners
        )
        for edge, owners in controlled.items()
    }

    class OutwardEdges:
        def of(self, face):
            return tuple(
                edge
                for edge, owners in outward_incidence.items()
                if any(owner.wrapped.IsSame(face.wrapped) for owner in owners)
            )

    class OutwardPart:
        def faces(self):
            return [
                *(
                    face
                    for face in ControlledPart().faces()
                    if not face.wrapped.IsSame(patches[1].wrapped)
                ),
                outward,
            ]

    monkeypatch.setattr(module, "edge_face_map", lambda *_args, **_kwargs: outward_incidence)
    assert (
        module._complete_wall_component(
            OutwardPart(),
            low_wire,
            high_wire,
            low_face,
            high_face,
            "z",
            profile,
            bbox.min.Z,
            bbox.max.Z,
            1e-5,
            face_edges=cast(FaceEdges, OutwardEdges()),
        )
        == ()
    )

    ambiguous_incidence = dict(controlled)
    ambiguous_incidence[low_edges[0]] = (
        low_face,
        low_owners[0],
        low_owners[1],
    )
    monkeypatch.setattr(module, "edge_face_map", lambda *_args, **_kwargs: ambiguous_incidence)
    assert (
        module._complete_wall_component(
            ControlledPart(),
            low_wire,
            high_wire,
            low_face,
            high_face,
            "z",
            profile,
            bbox.min.Z,
            bbox.max.Z,
            1e-5,
            face_edges=cast(FaceEdges, ControlledEdges()),
        )
        == ()
    )

    def substituted(seed):
        changed = {
            edge: tuple(
                seed if owner.wrapped.IsSame(low_owners[0].wrapped) else owner for owner in owners
            )
            for edge, owners in controlled.items()
        }

        class SubstitutedEdges:
            def of(self, face):
                return tuple(
                    edge
                    for edge, owners in changed.items()
                    if any(owner.wrapped.IsSame(face.wrapped) for owner in owners)
                )

        class SubstitutedPart:
            def faces(self):
                return [
                    *(
                        face
                        for face in ControlledPart().faces()
                        if not face.wrapped.IsSame(low_owners[0].wrapped)
                    ),
                    seed,
                ]

        monkeypatch.setattr(module, "edge_face_map", lambda *_args, **_kwargs: changed)
        return module._complete_wall_component(
            SubstitutedPart(),
            low_wire,
            high_wire,
            low_face,
            high_face,
            "z",
            profile,
            bbox.min.Z,
            bbox.max.Z,
            1e-5,
            face_edges=cast(FaceEdges, SubstitutedEdges()),
        )

    spherical = Sphere(2).faces()[0]
    assert substituted(spherical) == ()
    outside = copy.deepcopy(low_owners[0]).translate((0, 0, 20))
    assert substituted(outside) == ()
    wrong_support = copy.deepcopy(low_owners[0]).translate((2, 0, 0))
    assert substituted(wrong_support) == ()

    ledger = ClaimLedger(FaceGraph(part))
    monkeypatch.setattr(module, "_complete_wall_component", lambda *_args, **_kwargs: walls)
    with pytest.raises(ValueError):
        _discover_double_d_bores(part, writer=ledger.writer)
    assert ledger.candidate_set_for(FamilyId.DOUBLE_D_BORES, ()).candidates == ()


def _qualified_calls(tree: ast.AST) -> list[tuple[str, ast.Call]]:
    aliases: dict[str, str] = {}
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            for alias in node.names:
                local = alias.asname or alias.name.split(".")[0]
                aliases[local] = alias.name if alias.asname else alias.name.split(".")[0]
        elif isinstance(node, ast.ImportFrom) and node.module:
            for alias in node.names:
                aliases[alias.asname or alias.name] = f"{node.module}.{alias.name}"

    def name(node: ast.expr) -> str:
        if isinstance(node, ast.Name):
            return aliases.get(node.id, node.id)
        if isinstance(node, ast.Attribute):
            base = name(node.value)
            return f"{base}.{node.attr}" if base else node.attr
        return ""

    return [(name(node.func), node) for node in ast.walk(tree) if isinstance(node, ast.Call)]


def _tool(height: float = 20, *, across: float = 7.2):
    return Cylinder(5, height, align=_CENTRE) & Box(across, 20, 2 * height, align=_CENTRE)


def _plate():
    return Box(30, 30, 10, align=_CENTRE) - _tool()


def _duplicate_hole_plate():
    return Box(30, 30, 12, align=_CENTRE) - (
        Cylinder(4, 20, align=_CENTRE) & Box(6, 30, 30, align=_CENTRE)
    )


def _claimed(part):
    public = recognise_double_d_bores(part)
    ledger = ClaimLedger(FaceGraph(part))
    records = _discover_double_d_bores(part, writer=ledger.writer)
    candidates = ledger.candidate_set_for(FamilyId.DOUBLE_D_BORES, records).candidates
    assert [type(record) for record in records] == [type(record) for record in public]
    assert records == public
    assert [record.to_dict() for record in records] == [record.to_dict() for record in public]
    assert len(candidates) == len(records)
    assert all(
        candidate.record is record for candidate, record in zip(candidates, records, strict=True)
    )
    return ledger, records, candidates


def _assert_wall_role(part, ledger, records, occurrence: int, candidate) -> None:
    record = records[occurrence]
    axis = next(at for at, value in enumerate(record.axis) if value)
    metric_tol = record.major_diameter * 1e-3
    faces = [ledger.graph.face(node) for node in ledger.graph.nodes]
    incidence = edge_face_map(faces, face_edges=FaceEdges())

    owner_solids = []
    for solid in part.solids():
        nodes = tuple(ledger.graph.require_node(face) for face in solid.faces())
        owner = ledger.graph.common_valid_solid(nodes)
        if owner is not None:
            owner_solids.append((owner, solid))

    equal_before = sum(known == record for known in records[:occurrence])

    def profile_of(wire):
        edges = wire.edges()
        lines = [edge for edge in edges if edge.geom_type == GeomType.LINE]
        arcs = [edge for edge in edges if edge.geom_type == GeomType.CIRCLE]
        if len(edges) != 4 or len(lines) != 2 or len(arcs) != 2:
            return None
        radius = float(arcs[0].radius)
        centre = arcs[0].arc_center
        centre_xyz = (float(centre.X), float(centre.Y), float(centre.Z))
        if any(
            abs(float(arc.radius) - radius) > metric_tol
            or (arc.arc_center - centre).length > metric_tol
            for arc in arcs[1:]
        ):
            return None
        midpoints = []
        directions = []
        for line in lines:
            vertices = line.vertices()
            if len(vertices) != 2:
                return None
            ends = [(float(vertex.X), float(vertex.Y), float(vertex.Z)) for vertex in vertices]
            delta = tuple(ends[1][i] - ends[0][i] for i in range(3))
            length = sqrt(sum(value * value for value in delta))
            directions.append(tuple(value / length for value in delta))
            midpoints.append(tuple((ends[0][i] + ends[1][i]) / 2 for i in range(3)))
            if any(
                abs(sqrt(sum((end[i] - centre_xyz[i]) ** 2 for i in range(3))) - radius)
                > metric_tol
                for end in ends
            ):
                return None
        if abs(abs(sum(directions[0][i] * directions[1][i] for i in range(3))) - 1) > 1e-4:
            return None
        direction = directions[0]
        flat = [0.0, 0.0, 0.0]
        plane = [at for at in range(3) if at != axis]
        flat[plane[0]] = -direction[plane[1]]
        flat[plane[1]] = direction[plane[0]]
        first = next(value for value in flat if abs(value) > 1e-12)
        if first < 0:
            flat = [-value for value in flat]
        offsets = sorted(
            sum((point[i] - centre_xyz[i]) * flat[i] for i in range(3)) for point in midpoints
        )
        if offsets[0] >= -metric_tol or offsets[1] <= metric_tol:
            return None
        if abs(offsets[0] + offsets[1]) > metric_tol:
            return None
        across = offsets[1] - offsets[0]
        if across <= metric_tol or across >= 2 * radius - metric_tol:
            return None
        expected_chord = 2 * sqrt(radius * radius - (across / 2) ** 2)
        assert all(abs(float(line.length) - expected_chord) <= metric_tol for line in lines)
        expected_arc = 2 * radius * asin((across / 2) / radius)
        if any(abs(float(arc.length) - expected_arc) > metric_tol for arc in arcs):
            return None
        return (
            centre_xyz,
            round(2 * radius, 4),
            round(across, 4),
            tuple(0.0 if abs(value) < 1e-12 else round(value, 12) for value in flat),
        )

    def opening_sets(owner, at: float):
        found = []
        for node in ledger.graph.nodes:
            if ledger.graph.common_valid_solid((node,)) is not owner:
                continue
            face = ledger.graph.face(node)
            if face.geom_type != GeomType.PLANE:
                continue
            bounds = ledger.graph.bounds(node)[axis]
            if abs(bounds[0] - at) > metric_tol or abs(bounds[1] - at) > metric_tol:
                continue
            for wire in face.inner_wires():
                edges = wire.edges()
                if len(edges) != 4:
                    continue
                center = wire.bounding_box().center()
                point = (float(center.X), float(center.Y), float(center.Z))
                if any(
                    abs(point[i] - record.location[i]) > metric_tol for i in range(3) if i != axis
                ):
                    continue
                partners = []
                for edge in edges:
                    others = [
                        other
                        for other in incidence.get(edge, ())
                        if not other.wrapped.IsSame(face.wrapped)
                    ]
                    assert len(others) == 1
                    partners.append(ledger.graph.require_node(others[0]))
                profile = profile_of(wire)
                if profile is not None:
                    found.append((frozenset(partners), wire, profile))
        return found

    matching = []
    for owner, solid in owner_solids:
        bbox = solid.bounding_box()
        low = (bbox.min.X, bbox.min.Y, bbox.min.Z)[axis]
        high = (bbox.max.X, bbox.max.Y, bbox.max.Z)[axis]
        lows = opening_sets(owner, low)
        highs = opening_sets(owner, high)
        if (
            len(lows) == len(highs) == 1
            and lows[0][2][1:] == highs[0][2][1:]
            and all(
                abs(lows[0][2][0][i] - highs[0][2][0][i]) <= metric_tol
                for i in range(3)
                if i != axis
            )
            and highs[0][2][1] == record.major_diameter
            and highs[0][2][2] == record.across_flats
            and highs[0][2][3] == record.flat_direction
        ):
            matching.append((owner, solid, low, high, lows[0], highs[0]))
    assert len(matching) > equal_before
    owner, owner_solid, low, high, low_opening, high_opening = matching[equal_before]
    low_sets = [low_opening[0]]
    high_sets = [high_opening[0]]
    assert len(low_sets) == len(high_sets) == 1
    assert low_sets[0] == high_sets[0]
    centre, diameter, across, flat = high_opening[2]
    assert record.major_diameter == diameter
    assert record.across_flats == across
    assert record.flat_direction == flat
    assert record.location[axis] == high
    assert record.depth == round(high - low, 4)
    assert record.through is True
    assert all(abs(record.location[i] - centre[i]) <= metric_tol for i in range(3) if i != axis)
    prism = Solid.extrude(
        Face(low_opening[1]),
        Vector(*(record.axis[i] * record.depth for i in range(3))),
    )
    overlap = owner_solid & prism
    assert overlap is None or float(overlap.volume) <= max(1e-15, float(prism.volume) * 1e-6)

    def support(node):
        face = ledger.graph.face(node)
        surface = BRepAdaptor_Surface(face.wrapped)
        if face.geom_type == GeomType.PLANE:
            plane = surface.Plane()
            normal = plane.Axis().Direction()
            values = [normal.X(), normal.Y(), normal.Z()]
            location = plane.Location()
            offset = sum(
                (location.X(), location.Y(), location.Z())[i] * values[i] for i in range(3)
            )
            if next(value for value in values if abs(value) > 1e-9) < 0:
                values = [-value for value in values]
                offset = -offset
            return ("plane", *values, offset)
        cylinder = surface.Cylinder()
        direction = cylinder.Axis().Direction()
        values = [direction.X(), direction.Y(), direction.Z()]
        if next(value for value in values if abs(value) > 1e-9) < 0:
            values = [-value for value in values]
        location = cylinder.Axis().Location()
        return (
            "cylinder",
            *values,
            *((location.X(), location.Y(), location.Z())[i] for i in range(3) if i != axis),
            cylinder.Radius(),
        )

    def same_support(left, right):
        return (
            left[0] == right[0]
            and len(left) == len(right)
            and all(abs(left[i] - right[i]) <= 1e-4 for i in range(1, 4))
            and all(abs(left[i] - right[i]) <= metric_tol for i in range(4, len(left)))
        )

    memo = FaceEdges()
    void_centre = low_opening[2][0]

    def inward(node) -> bool:
        face = ledger.graph.face(node)
        surface = BRepAdaptor_Surface(face.wrapped)
        native = surface.Value(
            0.5 * (surface.FirstUParameter() + surface.LastUParameter()),
            0.5 * (surface.FirstVParameter() + surface.LastVParameter()),
        )
        point = Vector(native.X(), native.Y(), native.Z())
        normal = face.normal_at(point)
        radial = [
            void_centre[i] - (float(point.X), float(point.Y), float(point.Z))[i] for i in range(3)
        ]
        radial[axis] = 0.0
        normal_values = (float(normal.X), float(normal.Y), float(normal.Z))
        return sum(radial[i] * normal_values[i] for i in range(3)) > metric_tol

    def complete_chain(seed):
        role = support(seed)
        pending = [seed]
        found = set()
        while pending:
            node = pending.pop()
            if node in found or not same_support(support(node), role) or not inward(node):
                continue
            bounds = ledger.graph.bounds(node)[axis]
            if bounds[0] < low - metric_tol or bounds[1] > high + metric_tol:
                continue
            found.add(node)
            for edge in memo.of(ledger.graph.face(node)):
                for other in incidence.get(edge, ()):
                    other_node = ledger.graph.require_node(other)
                    if ledger.graph.common_valid_solid((other_node,)) is owner:
                        pending.append(other_node)
        return frozenset(found)

    chains = [complete_chain(seed) for seed in low_opening[0]]
    assert len(chains) == 4 and all(chain for chain in chains)
    assert sum(len(chain) for chain in chains) == len(set().union(*chains))
    assignments = [
        next(at for at, seed in enumerate(high_opening[0]) if seed in chain) for chain in chains
    ]
    assert sorted(assignments) == [0, 1, 2, 3]
    for chain in chains:
        intervals = sorted(ledger.graph.bounds(node)[axis] for node in chain)
        cursor = low
        for start, end in intervals:
            assert abs(start - cursor) <= metric_tol
            assert end > start
            cursor = end
        assert abs(cursor - high) <= metric_tol
        internal_edges = []
        degrees = {node: 0 for node in chain}
        for node in chain:
            for edge in memo.of(ledger.graph.face(node)):
                partners = {
                    ledger.graph.require_node(face)
                    for face in incidence.get(edge, ())
                    if ledger.graph.require_node(face) in chain
                }
                if len(partners) == 2:
                    internal_edges.append(edge)
                    for node in partners:
                        degrees[node] += 1
        # Each seam is encountered twice, once from each incident face.
        assert len(internal_edges) // 2 == max(0, len(chain) - 1)
        if len(chain) == 1:
            assert next(iter(degrees.values())) == 0
        else:
            # Each seam was visited from both faces, so graph degree is doubled here.
            assert all(degree <= 4 for degree in degrees.values())
            assert list(degrees.values()).count(2) == 2
    expected = frozenset().union(*chains)
    defining = ledger.defining_of(candidate)
    assert expected == defining
    wall_faces = [ledger.graph.face(node) for node in expected]
    assert [face.geom_type for face in wall_faces].count(GeomType.PLANE) >= 2
    assert [face.geom_type for face in wall_faces].count(GeomType.CYLINDER) >= 2


@pytest.mark.parametrize("rotation", [Rot(), Rot(0, 90, 0), Rot(90, 0, 0)])
def test_each_principal_axis_issues_the_complete_wall_set(rotation) -> None:
    part = rotation * _plate()
    ledger, records, candidates = _claimed(part)
    assert len(records) == 1
    _assert_wall_role(part, ledger, records, 0, candidates[0])


def test_multiple_occurrences_keep_sorted_record_identity_and_wall_ownership() -> None:
    part = Compound(
        [
            Pos(-30, 0, 0) * _plate(),
            Pos(30, 0, 0) * (Box(34, 34, 12, align=_CENTRE) - _tool(20, across=6.4)),
        ]
    )
    ledger, records, candidates = _claimed(part)
    assert len(records) == 2
    assert [record.location[0] for record in records] == [-30.0, 30.0]
    for at, candidate in enumerate(candidates):
        _assert_wall_role(part, ledger, records, at, candidate)
    assert ledger.defining_of(candidates[0]).isdisjoint(ledger.defining_of(candidates[1]))


def test_equal_full_records_from_distinct_solids_remain_identity_distinct() -> None:
    first = _plate()
    part = Compound([first, copy.deepcopy(first)])
    ledger, records, candidates = _claimed(part)
    assert len(records) == len(candidates) == 2
    assert records[0] == records[1] and records[0] is not records[1]
    assert candidates[0].record is records[0]
    assert candidates[1].record is records[1]
    for at, candidate in enumerate(candidates):
        _assert_wall_role(part, ledger, records, at, candidate)
    assert ledger.defining_of(candidates[0]).isdisjoint(ledger.defining_of(candidates[1]))
    owners = [
        ledger.graph.common_valid_solid(ledger.defining_of(candidate)) for candidate in candidates
    ]
    assert owners[0] is not owners[1]


@pytest.mark.parametrize(
    "part",
    [
        Pos(17, -9, 4) * _plate(),
        _plate().mirror(),
        _plate().scale(0.1),
        _plate().scale(10),
    ],
)
def test_transform_and_scale_routes_keep_exact_wall_roles(part) -> None:
    ledger, records, candidates = _claimed(part)
    assert len(records) == 1
    _assert_wall_role(part, ledger, records, 0, candidates[0])


def test_step_round_trip_retains_original_imported_wall_roles(tmp_path) -> None:
    target = tmp_path / "double-d.step"
    assert export_step(_plate(), target)
    imported = import_step(target)
    ledger, records, candidates = _claimed(imported)
    assert len(records) == 1
    _assert_wall_role(imported, ledger, records, 0, candidates[0])


def test_reversed_face_traversal_preserves_occurrence_and_complete_roles(monkeypatch) -> None:
    part = Compound([Pos(-25, 0, 0) * _plate(), Pos(25, 0, 0) * _plate()])
    baseline = recognise_double_d_bores(part)
    original = type(part).faces
    monkeypatch.setattr(type(part), "faces", lambda self: list(reversed(original(self))))
    ledger, records, candidates = _claimed(part)
    assert [record.to_dict() for record in records] == [record.to_dict() for record in baseline]
    for at, candidate in enumerate(candidates):
        _assert_wall_role(part, ledger, records, at, candidate)


def test_opposite_extremal_orientation_keeps_canonical_flat_sign() -> None:
    part = Rot(180, 0, 0) * _plate()
    ledger, records, candidates = _claimed(part)
    assert records[0].axis == (0.0, 0.0, 1.0)
    assert next(value for value in records[0].flat_direction if abs(value) > 1e-12) > 0
    _assert_wall_role(part, ledger, records, 0, candidates[0])


def test_custom_tolerance_route_preserves_public_and_writer_roles() -> None:
    part = _plate().scale(0.1)
    public = recognise_double_d_bores(part, tol=1e-4)
    ledger = ClaimLedger(FaceGraph(part))
    records = _discover_double_d_bores(part, tol=1e-4, writer=ledger.writer)
    candidates = ledger.candidate_set_for(FamilyId.DOUBLE_D_BORES, records).candidates
    assert [record.to_dict() for record in records] == [record.to_dict() for record in public]
    assert len(records) == 1
    _assert_wall_role(part, ledger, records, 0, candidates[0])


def test_aggregate_inventory_publishes_terminal_double_d_wall_evidence() -> None:
    product = _take_inventory(_plate())
    candidates = product.physical.candidate_set(FamilyId.DOUBLE_D_BORES).candidates
    assert tuple(candidate.record for candidate in candidates) == product.result.double_d_bores
    assert product.accepted.candidate_set(FamilyId.DOUBLE_D_BORES).candidates == candidates
    assert len(candidates) == 1
    assert len(product.evidence.defining_of(candidates[0])) == 4


def test_aggregate_rejects_partial_circular_reading_of_same_double_d_boundary() -> None:
    part = _duplicate_hole_plate()
    product = _take_inventory(part)
    holes = product.physical.candidate_set(FamilyId.HOLES).candidates
    double_d = product.physical.candidate_set(FamilyId.DOUBLE_D_BORES).candidates

    assert len(recognise_holes(part)) == len(holes) == len(double_d) == 1
    assert product.result.holes == ()
    assert len(product.result.double_d_bores) == 1
    assert product.result.hole_patterns == ()
    assert product.evidence.defining_of(holes[0]) < product.evidence.defining_of(double_d[0])
    disposition = next(
        item for item in product.reconciliation.dispositions if item.candidate is holes[0]
    )
    assert disposition.outcome is Outcome.REJECTED
    assert disposition.reason is ReasonCode.HOLE_SUPERSEDED_BY_DOUBLE_D_BORE
    assert disposition.related == double_d

    hole_report = next(
        family for family in build_recognition_report(part).families if family.family == "holes"
    )
    assert (hole_report.proposed, hole_report.accepted, hole_report.rejected) == (1, 0, 1)
    assert hole_report.dispositions[0].reason.value == ("bore.hole_superseded_by_double_d_bore")
    assert hole_report.dispositions[0].related_occurrences == 1


def test_double_d_precedence_keeps_a_disjoint_ordinary_hole_on_the_same_solid() -> None:
    part = _duplicate_hole_plate() - Pos(10, 0, 0) * Cylinder(2, 20, align=_CENTRE)
    product = _take_inventory(part)
    proposed_holes = product.physical.candidate_set(FamilyId.HOLES).candidates
    accepted_holes = product.accepted.candidate_set(FamilyId.HOLES).candidates
    (double_d,) = product.physical.candidate_set(FamilyId.DOUBLE_D_BORES).candidates

    assert len(proposed_holes) == 2
    assert len(accepted_holes) == len(product.result.holes) == 1
    assert product.evidence.defining_of(accepted_holes[0]).isdisjoint(
        product.evidence.defining_of(double_d)
    )
    rejected = [
        item
        for item in product.reconciliation.dispositions
        if item.reason is ReasonCode.HOLE_SUPERSEDED_BY_DOUBLE_D_BORE
    ]
    assert len(rejected) == 1
    assert rejected[0].related == (double_d,)


def test_double_d_precedence_keeps_equal_diameter_hole_on_another_solid() -> None:
    circular = Box(30, 30, 12, align=_CENTRE) - Cylinder(4, 20, align=_CENTRE)
    part = Compound([Pos(-25, 0, 0) * _duplicate_hole_plate(), Pos(25, 0, 0) * circular])
    product = _take_inventory(part)
    proposed_holes = product.physical.candidate_set(FamilyId.HOLES).candidates
    accepted_holes = product.accepted.candidate_set(FamilyId.HOLES).candidates
    (double_d,) = product.physical.candidate_set(FamilyId.DOUBLE_D_BORES).candidates

    assert len(proposed_holes) == 2
    assert len(accepted_holes) == len(product.result.holes) == 1
    assert product.evidence.defining_of(accepted_holes[0]).isdisjoint(
        product.evidence.defining_of(double_d)
    )
    assert (
        sum(
            item.reason is ReasonCode.HOLE_SUPERSEDED_BY_DOUBLE_D_BORE
            for item in product.reconciliation.dispositions
        )
        == 1
    )


@pytest.mark.parametrize(
    "part",
    [
        Box(30, 30, 10, align=_CENTRE) - Pos(0, 0, 3) * _tool(4),
        Rot(0, 15, 0) * _plate(),
    ],
)
def test_rejected_geometry_issues_no_candidate(part) -> None:
    ledger = ClaimLedger(FaceGraph(part))
    assert _discover_double_d_bores(part, writer=ledger.writer) == []
    assert ledger.candidate_set_for(FamilyId.DOUBLE_D_BORES, ()).candidates == ()


def test_profile_throughness_and_constant_wall_negatives_issue_no_candidate() -> None:
    plate = Box(30, 30, 10, align=_CENTRE)
    opposed_blind = plate - Pos(0, 0, 3) * _tool(4) - Pos(0, 0, -3) * _tool(4)
    straight = 8.0
    obround = (
        Box(straight, 6, 20, align=_CENTRE)
        + Pos(straight / 2, 0, 0) * Cylinder(3, 20, align=_CENTRE)
        + Pos(-straight / 2, 0, 0) * Cylinder(3, 20, align=_CENTRE)
    )
    low = Circle(5) & Rectangle(7.2, 20, align=(Align.CENTER, Align.CENTER))
    high = Pos(0, 0, 20) * (Circle(6) & Rectangle(8, 20, align=(Align.CENTER, Align.CENTER)))
    tapered = Pos(0, 0, -10) * loft([low, high])

    for part in (opposed_blind, plate - obround, plate - tapered):
        assert recognise_double_d_bores(part) == []
        ledger = ClaimLedger(FaceGraph(part))
        assert _discover_double_d_bores(part, writer=ledger.writer) == []
        assert ledger.candidate_set(FamilyId.DOUBLE_D_BORES).candidates == ()


def test_late_body_validation_failure_leaves_no_prefix(monkeypatch) -> None:
    part = Pos(-20, 0, 0) * _plate() + Pos(20, 0, 0) * _plate()
    ledger = ClaimLedger(FaceGraph(part))
    original = ledger.graph.common_valid_solid
    calls = 0

    def fail_second(nodes):
        nonlocal calls
        calls += 1
        return original(nodes) if calls == 1 else None

    monkeypatch.setattr(ledger.graph, "common_valid_solid", fail_second)
    with pytest.raises(ValueError, match="one valid owner solid"):
        _discover_double_d_bores(part, writer=ledger.writer)
    assert ledger.candidate_set_for(FamilyId.DOUBLE_D_BORES, ()).candidates == ()


def test_late_binding_failure_leaves_no_prefix(monkeypatch) -> None:
    part = Compound([Pos(-30, 0, 0) * _plate(), Pos(30, 0, 0) * _plate()])
    ledger = ClaimLedger(FaceGraph(part))
    original = ledger.graph.require_node
    calls = 0

    def fail_late(face):
        nonlocal calls
        calls += 1
        if calls > 4:
            raise ValueError("late binding refusal")
        return original(face)

    monkeypatch.setattr(ledger.graph, "require_node", fail_late)
    with pytest.raises(ValueError, match="late binding refusal"):
        _discover_double_d_bores(part, writer=ledger.writer)
    assert ledger.candidate_set_for(FamilyId.DOUBLE_D_BORES, ()).candidates == ()


def test_cross_occurrence_wall_reuse_refuses_before_publication(monkeypatch) -> None:
    import quiddity.profiled_bores as module

    part = Compound([Pos(-30, 0, 0) * _plate(), Pos(30, 0, 0) * _plate()])
    ledger = ClaimLedger(FaceGraph(part))
    original = module._complete_wall_component
    first = None

    def reuse_first(*args, **kwargs):
        nonlocal first
        walls = original(*args, **kwargs)
        if first is None:
            first = walls
        return first

    monkeypatch.setattr(module, "_complete_wall_component", reuse_first)
    with pytest.raises(ValueError, match="assigned across occurrences"):
        _discover_double_d_bores(part, writer=ledger.writer)
    assert ledger.candidate_set_for(FamilyId.DOUBLE_D_BORES, ()).candidates == ()


def test_incomplete_wall_component_refuses_before_publication(monkeypatch) -> None:
    import quiddity.profiled_bores as module

    part = _plate()
    ledger = ClaimLedger(FaceGraph(part))
    monkeypatch.setattr(module, "_complete_wall_component", lambda *_args, **_kwargs: ())
    with pytest.raises(ValueError, match="no complete original wall component"):
        _discover_double_d_bores(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.DOUBLE_D_BORES).candidates == ()


def test_repeated_same_wall_reference_collapses_once(monkeypatch) -> None:
    import quiddity.profiled_bores as module

    part = _plate()
    ledger = ClaimLedger(FaceGraph(part))
    original = module._complete_wall_component

    def repeated(*args, **kwargs):
        walls = original(*args, **kwargs)
        return (*walls, walls[0])

    monkeypatch.setattr(module, "_complete_wall_component", repeated)
    records = _discover_double_d_bores(part, writer=ledger.writer)
    candidate = ledger.candidate_set_for(FamilyId.DOUBLE_D_BORES, records).candidates[0]
    assert len(ledger.defining_of(candidate)) == 4


@pytest.mark.parametrize("translated", [False, True])
def test_deep_or_translated_wall_clone_refuses_before_publication(
    monkeypatch, translated: bool
) -> None:
    import quiddity.profiled_bores as module

    part = _plate()
    ledger = ClaimLedger(FaceGraph(part))
    original = module._complete_wall_component

    def cloned(*args, **kwargs):
        walls = original(*args, **kwargs)
        changed = [copy.deepcopy(face) for face in walls]
        if translated:
            changed = [face.translate((1, 0, 0)) for face in changed]
        return tuple(changed)

    monkeypatch.setattr(module, "_complete_wall_component", cloned)
    with pytest.raises(ValueError):
        _discover_double_d_bores(part, writer=ledger.writer)
    assert ledger.candidate_set_for(FamilyId.DOUBLE_D_BORES, ()).candidates == ()


def test_open_shell_keeps_public_compatibility_but_refuses_aggregate() -> None:
    shell = Shell(_plate().faces())
    assert len(recognise_double_d_bores(shell)) == 1
    ledger = ClaimLedger(FaceGraph(shell))
    with pytest.raises(ValueError, match="one valid owner solid"):
        _discover_double_d_bores(shell, writer=ledger.writer)
    assert ledger.candidate_set_for(FamilyId.DOUBLE_D_BORES, ()).candidates == ()


def test_foreign_writer_refuses_before_publication() -> None:
    part = _plate()
    foreign = ClaimLedger(FaceGraph(Pos(50, 0, 0) * _plate()))
    with pytest.raises(ValueError, match="different part|does not belong"):
        _discover_double_d_bores(part, writer=foreign.writer)
    assert foreign.candidate_set_for(FamilyId.DOUBLE_D_BORES, ()).candidates == ()


def test_only_registry_may_call_writer_enabled_core() -> None:
    root = Path(__file__).parents[1]
    sites: list[tuple[str, bool]] = []
    importers: list[str] = []
    for path in (root / "src").rglob("*.py"):
        if path.name == "profiled_bores.py":
            continue
        tree = ast.parse(path.read_text(encoding="utf-8"))
        if any(
            isinstance(node, ast.ImportFrom)
            and node.module == "quiddity.profiled_bores"
            and any(alias.name == "_discover_double_d_bores" for alias in node.names)
            for node in ast.walk(tree)
        ):
            importers.append(path.name)
        for qualified, node in _qualified_calls(tree):
            if qualified == "quiddity.profiled_bores._discover_double_d_bores":
                sites.append(
                    (
                        path.name,
                        any(
                            keyword.arg == "writer" and ast.unparse(keyword.value) == "s.writer"
                            for keyword in node.keywords
                        ),
                    )
                )
    assert importers == ["_registry.py"]
    assert sites == [("_registry.py", True)]


def test_constructor_and_void_prism_path_roster_is_closed() -> None:
    path = Path(__file__).parents[1] / "src/quiddity/profiled_bores.py"
    tree = ast.parse(path.read_text(encoding="utf-8"))
    sites: list[tuple[str, str]] = []
    for qualified, node in _qualified_calls(tree):
        if qualified in {"DoubleDBore", "build123d.Solid.extrude"}:
            function = next(
                (
                    parent.name
                    for parent in ast.walk(tree)
                    if isinstance(parent, ast.FunctionDef) and node in ast.walk(parent)
                ),
                "",
            )
            sites.append((qualified, function))
    assert sites == [
        ("DoubleDBore", "double_d_bores_from_openings"),
        ("build123d.Solid.extrude", "double_d_bores_from_openings"),
        ("build123d.Solid.extrude", "read_double_d_tool"),
    ]
