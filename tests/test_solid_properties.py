# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

"""The run's whole-solid query cache: same answers, asked once.

The cache exists for one reason and must be held to it. It must return exactly what the kernel
returns -- these were 48% of a run on the largest pinned part, and a cheaper answer would have
been a behaviour change wearing a performance change's clothes -- and it must actually stop the
repeat asks.
"""

from __future__ import annotations

import pytest
from build123d import Box, Compound, Pos
from OCP.BRepBndLib import BRepBndLib

from quiddity._adjacency import FaceGraph
from quiddity._body_identity import body_signature, unambiguous_body_keys
from quiddity._solid_properties import SolidProperties, solid_properties


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
