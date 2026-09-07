# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

"""The run's whole-solid query cache: same answers, asked once.

The cache exists for one reason and must be held to it. It must return exactly what the kernel
returns -- these were 48% of a run on the largest pinned part, and a cheaper answer would have
been a behaviour change wearing a performance change's clothes -- and it must actually stop the
repeat asks, which is what the operation-count sentinel at the bottom pins.
"""

from __future__ import annotations

from pathlib import Path

import pytest
from build123d import Box, Compound, Pos, import_step
from OCP.BRepBndLib import BRepBndLib
from OCP.TopAbs import TopAbs_COMPOUND, TopAbs_COMPSOLID, TopAbs_SHELL, TopAbs_SOLID

from quiddity._adjacency import FaceGraph
from quiddity._body_identity import body_signature, unambiguous_body_keys
from quiddity._solid_properties import SolidProperties, solid_properties
from quiddity.census import feature_census
from quiddity.grooves import recognise_grooves

CORPUS = Path(__file__).parent / "corpus"


def _two_bodies() -> Compound:
    return Compound([Box(30, 20, 10), Pos(60, 0, 0) * Box(12, 12, 40)])


def test_every_cached_value_is_the_value_the_kernel_returns() -> None:
    part = _two_bodies()
    memo = SolidProperties()
    for solid in (part, *part.solids()):
        expected = solid.bounding_box()
        cached = memo.bounding_box(solid)
        assert (cached.min.X, cached.min.Y, cached.min.Z) == (
            expected.min.X,
            expected.min.Y,
            expected.min.Z,
        )
        assert (cached.max.X, cached.max.Y, cached.max.Z) == (
            expected.max.X,
            expected.max.Y,
            expected.max.Z,
        )
        assert memo.volume(solid) == float(solid.volume)
        assert memo.area(solid) == float(solid.area)
        assert memo.is_valid(solid) is bool(solid.is_valid)


def test_a_second_wrapper_for_one_solid_hits_the_same_entry(monkeypatch) -> None:
    """Identity is ``IsSame``, so two ``part.solids()`` calls must not pay twice.

    This is the whole mechanism: every recogniser walks the part's solids for itself, and each
    walk hands back fresh wrappers around the same ``TopoDS_Shape``.
    """

    part = _two_bodies()
    memo = SolidProperties()
    calls = 0
    original = BRepBndLib.AddOptimal_s

    def counted(*args, **kwargs):
        nonlocal calls
        calls += 1
        return original(*args, **kwargs)

    monkeypatch.setattr(BRepBndLib, "AddOptimal_s", staticmethod(counted))

    first, second = part.solids(), part.solids()
    assert first[0] is not second[0]
    for solid in (*first, *second, *first):
        memo.bounding_box(solid)
    assert calls == 2  # one per distinct solid, not one per wrapper


def test_distinct_solids_never_share_an_entry() -> None:
    part = _two_bodies()
    small, large = sorted(part.solids(), key=lambda solid: solid.volume)
    memo = SolidProperties()
    assert memo.volume(small) != memo.volume(large)
    assert memo.bounding_box(small).diagonal != memo.bounding_box(large).diagonal


def test_derived_values_are_memoised_per_name_and_per_solid() -> None:
    part = _two_bodies()
    memo = SolidProperties()
    seen: list[tuple[str, float]] = []

    def compute(label: str):
        def call(solid):
            seen.append((label, float(solid.volume)))
            return label

        return call

    solids = part.solids()
    assert memo.derived("a", solids[0], compute("a")) == "a"
    assert memo.derived("a", solids[0], compute("a")) == "a"
    assert len(seen) == 1
    assert memo.derived("b", solids[0], compute("b")) == "b"
    assert memo.derived("a", solids[1], compute("a")) == "a"
    assert len(seen) == 3


def test_the_resolver_shares_a_run_and_isolates_a_standalone_call() -> None:
    graph = FaceGraph(_two_bodies())
    assert solid_properties(graph) is graph.solid_properties
    existing = SolidProperties()
    assert solid_properties(existing) is existing
    first, second = solid_properties(None), solid_properties(None)
    assert isinstance(first, SolidProperties) and first is not second


def test_body_keys_are_identical_with_and_without_the_run_cache() -> None:
    part = _two_bodies()
    scopes = list(part.solids())
    assert unambiguous_body_keys(scopes, require_valid_solid=True) == unambiguous_body_keys(
        scopes, require_valid_solid=True, properties=SolidProperties()
    )
    assert body_signature(scopes[0]) == body_signature(scopes[0], properties=SolidProperties())


def _without_the_memo(monkeypatch) -> None:
    """Make every accessor ask the kernel again, as the code did before this cache existed."""

    monkeypatch.setattr(SolidProperties, "bounding_box", lambda _self, s: s.bounding_box())
    monkeypatch.setattr(SolidProperties, "is_valid", lambda _self, s: bool(s.is_valid))
    monkeypatch.setattr(SolidProperties, "volume", lambda _self, s: float(s.volume))
    monkeypatch.setattr(SolidProperties, "area", lambda _self, s: float(s.area))


def test_a_whole_census_answers_exactly_what_it_answered_uncached(monkeypatch) -> None:
    """The one property the cache must never buy its speed with."""

    part = import_step(CORPUS / "gramel" / "GRM-05_depth_lock_bolt.step")
    cached = feature_census(part)
    _without_the_memo(monkeypatch)
    assert feature_census(part) == cached


def test_a_standalone_recogniser_answers_the_same_without_a_run(monkeypatch) -> None:
    """The public entry points keep working with no graph to share, computing what they did."""

    part = import_step(CORPUS / "gramel" / "GRM-05_depth_lock_bolt.step")
    standalone = recognise_grooves(part)
    _without_the_memo(monkeypatch)
    assert recognise_grooves(part) == standalone


#: The sentinel part and its measured whole-solid box count.
#:
#: ``nist_ftc_10`` is a 214-face NIST part that exercises the families this cache serves --
#: grooves and the turned ladder (which asked one solid for its box once per shaft), the body
#: signature (recomputed per solid on each of the nine calls a run makes of it), and the twenty
#: recognisers that each asked the part for its box once. Before the cache it computed **65**
#: optimal boxes of whole solids in one census, and the census took 10.3 s; it now computes
#: those below, in 0.8 s.
#:
#: This counts only *whole-solid* boxes, deliberately. Face boxes are the cheap majority of the
#: kernel calls and belong to other work; mixing them in would make this fail for reasons that
#: have nothing to do with the property it guards.
_SENTINEL_PART = CORPUS / "nist" / "nist_ftc_10_asme1_rb.stp"
_WHOLE_SOLID_BOXES_BEFORE = 65
_WHOLE_SOLID_BOXES = 13

_WHOLE_BODY_TYPES = frozenset({TopAbs_SOLID, TopAbs_COMPOUND, TopAbs_COMPSOLID, TopAbs_SHELL})


def test_one_census_computes_no_more_whole_solid_boxes_than_it_needs(monkeypatch) -> None:
    """An operation-count sentinel, not a wall-clock one.

    The regression this guards against is silent: a new family, or one that stops threading the
    run's cache, simply asks the kernel for the same box again and the answers stay correct
    while the run gets slower. Counting the kernel call is the only way that shows up in CI.
    """

    counted: list[int] = []
    original = BRepBndLib.AddOptimal_s

    def count(shape, *args, **kwargs):
        if shape.ShapeType() in _WHOLE_BODY_TYPES:
            counted.append(1)
        return original(shape, *args, **kwargs)

    monkeypatch.setattr(BRepBndLib, "AddOptimal_s", staticmethod(count))
    feature_census(import_step(_SENTINEL_PART))

    assert len(counted) == _WHOLE_SOLID_BOXES, (
        f"a census of {_SENTINEL_PART.name} computed {len(counted)} whole-solid bounding boxes; "
        f"{_WHOLE_SOLID_BOXES} is the count after the run-scoped cache and "
        f"{_WHOLE_SOLID_BOXES_BEFORE} was the count before it. A rise means a consumer stopped "
        "going through quiddity._solid_properties; a fall means this sentinel needs updating."
    )


@pytest.mark.parametrize(
    "solid",
    [Box(10, 10, 10), Compound([Box(4, 4, 4)])],
)
def test_a_falsy_cached_value_is_still_a_cache_hit(solid) -> None:
    """``volume`` of an empty compound is ``0.0`` and validity can be ``False``.

    A membership test written as a truthiness test would recompute both forever, which is the
    one way a memo can be slower than no memo at all.
    """

    memo = SolidProperties()
    memo._volume[solid] = 0.0
    memo._valid[solid] = False
    memo._area[solid] = 0.0
    assert memo.volume(solid) == 0.0
    assert memo.is_valid(solid) is False
    assert memo.area(solid) == 0.0
