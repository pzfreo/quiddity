# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

"""What a volumetric probe is measured against: the part's solids, and only its solids.

An imported STEP part is routinely a compound holding the machined solid beside loose
construction geometry, and ``Compound.intersect`` distributes over every child of it. The
fragments a non-solid child can return have volume ``0.0`` by construction, so those booleans
could never move the answer -- which is exactly what the equality tests here pin, from both
directions: the narrowed measurement equals build123d's full distribution, and a whole census
is unchanged when the narrowing is monkeypatched away.

The booleans that survive that narrowing are then the ones a run does not need to ask at all:
a repeat of an earlier probe, a probe standing clear of the solid's bounding box, or a probe
that touches none of its face boxes and so lies wholly inside the material or wholly outside it.
Each of those has a test here that checks the short-circuit against the boolean it replaces on
constructed geometry -- outside, inside, straddling, touching, offset either side of a face by
1e-9, 1e-7 and 1e-6, reversed, and over a multi-solid compound -- and the census equality test is
repeated for them as a whole. One of them is not constructed: a probe ``nist_ftc_08`` really
builds, inset ``COORD_FLOOR`` into a pocket corner, where the point classifier calls a corner of
an empty region ``IN`` and the run has to survive it.

The operation-count sentinel at the bottom pins the other half: that the booleans really are
gone. Nothing here asserts wall-clock.
"""

from __future__ import annotations

from pathlib import Path
from typing import cast

import pytest
from build123d import Box, Compound, Edge, Pos, Shell, Solid, Vertex, import_step
from build123d.topology.shape_core import Shape
from OCP.BRepAlgoAPI import BRepAlgoAPI_Common
from OCP.BRepClass3d import BRepClass3d_SolidClassifier
from OCP.gp import gp_Pnt
from OCP.TopAbs import TopAbs_IN, TopAbs_OUT

from quiddity import _volume_probe
from quiddity._adjacency import FaceGraph
from quiddity._solid_properties import SolidProperties
from quiddity._typing import Part
from quiddity._volume_probe import (
    intersection_volume,
    material_fraction,
    probe_solids,
    probe_volume,
)
from quiddity.census import feature_census

CORPUS = Path(__file__).parent / "corpus"


def _loose_geometry() -> tuple[Compound, Shell, Vertex]:
    """The kinds of child the NIST compounds carry beside their solid, at their most hazardous.

    The shell is deliberately *closed*. A manifold shell is the one non-solid shape build123d
    gives a non-zero ``volume`` (this one encloses 2744 mm3), so it is the only way narrowing
    the probe to solids could move a number -- and it does not, because ``ShapeList.expand``
    dissolves a shell into its faces before the sum ever sees it. An open shell would leave the
    equality tests below unable to tell that apart from "this shell had no volume anyway".
    """

    edges = Compound([Edge.make_line((-40.0, y, -40.0), (40.0, y, 40.0)) for y in (-6.0, 0.0, 6.0)])
    closed_shell = Shell((Pos(0.0, 0.0, 24.0) * Box(14.0, 14.0, 14.0)).faces())
    assert closed_shell.is_manifold and closed_shell.volume == pytest.approx(2744.0)
    return edges, closed_shell, Vertex(3.0, 3.0, 3.0)


def _one_body_with_clutter() -> tuple[Compound, Solid]:
    body = Box(20.0, 20.0, 20.0)
    return Compound([body, *_loose_geometry()]), body


def _two_bodies_with_clutter() -> tuple[Compound, tuple[Solid, ...]]:
    bodies = (Box(20.0, 20.0, 20.0), Pos(26.0, 0.0, 0.0) * Box(10.0, 20.0, 20.0))
    return Compound([*bodies, *_loose_geometry()]), bodies


def _full_distribution_fraction(part: Part, probe: Solid) -> float:
    """What build123d computes when it distributes the intersection over every child."""

    return intersection_volume(part.intersect(probe)) / float(probe.volume)


def test_loose_children_do_not_change_the_measurement() -> None:
    """The claim the narrowing rests on, checked against the distribution it replaces."""

    part, body = _one_body_with_clutter()
    probe = Pos(6.0, 0.0, 0.0) * Box(20.0, 8.0, 8.0)

    assert material_fraction(part, probe) == _full_distribution_fraction(part, probe)
    assert material_fraction(part, probe) == material_fraction(body, probe)
    assert material_fraction(part, probe) == pytest.approx(0.7)  # 14 mm of the probe's 20

    # The same, for a probe that reaches the closed shell and cuts it open on the way past.
    spanning = Pos(6.0, 0.0, 10.0) * Box(20.0, 8.0, 40.0)
    assert material_fraction(part, spanning) == _full_distribution_fraction(part, spanning)
    assert material_fraction(part, spanning) == material_fraction(body, spanning)


def test_every_body_of_a_multi_solid_part_still_contributes() -> None:
    """Narrowing to solids is not narrowing to *one* solid: the sum is over all of them."""

    part, bodies = _two_bodies_with_clutter()
    probe = Pos(15.0, 0.0, 0.0) * Box(40.0, 4.0, 4.0)
    reached = [material_fraction(body, probe) for body in bodies]

    assert min(reached) > 0.0  # the probe genuinely spans both bodies
    assert material_fraction(part, probe) == _full_distribution_fraction(part, probe)
    assert probe_volume(part, probe) == sum(probe_volume(body, probe) for body in bodies)


def test_a_probe_that_swallows_a_closed_shell_whole_still_measures_empty() -> None:
    """The hazardous case at its worst: nothing cuts the shell, so nothing opens it.

    A manifold shell has a volume, and ``BRepAlgoAPI_Common`` of a probe that contains it
    entirely returns the shell intact rather than an open piece of it -- the kernel result
    really does carry 2744 mm3, which the first assertion below pins. It still contributes
    nothing, because ``ShapeList.expand`` dissolves it into faces before the sum sees it. This
    probe sits clear of the body and holds the whole shell, plus a length of each loose edge.
    """

    part, _body = _one_body_with_clutter()
    _edges, shell, _vertex = _loose_geometry()
    probe = Pos(0.0, 0.0, 25.0) * Box(30.0, 30.0, 26.0)

    # The kernel hands back a whole manifold shell; build123d's expansion is what disarms it.
    raw = probe.solids()[0]._bool_op_list((probe.solids()[0],), (shell,), BRepAlgoAPI_Common())
    assert [fragment.volume for fragment in raw] == [pytest.approx(2744.0)]
    assert {type(fragment).__name__ for fragment in raw.expand()} == {"Face"}

    assert _full_distribution_fraction(part, probe) == 0.0
    assert material_fraction(part, probe) == 0.0


def test_a_part_that_is_already_a_solid_is_its_own_probe_target() -> None:
    """The common case pays nothing: most call sites arrive with ``graph.solid_shape(owner)``."""

    body = Solid.make_box(10.0, 10.0, 10.0)
    assert probe_solids(body) == (body,)


def test_anything_that_is_not_a_compound_is_passed_through_untouched() -> None:
    """Standalone and test callers hand these helpers bare objects with an ``intersect``."""

    class Probeable:
        def intersect(self, _probe):
            return None

    stub = Probeable()
    assert probe_solids(stub) == (stub,)  # type: ignore[arg-type]
    assert material_fraction(stub, Box(2.0, 2.0, 2.0)) == 0.0  # type: ignore[arg-type]


def test_the_probe_target_is_derived_once_per_run(monkeypatch) -> None:
    """It hangs off ``quiddity._solid_properties.SolidProperties.derived``: one walk per run."""

    part, body = _one_body_with_clutter()
    memo = SolidProperties()
    walks = 0
    original = Compound.solids

    def counted(self):
        nonlocal walks
        walks += 1
        return original(self)

    monkeypatch.setattr(Compound, "solids", counted)
    first = probe_solids(part, properties=memo)
    second = probe_solids(part, properties=memo)
    uncached = (probe_solids(part), probe_solids(part))

    assert first is second
    assert walks == 3  # once through the run's memo, once per call without one
    assert [solid.volume for solid in first] == [body.volume]
    assert all(len(target) == 1 for target in uncached)


def test_a_graph_supplies_the_run_cache_the_probes_use() -> None:
    part, _body = _one_body_with_clutter()
    graph = FaceGraph(part)
    assert probe_solids(part, properties=graph) is probe_solids(
        part, properties=graph.solid_properties
    )


#: The sentinel part and its measured boolean-build count inside volume probes.
#:
#: ``nist_ctc_01`` is a 139-face NIST part imported as one solid beside a compound of 78 loose
#: datum edges, so distributing one probe over every child cost **79** ``BRepAlgoAPI`` builds
#: instead of one. A census of it built 1772 booleans inside volume probes before the probes
#: were narrowed to the part's solids, 56 after it, and builds the count below now that a probe
#: whose answer is already known, or provable from bounding boxes and a point classifier, does
#: not reach the kernel at all.
#:
#: It counts only the builds *inside* a probe, deliberately. The exact-area support proof does
#: its own booleans on faces, and it is neither what this change touches nor what a regression
#: here would be about.
_SENTINEL_PART = CORPUS / "nist" / "nist_ctc_01_asme1_rd.stp"
_PROBE_BOOLEANS_UNDISTRIBUTED = 1772
_PROBE_BOOLEANS_BEFORE = 56
# #556 rejects a ring without observed planar ends before its interior Boolean (27 -> 26).
_PROBE_BOOLEANS = 26


def test_one_census_builds_no_more_probe_booleans_than_it_needs(monkeypatch) -> None:
    """An operation-count sentinel, not a wall-clock one.

    The regression this guards against is silent: a probe handed the whole imported compound
    still returns the right number, having asked the kernel about every loose edge in the file
    to get it. Counting the builds is the only way that shows up in CI.
    """

    depth = 0
    counted: list[int] = []
    original_bool_op = Shape._bool_op
    original_probe_volume = _volume_probe.probe_volume

    def count(self, args, tools, operation):
        if depth:
            counted.append(1)
        return original_bool_op(self, args, tools, operation)

    def measured(*args, **kwargs):
        nonlocal depth
        depth += 1
        try:
            return original_probe_volume(*args, **kwargs)
        finally:
            depth -= 1

    monkeypatch.setattr(Shape, "_bool_op", count)
    monkeypatch.setattr(_volume_probe, "probe_volume", measured)
    feature_census(import_step(_SENTINEL_PART))

    assert len(counted) == _PROBE_BOOLEANS, (
        f"a census of {_SENTINEL_PART.name} built {len(counted)} booleans inside volume probes; "
        f"{_PROBE_BOOLEANS} is the count with the memo and the two short-circuits in place, "
        f"{_PROBE_BOOLEANS_BEFORE} was the count with every probe reaching the kernel and "
        f"{_PROBE_BOOLEANS_UNDISTRIBUTED} was the count before each probe was narrowed to the "
        "part's solids. A rise means a probe is reaching the kernel that need not, or is being "
        "handed a whole imported compound again; a fall means this sentinel needs updating."
    )


def test_a_whole_census_answers_exactly_what_it_answered_undistributed(monkeypatch) -> None:
    """The one property the narrowing must never buy its speed with.

    ``nist_ctc_01`` is the pinned part with the most loose construction geometry in the corpus,
    so it is the one where a wrongly dropped child would show.
    """

    part = import_step(_SENTINEL_PART)
    narrowed = feature_census(part)
    monkeypatch.setattr(_volume_probe, "probe_solids", lambda part, *, properties=None: (part,))
    assert feature_census(part) == narrowed


def test_a_whole_census_answers_exactly_what_it_answered_through_the_kernel(monkeypatch) -> None:
    """The same property for the memo and the short-circuits: nothing may move.

    ``_describe`` returning ``None`` is how a probe with no exact key says "measure me": it turns
    off the memo *and* both short-circuits at once, leaving every probe to reach
    ``BRepAlgoAPI_Common``. The census either way is the same census.
    """

    part = import_step(_SENTINEL_PART)
    shortcut = feature_census(part)
    monkeypatch.setattr(_volume_probe, "_describe", lambda probe: None)
    assert feature_census(part) == shortcut


def _count_booleans(monkeypatch) -> list[int]:
    """Every ``BRepAlgoAPI`` build from here on, counted."""

    counted: list[int] = []
    original = Shape._bool_op

    def count(self, args, tools, operation):
        counted.append(1)
        return original(self, args, tools, operation)

    monkeypatch.setattr(Shape, "_bool_op", count)
    return counted


def _tube() -> Solid:
    """A 20 mm cube with an 8 mm square hole bored through it.

    The hole is what makes the part interesting: a probe inside it is inside the solid's own
    bounding box and still shares no volume with the material, so it is the case a box test
    alone cannot answer and the point classifier must.
    """

    return cast(Solid, (Box(20.0, 20.0, 20.0) - Box(8.0, 8.0, 30.0)).solids()[0])


def test_a_probe_outside_the_solids_box_is_answered_without_the_kernel(monkeypatch) -> None:
    body = Box(20.0, 20.0, 20.0)
    probe = Pos(30.0, 0.0, 0.0) * Box(4.0, 4.0, 4.0)
    expected = intersection_volume(body.intersect(probe))

    counted = _count_booleans(monkeypatch)
    assert probe_volume(body, probe, properties=SolidProperties()) == expected == 0.0
    assert counted == []


def test_a_probe_inside_a_hole_is_answered_by_the_classifier(monkeypatch) -> None:
    """Inside the solid's box, outside its material, and touching none of its faces."""

    body = _tube()
    probe = Pos(0.0, 0.0, 0.0) * Box(2.0, 2.0, 2.0)
    expected = intersection_volume(body.intersect(probe))

    counted = _count_booleans(monkeypatch)
    assert probe_volume(body, probe, properties=SolidProperties()) == expected == 0.0
    assert counted == []


def test_a_probe_buried_in_material_is_answered_as_its_own_volume(monkeypatch) -> None:
    """The one short-circuit that returns something other than zero -- to the last bit."""

    body = Box(20.0, 20.0, 20.0)
    probe = Pos(4.0, 0.0, 0.0) * Box(4.0, 4.0, 4.0)
    expected = intersection_volume(body.intersect(probe))
    assert expected == probe.volume

    counted = _count_booleans(monkeypatch)
    assert probe_volume(body, probe, properties=SolidProperties()) == expected
    assert counted == []


def test_a_probe_that_straddles_a_face_still_reaches_the_kernel(monkeypatch) -> None:
    body = Box(20.0, 20.0, 20.0)
    probe = Pos(10.0, 0.0, 0.0) * Box(4.0, 4.0, 4.0)  # half in the material, half out
    expected = intersection_volume(body.intersect(probe))
    assert expected == pytest.approx(32.0)

    counted = _count_booleans(monkeypatch)
    assert probe_volume(body, probe, properties=SolidProperties()) == expected
    assert len(counted) == 1


@pytest.mark.parametrize(
    "probe",
    [
        pytest.param(Pos(0.0, 0.0, 15.0) * Box(4.0, 4.0, 10.0), id="face to face"),
        pytest.param(Pos(15.0, 15.0, 15.0) * Box(10.0, 10.0, 10.0), id="corner to corner"),
    ],
)
def test_a_probe_that_only_touches_the_material_answers_what_the_kernel_answers(probe) -> None:
    """Contact of measure zero: the boxes still overlap, so the boolean still runs.

    Both boxes are padded outwards by the kernel's box gap, which is exactly why a *touching*
    probe is never mistaken for a clear one -- the padding makes the two boxes overlap and the
    question goes to ``BRepAlgoAPI_Common``, which is the only thing entitled to answer it.
    """

    body = Box(20.0, 20.0, 20.0)
    expected = intersection_volume(body.solids()[0].intersect(probe))
    assert expected == 0.0
    assert probe_volume(body, probe, properties=SolidProperties()) == expected


@pytest.mark.parametrize(
    ("offset", "outside", "booleans"),
    [
        (1e-9, True, 1),
        (1e-7, True, 0),
        (1e-6, True, 0),
        (1e-9, False, 1),
        (1e-7, False, 1),
        (1e-6, False, 0),
    ],
)
def test_the_box_gaps_decide_how_near_a_face_a_probe_may_stand(
    monkeypatch, offset: float, outside: bool, booleans: int
) -> None:
    """How much clearance each short-circuit needs, pinned either side of one face.

    The argument in the module docstring rests on both boxes being supersets of what they bound,
    and the margin that buys is small and *different for the two tests*, so it is worth having in
    a test rather than only in prose. Standing **outside** the material is measured against the
    solid's ``optimal=True`` box, which has no pad at all, so the probe's own ``1e-7`` gap is the
    whole margin and it clears at ``1e-7``. Standing **inside** it is measured against the face
    boxes, which are padded too, so it takes more than two gaps and only clears at ``1e-6``.
    Nearer than that the boxes overlap and the boolean runs -- and either way the answer is the
    boolean's answer, which is what the first assertion pins.

    The truth is measured on a *separate* pair of shapes, because running a boolean over a shape
    can leave a triangulation on it and a triangulated shape's loose box is not the same box. It
    is still a superset, so nothing about the answer moves -- but the count of booleans avoided
    does, and this test is about that count.
    """

    def geometry() -> tuple[Solid, Solid]:
        body = Box(20.0, 20.0, 20.0)  # top face at z = +10
        low = 10.0 + offset if outside else 10.0 - offset - 4.0
        return body, Pos(0.0, 0.0, low + 2.0) * Box(4.0, 4.0, 4.0)

    measured, measured_probe = geometry()
    truth, truth_probe = geometry()
    expected = intersection_volume(truth.solids()[0].intersect(truth_probe))
    assert expected == (0.0 if outside else pytest.approx(64.0))

    counted = _count_booleans(monkeypatch)
    assert probe_volume(measured, measured_probe, properties=SolidProperties()) == expected
    assert len(counted) == booleans


def test_a_reversed_probe_is_left_to_the_kernel(monkeypatch) -> None:
    """The one shape whose ``volume`` is not the volume of the region it looks like.

    ``Solid(box.wrapped.Reversed())`` *is* the complement of that box: it has volume ``-64``, and
    ``BRepAlgoAPI_Common`` intersects the body with everything outside the box rather than with
    the box. Short-circuiting it as "contained, so answer its own volume" would return ``-64.0``
    where the kernel returns ``7936.0``, and ``material_fraction`` would swing from ``-124`` to
    ``+1`` -- through every gate that reads it. Nothing builds one today, which is exactly what
    :func:`quiddity._bevel._material_at` and :mod:`quiddity._solid_properties` say about the
    reversed solids they nonetheless keep separate; this holds the same line.
    """

    body = Box(20.0, 20.0, 20.0)
    forward = (Pos(4.0, 0.0, 0.0) * Box(4.0, 4.0, 4.0)).solids()[0]
    probe = Solid(forward.wrapped.Reversed())
    assert probe.volume == pytest.approx(-64.0)

    expected = intersection_volume(body.solids()[0].intersect(probe))
    assert expected == pytest.approx(7936.0)  # the complement, not the box

    counted = _count_booleans(monkeypatch)
    assert probe_volume(body, probe, properties=SolidProperties()) == expected
    assert len(counted) == 1


#: A probe the corpus really builds, inset ``COORD_FLOOR`` into a pocket corner, and the part it
#: belongs to. Its spans are the ones ``_section_recess_geometry`` derived for that pocket.
_POCKET_CORNER_PART = CORPUS / "nist" / "nist_ftc_08_asme1_rc.stp"
_POCKET_CORNER_SPANS = (
    (143.47825100050798, 146.018249000508),
    (22.518108568469998, 28.92739191599),
    (37.238990334419995, 40.39999900019399),
)


def test_a_probe_inset_into_a_pocket_corner_is_not_lost_to_a_wrong_classification() -> None:
    """The case that made the box centre one of the classified points.

    This probe stands ``1e-6`` off two walls of a pocket -- the clearance ``inset=COORD_FLOOR``
    is *for*, and just past what the box gaps cover, so it reaches the classifier. Every one of
    its eight corners is ``1e-6`` outside the shell by ``BRepExtrema`` and the boolean answers
    ``0.0``, and yet ``BRepClass3d_SolidClassifier`` calls one corner ``IN``, reproducibly and at
    every tolerance from ``0`` upwards. Unanimity is what stops that one wrong answer from being
    the answer, and the box centre -- which is 1.27 mm from anything, not ``1e-6`` -- is what the
    decision would rest on if all eight corners ever went wrong together.

    The middle assertion pins third-party behaviour on purpose. If it starts failing, OCCT has
    changed and the classifier claims in ``_volume_probe``'s docstring want re-measuring; the
    recognition answer either side of that is the last assertion, and it does not depend on it.
    """

    solid = import_step(_POCKET_CORNER_PART).solids()[0]
    centre = tuple((low + high) / 2 for low, high in _POCKET_CORNER_SPANS)
    size = tuple(high - low for low, high in _POCKET_CORNER_SPANS)
    probe = Pos(*centre) * Box(*size)
    geometry = _volume_probe._describe(probe)
    assert geometry is not None

    # It gets past both box tests, so the classifier is what decides it.
    faces = _volume_probe._face_bounds(solid)
    assert all(_volume_probe._apart(geometry.box, face) for face in faces)

    classifier = BRepClass3d_SolidClassifier(solid.wrapped)

    def classify(point) -> int:
        classifier.Perform(gp_Pnt(*point), _volume_probe._CLASSIFIER_TOLERANCE)
        return classifier.State()

    assert classify(geometry.centre) == TopAbs_OUT, (
        "the box centre is the sample that is far from the boundary"
    )
    assert {classify(corner) for corner in geometry.points} == {TopAbs_IN, TopAbs_OUT}, (
        "OCCT used to call one corner of this probe IN although BRepExtrema puts all eight 1e-6 "
        "outside the shell and the boolean answers 0.0; if it no longer does, re-measure the "
        "classifier claims in quiddity._volume_probe"
    )

    assert intersection_volume(solid.intersect(probe)) == 0.0
    assert probe_volume(solid, probe, properties=SolidProperties()) == 0.0


def test_each_body_of_a_multi_solid_part_is_short_circuited_on_its_own(monkeypatch) -> None:
    """Per solid and summed, exactly as the boolean loop it replaces."""

    bodies = (Box(20.0, 20.0, 20.0), Pos(40.0, 0.0, 0.0) * Box(10.0, 10.0, 10.0))
    part = Compound([*bodies, *_loose_geometry()])
    probe = Pos(4.0, 0.0, 0.0) * Box(4.0, 4.0, 4.0)  # buried in the first, clear of the second
    expected = sum(intersection_volume(body.intersect(probe)) for body in bodies)
    assert expected == pytest.approx(64.0)

    counted = _count_booleans(monkeypatch)
    assert probe_volume(part, probe, properties=SolidProperties()) == expected
    assert counted == []


def test_a_repeated_probe_returns_the_identical_float_without_asking_again(monkeypatch) -> None:
    """The memo, on the case it exists for: the same region asked twice in one run.

    The second probe is a *different shape object* built from the same numbers, which is what a
    repeat looks like in a run -- two families reaching the same conclusion by different routes.
    """

    body = Box(20.0, 20.0, 20.0)
    memo = SolidProperties()
    counted = _count_booleans(monkeypatch)

    first = probe_volume(body, Pos(10.0, 0.0, 0.0) * Box(4.0, 4.0, 4.0), properties=memo)
    second = probe_volume(body, Pos(10.0, 0.0, 0.0) * Box(4.0, 4.0, 4.0), properties=memo)

    assert first == second  # identical, not merely close
    assert len(counted) == 1


def test_a_probe_of_a_different_shape_is_not_a_repeat(monkeypatch) -> None:
    """The memo is keyed on the shape asked about as well as on the probe."""

    memo = SolidProperties()
    counted = _count_booleans(monkeypatch)
    probes = (Pos(10.0, 0.0, 0.0) * Box(4.0, 4.0, 4.0) for _ in range(2))

    straddling = probe_volume(Box(20.0, 20.0, 20.0), next(probes), properties=memo)
    taller = probe_volume(Box(20.0, 20.0, 30.0), next(probes), properties=memo)

    assert straddling == taller == pytest.approx(32.0)
    assert len(counted) == 2


def test_a_probe_with_no_run_to_share_with_asks_the_kernel(monkeypatch) -> None:
    """The documented cost gate: the short-circuits need a run's cache to pay for themselves.

    Without one, every probe would build the solid's face boxes and its point classifier and
    then throw them away -- measured, that made the corpus almost three times slower. A caller
    with no run gets the plain boolean, which is what it always got.
    """

    body = Box(20.0, 20.0, 20.0)
    probe = Pos(30.0, 0.0, 0.0) * Box(4.0, 4.0, 4.0)

    counted = _count_booleans(monkeypatch)
    assert probe_volume(body, probe) == 0.0
    assert len(counted) == 1


def test_the_solids_boundary_boxes_are_walked_once_for_a_run(monkeypatch) -> None:
    """Both per-solid values hang off the run's cache, so a second probe pays for neither."""

    body = _tube()
    memo = SolidProperties()
    walks = 0
    original = Solid.faces

    def counted(self):
        nonlocal walks
        walks += 1
        return original(self)

    monkeypatch.setattr(Solid, "faces", counted)
    for _ in range(3):
        assert probe_volume(body, Pos(0.0, 0.0, 0.0) * Box(2.0, 2.0, 2.0), properties=memo) == 0.0
    assert walks == 1
