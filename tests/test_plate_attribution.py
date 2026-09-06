"""F5: Plates own their complete low-negative/high-positive planar groups."""

from __future__ import annotations

import ast
import copy
import math
from dataclasses import dataclass, replace
from pathlib import Path

import pytest
from build123d import (
    Align,
    Axis,
    Box,
    Compound,
    Face,
    Plane,
    Pos,
    Rot,
    Shell,
    Vector,
    export_step,
    import_step,
)
from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.BRepGProp import BRepGProp
from OCP.GeomAbs import GeomAbs_Plane
from OCP.GProp import GProp_GProps

from quiddity import (
    FramedRecognitionResult,
    build_framed_recognition_result,
    build_recognition_result,
    recognise_plates,
)
from quiddity._adjacency import FaceGraph, FaceNode, SolidRef
from quiddity._candidates import FamilyId
from quiddity._claims import ClaimLedger
from quiddity._geometry import clears_threshold, cluster_coordinates
from quiddity._registry import PHYSICAL_DEFINITIONS
from quiddity._run import start
from quiddity.plates import Plate, _discover_plates, _PlateAttributionError
from quiddity.result import _discover_all, _take_inventory
from tests.golden.plates_pads_levels_and_slanted_steps.fixture import build_fixture

ROOT = Path(__file__).parents[1]


def _qualified_calls(tree: ast.AST) -> list[tuple[str, ast.Call]]:
    aliases: dict[str, str] = {}
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            for alias in node.names:
                aliases[alias.asname or alias.name.split(".")[0]] = alias.name
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


@dataclass(frozen=True)
class _Expected:
    record: Plate
    nodes: frozenset[FaceNode]
    solid: SolidRef


def _clusters(values: list[float], tolerance: float) -> list[list[int]]:
    ordered = sorted(range(len(values)), key=values.__getitem__)
    groups: list[list[int]] = []
    for index in ordered:
        if not groups or values[index] - values[groups[-1][0]] > tolerance:
            groups.append([index])
        else:
            groups[-1].append(index)
    return groups


def _fresh_expected(part, graph: FaceGraph, *, min_area=0.4, max_thick=0.5, tol=0.5):
    solids = list(part.solids())
    if len(solids) > 1:
        expected = [
            item
            for solid in solids
            for item in _fresh_expected(
                solid,
                graph,
                min_area=min_area,
                max_thick=max_thick,
                tol=tol,
            )
        ]
        return sorted(
            expected,
            key=lambda item: (
                item.record.axis,
                item.record.lo,
                item.record.hi,
                item.record.u,
                item.record.v,
            ),
        )
    bb = part.bounding_box()
    ext = (bb.max.X - bb.min.X, bb.max.Y - bb.min.Y, bb.max.Z - bb.min.Z)
    facts = []
    for face in part.faces():
        surface = BRepAdaptor_Surface(face.wrapped)
        if surface.GetType() != GeomAbs_Plane:
            continue
        normal = face.normal_at()
        vector = (normal.X, normal.Y, normal.Z)
        props = GProp_GProps()
        BRepGProp.SurfaceProperties_s(face.wrapped, props)
        centre = props.CentreOfMass()
        point = (centre.X(), centre.Y(), centre.Z())
        plane = surface.Plane().Location()
        location = (plane.X(), plane.Y(), plane.Z())
        facts.append((graph.require_node(face), vector, props.Mass(), point, location))

    proposals = []
    for axis in range(3):
        cross = math.prod(ext[index] for index in range(3) if index != axis)
        if cross <= 0:
            continue
        sides: tuple[list[tuple], list[tuple]] = ([], [])
        for fact in facts:
            component = fact[1][axis]
            # Independently pin the frozen production threshold rather than importing it.
            if abs(component) < 0.99:
                continue
            sides[component > 0].append(fact)
        groups = []
        for side in sides:
            grouped = {}
            locations = [fact[4][axis] for fact in side]
            for cluster in _clusters(locations, tol):
                members = [side[index] for index in cluster]
                coordinate = min(item[4][axis] for item in members)
                area = sum(item[2] for item in members)
                in_plane = [index for index in range(3) if index != axis]
                grouped[coordinate] = (
                    area,
                    sum(item[3][in_plane[0]] * item[2] for item in members),
                    sum(item[3][in_plane[1]] * item[2] for item in members),
                    frozenset(item[0] for item in members),
                )
            groups.append(grouped)
        events = []
        for sign, grouped in zip((-1, 1), groups, strict=True):
            for coordinate, group in grouped.items():
                if group[0] > min_area * cross:
                    events.append((coordinate, sign, group))
        events.sort(key=lambda event: (event[0], event[1]))
        for low, high in zip(events, events[1:], strict=False):
            if low[1] != -1 or high[1] != 1:
                continue
            thickness = high[0] - low[0]
            if thickness <= tol or thickness >= max_thick * ext[axis]:
                continue
            area = low[2][0] + high[2][0]
            record = Plate(
                "xyz"[axis],
                round(low[0], 3),
                round(high[0], 3),
                (low[2][1] + high[2][1]) / area,
                (low[2][2] + high[2][2]) / area,
            )
            nodes = low[2][3] | high[2][3]
            solid = graph.common_valid_solid(nodes)
            assert solid is not None
            proposals.append(_Expected(record, nodes, solid))
    by_key: dict[tuple[str, float, float], _Expected] = {}
    for proposal in sorted(
        proposals, key=lambda item: (item.record.axis, item.record.lo, item.record.hi)
    ):
        by_key.setdefault((proposal.record.axis, proposal.record.lo, proposal.record.hi), proposal)
    return list(by_key.values())


def _claimed(part, **kwargs):
    ledger = ClaimLedger(FaceGraph(part))
    expected = _fresh_expected(
        part,
        ledger.graph,
        min_area=kwargs.get("min_area_frac", 0.4),
        max_thick=kwargs.get("max_thick_frac", 0.5),
        tol=0.5 if kwargs.get("tol") is None else kwargs["tol"],
    )
    public = recognise_plates(part, **kwargs)
    records = _discover_plates(part, writer=ledger.writer, **kwargs)
    assert [item.record.to_dict() for item in expected] == [
        replace(record, body_key=()).to_dict() for record in records
    ]
    assert [record.to_dict() for record in records] == [record.to_dict() for record in public]
    candidates = ledger.candidate_set(FamilyId.PLATES).candidates
    for item, record, candidate in zip(expected, records, candidates, strict=True):
        assert candidate.record is record
        assert ledger.defining_of(candidate) == item.nodes
        assert ledger.graph.common_valid_solid(item.nodes) == item.solid
    return records, candidates, ledger


@pytest.mark.parametrize(
    "part",
    [
        build_fixture(),
        Rot(90, 0, 0) * build_fixture(),
        Rot(0, 90, 0) * build_fixture(),
        build_fixture().mirror(Plane.YZ),
        build_fixture().scale(0.2),
        build_fixture().scale(5),
    ],
)
def test_plate_groups_survive_axes_mirror_and_scale(part) -> None:
    assert _claimed(part)[0]


@pytest.mark.parametrize(
    "part",
    [
        Box(40, 10, 30) + Pos(-15, 10, 0) * Box(10, 30, 30),
        Box(40, 10, 30) + Pos(0, 10, 0) * Box(10, 30, 30),
        Box(40, 10, 30) + Pos(-15, 10, 0) * Box(10, 30, 30) + Pos(15, 10, 0) * Box(10, 30, 30),
        Pos(123, -87, 41) * build_fixture(),
    ],
)
def test_l_t_u_multiple_occurrence_and_translation_lifecycle(part) -> None:
    records, candidates, _ledger = _claimed(part, min_area_frac=0.2, max_thick_frac=0.8)
    assert records and len(records) == len(candidates)


def test_u_structure_retains_two_unequal_location_occurrences() -> None:
    part = Box(40, 10, 30) + Pos(-15, 10, 0) * Box(10, 30, 30) + Pos(15, 10, 0) * Box(10, 30, 30)
    records, candidates, ledger = _claimed(part, min_area_frac=0.2, max_thick_frac=0.8)
    x_records = [record for record in records if record.axis == "x"]
    assert [(record.lo, record.hi) for record in x_records] == [(-20, -10), (10, 20)]
    x_candidates = [candidate for candidate in candidates if candidate.record.axis == "x"]
    assert len(x_candidates) == 2
    assert ledger.defining_of(x_candidates[0]).isdisjoint(ledger.defining_of(x_candidates[1]))


def test_separated_valid_bodies_retain_distinct_occurrence_identity() -> None:
    part = Compound([build_fixture(), Pos(200, 200, 200) * build_fixture()])
    records, candidates, ledger = _claimed(part, min_area_frac=0.01)
    assert len(records) == len(candidates) == 8
    solids = [
        ledger.graph.common_valid_solid(ledger.defining_of(candidate)) for candidate in candidates
    ]
    assert len(set(solids)) == 2 and None not in solids
    for left, right in zip(candidates, candidates[1:], strict=False):
        assert left.record is not right.record
        assert ledger.defining_of(left).isdisjoint(ledger.defining_of(right))


def test_two_body_t_brackets_preserve_four_local_plate_occurrences() -> None:
    def bracket():
        base = Pos(0, 0, 5) * Box(80, 60, 10)
        wall = Pos(0, 0, 35) * Box(80, 10, 50)
        return base + wall

    left = Pos(-70, 0, 0) * bracket()
    right = Pos(70, 0, 0) * bracket()
    part = Compound(children=[left, right])

    public = recognise_plates(part)
    aggregate = list(build_recognition_result(part, rotational=False).plates)
    assert aggregate == public
    assert [(plate.axis, plate.u) for plate in aggregate] == [
        ("y", -70.0),
        ("y", 70.0),
        ("z", -70.0),
        ("z", 70.0),
    ]


def test_two_body_plates_survive_nested_compounds_and_arbitrary_framed_motion() -> None:
    def bracket():
        return (Pos(0, 0, 5) * Box(80, 60, 10)) + (Pos(0, 0, 35) * Box(80, 10, 50))

    nested = Compound(
        children=[
            Compound(children=[Pos(-70, 0, 0) * bracket()]),
            Compound(children=[Pos(70, 0, 0) * bracket()]),
        ]
    )
    moved = Pos(17, -23, 9) * Rot(31, 47, 13) * nested

    framed = build_framed_recognition_result(moved, rotational=False)

    assert isinstance(framed, FramedRecognitionResult)
    assert len(framed.result.plates) == 4
    assert list(framed.result.plates) == recognise_plates(framed.part)
    product = _take_inventory(framed.part)
    candidates = product.physical.candidate_set(FamilyId.PLATES).candidates
    assert len(candidates) == 4
    owners = [
        product.context.graph.common_valid_solid(product.evidence.defining_of(candidate))
        for candidate in candidates
    ]
    assert len(set(owners)) == 2 and None not in owners


def test_compound_plate_order_is_geometry_deterministic() -> None:
    left = Pos(-100, 0, 0) * build_fixture()
    right = Pos(100, 0, 0) * build_fixture()
    forward = recognise_plates(Compound(children=[left, right]))
    reverse = recognise_plates(Compound(children=[right, left]))

    assert [record.to_dict() for record in forward] == [record.to_dict() for record in reverse]


def test_coincident_planes_from_other_solids_do_not_contaminate_plate_roles() -> None:
    part = Box(80, 60, 10) + Pos(0, -25, 30) * Box(80, 10, 40) + Pos(0, 25, 30) * Box(80, 10, 40)

    ledger = ClaimLedger(FaceGraph(part))
    public = recognise_plates(part)
    records = _discover_plates(part, writer=ledger.writer)
    candidates = ledger.candidate_set(FamilyId.PLATES).candidates

    # Each disconnected member is one flat envelope plate, deliberately excluded. Whole-part
    # grouping used to combine their planes into three fictitious multi-slab occurrences.
    assert records == []
    assert records == public
    assert len(candidates) == len(records)
    assert all(
        ledger.graph.common_valid_solid(ledger.defining_of(candidate)) is not None
        for candidate in candidates
    )


@pytest.mark.parametrize("offset", [(0, 0, 0), (0.2, 0.2, 0.2)])
def test_coincident_and_near_interleaved_bodies_retain_multiplicity(offset) -> None:
    part = Compound([build_fixture(), Pos(*offset) * copy.deepcopy(build_fixture())])
    records, candidates, ledger = _claimed(part)
    assert len(records) == len(candidates) == 4
    owners = [ledger.graph.common_valid_solid(ledger.defining_of(item)) for item in candidates]
    assert len(set(owners)) == 2 and None not in owners


def test_fragmented_groups_keep_every_original_patch() -> None:
    part = build_fixture() - Pos(-50, 0, 20) * Box(20, 4, 6)
    records, candidates, ledger = _claimed(part)
    assert records
    assert any(len(ledger.defining_of(candidate)) > 2 for candidate in candidates)


def test_bound_identity_collapses_wrappers_duplicates_and_reversed_order(monkeypatch) -> None:
    part = build_fixture()
    ledger = ClaimLedger(FaceGraph(part))
    original = list(part.faces())
    kind = type(part)
    monkeypatch.setattr(
        kind,
        "faces",
        lambda _self: (
            list(reversed([copy.copy(face) for face in original]))
            + [copy.copy(face) for face in original]
        ),
    )
    records = _discover_plates(part, writer=ledger.writer)
    candidates = ledger.candidate_set(FamilyId.PLATES).candidates
    assert len(records) == len(candidates) == 2
    assert tuple(candidate.record for candidate in candidates) == tuple(records)
    assert all(ledger.defining_of(candidate) for candidate in candidates)


def test_same_key_distinct_bound_role_pairs_refuse_atomically(monkeypatch) -> None:
    import quiddity.plates as module

    part = build_fixture()
    ledger = ClaimLedger(FaceGraph(part))
    record = Plate("x", -55, -45, 0, 20)
    monkeypatch.setattr(module, "Plate", lambda **_kwargs: record)
    with pytest.raises(_PlateAttributionError, match="competing defining groups"):
        _discover_plates(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.PLATES).candidates == ()


def test_step_traversal_custom_thresholds_and_shared_graph(monkeypatch, tmp_path: Path) -> None:
    target = tmp_path / "plates.step"
    assert export_step(build_fixture(), target)
    _claimed(import_step(target), min_area_frac=0.3, max_thick_frac=0.7, tol=0.25)

    part = build_fixture()
    kind = type(part)
    original = kind.faces
    monkeypatch.setattr(kind, "faces", lambda self: type(original(self))(reversed(original(self))))
    _claimed(part)


@pytest.mark.parametrize(
    ("kwargs", "present"),
    [
        ({"tol": 7.9999}, True),
        ({"tol": 8.0}, False),
        ({"max_thick_frac": 0.200001}, True),
        ({"max_thick_frac": 0.2}, False),
    ],
)
def test_plate_thickness_boundaries_are_strict(kwargs, present: bool) -> None:
    records = _discover_plates(build_fixture(), **kwargs)
    assert any(record.axis == "z" for record in records) is present


def test_plate_area_and_coordinate_cluster_boundaries_are_strict() -> None:
    part = Box(40, 10, 30) + Pos(-15, 10, 0) * Box(10, 30, 30)
    below = recognise_plates(part, min_area_frac=0.749999, max_thick_frac=0.8)
    tied = recognise_plates(part, min_area_frac=0.75, max_thick_frac=0.8)
    assert any(record.axis == "y" for record in below)
    assert tied == []
    assert clears_threshold(75.0, 75.0) is False
    assert clears_threshold(75.0001, 75.0) is True
    assert cluster_coordinates([1.5, 1.0, 2.0001], tol=0.5) == [[1, 0], [2]]


def _roll_sensitive_plate():
    align = (Align.CENTER, Align.CENTER, Align.MIN)
    base = Box(40, 10, 4, align=align)
    narrow_post = Pos(0, 7, 4) * Box(1, 14, 20, align=align)
    return base + narrow_post


@pytest.mark.parametrize(
    ("axis", "principal", "rotation"),
    [
        ("z", _roll_sensitive_plate(), Axis.Z),
        ("x", Rot(0, 90, 0) * _roll_sensitive_plate(), Axis.X),
        ("y", Rot(90, 0, 0) * _roll_sensitive_plate(), Axis.Y),
    ],
)
@pytest.mark.parametrize("angle", [17.0, 37.0, 73.0])
def test_plate_area_authority_is_covariant_to_in_plane_roll(
    axis, principal, rotation, angle
) -> None:
    baseline = next(record for record in recognise_plates(principal) if record.axis == axis)
    rolled = next(
        record
        for record in recognise_plates(principal.rotate(rotation, angle))
        if record.axis == axis
    )
    assert rolled.thickness == pytest.approx(baseline.thickness, abs=1e-3)


@pytest.mark.parametrize("angle", [0.0, 37.0])
def test_oriented_plate_area_boundary_is_strict_on_both_sides(angle) -> None:
    part = _roll_sensitive_plate().rotate(Axis.Z, angle)
    boundary = 395.0 / (40.0 * 19.0)
    below = recognise_plates(part, min_area_frac=boundary - 1e-6)
    tied = recognise_plates(part, min_area_frac=boundary)
    above = recognise_plates(part, min_area_frac=boundary + 1e-6)
    assert any(record.axis == "z" for record in below)
    assert all(record.axis != "z" for record in tied)
    assert all(record.axis != "z" for record in above)


def test_rolled_plate_area_authority_survives_step_and_arbitrary_framing(tmp_path) -> None:
    rolled = _roll_sensitive_plate().rotate(Axis.Z, 37)
    target = tmp_path / "rolled-plate.step"
    assert export_step(rolled, target)
    assert any(record.axis == "z" for record in recognise_plates(import_step(target)))

    presented = Pos(17, -23, 9) * Rot(31, 47, 13) * rolled
    framed = build_framed_recognition_result(presented, rotational=False)
    assert isinstance(framed, FramedRecognitionResult)
    assert len(framed.result.plates) == 2


def test_framed_plate_maximum_thickness_tie_is_rigid_motion_covariant() -> None:
    aligned = (Align.CENTER, Align.CENTER, Align.MIN)
    part = Box(60, 40, 10, align=aligned) + Pos(-15, 0, 10) * Box(30, 40, 15, align=aligned)
    moved = Pos(91, -37, 48) * Rot(31, 47, 13) * part

    baseline = build_framed_recognition_result(part, rotational=False)
    presented = build_framed_recognition_result(moved, rotational=False)
    assert isinstance(baseline, FramedRecognitionResult)
    assert isinstance(presented, FramedRecognitionResult)

    (baseline_plate,) = baseline.result.plates
    (presented_plate,) = presented.result.plates
    assert (
        (baseline_plate.axis, baseline_plate.lo, baseline_plate.hi)
        == (
            presented_plate.axis,
            presented_plate.lo,
            presented_plate.hi,
        )
        == ("x", -10.357, -0.357)
    )
    assert (baseline_plate.u, baseline_plate.v) == pytest.approx(
        (presented_plate.u, presented_plate.v), abs=1e-9
    )
    assert baseline.result.through_steps == presented.result.through_steps
    (baseline_level,) = baseline.result.step_levels
    (presented_level,) = presented.result.step_levels
    assert (
        baseline_level.z,
        *baseline_level.x_span,
        *baseline_level.y_span,
    ) == pytest.approx(
        (
            presented_level.z,
            *presented_level.x_span,
            *presented_level.y_span,
        ),
        abs=1e-9,
    )


@pytest.mark.parametrize(("delta", "z_nodes"), [(0.4999, 3), (0.5, 3), (0.5001, 2)])
def test_real_face_coordinate_clusters_include_exact_tolerance_only(delta, z_nodes) -> None:
    part = (Pos(-10, 0, 0) * Box(20, 20, 8)) + (Pos(10, 0, delta / 2) * Box(20, 20, 8 + delta))
    records, candidates, ledger = _claimed(part, max_thick_frac=1.1)
    candidate = next(candidate for candidate in candidates if candidate.record.axis == "z")
    assert next(record for record in records if record.axis == "z").hi == 4.0
    assert len(ledger.defining_of(candidate)) == z_nodes


def test_intervening_event_selects_two_adjacent_slabs_never_the_void_span() -> None:
    part = Box(40, 40, 5) + Pos(0, 0, 20) * Box(40, 40, 5) + Pos(0, 0, 10) * Box(4, 4, 20)
    records, _candidates, _ledger = _claimed(part, max_thick_frac=0.9)
    z_intervals = [(record.lo, record.hi) for record in records if record.axis == "z"]
    assert z_intervals == [(-2.5, 2.5), (17.5, 22.5)]
    assert (-2.5, 22.5) not in z_intervals


def test_same_coordinate_event_tie_orders_negative_before_positive() -> None:
    touching = Compound([Box(40, 40, 5), Pos(0, 0, 5) * Box(40, 40, 5)])
    # At their common coordinate the second solid's negative event precedes the first solid's
    # positive event.  The resulting zero-thickness adjacent pair is rejected; neither event may
    # be skipped to manufacture a Plate across the representation boundary.
    assert recognise_plates(touching, max_thick_frac=0.9) == []


@pytest.mark.parametrize(
    ("component", "accepted"),
    [(0.989999, False), (0.99, True), (0.990001, True)],
)
def test_axis_alignment_gate_below_equal_and_above(monkeypatch, component, accepted) -> None:
    original = Face.normal_at

    def adjusted(face, *args, **kwargs):
        normal = original(face, *args, **kwargs)
        values = [normal.X, normal.Y, normal.Z]
        dominant = max(range(3), key=lambda index: abs(values[index]))
        values[dominant] = math.copysign(component, values[dominant])
        return Vector(*values)

    monkeypatch.setattr(Face, "normal_at", adjusted)
    assert bool(recognise_plates(build_fixture())) is accepted


def test_record_projection_order_and_weighted_centroid_are_not_evidence_rematches() -> None:
    records, _candidates, _ledger = _claimed(build_fixture())
    assert [(record.axis, record.lo, record.hi) for record in records] == sorted(
        (record.axis, record.lo, record.hi) for record in records
    )
    z_plate = next(record for record in records if record.axis == "z")
    assert (z_plate.lo, z_plate.hi) == (-4.0, 4.0)
    assert z_plate.u == pytest.approx(0.9025270758122739)
    assert z_plate.u != round(z_plate.u, 3)


def test_compound_mixed_provenance_is_bound_per_solid() -> None:
    part = Compound([build_fixture(), copy.deepcopy(build_fixture())])
    records, candidates, ledger = _claimed(part)
    product = _take_inventory(part)
    assert len(records) == len(candidates) == len(product.result.plates) == 4
    assert all(
        ledger.graph.common_valid_solid(ledger.defining_of(candidate)) is not None
        for candidate in candidates
    )


@pytest.mark.parametrize(
    "part",
    [
        Box(40, 30, 10),
        Rot(7, 11, 0) * Box(40, 5, 30),
        Box(40, 30, 20) - Pos(0, 0, 5) * Box(20, 10, 20),
    ],
)
def test_blocks_oblique_slabs_and_cavities_do_not_leak_plate_evidence(part) -> None:
    ledger = ClaimLedger(FaceGraph(part))
    assert _discover_plates(part, writer=ledger.writer) == recognise_plates(part)
    assert ledger.candidate_set(FamilyId.PLATES).candidates == ()


def test_open_shell_keeps_public_geometry_but_refuses_attribution() -> None:
    part = Shell(build_fixture().faces())
    assert all(plate.body_key is None for plate in recognise_plates(part))
    ledger = ClaimLedger(FaceGraph(part))
    with pytest.raises(_PlateAttributionError):
        _discover_plates(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.PLATES).candidates == ()


def test_foreign_and_late_body_failure_are_atomic(monkeypatch) -> None:
    part = build_fixture()
    foreign = ClaimLedger(FaceGraph(Pos(200, 0, 0) * build_fixture()))
    with pytest.raises(_PlateAttributionError):
        _discover_plates(part, writer=foreign.writer)
    assert foreign.candidate_set(FamilyId.PLATES).candidates == ()

    ledger = ClaimLedger(FaceGraph(part))
    monkeypatch.setattr(ledger.graph, "common_valid_solid", lambda _nodes: None)
    with pytest.raises(_PlateAttributionError):
        _discover_plates(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.PLATES).candidates == ()


@pytest.mark.parametrize("mode", ["deep", "stale", "reuse"])
def test_invalid_proposal_identity_refuses_before_any_issue(monkeypatch, mode: str) -> None:
    import quiddity.plates as module

    part = build_fixture()
    ledger = ClaimLedger(FaceGraph(part))
    proposal_type = module._PlateProposal
    first = None

    def changed(record, low_faces, high_faces):
        nonlocal first
        if mode == "deep":
            low_faces = (copy.deepcopy(low_faces[0]), *low_faces[1:])
        elif mode == "stale":
            low_faces = (Pos(1, 0, 0) * copy.deepcopy(low_faces[0]), *low_faces[1:])
        elif first is None:
            first = (low_faces, high_faces)
        else:
            assert first is not None
            low_faces, high_faces = first
        return proposal_type(record, tuple(low_faces), tuple(high_faces))

    monkeypatch.setattr(module, "_PlateProposal", changed)
    with pytest.raises(_PlateAttributionError):
        _discover_plates(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.PLATES).candidates == ()


@pytest.mark.parametrize("mode", ["empty", "overlap"])
def test_empty_and_overlapping_bound_roles_refuse_atomically(monkeypatch, mode: str) -> None:
    import quiddity.plates as module

    part = build_fixture()
    ledger = ClaimLedger(FaceGraph(part))
    proposal_type = module._PlateProposal

    def changed(record, low_faces, high_faces):
        if mode == "empty":
            low_faces = ()
        else:
            high_faces = (low_faces[0], *high_faces)
        return proposal_type(record, tuple(low_faces), tuple(high_faces))

    monkeypatch.setattr(module, "_PlateProposal", changed)
    with pytest.raises(_PlateAttributionError, match="empty or overlap"):
        _discover_plates(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.PLATES).candidates == ()


def test_late_second_body_failure_leaves_no_candidate_prefix(monkeypatch) -> None:
    part = Compound([build_fixture(), Pos(200, 200, 200) * build_fixture()])
    ledger = ClaimLedger(FaceGraph(part))
    original = ledger.graph.common_valid_solid
    bodies = []

    def fail_second(nodes):
        solid = original(nodes)
        if solid is not None and solid not in bodies:
            bodies.append(solid)
        return None if len(bodies) > 1 else solid

    monkeypatch.setattr(ledger.graph, "common_valid_solid", fail_second)
    with pytest.raises(_PlateAttributionError):
        _discover_plates(part, min_area_frac=0.01, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.PLATES).candidates == ()


def test_equal_body_occurrences_complete_with_distinct_capabilities() -> None:
    part = Compound([build_fixture(), copy.deepcopy(build_fixture())])
    context = start(part)
    ledger = ClaimLedger(context.graph, definitions=PHYSICAL_DEFINITIONS)
    _discover_all(context, ledger)
    candidates = ledger.candidate_set(FamilyId.PLATES).candidates
    assert len(candidates) == 4
    assert FamilyId.PLATES in ledger._issuer._completed
    assert len(ledger._issuer._completed_occurrences[FamilyId.PLATES]) == 4


def test_terminal_plate_identity_and_evidence() -> None:
    product = _take_inventory(build_fixture())
    candidates = product.physical.candidate_set(FamilyId.PLATES).candidates
    assert candidates
    assert tuple(candidate.record for candidate in candidates) == product.result.plates
    assert all(product.evidence.defining_of(candidate) for candidate in candidates)


def test_empty_completed_turned_roster_does_not_veto_plate_solids() -> None:
    product = _take_inventory(build_fixture())
    assert product.physical.candidate_set(FamilyId.TURNED_STEPS).candidates == ()
    assert product.physical.candidate_set(FamilyId.PLATES).candidates
    assert product.result.plates


def test_plate_private_core_and_registry_route_are_closed() -> None:
    registry = (ROOT / "src/quiddity/_registry.py").read_text(encoding="utf-8")
    tree = ast.parse(registry)
    plates = next(
        node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == "_plates"
    )
    calls = [node for node in ast.walk(plates) if isinstance(node, ast.Call)]
    discover = next(
        call
        for call in calls
        if isinstance(call.func, ast.Name) and call.func.id == "_discover_plates"
    )
    writer = {keyword.arg: keyword.value for keyword in discover.keywords}["writer"]
    assert isinstance(writer, ast.Attribute) and writer.attr == "writer"
    keywords = {keyword.arg: keyword.value for keyword in discover.keywords}
    assert "excluded_solids" in keywords


def test_plate_import_constructor_and_capability_rosters_are_closed() -> None:
    package = ROOT / "src/quiddity"
    core_sites: list[tuple[str, ast.Call]] = []
    constructors: list[tuple[str, ast.Call]] = []
    proposal_sites: list[tuple[str, ast.Call]] = []
    prohibited = {
        "CandidateSet",
        "EvidenceIndex",
        "AcceptedInputs",
        "ReconciliationResult",
        "CompletedOccurrence",
    }
    plate_tree = ast.parse((package / "plates.py").read_text(encoding="utf-8"))
    imported = {
        alias.name
        for node in ast.walk(plate_tree)
        if isinstance(node, ast.ImportFrom)
        for alias in node.names
    }
    assert imported.isdisjoint(prohibited)

    for path in package.glob("*.py"):
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
        for qualified, call in _qualified_calls(tree):
            leaf = qualified.rsplit(".", 1)[-1]
            if leaf == "_discover_plates":
                core_sites.append((path.name, call))
            if leaf == "Plate":
                constructors.append((path.name, call))
            if leaf == "_PlateProposal":
                proposal_sites.append((path.name, call))

    assert {path for path, _call in core_sites} == {"plates.py", "_registry.py"}
    assert sum(path == "_registry.py" for path, _call in core_sites) == 1
    registry_call = next(call for path, call in core_sites if path == "_registry.py")
    writer = next(keyword.value for keyword in registry_call.keywords if keyword.arg == "writer")
    assert isinstance(writer, ast.Attribute) and writer.attr == "writer"
    assert isinstance(writer.value, ast.Name) and writer.value.id == "services"
    public = next(
        node
        for node in plate_tree.body
        if isinstance(node, ast.FunctionDef) and node.name == "recognise_plates"
    )
    public_call = next(
        call
        for call in ast.walk(public)
        if isinstance(call, ast.Call)
        and isinstance(call.func, ast.Name)
        and call.func.id == "_discover_plates"
    )
    assert all(keyword.arg != "writer" for keyword in public_call.keywords)
    assert [(path, len(call.args)) for path, call in constructors] == [("plates.py", 0)]
    assert [(path, len(call.args)) for path, call in proposal_sites] == [("plates.py", 3)]

    discover = next(
        node
        for node in plate_tree.body
        if isinstance(node, ast.FunctionDef) and node.name == "_discover_plates"
    )
    proposal_builder = next(
        node
        for node in plate_tree.body
        if isinstance(node, ast.FunctionDef) and node.name == "_plate_proposals"
    )
    face_scans = [
        call
        for call in ast.walk(proposal_builder)
        if isinstance(call, ast.Call)
        and isinstance(call.func, ast.Attribute)
        and call.func.attr == "faces"
    ]
    assert len(face_scans) == 1
    assert (
        sum(
            isinstance(call, ast.Call)
            and isinstance(call.func, ast.Name)
            and call.func.id == "_plate_proposals"
            for call in ast.walk(discover)
        )
        == 1
    )


def test_registry_uses_restricted_turned_occurrences_for_body_local_plate_veto() -> None:
    tree = ast.parse((ROOT / "src/quiddity/_registry.py").read_text(encoding="utf-8"))
    function = next(
        node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == "_plates"
    )
    assert not any(isinstance(node, ast.Try) for node in ast.walk(function))
    calls = _qualified_calls(function)
    assert not any(name.endswith(".records") for name, _call in calls)
    assert sum(name.endswith(".occurrences") for name, _call in calls) == 1
    assert not any(name.endswith(".from_steps") for name, _call in calls)
    discover = next(call for name, call in calls if name.endswith("_discover_plates"))
    keywords = {keyword.arg: keyword.value for keyword in discover.keywords}
    writer = keywords["writer"]
    assert isinstance(writer, ast.Attribute) and writer.attr == "writer"
    assert isinstance(writer.value, ast.Name) and writer.value.id == "services"
    assert "excluded_solids" in keywords
