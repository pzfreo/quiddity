# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

"""A passage, and the two things it is not.

Every test here is built around one block with one void through it, varied in exactly one way
at a time: capped at an end (a pocket), filled instead of hollow (a boss), or left open (a
passage). The pairing is the point -- a fixture that differed in several ways at once could
pass for reasons unrelated to the gate it names.
"""

from __future__ import annotations

from build123d import (
    Box,
    BuildPart,
    BuildSketch,
    Compound,
    Locations,
    Plane,
    Polygon,
    Pos,
    RegularPolygon,
    chamfer,
    extrude,
    fillet,
)
from OCP.BRepClass3d import BRepClass3d_SolidClassifier
from OCP.gp import gp_Pnt
from OCP.TopAbs import TopAbs_IN

from quiddity import (
    recognise_slots,
)
from quiddity._adjacency import FaceEdges, FaceGraph
from quiddity._candidates import FamilyId
from quiddity._claims import ClaimLedger
from quiddity._reconcile import reconcile_recess_candidates
from quiddity._rings import _canonical, _centroid, _interior_point
from tools._legacy_recognition import (
    build_recognition_result,
    recognise_passages,
    recognise_section_passages,
)


def _block() -> Box:
    return Box(60, 40, 20)


def _hexagonal_passage():
    """A six-walled void running the full depth in Z, open at both ends."""

    with BuildPart() as bore:
        with BuildSketch(Plane.XY):
            RegularPolygon(9, 6)
        extrude(amount=40, both=True)
    return _block() - bore.part


def test_a_void_open_at_both_ends_is_a_passage():
    passages = recognise_passages(_hexagonal_passage())

    assert len(passages) == 1
    passage = passages[0]
    assert passage.axis == "z"
    assert passage.sides == 6
    assert passage.length == 20.0


def test_a_four_wall_passage_survives_when_no_slot_candidate_claims_it() -> None:
    """Empty frozen evidence cannot manufacture the Slot precedence relation."""

    part = _block() - Box(10, 10, 60)
    ledger = ClaimLedger(FaceGraph(part))
    passages_found = recognise_section_passages(part, ledger=ledger)
    empty_slots = ledger.candidate_set_for(FamilyId.SLOTS, ())
    empty_pockets = ledger.candidate_set_for(FamilyId.POCKETS, ())
    empty_rings = ledger.candidate_set_for(FamilyId.PRISMATIC_POCKETS, ())
    passages = ledger.candidate_set_for(FamilyId.PASSAGES, passages_found)

    assert (
        reconcile_recess_candidates(
            empty_slots,
            empty_pockets,
            empty_rings,
            passages,
            ledger.snapshot_index(),
        )
        == ()
    )


def test_a_through_slot_is_reported_here_too_and_the_aggregate_resolves_it():
    """The reconciliation is the aggregate's, and it compares faces rather than coordinates.

    A through slot *is* a closed uncapped ring, so this family sees it. An earlier draft
    suppressed it inside the recogniser by asking `recognise_slots` what it had found, which
    ADR 0002 forbids -- recognisers do not call siblings -- and ADR 0003 forbids by name. So
    the recogniser now reports the ring, and `build_recognition_result` drops it because the
    slot's two walls sit inside the passage's ring.
    """

    slotted = Box(130, 150, 16) - Box(30, 8, 60)
    assert recognise_slots(slotted), "the fixture must be a recognisable slot"
    assert [p.sides for p in recognise_passages(slotted)] == [4], "the candidate is reported"

    result = build_recognition_result(slotted)
    assert result.slots, "the slot is what survives, because it dimensions the void"
    assert result.passages == (), "and the passage it also is, does not"

    # A four-walled void no slot claims is still a passage: the side count was never the point.
    square = Box(60, 40, 20) - Box(10, 10, 60)
    assert recognise_slots(square) == []
    assert [p.sides for p in build_recognition_result(square).passages] == [4]

    ledger, passages = _attributed_sections(square)
    (candidate,) = ledger.candidate_set(FamilyId.PASSAGES).candidates
    assert candidate.record is passages[0]
    assert len(ledger.defining_of(candidate)) == len(passages[0].section.boundary) == 4


def test_a_passage_crossing_a_slot_only_in_projection_survives():
    """The regression for the heuristic that was replaced.

    The old rule compared a ring's averaged centre with a slot record's centre in X and Y
    within 1e-6, ignoring the run axis and Z entirely. Two solids, one carrying a Z-through
    slot and one an X-through passage at the same XY, are therefore the same feature to it and
    share not one face. Reconciling on claimed faces cannot make this mistake.
    """

    slotted = Box(120, 60, 20) - Box(10, 30, 60)
    bored = Pos(0, 0, 100) * (Box(120, 60, 20) - Box(200, 8, 8))
    part = Compound(children=[slotted, bored])

    result = build_recognition_result(part)
    assert result.slots, "the Z slot"
    assert [p.axis for p in result.passages] == ["x"], "the X passage, at the same XY, survives"
    ledger, passages = _attributed_sections(part)
    assert (1.0, 0.0, 0.0) in {passage.frame.run for passage in passages}
    assert len(ledger.candidate_set(FamilyId.PASSAGES).candidates) == len(passages)


def test_the_same_void_with_a_floor_is_a_pocket_and_not_a_passage():
    """The control, differing in one way: the void stops inside the block.

    A pocket's ring is capped by a face perpendicular to the run axis that fills the ring's
    cross-section. That the cap must *fill* it is the whole subtlety -- at a passage mouth the
    block's own end face is perpendicular and edge-adjacent too, and a test that only looked
    for a perpendicular neighbour rejected every passage there is.
    """

    with BuildPart() as bore:
        with BuildSketch(Plane.XY):
            RegularPolygon(9, 6)
        extrude(amount=14)
    blind = _block() - Pos(0, 0, -4) * bore.part

    assert recognise_passages(blind) == []


def test_a_column_of_material_is_not_a_passage():
    """The same ring of walls, with the material inside it rather than outside.

    A hexagonal column joining two plates is bounded by an identical closed uncapped ring, and
    only the solid-classifier probe separates it from a void. The fixture matters: a boss
    standing *on* a plate is rejected by the cap test instead, so a test written around one
    passes with the probe deleted and proves nothing about it. This one does not -- without the
    probe it reports a passage.
    """

    with BuildPart() as column:
        with BuildSketch(Plane.XY):
            RegularPolygon(10, 6)
        extrude(amount=30)
    joined = _block() + Pos(0, 0, 26) * _block() + Pos(0, 0, -4) * column.part

    assert recognise_passages(joined) == []


def test_the_side_count_is_the_polygon_and_not_a_class():
    """MFCAD++ names triangular, rectangular and six-sided passages separately; the geometry
    does not, so one recogniser reports the count and the caller reads it."""

    with BuildPart() as tri:
        with BuildSketch(Plane.XZ):
            Polygon((-8, -6), (8, -6), (0, 8))
        extrude(amount=60, both=True)
    triangular = _block() - tri.part

    _ledger, found = _attributed_sections(triangular)
    assert len(found) == 1
    assert len(found[0].section.boundary) == 3
    assert found[0].frame.run == (0.0, 1.0, 0.0)


def test_two_passages_on_one_part_are_reported_separately_and_in_order():
    with BuildPart() as bores:
        with BuildSketch(Plane.XY):
            RegularPolygon(7, 6)
            with Locations((22, 0)):
                RegularPolygon(6, 6)
        extrude(amount=40, both=True)
    part = _block() - bores.part
    ledger, found = _attributed_sections(part)

    assert len(found) == 2
    assert found == sorted(found)
    assert len(recognise_passages(part)) == len(found)
    assert len(ledger.candidate_set(FamilyId.PASSAGES).candidates) == 2


def test_a_passage_is_a_passage_at_any_scale():
    """No gate mentions the part, so scaling the model changes nothing but the numbers."""

    small = recognise_passages(_hexagonal_passage())
    with BuildPart() as big_bore:
        with BuildSketch(Plane.XY):
            RegularPolygon(180, 6)
        extrude(amount=800, both=True)
    big = recognise_passages(Box(1200, 800, 400) - big_bore.part)

    assert len(small) == len(big) == 1
    assert small[0].axis == big[0].axis and small[0].sides == big[0].sides
    assert round(big[0].length / 20, 3) == small[0].length


def test_a_part_with_no_void_has_no_passages():
    assert recognise_section_passages(_block()) == []


def test_a_shared_face_edge_memo_does_not_change_the_result():
    """``face_edges=`` is an optimisation, never a behaviour switch."""

    part = _hexagonal_passage()
    plain = recognise_passages(part)

    assert plain, "the fixture must reach the scan for this comparison to mean anything"
    assert plain == recognise_passages(part, face_edges=FaceEdges())


def test_a_blind_void_stays_a_pocket_when_its_floor_edge_is_blended():
    """A fillet or chamfer at the bottom of a pocket does not open it into a passage.

    The cap test originally looked only at planar axis-aligned neighbours, so breaking the
    floor edge removed the only candidate and an ordinary blind pocket came back as a through
    passage. `docs/capabilities.md` publishes capped voids as an exclusion; this is what makes
    that true for manufactured geometry rather than only for sharp corners.
    """

    with BuildPart() as bore:
        with BuildSketch(Plane.XY):
            RegularPolygon(9, 6)
        extrude(amount=14)
    blind = _block() - Pos(0, 0, -4) * bore.part
    floor_edges = [e for e in blind.edges() if abs(e.center().Z - (-4)) < 1e-6]
    assert floor_edges, "the fixture must have a floor edge to blend"

    assert recognise_passages(blind) == []
    assert recognise_passages(fillet(floor_edges, 2.0)) == []
    assert recognise_passages(chamfer(floor_edges, 1.5)) == []


def _twice_area(section) -> float:
    count = len(section)
    return sum(
        section[at][0] * section[(at + 1) % count][1]
        - section[(at + 1) % count][0] * section[at][1]
        for at in range(count)
    )


def _is_material(part, point, along) -> bool:
    probe = BRepClass3d_SolidClassifier(part.wrapped)
    probe.Perform(gp_Pnt(point[0], point[1], along), 1e-6)
    return probe.State() == TopAbs_IN


def test_the_cross_section_is_the_corners_a_consumer_can_dimension_from():
    """`sides` names the shape; `section` measures it.

    A record carrying only a side count cannot distinguish passages of different size, aspect
    or rotation, which is what made the first version a taxonomy label rather than a dimension.
    The corners are pinned against the sketch they were cut from, in part coordinates.
    """

    with BuildPart() as tri:
        with BuildSketch(Plane.XZ):
            Polygon((-8, -6), (8, -6), (0, 8))
        extrude(amount=60, both=True)
    (passage,) = recognise_passages(Box(120, 60, 40) - tri.part)

    assert passage.axis == "y"
    assert passage.sides == 3
    assert passage.section == ((-8.0, -6.0), (8.0, -6.0), (0.0, 8.0))
    # Canonical, so the kernel's traversal cannot reach the record: anticlockwise, from the
    # lexicographically smallest corner.
    assert passage.section[0] == min(passage.section)
    assert _twice_area(passage.section) > 0


def test_a_concave_cross_section_is_probed_from_a_point_inside_it():
    """The material test needs a point in the void, and an average is not one.

    A U-shaped passage's own area centroid falls in the material between its arms. Probing
    there answers "material inside the ring" and calls the void a prism -- so the fixture is
    built to prove the construction rather than to exercise it: the centroid is asserted to be
    in material while the passage is still reported.
    """

    with BuildPart() as slot_u:
        with BuildSketch(Plane.XY):
            Polygon(
                (-15, -15), (15, -15), (15, 15), (9, 15), (9, -9), (-9, -9), (-9, 15), (-15, 15)
            )
        extrude(amount=40, both=True)
    part = Box(60, 60, 20) - slot_u.part

    ledger, found = _attributed_sections(part)
    (passage,) = found
    assert len(passage.section.boundary) == 8
    (candidate,) = ledger.candidate_set(FamilyId.PASSAGES).candidates
    assert len(ledger.defining_of(candidate)) == 8
    legacy = recognise_passages(part)[0]
    assert _is_material(part, _centroid(legacy.section), legacy.at[2]), (
        "the fixture must be one the centroid gets wrong"
    )
    assert not _is_material(part, _interior_point(legacy.section), legacy.at[2])


def test_a_passage_records_the_ring_it_was_built_from():
    """The claim is the ring, and nothing it merely touched.

    Reconciliation reads these, so a claim over the end faces a passage opens onto would have
    it contest every feature at its mouth.
    """

    part = _hexagonal_passage()
    ledger, found = _attributed_sections(part)
    (passage,) = found

    (claim,) = ledger.claims
    (candidate,) = ledger.candidate_set(FamilyId.PASSAGES).candidates
    assert candidate.record is passage
    assert candidate.evidence.defining == claim.defining
    assert claim.claimant is passage
    assert len(claim.defining) == len(passage.section.boundary)
    for node in claim.defining:
        assert ledger.graph.is_planar(node)
        normal = ledger.graph.normal(node)
        assert abs(normal[2]) <= 0.01, "a wall runs along the passage, not across it"


def test_aggregate_discovers_passages_once_before_reconciliation(monkeypatch) -> None:
    import quiddity._registry as registry_module

    original = registry_module.recognise_section_passages
    calls = 0

    def counted(*args, **kwargs):
        nonlocal calls
        calls += 1
        return original(*args, **kwargs)

    monkeypatch.setattr(registry_module, "recognise_section_passages", counted)

    build_recognition_result(_hexagonal_passage())

    assert calls == 1


def test_a_cross_section_that_is_not_a_simple_polygon_is_refused():
    """The corners are read from the kernel, so the walk must not assume they form a shape.

    Declining is what a decline looks like here: `_canonical` returns None and the ring is
    dropped, rather than a record being emitted with an area of zero or a corner counted twice.
    """

    assert _canonical([(0.0, 0.0), (1.0, 0.0), (0.0, 0.0)]) is None, "a repeated corner"
    assert _canonical([(0.0, 0.0), (1.0, 0.0), (2.0, 0.0)]) is None, "collinear corners"
    assert _canonical([(1.0, 1.0), (0.0, 0.0), (1.0, 0.0)]) == (
        (0.0, 0.0),
        (1.0, 0.0),
        (1.0, 1.0),
    ), "and a real triangle comes back anticlockwise from its smallest corner"


def test_an_interior_point_is_inside_the_polygon_and_not_merely_near_it():
    """Checked against the polygon itself, so the construction stands without a solid.

    The two branches are the ones that matter: a convex shape, where the ear is empty, and a
    concave one, where another corner intrudes into it and the diagonal is used instead.
    """

    triangle = ((-8.0, -6.0), (8.0, -6.0), (0.0, 8.0))
    u_shape = (
        (-15.0, -15.0),
        (15.0, -15.0),
        (15.0, 15.0),
        (9.0, 15.0),
        (9.0, -9.0),
        (-9.0, -9.0),
        (-9.0, 15.0),
        (-15.0, 15.0),
    )
    for polygon in (triangle, u_shape):
        assert _winds_around(polygon, _interior_point(polygon)), polygon
    # The centroid is the thing this replaces, and the U is where it fails.
    assert not _winds_around(u_shape, _centroid(u_shape))


def _winds_around(polygon, point) -> bool:
    """Crossing-number point-in-polygon, written out rather than reusing the module's own."""

    inside = False
    count = len(polygon)
    for at in range(count):
        (ux, uy), (vx, vy) = polygon[at], polygon[(at + 1) % count]
        if (uy > point[1]) != (vy > point[1]):
            crossing = ux + (point[1] - uy) * (vx - ux) / (vy - uy)
            if point[0] < crossing:
                inside = not inside
    return inside


def test_a_feature_cut_into_a_wall_partway_along_does_not_cap_the_passage():
    """A neighbour inside the run, touching neither end, is not a floor.

    The cap test asks whether something sits *across* an end of the span. A notch milled into
    one wall halfway along overlaps the span and reaches neither end, and rejecting it on
    proximity alone would lose the passage. This is the branch that says so.
    """

    part = Box(60, 40, 20) - Box(10, 10, 60) - Pos(7, 0, 0) * Box(6, 6, 4)
    (passage,) = recognise_passages(part)

    assert passage.axis == "z"
    assert passage.length == 20.0
    assert passage.section == ((-5.0, -5.0), (5.0, -5.0), (5.0, 5.0), (-5.0, 5.0))


def test_a_ledger_built_from_another_part_is_refused_rather_than_answered():
    """A family that *walks* the graph is not refused for free, unlike one that resolves faces.

    Every other family turns a face into a node as it goes, so a graph paired with the wrong
    part raises on the first lookup. This one starts from the graph's own nodes and never asks
    it about *this* part at all -- so before the shared walk checked, it happily reported a
    record describing the other solid. Silently answering the wrong question is worse than
    refusing, which is the whole reason `require_node` exists.
    """

    part, twin = _hexagonal_passage(), _hexagonal_passage()
    assert recognise_passages(twin) == recognise_passages(part), "the twin is this part"

    foreign = ClaimLedger(FaceGraph(twin))
    try:
        recognise_section_passages(part, ledger=foreign)
    except ValueError as refusal:
        assert "built from a different part" in str(refusal)
    else:
        raise AssertionError("recognise_section_passages answered about another part's graph")


def _attributed_sections(part):
    plain = recognise_section_passages(part)
    ledger = ClaimLedger(FaceGraph(part))
    measured = recognise_section_passages(part, ledger=ledger)
    assert measured == plain
    candidates = ledger.candidate_set(FamilyId.PASSAGES).candidates
    assert len(candidates) == len(measured)
    assert all(
        candidate.record is record for candidate, record in zip(candidates, measured, strict=True)
    )
    return ledger, measured
