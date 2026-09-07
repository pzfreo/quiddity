"""Regression checks for exact support union and supported kernel return shapes."""

import builtins
import sys
from pathlib import Path

import pytest
from build123d import Face, Pos, Rectangle, ShapeList, import_step

from quiddity import _support_patches as support_patches
from quiddity._support_patches import covered_patch
from quiddity.census import feature_census


@pytest.mark.parametrize("return_kind", ["native", "fragments", "none_when_empty"])
def test_exact_support_union_handles_kernel_result_forms(monkeypatch, return_kind):
    original = Face.cut

    def adapted(face, *others, **kwargs):
        result = original(face, *others, **kwargs)
        if return_kind == "native":
            return result
        if isinstance(result, ShapeList):
            faces = [f for shape in result for f in shape.faces()]
        else:
            faces = [] if result is None else list(result.faces())
        if return_kind == "none_when_empty" and not faces:
            return None
        return ShapeList(faces)

    monkeypatch.setattr(Face, "cut", adapted)
    patch = Rectangle(10, 10).face()
    middle = Rectangle(2, 10).face()
    left = (Pos(-3, 0, 0) * Rectangle(4, 10)).face()
    right = (Pos(3, 0, 0) * Rectangle(4, 10)).face()
    assert covered_patch(patch, (middle, left, right))
    assert not covered_patch(patch, (middle, left))
    assert not covered_patch(patch, (left, left, left))
    assert not covered_patch(patch, ())


def test_support_union_preserves_a_small_real_gap():
    patch = Rectangle(10, 10).face()
    left = (Pos(-2.5005, 0, 0) * Rectangle(4.999, 10)).face()
    right = (Pos(2.5005, 0, 0) * Rectangle(4.999, 10)).face()
    assert not covered_patch(patch, (left, right))


# The bounding-box rejection: cutting a fragment by a support it cannot reach is a no-op the
# kernel charges full price for, and the swept-prism proof in
# ``quiddity._section_recess_geometry`` makes about (walls + 2)^2 of them per candidate. The
# tests below hold the rejection to the two things it promises: the same answer, and fewer cuts.


def _without_the_rejection(monkeypatch) -> None:
    """Cut every fragment by every support, as ``covered_patch`` did before the rejection."""

    monkeypatch.setattr(support_patches, "_separated", lambda one, other: False)


def _traced(monkeypatch) -> list[float]:
    """The remaining-area sum the loop evaluates after each support, in order.

    ``covered_patch`` returns only a bool, and the sequence of sums is the part that has to be
    identical -- it is what the early exit is tested against, one support at a time. Shadowing
    the module's ``sum`` records exactly the value the loop saw, without the production code
    growing a hook for the test's benefit.
    """

    trace: list[float] = []

    def recording(values):
        total = builtins.sum(values)
        trace.append(total)
        return total

    monkeypatch.setattr(support_patches, "sum", recording, raising=False)
    return trace


def _cut_count(monkeypatch) -> list[int]:
    """``Face.cut`` calls made directly by ``covered_patch``.

    Narrower than "Booleans", deliberately: a fragment that came back as a ``Compound`` would
    cut through ``Shape.cut`` and go uncounted. Every fragment on the parts here is a ``Face``,
    and a count that drifts either way is the regression this is here to catch.
    """

    counted: list[int] = []
    original = Face.cut

    def counting(face, *others, **kwargs):
        if sys._getframe(1).f_code is covered_patch.__code__:
            counted.append(1)
        return original(face, *others, **kwargs)

    monkeypatch.setattr(Face, "cut", counting)
    return counted


#: An overlapping cover, a cover whose supports only touch at their shared boundary, and a
#: support the patch cannot reach at all. The middle case is the one the rejection must not
#: take: abutting boxes are not separated, so the Boolean still runs.
_CASES = {
    "overlapping": lambda: (
        Rectangle(10, 10).face(),
        (
            Rectangle(2, 10).face(),
            (Pos(-3, 0, 0) * Rectangle(4, 10)).face(),
            (Pos(3, 0, 0) * Rectangle(4, 10)).face(),
        ),
    ),
    "touching_at_boundary": lambda: (
        Rectangle(10, 10).face(),
        (
            (Pos(-2.5, 0, 0) * Rectangle(5, 10)).face(),
            (Pos(2.5, 0, 0) * Rectangle(5, 10)).face(),
        ),
    ),
    "disjoint": lambda: (
        Rectangle(10, 10).face(),
        (
            (Pos(40, 0, 0) * Rectangle(4, 10)).face(),
            (Pos(-2.5, 0, 0) * Rectangle(5, 10)).face(),
            (Pos(0, 40, 0) * Rectangle(4, 10)).face(),
            (Pos(2.5, 0, 0) * Rectangle(5, 10)).face(),
        ),
    ),
    "disjoint_and_never_covered": lambda: (
        Rectangle(10, 10).face(),
        (
            (Pos(40, 0, 0) * Rectangle(4, 10)).face(),
            (Pos(-3, 0, 0) * Rectangle(4, 10)).face(),
        ),
    ),
}


@pytest.mark.parametrize("case", sorted(_CASES))
def test_the_rejection_returns_the_same_bool_and_the_same_area_trace(case):
    patch, supports = _CASES[case]()

    with pytest.MonkeyPatch.context() as rejecting:
        trace_on = _traced(rejecting)
        result_on = covered_patch(patch, supports)

    with pytest.MonkeyPatch.context() as plain:
        _without_the_rejection(plain)
        trace_off = _traced(plain)
        result_off = covered_patch(patch, supports)

    assert result_on == result_off
    assert trace_on == trace_off


def test_a_support_the_patch_cannot_reach_costs_no_boolean(monkeypatch):
    """The rejection is exact, so it must actually fire on the case it exists for."""

    patch, supports = _CASES["disjoint"]()
    counted = _cut_count(monkeypatch)
    assert covered_patch(patch, supports)
    rejected = len(counted)

    counted.clear()
    _without_the_rejection(monkeypatch)
    assert covered_patch(patch, supports)
    assert rejected < len(counted)


def test_a_touching_support_is_still_cut(monkeypatch):
    """Boxes that abut are not separated: the conservative side of the test."""

    patch, supports = _CASES["touching_at_boundary"]()
    counted = _cut_count(monkeypatch)
    assert covered_patch(patch, supports)
    assert len(counted) == 2


#: The sentinel part and its measured ``Face.cut`` count inside ``covered_patch``.
#:
#: ``nist_ctc_01`` is a NIST part whose mixed-section recesses drive the swept-prism proof, the
#: caller that made 1516 of the 13.1 s profile's ``Face.cut`` calls. One census of it made
#: **382** of them before the bounding-box rejection and makes those below after; across the
#: six largest NIST parts the same change took 1536 down to 664.
#:
#: Chosen for separation, not only for cost. ``nist_ftc_10`` -- the sentinel part in
#: ``test_solid_properties`` -- makes 12 of these with the rejection and 12 without, so it
#: cannot pin anything; ``nist_ftc_07`` makes none at all. The only cheaper NIST part that
#: separates the counts widely is ``nist_ctc_03`` (172 against 394) and it saves 0.1 s of a
#: 1.0 s test, which does not pay for re-pinning the constants.
_SENTINEL_PART = Path(__file__).parent / "corpus" / "nist" / "nist_ctc_01_asme1_rd.stp"
_SUPPORT_CUTS_BEFORE = 382
_SUPPORT_CUTS = 164


def test_one_census_cuts_no_more_patches_than_it_needs(monkeypatch):
    """An operation-count sentinel, not a wall-clock one.

    The regression this guards is silent: a caller that grows another support, or a rejection
    that stops rejecting, keeps returning the right answer and simply pays the kernel again.
    """

    counted = _cut_count(monkeypatch)
    feature_census(import_step(_SENTINEL_PART))

    assert len(counted) == _SUPPORT_CUTS, (
        f"a census of {_SENTINEL_PART.name} cut {len(counted)} patches inside covered_patch; "
        f"{_SUPPORT_CUTS} is the count after the disjoint-support rejection and "
        f"{_SUPPORT_CUTS_BEFORE} was the count before it. A rise means a caller started proving "
        "supports it cannot reach; a fall means this sentinel needs updating."
    )
