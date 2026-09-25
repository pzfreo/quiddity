"""Public capability claims must match the deliberately narrow implementation."""

import inspect
import re
from pathlib import Path

import pytest
from build123d import (
    Align,
    Box,
    Circle,
    Cylinder,
    Pos,
    Rectangle,
    RegularPolygon,
    Rot,
    extrude,
    loft,
)

import quiddity as recognition
from quiddity import (
    recognise_double_d_bores,
    recognise_polygonal_bosses,
    recognise_polygonal_stock,
)
from quiddity._record import Record
from quiddity.profiled_bores import read_double_d_tool

ROOT = Path(__file__).parents[1]
_CENTRE = (Align.CENTER, Align.CENTER, Align.CENTER)


def test_capability_table_covers_every_public_recogniser_once() -> None:
    documentation = (ROOT / "docs" / "capabilities.md").read_text(encoding="utf-8")
    documented = re.findall(r"^\| `(recognise_[a-z0-9_]+)` \|", documentation, re.MULTILINE)
    exported = sorted(name for name in recognition.__all__ if name.startswith("recognise_"))

    assert sorted(documented) == exported
    assert len(documented) == len(set(documented)), "duplicate capability-table row"


def test_record_audit_covers_every_public_record_once() -> None:
    documentation = (ROOT / "docs" / "capabilities.md").read_text(encoding="utf-8")
    documented = re.findall(r"^\| `([A-Z][A-Za-z0-9]+)` \|", documentation, re.MULTILINE)
    exported = sorted(
        name
        for name in recognition.__all__
        if inspect.isclass(candidate := getattr(recognition, name))
        and issubclass(candidate, Record)
        and candidate is not Record
    )

    assert sorted(documented) == exported
    assert len(documented) == len(set(documented)), "duplicate public-record audit row"


def _polygonal_prism(side_count: int):
    return extrude(RegularPolygon(20, side_count), 30)


def test_polygonal_bosses_are_square_or_hexagonal_with_hex_stock() -> None:
    square = _polygonal_prism(4)
    assert [
        record.side_count
        for record in recognise_polygonal_bosses(Box(100, 80, 10) + Pos(0, 0, 5) * square)
    ] == [4]
    assert recognise_polygonal_stock(square) == []

    octagon = _polygonal_prism(8)
    assert recognise_polygonal_bosses(Box(100, 80, 10) + Pos(0, 0, 5) * octagon) == []
    assert recognise_polygonal_stock(octagon) == []

    x_axis_prism = Rot(0, 90, 0) * _polygonal_prism(6)
    x_axis_boss = Box(10, 100, 80) + Pos(5, 0, 0) * x_axis_prism
    assert [record.axis for record in recognise_polygonal_bosses(x_axis_boss)] == ["x"]
    assert [record.axis for record in recognise_polygonal_stock(x_axis_prism)] == ["x"]


def _double_d_tool(height: float):
    return Cylinder(5, height, align=_CENTRE) & Box(7.2, 20, 2 * height, align=_CENTRE)


def _tapered_double_d_tool(height: float):
    sketch_align = (Align.CENTER, Align.CENTER)
    low = Circle(5) & Rectangle(7.2, 20, align=sketch_align)
    high = Pos(0, 0, height) * (Circle(6) & Rectangle(8, 20, align=sketch_align))
    return Pos(0, 0, -height / 2) * loft([low, high])


@pytest.mark.parametrize(
    ("rotation", "expected_axis"),
    [
        (Rot(), (0.0, 0.0, 1.0)),
        (Rot(0, 90, 0), (1.0, 0.0, 0.0)),
        (Rot(90, 0, 0), (0.0, 1.0, 0.0)),
    ],
)
def test_profiled_bore_supports_each_principal_axis(rotation, expected_axis) -> None:
    plate = Box(30, 30, 10, align=_CENTRE) - _double_d_tool(20)

    assert [bore.axis for bore in recognise_double_d_bores(rotation * plate)] == [expected_axis]


def test_profiled_bores_are_owned_independently_per_solid() -> None:
    plate = Box(30, 30, 10, align=_CENTRE) - _double_d_tool(20)
    assembly = Pos(-25, 0, 0) * plate + Pos(25, 0, 0) * plate

    bores = recognise_double_d_bores(assembly)

    assert len(bores) == 2
    assert [round(bore.location[0]) for bore in bores] == [-25, 25]


@pytest.mark.parametrize(
    ("rotation", "expected_axis"),
    [(Rot(), "z"), (Rot(0, 90, 0), "x"), (Rot(90, 0, 0), "y")],
)
def test_double_d_declaration_reader_supports_each_principal_axis(rotation, expected_axis) -> None:
    axis, major, across, _center, depth, _flat_direction = read_double_d_tool(
        rotation * _double_d_tool(20)
    )

    assert axis == expected_axis
    assert major == 10.0
    assert across == 7.2
    assert depth == pytest.approx(20.0)


def test_profiled_bore_rejects_blind_double_d_and_through_obround() -> None:
    plate = Box(30, 30, 10, align=_CENTRE)
    blind_double_d = plate - Pos(0, 0, 3) * _double_d_tool(4)
    opposed_blind_recesses = (
        plate - Pos(0, 0, 3) * _double_d_tool(4) - Pos(0, 0, -3) * _double_d_tool(4)
    )

    straight = 8.0
    obround = (
        Box(straight, 6, 20, align=_CENTRE)
        + Pos(straight / 2, 0, 0) * Cylinder(3, 20, align=_CENTRE)
        + Pos(-straight / 2, 0, 0) * Cylinder(3, 20, align=_CENTRE)
    )
    through_obround = plate - obround

    assert recognise_double_d_bores(blind_double_d) == []
    assert recognise_double_d_bores(opposed_blind_recesses) == []
    assert recognise_double_d_bores(through_obround) == []

    with pytest.raises(ValueError, match="constant extrusion"):
        read_double_d_tool(obround)
    with pytest.raises(ValueError, match="constant extrusion"):
        read_double_d_tool(_double_d_tool(20) + Pos(8, 0, 0) * Box(2, 2, 2, align=_CENTRE))


def test_profiled_bore_rejects_tapered_and_non_principal_profiles() -> None:
    plate = Box(30, 30, 10, align=_CENTRE)
    tapered = _tapered_double_d_tool(20)

    # Matching topology at the two openings is not enough: their unequal circle/chord metrics
    # prove this is a taper, not the constant-profile feature represented by DoubleDBore.
    assert recognise_double_d_bores(plate - tapered) == []
    with pytest.raises(ValueError, match="constant extrusion"):
        read_double_d_tool(tapered)

    # The public contract is deliberately principal-axis-only. Rotating a valid bore must fail
    # closed instead of snapping its axis or dimensions to the nearest drawing plane.
    tilted = Rot(0, 15, 0) * (plate - _double_d_tool(20))
    assert recognise_double_d_bores(tilted) == []
