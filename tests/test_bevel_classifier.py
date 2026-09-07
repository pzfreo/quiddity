# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

"""The corner probe's point classifier: one per solid per run, same answers.

``BRepClass3d_SolidClassifier`` is two things in one object -- a loaded, indexed copy of the
solid, and a ``Perform`` that classifies a point against it. The bevel probes built a fresh one
per point, which paid the loading 54 times on the 664-face NIST part for 54 classifications and
measured 0.6 s of that part's run.

The classifier now comes from the run's whole-solid cache. What must not move is the *answer*,
including at the awkward points these probes deliberately choose -- a nudge off a virtual sharp
corner, near enough the boundary that a stale classifier state would show.
"""

from __future__ import annotations

from pathlib import Path

import pytest
from build123d import Axis, Box, Compound, Pos, Solid
from OCP.BRepClass3d import BRepClass3d_SolidClassifier

from quiddity import _bevel
from quiddity._bevel import _CLASSIFIER, _material_at, convex_bevel, material_beyond_corner
from quiddity._solid_properties import SolidProperties
from quiddity.angled_steps import recognise_angled_steps
from quiddity.census import feature_census
from quiddity.chamfers import recognise_chamfers
from quiddity.fillets import recognise_fillets
from quiddity.step_io import import_step_geometry

CORPUS = Path(__file__).parent / "corpus"

#: A NIST part whose chamfers, fillets and slants reach the corner probes 40 times in one census
#: -- enough to show the sharing, and a third of the cost of the 664-face part the analysis used.
_SENTINEL_PART = CORPUS / "nist" / "nist_ctc_01_asme1_rd.stp"
_CLASSIFIERS_BEFORE = 40
_CLASSIFIERS = 1


def _chamfered_part():
    """Two bodies, one with a bevelled corner, so the probes have a real inside and outside."""

    return Compound(
        [
            Box(30, 20, 10) - Pos(15, 0, 5) * Box(8, 30, 8).rotate(Axis.Y, 45),
            Pos(60, 0, 0) * Box(12, 12, 40),
        ]
    )


def _probe_points(part) -> list[tuple[float, float, float]]:
    """Points spanning material, vacuum and the boundary itself, on a lattice over the box."""

    box = part.bounding_box()
    steps = [i / 6 for i in range(-1, 8)]
    return [
        (
            box.min.X + fx * (box.max.X - box.min.X),
            box.min.Y + fy * (box.max.Y - box.min.Y),
            box.min.Z + fz * (box.max.Z - box.min.Z),
        )
        for fx in steps
        for fy in steps
        for fz in steps
    ]


def test_a_reused_classifier_answers_what_a_fresh_one_answers() -> None:
    """The property the whole change rests on, checked point by point.

    ``Perform`` is documented as the repeatable half; this is the measurement that says so for
    the states these probes care about, including points sitting exactly on a face.
    """

    part = _chamfered_part()
    points = _probe_points(part)
    memo = SolidProperties()
    fresh = [_material_at(part, point) for point in points]
    shared = [_material_at(part, point, properties=memo) for point in points]
    assert shared == fresh
    assert any(fresh) and not all(fresh)  # the lattice must straddle the boundary
    # And the order of the questions must not matter either.
    assert [_material_at(part, point, properties=memo) for point in reversed(points)] == list(
        reversed(fresh)
    )


def test_each_shape_asked_about_gets_its_own_classifier() -> None:
    """Two shapes are two entries, and must not answer for each other.

    Production always asks about the whole part, so this is the property that keeps that from
    being load-bearing: the cache is keyed on the shape handed in, whatever it is.
    """

    memo = SolidProperties()
    left, right = Box(10, 10, 10), Pos(50, 0, 0) * Box(10, 10, 10)
    inside_left = (0.0, 0.0, 0.0)
    inside_right = (50.0, 0.0, 0.0)
    assert _material_at(left, inside_left, properties=memo)
    assert not _material_at(left, inside_right, properties=memo)
    assert _material_at(right, inside_right, properties=memo)
    assert not _material_at(right, inside_left, properties=memo)


def test_a_reversed_solid_never_shares_the_forward_solid_s_classifier() -> None:
    """The orientation half of the cache key, from the side that needs it most.

    ``IsSame`` ignores orientation, so a solid and its reverse are ``==`` and hash equal. A
    classifier does not ignore it: reversing *inverts* ``IN`` and ``OUT``, rather than flipping
    a sign the way ``volume`` does. Keyed on the wrapper alone these two would share one entry
    and one of them would read the other's answer, so this pins that they do not -- and pins it
    through a shared cache, which is the only place it could go wrong.
    """

    forward = Box(10, 10, 10)
    reverse = Solid(forward.wrapped.Reversed())
    assert forward == reverse and hash(forward) == hash(reverse)  # IsSame, orientation ignored

    memo = SolidProperties()
    inside, outside = (0.0, 0.0, 0.0), (99.0, 0.0, 0.0)
    for point in (inside, outside):
        shared_forward = _material_at(forward, point, properties=memo)
        shared_reverse = _material_at(reverse, point, properties=memo)
        assert shared_forward != shared_reverse
        assert shared_forward == _material_at(forward, point)
        assert shared_reverse == _material_at(reverse, point)
    assert _material_at(forward, inside, properties=memo)  # and the entries stay put


def _counted_classifiers(monkeypatch) -> list[int]:
    built: list[int] = []
    real = BRepClass3d_SolidClassifier

    def counted(shape):
        built.append(1)
        return real(shape)

    monkeypatch.setattr(_bevel, "BRepClass3d_SolidClassifier", counted)
    return built


def test_a_probe_without_a_run_builds_its_own_classifier(monkeypatch) -> None:
    """The standalone path is unchanged: no run to share with, so no sharing."""

    built = _counted_classifiers(monkeypatch)
    part = Box(20, 20, 20)
    centre = {0: 5.0, 1: 5.0, 2: 5.0}
    neigh = {0: 10.0, 1: 10.0, 2: 0.0}
    assert convex_bevel(part, centre, 2, neigh) is convex_bevel(part, centre, 2, neigh)
    assert material_beyond_corner(part, centre, 2, neigh) is material_beyond_corner(
        part, centre, 2, neigh
    )
    assert len(built) == 4


def test_a_shared_cache_loads_each_solid_once(monkeypatch) -> None:
    built = _counted_classifiers(monkeypatch)
    memo = SolidProperties()
    part = Box(20, 20, 20)
    centre = {0: 5.0, 1: 5.0, 2: 5.0}
    neigh = {0: 10.0, 1: 10.0, 2: 0.0}
    for _ in range(3):
        convex_bevel(part, centre, 2, neigh, properties=memo)
        material_beyond_corner(part, centre, 2, neigh, properties=memo)
    assert len(built) == 1


def _without_the_shared_classifier(monkeypatch) -> None:
    """Make every probe load the shape again, as the code did before the cache existed.

    Keyed on this module's name so it unshares only the classifier: ``derived`` is class-level
    machinery other modules in this series hang their own values off, and a blanket passthrough
    would quietly be testing theirs too.
    """

    original = SolidProperties.derived

    def unshared(self, name, solid, compute):
        if name == _CLASSIFIER:
            return compute(solid)
        return original(self, name, solid, compute)

    monkeypatch.setattr(SolidProperties, "derived", unshared)


@pytest.mark.parametrize(
    "recognise", [recognise_chamfers, recognise_angled_steps, recognise_fillets]
)
def test_each_bevel_family_answers_the_same_without_the_shared_classifier(
    recognise, monkeypatch
) -> None:
    part = import_step_geometry(_SENTINEL_PART)
    shared = recognise(part)
    _without_the_shared_classifier(monkeypatch)
    assert recognise(part) == shared


def test_one_census_loads_each_solid_into_one_classifier(monkeypatch) -> None:
    """An operation-count sentinel, not a wall-clock one.

    A family that stops threading the run's cache goes back to loading the solid per point, and
    every answer stays right while the run gets slower -- counting the constructions is the only
    way that shows up in CI.
    """

    built = _counted_classifiers(monkeypatch)
    feature_census(import_step_geometry(_SENTINEL_PART))

    assert len(built) == _CLASSIFIERS, (
        f"a census of {_SENTINEL_PART.name} loaded {len(built)} solid classifiers; "
        f"{_CLASSIFIERS} is the count once the run shares one per solid, and "
        f"{_CLASSIFIERS_BEFORE} was the count when every corner probe built its own. A rise "
        "means a probe stopped passing the run's quiddity._solid_properties cache; a fall "
        "means this sentinel needs updating."
    )
