# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

"""A recess of any planar cross-section, found by walking its ring instead of pairing its walls.

The point of this family is a blind spot in the other one, and it is a blind spot in the *search*
rather than in a gate. `recognise_pockets` sorts walls into buckets by the axis their normal
aligns with and pairs walls within a bucket. A triangular recess has no two walls sharing an
axis, so no pair forms and no gate ever runs — measured over 600 MFCAD++ models, 94% of
triangular-pocket faces never reach a test at all.

So these tests are mostly about *reach*: geometry the pairing family cannot see, and geometry
this one cannot see either, because neither family is going away. The overlap where both see the
same recess is a reconciliation question and is tested as one.
"""

from __future__ import annotations

from pathlib import Path

import pytest
from attribution_audit import assert_ring_role, attributed_run
from build123d import (
    Axis,
    Box,
    BuildPart,
    BuildSketch,
    Compound,
    Cylinder,
    Plane,
    Polygon,
    Pos,
    chamfer,
    export_step,
    extrude,
    fillet,
    import_step,
    mirror,
)

from quiddity._adjacency import FaceGraph
from quiddity._candidates import FamilyId
from quiddity._claims import ClaimLedger
from quiddity._reconcile import prismatic_pockets_that_are_not_pockets
from quiddity._rings import rings
from quiddity.frames import (
    FramedRecognitionResult,
    build_framed_recognition_result,
)
from quiddity.prismatic_pockets import (
    SPAN_EPS,
    _axis_for_opening,
    _floor_seeded_regions,
    _material_fraction,
    _section_prism,
    _void_open_and_floored,
)
from tools._legacy_recognition import namespace

r = namespace()


def _prism(*corners, height=14):
    with BuildPart() as built:
        with BuildSketch(Plane.XY):
            Polygon(*corners)
        extrude(amount=height)
    return built.part


def _triangular():
    """A blind triangular recess: three walls, no two sharing a normal axis."""

    return Box(120, 80, 20) - Pos(0, 0, 2) * _prism((-12, -9), (12, -9), (0, 12))


def _rectangular():
    """The shape both families reach, so the overlap has something to be about."""

    return Box(120, 80, 20) - Pos(0, 0, 8) * Box(20, 12, 14)


def _hexagonal():
    return Box(120, 80, 20) - Pos(0, 0, 2) * _prism(
        (-12, -7), (-6, -12), (6, -12), (12, -7), (6, -2), (-6, -2)
    )


def _with_one_treated_mouth_edge(part, treatment):
    opening_edges = [
        edge
        for edge in part.edges()
        if abs(edge.center().Z - 10.0) < 1e-6
        and all(abs(vertex.Z - 10.0) < 1e-6 for vertex in edge.vertices())
        and abs(edge.center().X) < 20.0
        and abs(edge.center().Y) < 20.0
    ]
    assert opening_edges
    return treatment([opening_edges[0]], 1.0)


def _with_deep_side_opening(part, *, width: float = 12.0):
    """Interrupt a wall pair below the mouth while leaving the original floor intact."""

    return part - Pos(0, 20, 8) * Box(width, 50, 4)


def _through():
    """The same triangular void cut clean through: a passage, not a pocket."""

    return Box(120, 80, 20) - Pos(0, 0, -20) * _prism((-12, -9), (12, -9), (0, 12), height=60)


def test_partial_mouth_probe_helpers_fail_closed_at_their_kernel_boundaries() -> None:
    section = ((-2.0, -2.0), (2.0, -2.0), (0.0, 2.0))

    with pytest.raises(ValueError, match="too short"):
        _section_prism(section, 2, 0.0, 2 * SPAN_EPS)

    class RaisedKernelError:
        def intersect(self, _probe):
            raise RuntimeError("forced kernel refusal")

    assert not _void_open_and_floored(RaisedKernelError(), section, 2, 10.0, 2.0)


def test_partial_mouth_probe_accepts_a_single_shape_intersection_value() -> None:
    class Intersection:
        volume = 3.0

    class Part:
        def intersect(self, _probe):
            return Intersection()

    class Probe:
        volume = 12.0

    assert _material_fraction(Part(), Probe()) == pytest.approx(0.25)


def test_opening_without_a_surface_normal_has_no_principal_axis() -> None:
    class Graph:
        def normal(self, _node):
            return None

    assert _axis_for_opening(Graph(), object()) is None


def _claimed(part):
    return attributed_run(
        part,
        FamilyId.PRISMATIC_POCKETS,
        r.recognise_prismatic_pockets,
    )


def test_a_triangular_recess_is_recognised_where_wall_pairing_cannot_see_it():
    """The reason this family exists, stated as the contrast rather than as a lone assertion.

    Both halves matter. `recognise_pockets` returning nothing is not this family succeeding
    where the other was merely stricter — the other never formed a candidate, because pairing
    walls that share a normal axis has nothing to pair when no two walls do.
    """

    part = _triangular()
    (pocket,) = r.recognise_prismatic_pockets(part)

    assert pocket.sides == 3
    assert pocket.depth == 8.0
    assert len(pocket.section) == 3
    assert r.recognise_pockets(part) == [], "the pairing family must be blind to this"


def test_both_cap_orientations_issue_wall_defining_and_floor_constituent_evidence() -> None:
    low_ledger, (low,) = _claimed(_triangular())
    high_ledger, (high,) = _claimed(mirror(_triangular(), about=Plane.XY))

    assert (low.open_sign, high.open_sign) == (1, -1)
    for ledger, pocket in ((low_ledger, low), (high_ledger, high)):
        (candidate,) = ledger.candidate_set(FamilyId.PRISMATIC_POCKETS).candidates
        defining = ledger.defining_of(candidate)
        constituent = ledger.snapshot_index().constituent_of(candidate)
        assert len(defining) == pocket.sides
        assert defining < constituent
        assert len(constituent - defining) == 1
        (floor,) = constituent - defining
        assert abs(ledger.graph.normal(floor)[2]) == pytest.approx(1.0)


@pytest.mark.parametrize(
    ("fixture", "sides"), ((_triangular, 3), (_rectangular, 4), (_hexagonal, 6))
)
def test_each_prismatic_section_retains_its_proved_floor_as_constituent(
    fixture, sides: int
) -> None:
    ledger, (pocket,) = _claimed(fixture())
    (candidate,) = ledger.candidate_set(FamilyId.PRISMATIC_POCKETS).candidates
    evidence = ledger.snapshot_index()

    assert pocket.sides == sides
    assert len(evidence.defining_of(candidate)) == sides
    assert len(evidence.constituent_of(candidate)) == sides + 1


@pytest.mark.parametrize(
    ("fixture", "sides", "depth"),
    ((_triangular, 3, 8.0), (_rectangular, 4, 9.0), (_hexagonal, 6, 8.0)),
)
@pytest.mark.parametrize("treatment", (chamfer, fillet))
def test_partial_mouth_treatment_does_not_hide_a_uniquely_bounded_pocket(
    fixture, sides: int, depth: float, treatment
) -> None:
    part = _with_one_treated_mouth_edge(fixture(), treatment)
    ledger = ClaimLedger(FaceGraph(part))

    direct = r.recognise_prismatic_pockets(part)
    attributed = r.recognise_prismatic_pockets(part, ledger=ledger)
    (pocket,) = attributed
    (candidate,) = ledger.candidate_set(FamilyId.PRISMATIC_POCKETS).candidates
    evidence = ledger.snapshot_index()

    assert direct == attributed
    assert candidate.record is pocket
    assert pocket.sides == sides
    assert pocket.depth == depth
    assert len(evidence.defining_of(candidate)) == sides
    assert len(evidence.constituent_of(candidate)) == sides + 2
    assert evidence.defining_of(candidate) < evidence.constituent_of(candidate)


@pytest.mark.parametrize("treatment", (chamfer, fillet))
def test_partial_treatment_does_not_turn_a_through_void_into_a_pocket(treatment) -> None:
    part = _with_one_treated_mouth_edge(_through(), treatment)

    assert r.recognise_prismatic_pockets(part) == []


def test_partial_mouth_recovery_refuses_a_breached_floor() -> None:
    pocket = _with_one_treated_mouth_edge(_triangular(), chamfer)
    breached = pocket - Pos(0, 0, -5) * Cylinder(1, 30)

    assert r.recognise_prismatic_pockets(breached) == []


def test_partial_mouth_recovery_is_covariant_and_body_local() -> None:
    treated = _with_one_treated_mouth_edge(_hexagonal(), fillet)
    first = Pos(-90, 0, 0) * treated.rotate(Axis.X, -90)
    second = Pos(90, 0, 0) * treated.rotate(Axis.X, -90)

    records = r.recognise_prismatic_pockets(Compound([second, first]))

    assert len(records) == 2
    assert all(
        (record.axis, record.sides, record.depth, record.open_sign) == ("y", 6, 8.0, 1)
        for record in records
    )
    assert [record.at[0] for record in records] == [-90.0, 90.0]


def test_partial_mouth_recovery_survives_step_round_trip(tmp_path: Path) -> None:
    treated = _with_one_treated_mouth_edge(_triangular(), chamfer).rotate(Axis.Y, 90)
    path = tmp_path / "partial-mouth-triangular-pocket.step"

    assert export_step(treated, path)
    imported = import_step(path)

    (record,) = r.recognise_prismatic_pockets(imported)
    assert (record.axis, record.sides, record.depth, record.open_sign) == ("x", 3, 8.0, 1)


def test_an_intact_floor_recovers_a_six_sided_pocket_through_a_deep_side_opening() -> None:
    part = _with_deep_side_opening(_hexagonal())
    ledger = ClaimLedger(FaceGraph(part))

    direct = r.recognise_prismatic_pockets(part)
    attributed = r.recognise_prismatic_pockets(part, ledger=ledger)
    (pocket,) = attributed
    (candidate,) = ledger.candidate_set(FamilyId.PRISMATIC_POCKETS).candidates
    evidence = ledger.snapshot_index()

    assert direct == attributed
    assert candidate.record is pocket
    assert (pocket.axis, pocket.sides, pocket.depth, pocket.open_sign) == ("z", 6, 8.0, 1)
    assert len(evidence.defining_of(candidate)) == 6
    assert len(evidence.constituent_of(candidate)) == 7


def test_floor_seeded_recovery_is_covariant_and_body_local() -> None:
    interrupted = _with_deep_side_opening(_hexagonal())
    first = Pos(-90, 0, 0) * interrupted.rotate(Axis.X, -90)
    second = Pos(90, 0, 0) * interrupted.rotate(Axis.X, -90)

    records = r.recognise_prismatic_pockets(Compound([second, first]))

    assert len(records) == 2
    assert all(
        (record.axis, record.sides, record.depth, record.open_sign) == ("y", 6, 8.0, 1)
        for record in records
    )
    assert [record.at[0] for record in records] == [-90.0, 90.0]


def test_floor_seeded_recovery_refuses_a_breached_floor() -> None:
    interrupted = _with_deep_side_opening(_hexagonal())
    breached = interrupted - Pos(0, -7, -5) * Cylinder(1, 30)

    assert r.recognise_prismatic_pockets(breached) == []


def test_floor_seeded_recovery_leaves_four_sided_cavities_to_specific_families() -> None:
    interrupted = _with_deep_side_opening(_rectangular())
    graph = FaceGraph(interrupted)

    assert _floor_seeded_regions(interrupted, graph) == ()
    assert r.recognise_pockets(interrupted)


@pytest.mark.parametrize("scale", (0.1, 10.0))
def test_floor_seeded_recovery_is_scale_independent(scale: float) -> None:
    interrupted = _with_deep_side_opening(_hexagonal()).scale(scale)

    (record,) = r.recognise_prismatic_pockets(interrupted)

    assert (record.sides, record.depth) == (6, 8.0 * scale)


def test_floor_seeded_recovery_survives_step_round_trip(tmp_path: Path) -> None:
    interrupted = _with_deep_side_opening(_hexagonal()).rotate(Axis.Y, 90)
    path = tmp_path / "floor-seeded-six-sided-pocket.step"

    assert export_step(interrupted, path)
    imported = import_step(path)

    (record,) = r.recognise_prismatic_pockets(imported)
    assert (record.axis, record.sides, record.depth, record.open_sign) == ("x", 6, 8.0, 1)


def test_every_selected_blended_cap_patch_is_constituent_but_not_defining() -> None:
    sharp = _hexagonal()
    floor_edges = [edge for edge in sharp.edges() if abs(edge.center().Z - 2.0) < 1e-6]
    assert len(floor_edges) == 6
    part = fillet(floor_edges, 2.0)
    ledger = ClaimLedger(FaceGraph(part))
    selected = tuple(ring for ring in rings(part, ledger.graph) if any(ring.caps))

    (pocket,) = r.recognise_prismatic_pockets(part, ledger=ledger)
    (ring,) = selected
    (candidate,) = ledger.candidate_set(FamilyId.PRISMATIC_POCKETS).candidates
    evidence = ledger.snapshot_index()
    cap_nodes = ring.cap_nodes[0] | ring.cap_nodes[1]

    assert pocket.sides == len(evidence.defining_of(candidate)) == 6
    assert len(cap_nodes) == 6
    assert evidence.constituent_of(candidate) - evidence.defining_of(candidate) == cap_nodes


def test_both_capped_internal_cavity_issues_no_record_or_evidence() -> None:
    enclosed = Box(120, 80, 20) - Pos(0, 0, -3) * _prism((-12, -9), (12, -9), (0, 12), height=6)
    ledger = ClaimLedger(FaceGraph(enclosed))

    assert r.recognise_prismatic_pockets(enclosed, ledger=ledger) == []
    assert ledger.candidate_set(FamilyId.PRISMATIC_POCKETS).candidates == ()


def test_multiple_pockets_keep_sorted_occurrence_identity() -> None:
    cutter = _prism((-8, -6), (8, -6), (0, 8))
    part = Box(120, 80, 20) - Pos(-25, 0, 2) * cutter - Pos(25, 0, 2) * cutter
    ledger, pockets = _claimed(part)

    assert len(pockets) == 2
    candidates = ledger.candidate_set(FamilyId.PRISMATIC_POCKETS).candidates
    assert all(
        candidate.record is pocket for candidate, pocket in zip(candidates, pockets, strict=True)
    )
    assert len({frozenset(ledger.defining_of(candidate)) for candidate in candidates}) == 2


def test_the_section_is_what_separates_a_triangle_from_a_hexagon():
    """`sides` alone would not, and neither would depth.

    A record that could not tell those apart would collapse two distinct machined shapes into
    one — the same reason `Passage` carries a section, and the reason this is not folded into
    `Pocket`, whose width-and-length cannot describe either.
    """

    triangle = _triangular()
    hexagon = _hexagonal()

    tri_ledger, (tri,) = _claimed(triangle)
    hex_ledger, (hexa,) = _claimed(hexagon)

    assert (tri.sides, hexa.sides) == (3, 6)
    assert tri.section != hexa.section
    assert len(tri.section) == 3 and len(hexa.section) == 6
    for ledger, pocket in ((tri_ledger, tri), (hex_ledger, hexa)):
        (candidate,) = ledger.candidate_set(FamilyId.PRISMATIC_POCKETS).candidates
        defining = ledger.defining_of(candidate)
        assert len(defining) == pocket.sides
        assert all(abs(ledger.graph.normal(node)[2]) < 1e-6 for node in defining)


@pytest.mark.parametrize(
    ("rotation_axis", "degrees", "expected_axis", "expected_open_sign"),
    [
        (Axis.X, 0, "z", 1),
        (Axis.Y, 180, "z", -1),
        (Axis.X, -90, "y", 1),
        (Axis.X, 90, "y", -1),
        (Axis.Y, 90, "x", 1),
        (Axis.Y, -90, "x", -1),
    ],
)
@pytest.mark.parametrize(("fixture", "sides"), [(_triangular, 3), (_hexagonal, 6)])
def test_polygonal_pockets_are_covariant_across_signed_principal_axes(
    fixture,
    sides: int,
    rotation_axis: Axis,
    degrees: float,
    expected_axis: str,
    expected_open_sign: int,
) -> None:
    part = Pos(17, -11, 9) * fixture().rotate(rotation_axis, degrees)

    (direct,) = r.recognise_prismatic_pockets(part)
    result = r.build_recognition_result(part)

    assert (direct.axis, direct.sides, direct.depth, direct.open_sign) == (
        expected_axis,
        sides,
        8.0,
        expected_open_sign,
    )
    assert result.prismatic_pockets == (direct,)


@pytest.mark.parametrize(
    ("rotation_axis", "degrees", "expected_axis", "expected_open_sign"),
    [
        (Axis.X, 0, "z", 1),
        (Axis.Y, 180, "z", -1),
        (Axis.X, -90, "y", 1),
        (Axis.X, 90, "y", -1),
        (Axis.Y, 90, "x", 1),
        (Axis.Y, -90, "x", -1),
    ],
)
def test_rectangular_ring_covariance_preserves_aggregate_pocket_precedence(
    rotation_axis: Axis,
    degrees: float,
    expected_axis: str,
    expected_open_sign: int,
) -> None:
    part = Pos(17, -11, 9) * _rectangular().rotate(rotation_axis, degrees)

    (ring,) = r.recognise_prismatic_pockets(part)
    result = r.build_recognition_result(part)

    assert (ring.axis, ring.sides, ring.depth, ring.open_sign) == (
        expected_axis,
        4,
        9.0,
        expected_open_sign,
    )
    assert result.prismatic_pockets == ()
    assert len(result.pockets) == 1


@pytest.mark.parametrize(("fixture", "sides"), [(_triangular, 3), (_hexagonal, 6)])
def test_principal_y_polygonal_pocket_survives_step_round_trip(
    fixture,
    sides: int,
    tmp_path: Path,
) -> None:
    part = Pos(17, -11, 9) * fixture().rotate(Axis.X, -90)
    path = tmp_path / f"principal-y-{sides}-sided-pocket.step"

    assert export_step(part, path)
    imported = import_step(path)

    (pocket,) = r.recognise_prismatic_pockets(imported)
    assert (pocket.axis, pocket.sides, pocket.depth, pocket.open_sign) == (
        "y",
        sides,
        8.0,
        1,
    )


def test_principal_y_rectangular_ring_round_trip_keeps_pocket_precedence(
    tmp_path: Path,
) -> None:
    part = Pos(17, -11, 9) * _rectangular().rotate(Axis.X, -90)
    path = tmp_path / "principal-y-rectangular-pocket.step"

    assert export_step(part, path)
    imported = import_step(path)
    result = r.build_recognition_result(imported)

    (ring,) = r.recognise_prismatic_pockets(imported)
    assert (ring.axis, ring.sides, ring.depth, ring.open_sign) == ("y", 4, 9.0, 1)
    assert result.prismatic_pockets == ()
    assert len(result.pockets) == 1


@pytest.mark.parametrize(
    ("fixture", "sides", "rectangular"),
    [(_triangular, 3, False), (_hexagonal, 6, False), (_rectangular, 4, True)],
)
def test_principal_ring_contract_survives_arbitrary_rigid_presentation_in_framed_aggregate(
    fixture,
    sides: int,
    rectangular: bool,
) -> None:
    baseline = build_framed_recognition_result(fixture())
    presented = Pos(-31, 17, 23) * fixture().rotate(Axis((0, 0, 0), (1, 1, 0)), 37)

    framed = build_framed_recognition_result(presented)

    assert isinstance(baseline, FramedRecognitionResult)
    assert isinstance(framed, FramedRecognitionResult)
    (original,) = baseline.result.section_recesses
    (pocket,) = framed.result.section_recesses
    assert pocket.classification == original.classification
    assert pocket.classification.feature_kind == "pocket"
    assert len(pocket.geometry.profile.boundary) == sides
    assert pocket.geometry.run_interval[1] - pocket.geometry.run_interval[0] == pytest.approx(
        9 if rectangular else 8
    )
    assert pocket.geometry == original.geometry


def test_a_void_open_at_both_ends_is_a_passage_and_not_reported_here():
    """The cap count is the whole discriminator, so it is tested at both ends of its range."""

    part = _through()
    from attribution_audit import unattributed_run

    unattributed_run(part, FamilyId.PRISMATIC_POCKETS, r.recognise_prismatic_pockets)
    assert r.recognise_passages(part), "the same void must still be a passage"


def test_the_pocket_claims_its_walls_and_not_the_floor_that_makes_it_blind():
    """The floor is consulted, not consumed — the line every recess family here draws.

    `depth` is the walls' own span along the run axis, not a measurement taken off the floor, so
    the floor is what makes the recess blind rather than what it is measured by. Claiming it
    would have every pocket contest whatever else owns that face.
    """

    ledger, found = _claimed(_triangular())
    (pocket,) = found
    (claim,) = ledger.claims

    assert len(claim.defining) == pocket.sides == 3
    for node in claim.defining:
        normal = ledger.graph.normal(node)
        assert normal is not None
        assert abs(normal[2]) < 0.01, "a claimed face is a wall, not the floor"


def test_a_rectangular_recess_is_reported_by_both_families_and_reconciled_to_one():
    """The overlap, and the direction it resolves.

    Both records are true of the geometry. The `Pocket` wins because `width` and `length` on
    named axes are the numbers a drawing calls out, where a four-corner section says the same
    thing less directly — and for every shape `Pocket` cannot express, this rule does not fire
    and the prismatic record is the only one there is.
    """

    part = _rectangular()
    rect_ledger, rect_records = _claimed(part)
    assert len(rect_records) == 1
    (rect_candidate,) = rect_ledger.candidate_set(FamilyId.PRISMATIC_POCKETS).candidates
    assert_ring_role(rect_ledger, rect_candidate, rect_records[0])

    ledger = ClaimLedger(FaceGraph(part))
    pockets = r.recognise_pockets(part, ledger=ledger)
    prismatic = r.recognise_prismatic_pockets(part, ledger=ledger)

    assert len(pockets) == 1 and len(prismatic) == 1, "both families see this recess"
    (candidate,) = ledger.candidate_set(FamilyId.PRISMATIC_POCKETS).candidates
    assert candidate.record is prismatic[0]
    assert len(ledger.defining_of(candidate)) == prismatic[0].sides == 4
    assert_ring_role(ledger, candidate, prismatic[0])
    assert prismatic_pockets_that_are_not_pockets(prismatic, pockets, ledger.snapshot_index()) == []

    # And the rule is not simply "drop everything": a shape `Pocket` cannot express survives it.
    # One part, built once -- a second `_triangular()` is a different solid, and the ledger
    # would refuse its faces rather than quietly answering about the wrong one.
    triangle = _triangular()
    tri_ledger = ClaimLedger(FaceGraph(triangle))
    tri_pockets = r.recognise_pockets(triangle, ledger=tri_ledger)
    tri = r.recognise_prismatic_pockets(triangle, ledger=tri_ledger)
    assert (
        len(prismatic_pockets_that_are_not_pockets(tri, tri_pockets, tri_ledger.snapshot_index()))
        == 1
    )


def test_an_obround_recess_is_the_other_family_s_to_find():
    """Neither family subsumes the other, and this is the half that runs the other way.

    An obround pocket's ends are cylindrical, so its walls form no closed *planar* ring and this
    family sees nothing. Measured over 250 MFCAD++ models: zero rings on the whole *Circular end
    pocket* class. That is why the pairing family stays rather than being replaced.
    """

    end = Cylinder(6, 14)
    stub = Box(8, 12, 14) + Pos(-4, 0, 0) * end + Pos(4, 0, 0) * end
    part = Box(120, 80, 20) - Pos(0, 0, 8) * stub

    assert r.recognise_prismatic_pockets(part) == []
    assert r.recognise_pockets(part), "the pairing family must still find it"


def test_a_ledger_built_from_another_part_is_refused_rather_than_left_empty():
    """A graph from a different part resolves nothing, and empty reads as "claims nothing"."""

    part, twin = _triangular(), _triangular()
    assert r.recognise_prismatic_pockets(twin) == r.recognise_prismatic_pockets(part)

    foreign = ClaimLedger(FaceGraph(twin))
    try:
        r.recognise_prismatic_pockets(part, ledger=foreign)
    except ValueError as refusal:
        assert "built from a different part" in str(refusal)
    else:
        raise AssertionError("recognise_prismatic_pockets accepted another part's graph")
