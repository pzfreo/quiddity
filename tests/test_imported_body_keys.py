"""One physical solid keeps one correlation key despite STEP construction geometry."""

from dataclasses import fields, is_dataclass

import pytest
from build123d import Box, Compound, Edge, Pos, Rot, export_step

from quiddity import (
    FramedRecognitionResult,
    build_framed_recognition_result,
    build_raw_recognition_result,
    import_step_geometry,
    recognise_double_d_bores,
    recognise_face_levels,
    recognise_polygonal_bosses,
    recognise_rectangular_pads,
)
from quiddity._body_identity import body_signature
from quiddity._recess_features import recognise_pockets, recognise_slots
from quiddity._solid_properties import SolidProperties
from tests.golden.double_d_bore.fixture import build_fixture as bore_part
from tests.golden.plates_pads_levels_and_slanted_steps.fixture import build_fixture as pad_part
from tests.golden.polygonal_boss.fixture import build_fixture as boss_part


def _with_construction_edges(solid, reverse=False):
    children = [solid, Edge.make_line((-1000, -800, -600), (1000, 800, 600))]
    return Compound(children[::-1] if reverse else children)


def _slotted_plate():
    return Box(100, 70, 10) - Pos(-20, 0, 0) * Box(10, 25, 20)


def _pocketed_plate():
    return Box(100, 70, 10) - Pos(-20, 0, 5) * Box(10, 25, 10)


@pytest.mark.parametrize("reverse", [False, True])
@pytest.mark.parametrize(
    ("fixture", "recognise"),
    [(_slotted_plate, recognise_slots), (_pocketed_plate, recognise_pockets)],
)
def test_direct_recess_keys_join_other_families_on_one_solid(fixture, recognise, reverse):
    solid = fixture().solids()[0]
    part = _with_construction_edges(solid, reverse)
    assert body_signature(part) != body_signature(solid)
    records = recognise(part)
    levels = recognise_face_levels(part)
    assert records and levels
    assert records == recognise(solid)
    assert {record.body_key for record in [*records, *levels]} == {body_signature(solid)}
    memo = SolidProperties()
    assert body_signature(solid, properties=memo) == records[0].body_key


def _keys(value):
    if is_dataclass(value):
        for field in fields(value):
            member = getattr(value, field.name)
            if field.name == "body_key":
                yield type(value).__name__, member
            else:
                yield from _keys(member)
    elif isinstance(value, (tuple, list)):
        for member in value:
            yield from _keys(member)


@pytest.mark.parametrize(
    "name", ["nist_ctc_01_asme1_rd.stp", "nist_ctc_02_asme1_rc.stp", "nist_ctc_03_asme1_rc.stp"]
)
def test_imported_nist_aggregate_has_one_physical_body_key(name):
    part = import_step_geometry(f"tests/corpus/nist/{name}")
    (solid,) = part.solids()
    assert body_signature(part) != body_signature(solid)
    result = build_raw_recognition_result(part)
    keys = list(_keys(result))
    assert len({family for family, _ in keys}) >= (1 if "ctc_02" in name else 2)
    assert {key for _, key in keys} == {body_signature(solid)}


def test_step_round_trip_preserves_cross_family_join(tmp_path):
    path = tmp_path / "cluttered-slot.step"
    assert export_step(_with_construction_edges(_slotted_plate()), path)
    imported = import_step_geometry(path)
    (solid,) = imported.solids()
    assert body_signature(imported) != body_signature(solid)
    slots = recognise_slots(imported)
    levels = recognise_face_levels(imported)
    assert slots and levels
    assert {record.body_key for record in [*slots, *levels]} == {body_signature(solid)}


def test_framed_aggregate_signs_the_local_solid_not_its_wrapper():
    part = Pos(13, 29, 41) * Rot(17, 31, 43) * _with_construction_edges(_slotted_plate())
    framed = build_framed_recognition_result(part)
    assert isinstance(framed, FramedRecognitionResult)
    keys = list(_keys(framed.result))
    assert keys
    assert {key for _, key in keys} == {body_signature(framed.part.solids()[0])}


@pytest.mark.parametrize(
    ("fixture", "recognise", "field"),
    [
        (pad_part, recognise_rectangular_pads, "pads"),
        (bore_part, recognise_double_d_bores, "double_d_bores"),
        (boss_part, recognise_polygonal_bosses, "polygonal_bosses"),
    ],
)
def test_other_solid_scoped_families_ignore_construction_edges(fixture, recognise, field):
    solid = fixture().solids()[0]
    expected = recognise(solid)
    assert expected
    part = _with_construction_edges(solid)
    assert recognise(part) == expected
    assert getattr(build_raw_recognition_result(part), field) == tuple(expected)
