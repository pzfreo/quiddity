# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

"""An angled blind step, and the line between it and a chamfer.

Every test here is built around one pair of parts: the same 45° wedge taken out of the same
edge of the same block, once stopping inside the part and once running right through. That
pairing is the point. The two differ in exactly one way — whether a triangular flat closes
the end — and nothing else about them changes, so a test that passes on one and fails on the
other has isolated the discriminator rather than some incidental property of the geometry.

Deliberately *not* a size distinction. The legs, angle, block and orientation are identical
across the pair; only the topology differs. If the recogniser ever started separating these
two on size, the equal-legs assertions here would keep passing and
``test_length_alone_does_not_decide_which_family_claims_a_slant`` would fail.
"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path

import pytest
from attribution_audit import attributed_run, unattributed_run
from build123d import (
    Align,
    Axis,
    Box,
    BuildPart,
    BuildSketch,
    Cylinder,
    Face,
    Plane,
    Polygon,
    Pos,
    Rot,
    Vector,
    Wire,
    chamfer,
    export_step,
    extrude,
    import_step,
)

from quiddity import (
    AngledStep,
    build_recognition_result,
    recognise_angled_steps,
    recognise_chamfers,
)
from quiddity._adjacency import (
    FaceEdges,
    FaceGraph,
    edge_face_map,
    nearest_axis_aligned_planes,
)
from quiddity._bevel import material_beyond_corner
from quiddity._candidates import FamilyId
from quiddity._claims import ClaimLedger
from quiddity._reconcile import chamfers_that_are_not_angled_steps
from quiddity.angled_steps import (
    _closed_by_a_triangular_flat,
    _effective_linear_sides,
)
from quiddity.chamfers import BevelReject, classify_bevel, convex_bevel

#: A 45° wedge whose in-plane legs are both 4 mm: rotating a square 45° puts its half-diagonal
#: on each axis, so a side of 4·√2 cuts 4 mm into each of the two faces meeting at the edge.
_WEDGE = 5.657
_SUBDIVIDED_TERMINAL = Path(__file__).parent / "corpus" / "mfcadpp_regressions" / "11512.step"
_SUBDIVIDED_TERMINAL_MANIFEST = _SUBDIVIDED_TERMINAL.with_name("MANIFEST.json")


def _block() -> Box:
    return Box(60, 40, 12)


def _blind():
    """The wedge stopped inside the part: open where it runs off the -X end, closed by a
    triangular flat at its +X end."""

    return _block() - Pos(-20, 20, 6) * Rot(45, 0, 0) * Box(30, _WEDGE, _WEDGE)


def _through():
    """The same wedge run edge to edge, so neither end is closed — a chamfer."""

    return _block() - Pos(0, 20, 6) * Rot(45, 0, 0) * Box(70, _WEDGE, _WEDGE)


def _linear_face(points: list[tuple[float, float, float]]) -> Face:
    return Face(Wire.make_polygon([Vector(*point) for point in points], close=True))


def test_split_triangle_predicate_is_not_a_four_sided_relaxation() -> None:
    split_triangle = _linear_face([(0, 0, 0), (2, 0, 0), (4, 0, 0), (0, 4, 0)])
    split_rectangle = _linear_face([(0, 0, 0), (2, 0, 0), (4, 0, 0), (4, 4, 0), (0, 4, 0)])
    near_collinear_quad = _linear_face([(0, 0, 0), (2, 1e-4, 0), (4, 0, 0), (0, 4, 0)])

    assert _effective_linear_sides(split_triangle) == 3
    assert _effective_linear_sides(split_rectangle) == 4
    assert _effective_linear_sides(near_collinear_quad) == 4

    with BuildPart() as prism:
        with BuildSketch():
            Polygon((0, 0), (10, 0), (0, 6))
        extrude(amount=2)
    reversed_triangle = next(
        face
        for face in prism.faces()
        if len(face.outer_wire().edges()) == 3
        and all(not edge.is_forward for edge in face.outer_wire().edges())
    )
    assert _effective_linear_sides(reversed_triangle) == 3


@pytest.mark.parametrize(
    ("offset", "expected"),
    [(1e-5, 3), (1e-4, 4)],
)
def test_split_triangle_direction_boundary_is_fail_closed(offset: float, expected: int) -> None:
    boundary = _linear_face([(0, 0, 0), (2, offset, 0), (4, 0, 0), (0, 4, 0)])

    assert _effective_linear_sides(boundary) == expected


@pytest.mark.skipif(
    not _SUBDIVIDED_TERMINAL.is_file(),
    reason="the independently authored STEP regression is excluded from the sdist",
)
@pytest.mark.parametrize(
    ("rotation_axis", "degrees", "expected_axis"),
    [(Axis.X, 0, "z"), (Axis.X, 90, "y"), (Axis.Y, 90, "x")],
)
def test_independently_authored_split_terminal_is_recognised_covariantly(
    rotation_axis: Axis, degrees: float, expected_axis: str
) -> None:
    source = import_step(_SUBDIVIDED_TERMINAL)
    part = Pos(91, -37, 48) * source.rotate(rotation_axis, degrees)

    steps = recognise_angled_steps(part)
    aggregate = build_recognition_result(part)

    assert len(steps) == 1
    assert aggregate.angled_steps == tuple(steps)
    assert steps[0].axis == expected_axis
    assert (steps[0].leg1, steps[0].leg2, steps[0].angle, steps[0].length) == (
        6.121,
        2.685,
        23.68,
        13.534,
    )
    assert all(chamfer.at != steps[0].at for chamfer in aggregate.chamfers)


@pytest.mark.skipif(
    not _SUBDIVIDED_TERMINAL.is_file(),
    reason="the independently authored STEP regression is excluded from the sdist",
)
def test_independently_authored_split_terminal_has_pinned_provenance() -> None:
    manifest = json.loads(_SUBDIVIDED_TERMINAL_MANIFEST.read_text(encoding="utf-8"))
    entry = manifest["models"][_SUBDIVIDED_TERMINAL.name]

    assert manifest["source"] == "MFCAD++ published test split"
    assert manifest["licence"] == "CC BY"
    assert hashlib.sha256(_SUBDIVIDED_TERMINAL.read_bytes()).hexdigest() == entry["sha256"]


def test_unreadable_terminal_boundary_fails_closed_without_changing_recognition() -> None:
    class BrokenEdge:
        geom_type = type("Geometry", (), {"name": "LINE"})()

        def tangent_at(self):
            raise RuntimeError("kernel tangent unavailable")

    class BrokenWire:
        def edges(self):
            return [BrokenEdge()]

    class BrokenFace:
        def outer_wire(self):
            return BrokenWire()

    assert _effective_linear_sides(BrokenFace()) is None  # type: ignore[arg-type]


def test_compatibility_terminal_boolean_reads_the_bounded_terminal_result() -> None:
    blind_faces = list(_blind().faces())
    blind_edges = edge_face_map(blind_faces)
    through_faces = list(_through().faces())
    through_edges = edge_face_map(through_faces)

    assert any(_closed_by_a_triangular_flat(face, blind_edges) for face in blind_faces)
    assert not any(_closed_by_a_triangular_flat(face, through_edges) for face in through_faces)


def test_a_wedge_stopped_inside_the_part_is_an_angled_step():
    """The blind end is what makes it a step, and the record carries how far it runs.

    ``length`` is the field a chamfer has no use for: a chamfer spans its whole edge, so its
    extent is not a chosen dimension. Here it is, and it is the reason a consumer can call
    the feature out at all.
    """

    steps = recognise_angled_steps(_blind())

    assert len(steps) == 1
    step = steps[0]
    assert step.axis == "x"
    assert (step.leg1, step.leg2) == (4.0, 4.0)
    assert step.angle == 45.0
    # The cutter spans x = -35..-5 and the block stops at -30, so 25 mm of it is inside.
    assert step.length == 25.0


@pytest.mark.parametrize(
    ("rotation_axis", "degrees", "expected_axis"),
    [
        (Axis.X, 0, "x"),
        (Axis.Y, 180, "x"),
        (Axis.X, 90, "x"),
        (Axis.Y, 90, "z"),
        (Axis.Y, -90, "z"),
        (Axis.Z, 90, "y"),
        (Axis.Z, -90, "y"),
    ],
)
def test_angled_step_is_covariant_across_principal_axes_and_signs(
    rotation_axis: Axis,
    degrees: float,
    expected_axis: str,
) -> None:
    """A signed axis permutation changes coordinates, never physical eligibility."""

    part = Pos(17, -11, 9) * _blind().rotate(rotation_axis, degrees)

    steps = recognise_angled_steps(part)
    result = build_recognition_result(part)

    assert len(steps) == 1
    assert steps[0].axis == expected_axis
    assert (steps[0].leg1, steps[0].leg2, steps[0].angle, steps[0].length) == (
        4.0,
        4.0,
        45.0,
        25.0,
    )
    assert result.angled_steps == tuple(steps)
    assert result.chamfers == ()


def test_principal_y_angled_step_survives_step_round_trip(tmp_path) -> None:
    part = Pos(17, -11, 9) * _blind().rotate(Axis.Z, 90)
    path = tmp_path / "principal-y-angled-step.step"

    assert export_step(part, path)
    imported = import_step(path)

    steps = recognise_angled_steps(imported)
    assert len(steps) == 1
    assert steps[0].axis == "y"
    assert (steps[0].leg1, steps[0].leg2, steps[0].angle, steps[0].length) == (
        4.0,
        4.0,
        45.0,
        25.0,
    )


def test_successful_step_owns_only_the_slant() -> None:
    part = _blind()
    ledger, steps = attributed_run(part, FamilyId.ANGLED_STEPS, recognise_angled_steps)
    step = steps[0]
    candidate = ledger.candidate_set_for(FamilyId.ANGLED_STEPS, [step]).candidates[0]
    evidence = ledger.snapshot_index()

    assert len(evidence.defining_of(candidate)) == 1
    assert len(evidence.constituent_of(candidate)) == 2
    assert evidence.defining_of(candidate) < evidence.constituent_of(candidate)
    assert len(ledger.claims) == 1


def test_the_same_wedge_run_through_is_a_chamfer_and_not_a_step():
    """The control. Identical legs, identical angle, identical block — only the end differs."""

    assert recognise_angled_steps(_through()) == []

    chamfers = recognise_chamfers(_through())
    assert len(chamfers) == 1
    assert (chamfers[0].leg1, chamfers[0].leg2) == (4.0, 4.0)


def test_the_two_families_never_claim_the_same_face():
    """The reconciliation, read off the aggregate that applies it.

    Every recognised face must belong to exactly one of the two families. If they ever
    disagreed the face would be reported twice, or — worse and quieter — by neither. Checked
    in both directions on the pair, and on a part carrying one of each so the exclusion is not
    an artefact of there being only one bevel to argue over.

    Asked of :func:`build_recognition_result` rather than of the two recognisers, because that
    is where the decision now lives. ``recognise_chamfers`` proposes the blind wedge, correctly
    on its own evidence — see
    ``test_the_chamfer_family_proposes_a_slant_and_the_reconciler_takes_it_back``.
    """

    assert build_recognition_result(_blind()).chamfers == ()
    assert recognise_angled_steps(_through()) == []

    both = _blind() - Pos(0, -20, 6) * Rot(45, 0, 0) * Box(70, _WEDGE, _WEDGE)
    result = build_recognition_result(both)
    steps, chamfers = result.angled_steps, result.chamfers

    assert len(steps) == 1, "the blind wedge must survive alongside a through one"
    assert len(chamfers) == 1, "the through wedge must survive alongside a blind one"
    assert steps[0].at != chamfers[0].at


def test_the_chamfer_family_proposes_a_slant_and_the_reconciler_takes_it_back():
    """What moved, stated on the pair the whole module is built around.

    The blind wedge and the through wedge are one 45° cut in one block, differing only in
    whether a triangular flat closes the end. ``recognise_chamfers`` reports *both* now: on the
    face alone a slant is a bevel bridging two perpendicular walls at a convex corner, which is
    a chamfer's entire signature, and the flat that says otherwise belongs to the other family's
    evidence. The rule reads the claims and drops the one the step already has.

    This is the assertion that fails if ``recognise_chamfers`` ever grows a private opinion
    about angled steps again — the hand-rolled ownership device #92 removed.
    """

    blind, through = _blind(), _through()

    assert len(recognise_chamfers(blind)) == 1, "the slant is a bevel on its own evidence"
    assert len(recognise_chamfers(through)) == 1

    for part, kept in ((blind, 0), (through, 1)):
        ledger = ClaimLedger(FaceGraph(part))
        chamfers = recognise_chamfers(part, ledger=ledger)
        steps = recognise_angled_steps(part, ledger=ledger)
        assert (
            chamfers_that_are_not_angled_steps(chamfers, steps, ledger.snapshot_index())
            == chamfers[:kept]
        )
        assert len(steps) == 1 - kept


def test_length_alone_does_not_decide_which_family_claims_a_slant():
    """A long blind step stays a step; a short through bevel stays a chamfer.

    This is the test that fails if the discriminator is ever quietly replaced by a size gate.
    The blind wedge here is *longer* than the part is wide in Y and cuts deeper than the
    chamfer above, and it is still a step; the through wedge is tiny and still a chamfer.
    """

    long_blind = _block() - Pos(-5, 20, 6) * Rot(45, 0, 0) * Box(45, 11.314, 11.314)
    small_through = _block() - Pos(0, -20, 6) * Rot(45, 0, 0) * Box(70, 1.414, 1.414)

    steps = recognise_angled_steps(long_blind)
    assert len(steps) == 1 and steps[0].leg1 == 8.0
    assert build_recognition_result(long_blind).chamfers == ()

    assert recognise_angled_steps(small_through) == []
    assert len(recognise_chamfers(small_through)) == 1


def test_a_pocket_wall_is_not_an_angled_step_though_it_has_a_triangular_floor():
    """A pocket whose plan is not axis-aligned has oblique walls over a triangular floor.

    That is the angled step's signature minus one thing, and prototyped without the
    "bridges two axis-aligned faces" gate such pockets outnumbered real steps three to one
    on the MFCAD++ corpus — 21% precision against 100%. A pocket wall's only axis-aligned
    neighbours are its floor and the top face, both normal to the same axis, so it never
    supplies the two distinct in-plane walls a bevel must bridge.
    """

    pocket = _block() - Pos(0, 0, 6) * Rot(0, 0, 30) * Box(20, 14, 6)

    assert recognise_angled_steps(pocket) == []


def test_a_right_triangular_pocket_wall_is_not_an_angled_step():
    """The pocket the "bridges two axis-aligned faces" gate does not catch.

    That gate excludes a pocket by asking what its wall bridges, and the pocket it was
    measured against had none to offer: a rotated-box pocket's walls are all oblique, so no
    two distinct in-plane axes are available. A pocket whose plan is a *right* triangle
    answers the question correctly instead — the hypotenuse bridges the two axis-aligned
    walls beside it, its floor is a triangle, and the corner it replaces has vacuum on the
    bevel side like any other. Four of the five gates pass and it is still a pocket.

    Found on held-out geometry, not designed for: ``corpus/mfcadpp_holdout``'s draw 1
    carried one and the design corpus carried none, so the defect was invisible to every
    figure this family quoted. What separates the two is what lies *beyond* the corner —
    stock for a step cut into an edge of the part, material for two walls of a recess
    meeting.
    """

    with BuildPart() as prism:
        with BuildSketch(Plane.XY):
            Polygon((0, 0), (14, 0), (0, 10))
        extrude(amount=6)
    pocket = _block() - prism.part

    # The premise: the hypotenuse really does clear all four earlier gates, so this part
    # exercises the far-corner probe rather than being rejected before reaching it.
    faces = list(pocket.faces())
    edge_faces = edge_face_map(faces)
    reached = 0
    for face in faces:
        try:
            edge_i, _nv, span, _hi, _lo = classify_bevel(face)
        except BevelReject:
            continue
        oi = [j for j in (0, 1, 2) if j != edge_i]
        centre = {i: 0.5 * (span[i][0] + span[i][1]) for i in (0, 1, 2)}
        neigh = nearest_axis_aligned_planes(face, edge_faces, centre, exclude_axis=edge_i)
        if (
            oi[0] in neigh
            and oi[1] in neigh
            and convex_bevel(pocket, centre, edge_i, neigh)
            and material_beyond_corner(pocket, centre, edge_i, neigh)
        ):
            reached += 1
    assert reached == 1, "this fixture must reject exactly at the far-corner probe"

    assert recognise_angled_steps(pocket) == []


def test_a_gusset_filling_a_concave_corner_is_not_an_angled_step():
    """The convex probe, on the only geometry that actually reaches it.

    A gusset satisfies every other gate: its hypotenuse is an oblique plane bridging two
    perpendicular axis-aligned walls, and its two ends are triangles. The single difference
    from a real step is that the corner it sits in is *filled* rather than cut away.

    The probe is load-bearing rather than defensive: over 120 MFCAD++ models it rejects 24
    of the 85 faces that reach it. This fixture is the *isolated* case, built so that only
    the convexity differs from an accepted step — the corpus proves the gate fires, and this
    proves what it fires on.
    """

    align_min = (Align.MIN, Align.MIN, Align.MIN)
    base = Box(40, 40, 5, align=align_min)
    wall = Box(5, 40, 30, align=align_min)
    with BuildPart() as web:
        with BuildSketch(Plane.XZ):
            Polygon((5, 5), (18, 5), (5, 18))
        extrude(amount=10)
    gusseted = base + wall + Pos(0, 15, 0) * web.part

    # The premise: the hypotenuse really does clear the gates before the convex one, so this
    # part exercises that probe rather than being rejected earlier for some other reason.
    faces = list(gusseted.faces())
    edge_faces = edge_face_map(faces)
    reached = 0
    for face in faces:
        try:
            edge_i, _nv, span, _hi, _lo = classify_bevel(face)
        except BevelReject:
            continue
        oi = [j for j in (0, 1, 2) if j != edge_i]
        centre = {i: 0.5 * (span[i][0] + span[i][1]) for i in (0, 1, 2)}
        neigh = nearest_axis_aligned_planes(face, edge_faces, centre, exclude_axis=edge_i)
        if oi[0] in neigh and oi[1] in neigh and not convex_bevel(gusseted, centre, edge_i, neigh):
            reached += 1
    assert reached == 1, "this fixture must reject exactly at the convex probe"

    assert recognise_angled_steps(gusseted) == []
    # Sized to clear `max_leg_frac` too, so the hypotenuse reaches `recognise_chamfers`' own
    # convex probe rather than being turned away earlier as an oversized bevel. Both
    # recognisers must refuse a gusset, and for the same reason.
    assert recognise_chamfers(gusseted) == []


def test_a_bolt_hole_through_the_blind_end_does_not_hide_the_step():
    """The companion test counts the *outer* wire, so an inner one cannot cost recall.

    The blind end is what makes a step a step, and it is recognised by being a triangle. Drill
    a hole through it — a bolt hole in the face closing an angled shoulder, which is ordinary
    — and the face has four edges: three sides and a circle. Counting all of them, the step
    vanished entirely, and the record it had produced was correct in every field.

    Relaxing the count itself is what the module docstring rules out, and rightly: a chamfer
    strip's end cap is an axis-aligned four-edge face, so a family that accepted four would
    take every chamfer back. The outer wire separates the two without relaxing anything —
    a rectangle's outer wire has four edges whatever is drilled through it, a triangle's has
    three.

    What this does *not* recover is the other subdivision the docstring names: a triangle
    whose *side* is split by a neighbouring feature has four edges in the outer wire itself,
    and is still missed. That case needs the companion recognised as geometrically triangular
    rather than topologically so.
    """

    plain = recognise_angled_steps(_blind())
    drilled_part = _blind() - Pos(0, 18.67, 4.67) * Rot(0, 90, 0) * Cylinder(0.6, 80)

    # The premise: the hole really did subdivide the blind end rather than missing it.
    ends = [f for f in drilled_part.faces() if abs(f.center().X + 5) < 1e-6 and f.area < 20]
    assert len(ends) == 1
    assert len(ends[0].edges()) == 4, "the fixture must actually add an edge to the flat"
    assert len(ends[0].outer_wire().edges()) == 3

    ledger, drilled = attributed_run(drilled_part, FamilyId.ANGLED_STEPS, recognise_angled_steps)
    assert drilled == plain
    (candidate,) = ledger.candidate_set(FamilyId.ANGLED_STEPS).candidates
    assert len(ledger.defining_of(candidate)) == 1
    assert len(ledger.snapshot_index().constituent_of(candidate)) == 2


def test_a_step_is_a_step_at_any_scale():
    """No gate here mentions the part, so scaling the whole model changes nothing.

    A size-based discriminator could not promise this — and the rejected alternative to this
    recogniser was exactly that. Built at 1× and 20×, the records must agree once the 20×
    part's lengths are divided back down.
    """

    small = recognise_angled_steps(_blind())
    big = recognise_angled_steps(
        Box(1200, 800, 240) - Pos(-400, 400, 120) * Rot(45, 0, 0) * Box(600, 113.14, 113.14)
    )

    assert len(small) == len(big) == 1
    assert small[0].axis == big[0].axis
    assert small[0].angle == big[0].angle
    assert round(big[0].leg1 / 20, 3) == small[0].leg1
    assert round(big[0].length / 20, 3) == small[0].length


def test_records_are_ordered_deterministically_and_are_plain_data():
    """Two steps on one part come back in a stable order that does not depend on traversal."""

    part = _blind() - Pos(20, -20, 6) * Rot(45, 0, 0) * Box(30, _WEDGE, _WEDGE)
    ledger, steps = attributed_run(part, FamilyId.ANGLED_STEPS, recognise_angled_steps)

    assert len(steps) == 2
    assert steps == sorted(steps, key=lambda s: (s.axis, s.at))
    assert all(isinstance(s, AngledStep) for s in steps)
    assert recognise_angled_steps(part) == steps
    candidates = ledger.candidate_set(FamilyId.ANGLED_STEPS).candidates
    for candidate, step in zip(candidates, steps, strict=True):
        assert candidate.record is step
        (slant,) = ledger.defining_of(candidate)
        constituent = ledger.snapshot_index().constituent_of(candidate)
        assert len(constituent) == 2 and slant in constituent
        bounds = ledger.graph.bounds(slant)
        axis = "xyz".index(step.axis)
        assert round(bounds[axis][1] - bounds[axis][0], 3) == step.length


def test_a_part_with_no_oblique_face_has_no_angled_steps():
    """The empty case, on geometry that exercises the scan rather than skipping it."""

    unattributed_run(
        _block() - Pos(0, 0, 0) * Cylinder(6, 12),
        FamilyId.ANGLED_STEPS,
        recognise_angled_steps,
    )


def test_a_shared_face_edge_memo_does_not_change_the_result():
    """``face_edges=`` is an optimisation, never a behaviour switch — the census shares one
    memo across every recogniser, so a mis-keyed lookup here would show up as a changed
    record count rather than as an exception."""

    part = chamfer(_blind().edges().filter_by(Axis.Z).group_by(Axis.X)[0], 1.5)
    plain = recognise_angled_steps(part)

    assert plain, "the fixture must reach the scan for this comparison to mean anything"
    assert plain == recognise_angled_steps(part, face_edges=FaceEdges())
