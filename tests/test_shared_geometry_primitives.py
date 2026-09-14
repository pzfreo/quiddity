"""Policy-neutral primitives retain fragments and leave proof decisions to callers."""

import ast
from pathlib import Path
from types import SimpleNamespace

import pytest
from build123d import Box, Compound, Pos

import quiddity
from quiddity._geometry import (
    DIRECTION_NORM_EPS,
    cross,
    dot,
    unit,
    unit_or_none,
    without_negative_zero,
)
from quiddity._volume_probe import intersection_volume, material_fraction
from quiddity._wire_seed import wire_seed


@pytest.mark.parametrize(
    "result,expected",
    [
        (None, 0.0),
        ([], 0.0),
        (SimpleNamespace(volume=2.5), 2.5),
        ([SimpleNamespace(volume=2.5), SimpleNamespace(volume=4.0)], 6.5),
    ],
)
def test_boolean_volume_result_forms(result, expected):
    assert intersection_volume(result) == expected


def test_real_fragmented_material_probe_retains_all_solids():
    material = Compound(children=[Pos(-3, 0, 0) * Box(2, 2, 2), Pos(3, 0, 0) * Box(2, 2, 2)])
    probe = Box(10, 4, 4)
    assert material_fraction(material, probe) == pytest.approx(16 / 160)


def test_volume_errors_are_not_converted_to_empty_geometry():
    class BrokenShape:
        @property
        def volume(self):
            raise RuntimeError("kernel failed")

    with pytest.raises(RuntimeError, match="kernel failed"):
        intersection_volume(BrokenShape())
    with pytest.raises(TypeError):
        intersection_volume(object())
    with pytest.raises(AttributeError):
        intersection_volume([object()])


def test_fraction_retains_division_and_kernel_error_boundaries():
    part = SimpleNamespace(intersect=lambda _: None)
    with pytest.raises(ZeroDivisionError):
        material_fraction(part, SimpleNamespace(volume=0.0))

    def fail(_):
        raise ValueError("bad intersection")

    with pytest.raises(ValueError, match="bad intersection"):
        material_fraction(SimpleNamespace(intersect=fail), Box(1, 1, 1))


def test_wire_seed_uses_exact_shared_edge_occurrences_without_growing_region():
    """Only the neighbours carrying an edge of *this* wire, and none of the rest.

    The graph's own index answers "which neighbours meet me along this edge"; that it is built
    from exactly the paired shared occurrences the earlier scan read is pinned on real geometry
    in :mod:`tests.test_wire_seed_index`.
    """

    edge = object()
    other = object()
    graph = SimpleNamespace(
        neighbours_by_occurrence_edge=lambda _: {edge: ("wall",), other: ("unrelated",)}
    )
    assert wire_seed(graph, "mouth", SimpleNamespace(edges=lambda: [edge])) == frozenset({"wall"})
    assert wire_seed(graph, "mouth", SimpleNamespace(edges=lambda: [])) == frozenset()


def test_recognisers_share_the_same_seed_and_fraction_implementations():
    from quiddity import (
        _recess_core,
        _section_passages,
        edge_open_circular_recesses,
        edge_open_prismatic_recesses,
        prismatic_pockets,
    )

    assert _recess_core._inner_wire_seed is _section_passages._wire_seed is wire_seed
    assert prismatic_pockets._wire_seed is wire_seed
    assert _section_passages._material_fraction is material_fraction
    assert prismatic_pockets._material_fraction is material_fraction
    # The two edge-open families each carried their own copy until the probe was narrowed to
    # the part's solids; pinned here so the copies cannot quietly return and miss that.
    assert edge_open_prismatic_recesses._material_fraction is material_fraction
    assert edge_open_circular_recesses._material_fraction is material_fraction


def test_dot_is_exactly_rounded_where_the_builtin_sum_is_not():
    """`dot` uses `math.fsum`, which matters exactly where a dot product cancels.

    Three of the nine copies this replaced already used fsum and six used the builtin, so the
    accuracy of a comparison depended on which module owned the helper. This is a case that
    separates them: left-to-right summation keeps the cancellation error, fsum does not.
    """

    left = (1.0, 1e100, 1.0)
    right = (1.0, 1.0, 1.0)
    assert sum(a * b for a, b in zip(left, right, strict=True)) == 1e100
    assert dot(left, right) == 1e100 + 2.0


def test_cross_is_right_handed():
    assert cross((1.0, 0.0, 0.0), (0.0, 1.0, 0.0)) == (0.0, 0.0, 1.0)
    assert cross((0.0, 1.0, 0.0), (1.0, 0.0, 0.0)) == (0.0, 0.0, -1.0)


@pytest.mark.parametrize(
    "vector",
    [
        (0.0, 0.0, 0.0),
        (DIRECTION_NORM_EPS, 0.0, 0.0),
        (float("nan"), 0.0, 0.0),
        (float("inf"), 0.0, 0.0),
    ],
)
def test_degenerate_and_nonfinite_directions_are_refused(vector):
    """One threshold, both spellings. The four copies replaced here disagreed: they refused at
    0.0, at 1e-9 and at 1e-12 (twice), so the same question had three different answers."""

    assert unit_or_none(vector) is None
    with pytest.raises(ValueError):
        unit(vector)


def test_a_direction_just_above_the_threshold_still_normalises():
    normalised = unit((DIRECTION_NORM_EPS * 2.0, 0.0, 0.0))
    assert normalised == (1.0, 0.0, 0.0)


def test_normalisation_and_negative_zero_removal_stay_distinct_operations():
    """`without_negative_zero` was itself called `_unit` while four modules used that name for
    genuine normalisation. It scales nothing; pinned so the two cannot merge by name again."""

    assert without_negative_zero((-0.0, 3.0, -0.0)) == (0.0, 3.0, 0.0)
    assert unit((0.0, 3.0, 0.0)) == (0.0, 1.0, 0.0)


def test_no_module_carries_its_own_direction_primitive():
    """`_dot` appeared in nine modules, `_cross` in five and `_unit` in four. Pinned so the
    copies cannot quietly return and re-diverge."""

    package = Path(quiddity.__file__).parent
    for path in package.glob("*.py"):
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
        local = {
            node.name
            for node in tree.body
            if isinstance(node, ast.FunctionDef)
            and node.name in {"_dot", "_cross", "_unit"}
        }
        assert not local, f"{path.name} redefines {sorted(local)}"
