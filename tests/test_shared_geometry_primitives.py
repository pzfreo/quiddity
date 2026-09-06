"""Policy-neutral primitives retain fragments and leave proof decisions to callers."""

from types import SimpleNamespace

import pytest
from build123d import Box, Compound, Pos

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
    edge = object()
    other = object()
    graph = SimpleNamespace(
        neighbours=lambda _: ("wall", "unrelated"),
        shared_occurrences=lambda _, neighbour: (
            SimpleNamespace(edge=edge if neighbour == "wall" else other),
        ),
    )
    assert wire_seed(graph, "mouth", SimpleNamespace(edges=lambda: [edge])) == frozenset({"wall"})
    assert wire_seed(graph, "mouth", SimpleNamespace(edges=lambda: [])) == frozenset()


def test_recognisers_share_the_same_seed_and_fraction_implementations():
    from quiddity import _recess_core, _section_passages, prismatic_pockets

    assert _recess_core._inner_wire_seed is _section_passages._wire_seed is wire_seed
    assert prismatic_pockets._wire_seed is wire_seed
    assert _section_passages._material_fraction is material_fraction
    assert prismatic_pockets._material_fraction is material_fraction
