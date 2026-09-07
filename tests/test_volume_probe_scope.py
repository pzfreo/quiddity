# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

"""What a volumetric probe is measured against: the part's solids, and only its solids.

An imported STEP part is routinely a compound holding the machined solid beside loose
construction geometry, and ``Compound.intersect`` distributes over every child of it. The
fragments a non-solid child can return have volume ``0.0`` by construction, so those booleans
could never move the answer -- which is exactly what the equality tests here pin, from both
directions: the narrowed measurement equals build123d's full distribution, and a whole census
is unchanged when the narrowing is monkeypatched away.

The operation-count sentinel at the bottom pins the other half: that the booleans really are
gone. Nothing here asserts wall-clock.
"""

from __future__ import annotations

from pathlib import Path

import pytest
from build123d import Box, Compound, Edge, Pos, Shell, Solid, Vertex, import_step
from build123d.topology.shape_core import Shape
from OCP.BRepAlgoAPI import BRepAlgoAPI_Common

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
    first = probe_solids(part, memo)
    second = probe_solids(part, memo)
    uncached = (probe_solids(part), probe_solids(part))

    assert first is second
    assert walks == 3  # once through the run's memo, once per call without one
    assert [solid.volume for solid in first] == [body.volume]
    assert all(len(target) == 1 for target in uncached)


def test_a_graph_supplies_the_run_cache_the_probes_use() -> None:
    part, _body = _one_body_with_clutter()
    graph = FaceGraph(part)
    assert probe_solids(part, graph) is probe_solids(part, graph.solid_properties)


#: The sentinel part and its measured boolean-build count inside volume probes.
#:
#: ``nist_ctc_01`` is a 139-face NIST part imported as one solid beside a compound of 78 loose
#: datum edges, so distributing one probe over every child cost **79** ``BRepAlgoAPI`` builds
#: instead of one. A census of it built 1772 booleans inside volume probes before this change
#: and builds the count below after it -- a whole recognition of the part ran 2154 boolean
#: builds in total, so this was the great majority of them.
#:
#: It counts only the builds *inside* a probe, deliberately. The exact-area support proof does
#: its own booleans on faces, and it is neither what this change touches nor what a regression
#: here would be about.
_SENTINEL_PART = CORPUS / "nist" / "nist_ctc_01_asme1_rd.stp"
_PROBE_BOOLEANS_BEFORE = 1772
_PROBE_BOOLEANS = 56


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
        f"{_PROBE_BOOLEANS} is the count after narrowing each probe to the part's solids and "
        f"{_PROBE_BOOLEANS_BEFORE} was the count before it. A rise means a probe is being handed "
        "a whole imported compound again; a fall means this sentinel needs updating."
    )


def test_a_whole_census_answers_exactly_what_it_answered_undistributed(monkeypatch) -> None:
    """The one property the narrowing must never buy its speed with.

    ``nist_ctc_01`` is the pinned part with the most loose construction geometry in the corpus,
    so it is the one where a wrongly dropped child would show.
    """

    part = import_step(_SENTINEL_PART)
    narrowed = feature_census(part)
    monkeypatch.setattr(_volume_probe, "probe_solids", lambda part, _properties=None: (part,))
    assert feature_census(part) == narrowed
