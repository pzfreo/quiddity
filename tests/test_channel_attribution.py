"""F5: Channels own exactly their opposed original planar side walls."""

from __future__ import annotations

import ast
import copy
import math
from dataclasses import replace
from pathlib import Path
from typing import cast

import pytest
from build123d import (
    Box,
    Compound,
    Cylinder,
    GeomType,
    Plane,
    Pos,
    Rot,
    export_step,
    import_step,
)
from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.GeomAbs import GeomAbs_Plane

import quiddity._recess_core as core_module
import quiddity._recess_features as feature_module
from quiddity._adjacency import FaceEdges, FaceGraph, FaceNode
from quiddity._candidates import FamilyId
from quiddity._claims import ClaimLedger
from quiddity._geometry import COORD_FLOOR
from quiddity._recess_core import (
    _bounds_one_void as production_bounds_one_void,
)
from quiddity._recess_core import (
    _recognise_channels_one,
    _uninterrupted_long_span,
)
from quiddity._recess_faces import (
    _AXIS_ALIGNED_TOL,
    _FLOOR_COVER_FRAC,
    _FLOOR_TOL,
    _MERGE_TOL,
    _dominant_axis,
    _end_capped,
    _Face,
    _is_wall,
)
from quiddity._recess_features import _discover_channels
from quiddity._recess_records import Channel
from quiddity.result import _take_inventory
from tests.golden.open_channels.fixture import build_fixture
from tools._legacy_recognition import (
    recognise_channels,
)

ROOT = Path(__file__).parents[1]


def _coordinate(point, axis: str) -> float:
    return (point.X, point.Y, point.Z)["xyz".index(axis)]


def _bbox_center(face, axis: str) -> float:
    bounds = face.bounding_box()
    return (_coordinate(bounds.min, axis) + _coordinate(bounds.max, axis)) / 2


def same_arc_kind(left, right) -> bool:
    return left == right


def is_opposed_nonsmooth(left, right) -> bool:
    return {left, right} == {"convex", "concave"}


def _bounds_one_void(graph: FaceGraph, left, right) -> bool:
    common = set(graph.neighbours(left)) & set(graph.neighbours(right))
    if common:
        boundary = {node for node in common if graph.is_planar(node)} or common
        return all(
            same_arc_kind(graph.arc(left, node), graph.arc(right, node)) for node in boundary
        )
    left_regions = {
        graph.smooth_region(node)
        for node in graph.neighbours(left)
        if graph.arc(left, node) == "concave"
    }
    right_regions = {
        graph.smooth_region(node)
        for node in graph.neighbours(right)
        if graph.arc(right, node) == "concave"
    }
    return bool(left_regions & right_regions) or right in graph.smooth_region(left)


def _uninterrupted_span(graph: FaceGraph, left, right, axis: str, span):
    lo, hi = span
    for node in set(graph.neighbours(left)) & set(graph.neighbours(right)):
        if graph.is_planar(node):
            continue
        if not is_opposed_nonsmooth(graph.arc(left, node), graph.arc(right, node)):
            continue
        node_lo, node_hi = graph.bounds(node)["xyz".index(axis)]
        if node_lo <= lo + 1e-6:
            lo = max(lo, node_hi)
        if node_hi >= hi - 1e-6:
            hi = min(hi, node_lo)
    return (lo, hi) if hi - lo > 1e-6 else None


def _whole_inset_prism_is_empty(part, spans) -> bool:
    size, centre = {}, {}
    for axis, (lo, hi) in spans.items():
        inset = min(1e-6, (hi - lo) / 4)
        size[axis] = hi - lo - 2 * inset
        centre[axis] = (lo + hi) / 2
    assert min(size.values()) > 0
    probe = Pos(centre["x"], centre["y"], centre["z"]) * Box(size["x"], size["y"], size["z"])
    intersection = part.intersect(probe)
    if intersection is None:
        volume = 0.0
    elif hasattr(intersection, "volume"):
        volume = intersection.volume
    else:
        volume = sum(shape.volume for shape in intersection)
    return volume == 0.0


def _fresh_expected_channels(part, graph: FaceGraph):
    """Reconstruct the supported Channel grammar before any recogniser/Candidate read."""

    expected = []
    solids = list(part.solids())
    sources = solids if len(solids) > 1 else [part]
    for solid in sources:
        bb = solid.bounding_box()
        bounds = {axis: (_coordinate(bb.min, axis), _coordinate(bb.max, axis)) for axis in "xyz"}
        facts = []
        for face in solid.faces():
            node = graph.require_node(face)
            if BRepAdaptor_Surface(face.wrapped).GetType() != GeomAbs_Plane:
                continue
            normal = graph.normal(node)
            assert normal is not None
            dominant = next(
                (
                    axis
                    for axis, value in zip("xyz", normal, strict=True)
                    if abs(abs(value) - 1) <= 1e-3
                ),
                None,
            )
            edge_types = [edge.geom_type for edge in face.edges()]
            wall = (
                bool(edge_types)
                and all(kind in (GeomType.LINE, GeomType.CIRCLE) for kind in edge_types)
                and sum(kind is GeomType.LINE for kind in edge_types) >= 1
                and sum(kind is GeomType.CIRCLE for kind in edge_types) <= 1
            )
            facts.append((node, face, normal, dominant, wall))

        proposals = []
        for width_axis in "xyz":
            wi = "xyz".index(width_axis)
            walls = [fact for fact in facts if fact[3] == width_axis and fact[4]]
            for left_index, left in enumerate(walls):
                for right in walls[left_index + 1 :]:
                    left_c = _bbox_center(left[1], width_axis)
                    right_c = _bbox_center(right[1], width_axis)
                    if left_c > right_c:
                        left, right = right, left
                        left_c, right_c = right_c, left_c
                    if not (left[2][wi] > 0 and right[2][wi] < 0):
                        continue
                    if not _bounds_one_void(graph, left[0], right[0]):
                        continue
                    other_axes = [axis for axis in "xyz" if axis != width_axis]
                    ranges = {}
                    for axis in other_axes:
                        lo = max(
                            _coordinate(left[1].bounding_box().min, axis),
                            _coordinate(right[1].bounding_box().min, axis),
                        )
                        hi = min(
                            _coordinate(left[1].bounding_box().max, axis),
                            _coordinate(right[1].bounding_box().max, axis),
                        )
                        ranges[axis] = (lo, hi)
                    if any(hi - lo <= 0 for lo, hi in ranges.values()):
                        continue
                    width = right_c - left_c
                    for depth_axis in other_axes:
                        long_axis = next(axis for axis in other_axes if axis != depth_axis)
                        d_lo, d_hi = ranges[depth_axis]
                        lo, hi = ranges[long_axis]
                        foot = {
                            width_axis: (left_c, right_c),
                            long_axis: (lo, hi),
                        }
                        capped = []
                        for end, wanted in ((d_lo, 1), (d_hi, -1)):
                            covered = 0.0
                            for _node, face, normal, face_axis, _wall in facts:
                                if (
                                    face_axis != depth_axis
                                    or normal["xyz".index(depth_axis)] * wanted <= 0
                                ):
                                    continue
                                if abs(_bbox_center(face, depth_axis) - end) > 0.3:
                                    continue
                                area = 1.0
                                face_bb = face.bounding_box()
                                for axis_name, (foot_lo, foot_hi) in foot.items():
                                    overlap = min(
                                        _coordinate(face_bb.max, axis_name), foot_hi
                                    ) - max(_coordinate(face_bb.min, axis_name), foot_lo)
                                    area *= max(overlap, 0)
                                covered += area
                            capped.append(covered >= 0.5 * width * (hi - lo))
                        if sum(capped) != 1:
                            continue
                        if not (
                            math.isclose(lo, bounds[long_axis][0], abs_tol=0.3)
                            and math.isclose(hi, bounds[long_axis][1], abs_tol=0.3)
                        ):
                            continue
                        long_span = _uninterrupted_span(
                            graph, left[0], right[0], long_axis, (lo, hi)
                        )
                        if long_span is None:
                            continue
                        spans = {
                            width_axis: (left_c, right_c),
                            long_axis: long_span,
                            depth_axis: (d_lo, d_hi),
                        }
                        if not _whole_inset_prism_is_empty(solid, spans):
                            continue
                        proposals.append(
                            (
                                Channel(
                                    width_axis,
                                    long_axis,
                                    round(width, 2),
                                    round((left_c + right_c) / 2, 2),
                                    round(lo, 2),
                                    round(hi, 2),
                                    round(d_lo, 2),
                                    round(d_hi, 2),
                                    1 if capped[0] else -1,
                                ),
                                frozenset((left[0], right[0])),
                            )
                        )
        by_record: dict[Channel, set[frozenset[FaceNode]]] = {}
        for record, nodes in proposals:
            by_record.setdefault(record, set()).add(nodes)
        for record, node_sets in by_record.items():
            assert len(node_sets) == 1
            expected.append((record, next(iter(node_sets))))
    return sorted(
        expected,
        key=lambda item: (
            item[0].long_axis,
            item[0].width_axis,
            item[0].lo,
            item[0].hi,
            item[0].w_center,
            item[0].width,
            item[0].d_lo,
            item[0].d_hi,
            item[0].open_sign,
        ),
    )


def _assert_roles(part, **kwargs):
    ledger = ClaimLedger(FaceGraph(part, face_edges=kwargs.get("face_edges")))
    expected = _fresh_expected_channels(part, ledger.graph)
    public = recognise_channels(part, **kwargs)
    records = _discover_channels(part, writer=ledger.writer, **kwargs)
    assert [record.to_dict() for record, _nodes in expected] == [
        replace(record, body_key=()).to_dict() for record in records
    ]
    assert [record.to_dict() for record in records] == [record.to_dict() for record in public]
    candidates = ledger.candidate_set(FamilyId.CHANNELS).candidates
    assert len(records) == len(candidates)
    for (expected_record, expected_nodes), record, candidate in zip(
        expected, records, candidates, strict=True
    ):
        assert replace(record, body_key=()) == expected_record
        assert candidate.record is record
        nodes = ledger.defining_of(candidate)
        assert nodes == expected_nodes
        assert ledger.graph.common_valid_solid(nodes) is not None
        constituent = ledger.snapshot_index().constituent_of(candidate)
        floor = constituent - nodes
        depth_axis = "xyz".index(record.depth_axis)
        assert nodes < constituent and floor
        assert ledger.graph.common_valid_solid(constituent) is not None
        assert all(ledger.graph.is_planar(node) for node in floor)
        assert all(
            ledger.graph.normal(node)[depth_axis] * record.open_sign > 0.99 for node in floor
        )
        centres = []
        signs = []
        axis = "xyz".index(record.width_axis)
        for node in nodes:
            face = ledger.graph.face(node)
            assert ledger.graph.is_planar(node)
            centres.append(_bbox_center(face, record.width_axis))
            normal = ledger.graph.normal(node)
            assert normal is not None
            signs.append(normal[axis])
        ordered = sorted(zip(centres, signs, strict=True))
        assert round(ordered[1][0] - ordered[0][0], 2) == record.width
        assert round((ordered[0][0] + ordered[1][0]) / 2, 2) == record.w_center
        assert ordered[0][1] > 0 and ordered[1][1] < 0
    return records, candidates, ledger


def test_canonical_channel_owns_only_two_opposed_walls() -> None:
    records, candidates, ledger = _assert_roles(build_fixture())
    assert len(records) == 1
    assert len(ledger.defining_of(candidates[0])) == 2
    assert len(ledger.snapshot_index().constituent_of(candidates[0])) == 3


@pytest.mark.parametrize("alias_wall", [False, True])
def test_missing_or_wall_aliased_floor_refuses_before_publication(monkeypatch, alias_wall) -> None:
    part = build_fixture()
    graph = FaceGraph(part)
    ledger = ClaimLedger(graph)
    proposals = core_module._channel_proposals_one(part, graph=graph)
    assert len(proposals) == 1
    proposal = proposals[0]
    floor = frozenset({proposal.low_wall}) if alias_wall else frozenset()
    monkeypatch.setattr(
        feature_module,
        "_channel_proposals_one",
        lambda *_args, **_kwargs: [replace(proposal, floor=floor)],
    )

    with pytest.raises(ValueError, match="floor identity is unavailable"):
        _discover_channels(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.CHANNELS).candidates == ()


def test_record_only_compatibility_wrapper_preserves_value_and_order() -> None:
    part = Compound([Pos(80, 0, 0) * build_fixture(), Pos(-80, 0, 0) * build_fixture()])
    records = [record for solid in part.solids() for record in _recognise_channels_one(solid)]
    assert records
    assert [record.to_dict() for record in records] == [
        replace(record, body_key=()).to_dict()
        for solid in part.solids()
        for record in recognise_channels(solid)
    ]


def test_public_ledger_remains_graph_only_and_writer_free() -> None:
    part = build_fixture()
    ledger = ClaimLedger(FaceGraph(part))
    assert recognise_channels(part, ledger=ledger) == recognise_channels(part)
    assert ledger.candidate_set(FamilyId.CHANNELS).candidates == ()


@pytest.mark.parametrize(
    "part",
    [
        Pos(13, -9, 7) * build_fixture(),
        Rot(0, 0, 90) * build_fixture(),
        Rot(90, 0, 0) * build_fixture(),
        Rot(0, 90, 0) * build_fixture(),
        Rot(180, 0, 0) * build_fixture(),
        build_fixture().mirror(Plane.YZ),
        build_fixture().scale(0.2),
        build_fixture().scale(5),
    ],
)
def test_axis_preserving_transforms_keep_exact_wall_roles(part) -> None:
    _assert_roles(part)


def test_multiple_bodies_keep_occurrence_and_body_identity() -> None:
    part = Compound([Pos(-80, 0, 0) * build_fixture(), Pos(80, 0, 0) * build_fixture()])
    records, candidates, ledger = _assert_roles(part)
    assert len(records) == 2
    defining = [ledger.defining_of(candidate) for candidate in candidates]
    assert defining[0].isdisjoint(defining[1])
    assert ledger.graph.common_valid_solid(defining[0]) != ledger.graph.common_valid_solid(
        defining[1]
    )


def test_equal_channels_on_one_and_coincident_bodies_keep_occurrence_identity() -> None:
    same_body = (
        Box(60, 70, 10)
        + Pos(0, -30, 13) * Box(60, 10, 16)
        + Pos(0, 0, 13) * Box(60, 10, 16)
        + Pos(0, 30, 13) * Box(60, 10, 16)
    )
    records, candidates, ledger = _assert_roles(same_body)
    assert len(records) == 2 and records[0].width == records[1].width
    assert ledger.defining_of(candidates[0]).isdisjoint(ledger.defining_of(candidates[1]))

    coincident = Compound([build_fixture(), copy.deepcopy(build_fixture())])
    records, candidates, ledger = _assert_roles(coincident)
    assert len(records) == 2 and records[0] == records[1] and records[0] is not records[1]
    defining = [ledger.defining_of(candidate) for candidate in candidates]
    assert defining[0].isdisjoint(defining[1])
    assert ledger.graph.common_valid_solid(defining[0]) != ledger.graph.common_valid_solid(
        defining[1]
    )


def test_round_clip_split_floor_and_depth_width_extremes_keep_roles() -> None:
    round_clipped = build_fixture() & Cylinder(38, 40)
    split_floor = build_fixture() - Pos(0, 0, 5) * Box(2, 50, 1)
    deep = Box(60, 50, 10) + Pos(0, -20, 30) * Box(60, 10, 50) + Pos(0, 20, 30) * Box(60, 10, 50)
    wider_than_long = (
        Box(30, 80, 10) + Pos(0, -30, 13) * Box(30, 20, 16) + Pos(0, 30, 13) * Box(30, 20, 16)
    )
    for part in (round_clipped, split_floor, deep, wider_than_long):
        assert _assert_roles(part)[0]


def test_real_nonchannel_predicates_issue_no_evidence() -> None:
    enclosed = Box(60, 60, 20) - Box(20, 20, 10)
    through_slot = Box(60, 60, 20) - Box(20, 60, 20)
    bridged = build_fixture() + Pos(0, 0, 13) * Box(4, 30, 6)
    off_centre_membrane = build_fixture() + Pos(0, 8, 13) * Box(4, 2, 16)
    capped_both_ends = build_fixture() + Pos(0, 0, 21) * Box(60, 30, 2)
    no_floor = Pos(0, -20, 13) * Box(60, 10, 16) + Pos(0, 20, 13) * Box(60, 10, 16)
    short_walls = (
        Box(60, 50, 10) + Pos(0, -20, 13) * Box(50, 10, 16) + Pos(0, 20, 13) * Box(50, 10, 16)
    )
    oblique = Rot(17, 23, 11) * build_fixture()
    for part in (
        Box(30, 30, 30),
        enclosed,
        through_slot,
        bridged,
        off_centre_membrane,
        capped_both_ends,
        no_floor,
        short_walls,
        oblique,
    ):
        assert recognise_channels(part) == []
        ledger = ClaimLedger(FaceGraph(part))
        assert _discover_channels(part, writer=ledger.writer) == []
        assert ledger.candidate_set(FamilyId.CHANNELS).candidates == ()


def test_closed_channel_threshold_and_projection_contract() -> None:
    assert (_AXIS_ALIGNED_TOL, _MERGE_TOL, _FLOOR_TOL, _FLOOR_COVER_FRAC) == (
        1e-3,
        0.5,
        0.3,
        0.5,
    )
    at = 1 - _AXIS_ALIGNED_TOL
    assert _dominant_axis((math.nextafter(at, 1), 0, 0)) == "x"
    assert _dominant_axis((math.nextafter(at, 0), 0, 0)) is None
    assert round(1.2349, 2) == 1.23 and round(1.2351, 2) == 1.24

    ordered = [
        Channel("y", "x", 2, 3, -5, 5, 0, 2, 1),
        Channel("x", "z", 2, 0, -4, 4, 1, 3, -1),
        Channel("x", "x", 4, 0, -5, 5, 0, 2, 1),
    ]
    assert sorted(ordered, key=feature_module._channel_sort_key) == [
        ordered[2],
        ordered[0],
        ordered[1],
    ]

    reference = Box(10, 10, 1).faces().sort_by().last
    cap = _Face((0, 0, 1), "z", reference.bounding_box(), True)
    centre = _bbox_center(reference, "z")
    foot = {"x": (-5, 5), "y": (-5, 5)}
    assert _end_capped([cap], foot, 200.0, "z", centre, 1)
    assert not _end_capped([cap], foot, math.nextafter(200.0, math.inf), "z", centre, 1)
    accepted_end = math.nextafter(centre + _FLOOR_TOL, centre)
    refused_end = math.nextafter(centre + _FLOOR_TOL, math.inf)
    assert _end_capped([cap], foot, 100.0, "z", accepted_end, 1)
    assert not _end_capped([cap], foot, 100.0, "z", refused_end, 1)


def test_closed_wall_cap_and_void_helper_boundaries() -> None:
    class Edge:
        def __init__(self, kind) -> None:
            self.geom_type = kind

    class Face:
        def __init__(self, kinds) -> None:
            self._edges = [Edge(kind) for kind in kinds]

        def edges(self):
            return self._edges

    assert not _is_wall(Face([]))
    assert _is_wall(Face([GeomType.LINE, GeomType.LINE]))
    assert _is_wall(Face([GeomType.LINE, GeomType.CIRCLE]))
    assert not _is_wall(Face([GeomType.CIRCLE, GeomType.CIRCLE]))
    assert not _is_wall(Face([GeomType.LINE, GeomType.BSPLINE]))

    reference = Box(10, 10, 1).faces().sort_by().last
    bb = reference.bounding_box()
    foot = {"x": (-5, 5), "y": (-5, 5)}
    plus = _Face((0, 0, 1), "z", bb, True)
    minus = _Face((0, 0, -1), "z", bb, True)
    end = _bbox_center(reference, "z")
    assert _end_capped([plus], foot, 100, "z", end, 1)
    assert not _end_capped([minus], foot, 100, "z", end, 1)
    assert _end_capped([minus], foot, 100, "z", end, -1)

    record = recognise_channels(build_fixture())[0]
    spans = {
        record.width_axis: (record.w_center - record.width / 2, record.w_center + record.width / 2),
        record.long_axis: (record.lo, record.hi),
        ({"x", "y", "z"} - {record.width_axis, record.long_axis}).pop(): (record.d_lo, record.d_hi),
    }
    assert _whole_inset_prism_is_empty(build_fixture(), spans)
    membrane = build_fixture() + Pos(0, 0, 13) * Box(2 * COORD_FLOOR, 2, 16)
    assert not _whole_inset_prism_is_empty(membrane, spans)


def test_closed_aag_boundary_and_long_span_branches() -> None:
    left, right, curved, a, b = object(), object(), object(), object(), object()
    face_box = Box(1, 1, 1).bounding_box()
    fa = _Face((1, 0, 0), "x", face_box, True, cast(FaceNode, left))
    fb = _Face((-1, 0, 0), "x", face_box, True, cast(FaceNode, right))

    class Graph:
        def __init__(self, *, common=(), arcs=None, regions=None, bounds=None) -> None:
            self.common = set(common)
            self.arcs = arcs or {}
            self.regions = regions or {}
            self.node_bounds = bounds or {}

        def neighbours(self, node):
            if node in (left, right):
                return self.common or ({a} if node is left else {b})
            return set()

        def is_planar(self, _node):
            return False

        def arc(self, first, second):
            return self.arcs[first, second]

        def smooth_region(self, node):
            return self.regions.get(node, frozenset({node}))

        def bounds(self, node):
            return self.node_bounds[node]

    shared = frozenset({a, b})
    fragmented = Graph(
        arcs={(left, a): "concave", (right, b): "concave"},
        regions={a: shared, b: shared},
    )
    assert production_bounds_one_void(fa, fb, cast(FaceGraph, fragmented))
    smooth = Graph(
        arcs={(left, a): "convex", (right, b): "convex"},
        regions={left: frozenset({left, right})},
    )
    assert production_bounds_one_void(fa, fb, cast(FaceGraph, smooth))

    trimming = Graph(
        common={curved},
        arcs={(left, curved): "convex", (right, curved): "concave"},
        bounds={curved: ((0, 0), (0, 0), (0, 2))},
    )
    assert _uninterrupted_long_span("z", (0, 10), fa, fb, cast(FaceGraph, trimming)) == (2, 10)
    same_turn = Graph(
        common={curved},
        arcs={(left, curved): "concave", (right, curved): "concave"},
        bounds={curved: ((0, 0), (0, 0), (0, 2))},
    )
    assert _uninterrupted_long_span("z", (0, 10), fa, fb, cast(FaceGraph, same_turn)) == (0, 10)


def test_channel_envelope_floor_tolerance_and_open_sign() -> None:
    inside = _FLOOR_TOL - COORD_FLOOR
    outside = _FLOOR_TOL + COORD_FLOOR
    accepted = (
        Box(60, 50, 10)
        + Pos(0, -20, 13) * Box(60 - 2 * inside, 10, 16)
        + Pos(0, 20, 13) * Box(60 - 2 * inside, 10, 16)
    )
    refused = (
        Box(60, 50, 10)
        + Pos(0, -20, 13) * Box(60 - 2 * outside, 10, 16)
        + Pos(0, 20, 13) * Box(60 - 2 * outside, 10, 16)
    )
    assert recognise_channels(accepted)
    assert recognise_channels(refused) == []

    low_floor = build_fixture()
    high_floor = Rot(180, 0, 0) * low_floor
    low = recognise_channels(low_floor)
    high = recognise_channels(high_floor)
    assert len(low) == len(high) == 1
    assert {low[0].open_sign, high[0].open_sign} == {-1, 1}


def test_equal_serialized_value_public_first_wins_writer_refuses(monkeypatch) -> None:
    part = build_fixture()
    graph = FaceGraph(part)
    original = feature_module._channel_proposals_one
    proposal = original(part, None, graph)[0]
    other = next(
        node for node in graph.nodes if node not in {proposal.low_wall, proposal.high_wall}
    )

    def collision(*_args, **_kwargs):
        return [proposal, replace(proposal, high_wall=other)]

    monkeypatch.setattr(feature_module, "_channel_proposals_one", collision)
    # Geometry-only compatibility retains the first occurrence for an equal 2dp record.
    projected = _discover_channels(part, graph=graph)
    assert [replace(record, body_key=()) for record in projected] == [proposal.record]
    assert projected[0].body_key is not None
    ledger = ClaimLedger(graph)
    with pytest.raises(ValueError, match="ambiguous"):
        _discover_channels(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.CHANNELS).candidates == ()


def test_open_shell_and_late_second_body_refuse_atomically(monkeypatch) -> None:
    from build123d import Shell

    shell = Shell(build_fixture().faces())
    assert recognise_channels(shell)
    ledger = ClaimLedger(FaceGraph(shell))
    with pytest.raises(ValueError, match="one valid solid"):
        _discover_channels(shell, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.CHANNELS).candidates == ()

    part = Compound([Pos(-80, 0, 0) * build_fixture(), Pos(80, 0, 0) * build_fixture()])
    ledger = ClaimLedger(FaceGraph(part))
    original = ledger.graph.common_valid_solid
    calls = 0

    def fail_second(nodes):
        nonlocal calls
        calls += 1
        return None if calls == 2 else original(nodes)

    monkeypatch.setattr(ledger.graph, "common_valid_solid", fail_second)
    with pytest.raises(ValueError, match="one valid solid"):
        _discover_channels(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.CHANNELS).candidates == ()


def test_step_traversal_and_supplied_edges_preserve_roles(monkeypatch, tmp_path: Path) -> None:
    target = tmp_path / "channel.step"
    assert export_step(build_fixture(), target)
    _assert_roles(import_step(target))

    part = build_fixture()
    solid_type = type(part)
    original = solid_type.faces

    def reversed_faces(self):
        faces = original(self)
        return type(faces)(reversed(faces))

    monkeypatch.setattr(solid_type, "faces", reversed_faces)
    _assert_roles(part, face_edges=FaceEdges())


def test_equal_record_competing_pair_refuses_without_prefix(monkeypatch) -> None:
    part = build_fixture()
    ledger = ClaimLedger(FaceGraph(part))
    original = feature_module._channel_proposals_one

    def ambiguous(*args, **kwargs):
        proposals = original(*args, **kwargs)
        other = next(
            node
            for node in ledger.graph.nodes
            if node
            not in {
                proposals[0].low_wall,
                proposals[0].high_wall,
            }
        )
        return [*proposals, replace(proposals[0], high_wall=other)]

    monkeypatch.setattr(feature_module, "_channel_proposals_one", ambiguous)
    with pytest.raises(ValueError, match="ambiguous"):
        _discover_channels(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.CHANNELS).candidates == ()


def test_distinct_channels_may_truthfully_share_one_original_wall() -> None:
    part = import_step(ROOT / "tests/corpus/mfcadpp_holdout/274.step")
    records, candidates, ledger = _assert_roles(part)
    assert len(records) == len(candidates) == 3
    defining = [ledger.defining_of(candidate) for candidate in candidates]
    shared = defining[0] & defining[1]
    assert len(shared) == 1
    assert defining[0] != defining[1]
    assert records[0] != records[1]
    assert records[0].open_sign == -records[1].open_sign
    assert defining[2].isdisjoint(defining[0] | defining[1])
    assert all(
        candidate.record is record for candidate, record in zip(candidates, records, strict=True)
    )


def test_foreign_graph_copied_node_and_late_body_failure_are_atomic(monkeypatch) -> None:
    part = build_fixture()
    foreign = ClaimLedger(FaceGraph(Pos(100, 0, 0) * build_fixture()))
    with pytest.raises(ValueError):
        _discover_channels(part, writer=foreign.writer)
    assert foreign.candidate_set(FamilyId.CHANNELS).candidates == ()

    local = ClaimLedger(FaceGraph(part))
    with pytest.raises(ValueError, match="one authority"):
        _discover_channels(part, graph=foreign.graph, writer=local.writer)
    assert local.candidate_set(FamilyId.CHANNELS).candidates == ()

    ledger = ClaimLedger(FaceGraph(part))
    original = feature_module._channel_proposals_one

    def copied(*args, **kwargs):
        proposals = original(*args, **kwargs)
        return [replace(proposals[0], low_wall=copy.copy(proposals[0].low_wall))]

    monkeypatch.setattr(feature_module, "_channel_proposals_one", copied)
    with pytest.raises(ValueError):
        _discover_channels(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.CHANNELS).candidates == ()

    monkeypatch.setattr(feature_module, "_channel_proposals_one", original)

    def same_node(*args, **kwargs):
        proposals = original(*args, **kwargs)
        return [replace(proposals[0], high_wall=proposals[0].low_wall)]

    monkeypatch.setattr(feature_module, "_channel_proposals_one", same_node)
    with pytest.raises(ValueError, match="distinct"):
        _discover_channels(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.CHANNELS).candidates == ()

    monkeypatch.setattr(feature_module, "_channel_proposals_one", original)
    monkeypatch.setattr(ledger.graph, "common_valid_solid", lambda _nodes: None)
    with pytest.raises(ValueError, match="one valid solid"):
        _discover_channels(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.CHANNELS).candidates == ()


def test_proposal_builder_refuses_a_candidate_without_graph_nodes(monkeypatch) -> None:
    part = build_fixture()
    graph = FaceGraph(part)
    faces = core_module._planar_faces(part, graph=graph)
    walls = [face for face in faces if face.wall and face.axis == "y"]
    assert len(walls) >= 2
    node_free = [replace(face, node=None) for face in faces]
    expected = recognise_channels(part)[0]

    monkeypatch.setattr(core_module, "_planar_faces", lambda *_args, **_kwargs: node_free)
    monkeypatch.setattr(core_module, "_channel_candidate", lambda *_args, **_kwargs: expected)
    with pytest.raises(ValueError, match="require graph nodes"):
        core_module._channel_proposals_one(part, graph=graph)


def test_proposal_builder_refuses_a_candidate_without_retained_floor_nodes(monkeypatch) -> None:
    part = build_fixture()
    graph = FaceGraph(part)
    expected = recognise_channels(part)[0]

    monkeypatch.setattr(core_module, "_channel_candidate", lambda *_args, **_kwargs: expected)
    with pytest.raises(ValueError, match="floor identity is unavailable"):
        core_module._channel_proposals_one(part, graph=graph)


def test_candidate_remains_compatible_without_a_floor_identity_consumer(monkeypatch) -> None:
    part = build_fixture()
    graph = FaceGraph(part)
    original = core_module._channel_candidate
    captured = {}

    def capture(*args, **kwargs):
        result = original(*args, **kwargs)
        if result is not None:
            captured["args"] = args
        return result

    monkeypatch.setattr(core_module, "_channel_candidate", capture)
    expected = core_module._channel_proposals_one(part, graph=graph)[0].record

    assert original(*captured["args"]) == expected


def test_translated_stale_and_mixed_solid_wall_snapshots_refuse(monkeypatch) -> None:
    part = build_fixture()
    ledger = ClaimLedger(FaceGraph(part))
    original = feature_module._channel_proposals_one
    stale_part = Pos(1, 0, 0) * build_fixture()
    stale_graph = FaceGraph(stale_part)
    stale = original(stale_part, None, stale_graph)[0].low_wall

    def translated(*args, **kwargs):
        proposals = original(*args, **kwargs)
        return [replace(proposals[0], low_wall=stale)]

    monkeypatch.setattr(feature_module, "_channel_proposals_one", translated)
    with pytest.raises(ValueError):
        _discover_channels(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.CHANNELS).candidates == ()

    monkeypatch.setattr(feature_module, "_channel_proposals_one", original)
    compound = Compound([Pos(-80, 0, 0) * build_fixture(), Pos(80, 0, 0) * build_fixture()])
    ledger = ClaimLedger(FaceGraph(compound))
    solids = list(compound.solids())
    proposals = [original(solid, None, ledger.graph)[0] for solid in solids]
    calls = 0

    def mixed(*_args, **_kwargs):
        nonlocal calls
        proposal = proposals[calls]
        calls += 1
        if calls == 1:
            proposal = replace(proposal, high_wall=proposals[1].high_wall)
        return [proposal]

    monkeypatch.setattr(feature_module, "_channel_proposals_one", mixed)
    with pytest.raises(ValueError, match="one valid solid"):
        _discover_channels(compound, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.CHANNELS).candidates == ()


def test_terminal_inventory_retains_channel_identity() -> None:
    product = _take_inventory(build_fixture())
    candidates = product.physical.candidate_set(FamilyId.CHANNELS).candidates
    assert len(candidates) == len(product._legacy_result.channels) == 1
    assert candidates[0].record is product._legacy_result.channels[0]
    assert len(product.evidence.defining_of(candidates[0])) == 2


def _qualified_calls(tree: ast.AST):
    aliases = {}
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            for alias in node.names:
                aliases[alias.asname or alias.name.split(".")[0]] = alias.name
        elif isinstance(node, ast.ImportFrom) and node.module:
            for alias in node.names:
                aliases[alias.asname or alias.name] = f"{node.module}.{alias.name}"

    def qualified(node):
        if isinstance(node, ast.Name):
            return aliases.get(node.id, node.id)
        if isinstance(node, ast.Attribute):
            return f"{qualified(node.value)}.{node.attr}"
        return ""

    return [(qualified(node.func), node) for node in ast.walk(tree) if isinstance(node, ast.Call)]


def test_channel_private_core_and_registry_writer_route_are_closed() -> None:
    sites: list[tuple[str, ast.Call]] = []
    for path in (ROOT / "src/quiddity").glob("*.py"):
        tree = ast.parse(path.read_text(encoding="utf-8"))
        sites.extend(
            (path.name, call)
            for name, call in _qualified_calls(tree)
            if name == "_discover_channels" or name.endswith("._discover_channels")
        )
    assert {name for name, _call in sites} == {"_recess_features.py", "_registry.py"}
    registry = next(call for name, call in sites if name == "_registry.py")
    writer = {keyword.arg: keyword.value for keyword in registry.keywords}["writer"]
    assert (
        isinstance(writer, ast.Attribute)
        and writer.attr == "writer"
        and isinstance(writer.value, ast.Name)
        and writer.value.id == "s"
    )

    feature_tree = ast.parse(
        (ROOT / "src/quiddity/_recess_features.py").read_text(encoding="utf-8")
    )
    public = next(
        node
        for node in feature_tree.body
        if isinstance(node, ast.FunctionDef) and node.name == "recognise_channels"
    )
    public_calls = [
        call
        for name, call in _qualified_calls(public)
        if name == "_discover_channels" or name.endswith("._discover_channels")
    ]
    assert len(public_calls) == 1
    (call,) = public_calls
    assert all(keyword.arg != "writer" for keyword in call.keywords)
    graph = {keyword.arg: keyword.value for keyword in call.keywords}["graph"]
    assert (
        isinstance(graph, ast.IfExp)
        and isinstance(graph.test, ast.Compare)
        and isinstance(graph.test.left, ast.Name)
        and graph.test.left.id == "ledger"
        and len(graph.test.ops) == 1
        and isinstance(graph.test.ops[0], ast.Is)
        and isinstance(graph.test.comparators[0], ast.Constant)
        and graph.test.comparators[0].value is None
        and isinstance(graph.orelse, ast.Attribute)
        and isinstance(graph.orelse.value, ast.Name)
        and graph.orelse.value.id == "ledger"
        and graph.orelse.attr == "graph"
    )

    constructors: list[tuple[str, str]] = []
    for path in (ROOT / "src/quiddity").glob("*.py"):
        tree = ast.parse(path.read_text(encoding="utf-8"))
        constructors.extend(
            (path.name, name)
            for name, _call in _qualified_calls(tree)
            if name == "Channel" or name.endswith(".Channel")
        )
    assert len(constructors) == 1
    assert constructors[0][0] == "_recess_core.py"
    assert constructors[0][1].endswith(".Channel") or constructors[0][1] == "Channel"

    core = ast.parse((ROOT / "src/quiddity/_recess_core.py").read_text(encoding="utf-8"))
    proposal = next(
        node
        for node in core.body
        if isinstance(node, ast.FunctionDef) and node.name == "_channel_proposals_one"
    )
    assert (
        sum(
            name.endswith("._planar_faces") or name == "_planar_faces"
            for name, _call in _qualified_calls(proposal)
        )
        == 1
    )

    forbidden = {
        "EvidenceIndex",
        "CandidateInventory",
        "ReconciliationResult",
        "reconcile_recess_candidates",
        "recognise_slots",
        "recognise_pockets",
    }
    imported = {
        alias.name
        for tree in (feature_tree, core)
        for node in tree.body
        if isinstance(node, ast.ImportFrom)
        for alias in node.names
    }
    assert imported.isdisjoint(forbidden)
    discover = next(
        node
        for node in feature_tree.body
        if isinstance(node, ast.FunctionDef) and node.name == "_discover_channels"
    )
    referenced = {node.id for node in ast.walk(discover) if isinstance(node, ast.Name)}
    assert referenced.isdisjoint(forbidden)

    from quiddity._effective_surfaces import SURFACE_READER_SITES

    assert not any("_discover_channels" in site for site in SURFACE_READER_SITES)
    assert "_recess_faces:_planar_faces:is_planar:1" in SURFACE_READER_SITES
