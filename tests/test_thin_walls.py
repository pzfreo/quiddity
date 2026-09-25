"""Body-level constant-wall evidence from solid geometry."""

from __future__ import annotations

import json
from pathlib import Path

import pytest
from build123d import Box, Compound, Cylinder, Pos, Rot

from quiddity import (
    build_recognition_result,
    import_step_geometry,
    recognise_thin_wall_bodies,
)
from quiddity.document import build_recognition_document
from quiddity.evidence import build_recognition_evidence


def _open_shell():
    return Box(100, 80, 40) - Box(94, 74, 40)


def test_open_shell_exposes_wall_pairs_and_leaves_mouth_faces_unpaired():
    shell = _open_shell()
    (record,) = recognise_thin_wall_bodies(shell)

    assert record.thickness == 3.0
    assert len(record.face_pairs) == 4
    assert len(record.unpaired_faces) == 2
    assert len(record.rim_regions) == 2
    assert record.history_hint.basis == "heuristic"
    assert record.history_hint.direction == "inward"
    assert len(record.history_hint.outer_faces) == 4
    assert len(record.history_hint.inner_faces) == 4
    assert record.history_hint.opening_rims == record.rim_regions
    assert record.history_hint.before_shell_collar_pairs == ()
    assert record.paired_area_fraction > 0.9
    assert (
        len({index for pair in record.face_pairs for index in (pair.first_face, pair.second_face)})
        == 8
    )
    aggregate = build_recognition_result(shell)
    assert aggregate.thin_wall_bodies == (record,)
    assert aggregate.plates == ()
    assert aggregate.risers == ()
    assert aggregate.bosses == ()
    assert all(pair.first_face != pair.second_face for pair in record.face_pairs)
    expected = json.loads(Path(__file__).with_name("thin_wall_expected.json").read_text())
    assert {
        "thickness": record.thickness,
        "paired_face_count": len(
            {index for pair in record.face_pairs for index in (pair.first_face, pair.second_face)}
        ),
        "mouth_face_count": len(record.unpaired_faces),
        "rim_region_count": len(record.rim_regions),
        "paired_area_fraction": round(record.paired_area_fraction, 6),
    } == expected


def test_pairs_are_proven_per_solid_and_follow_input_face_roster():
    first = _open_shell()
    second = Pos(200, 0, 0) * _open_shell()
    assembly = Compound([first, second])
    records = recognise_thin_wall_bodies(assembly)

    assert len(records) == 2
    assert {record.thickness for record in records} == {3.0}
    assert {record.body_index for record in records} == {0, 1}
    assert all(len(record.face_pairs) == 4 for record in records)
    sets = [
        {index for pair in record.face_pairs for index in (pair.first_face, pair.second_face)}
        for record in records
    ]
    assert sets[0].isdisjoint(sets[1])


def test_rotated_shell_keeps_thickness_and_exact_face_evidence():
    shell = Pos(17, -30, 8) * Rot(17, 23, 31) * _open_shell()
    (record,) = recognise_thin_wall_bodies(shell)
    assert abs(record.thickness - 3.0) < 1e-5

    view = build_recognition_evidence(shell)
    feature = next(ref for ref in view.features if view.family(ref) == "thin_wall_bodies")
    assert view.record(feature) == record
    claimed = view.defining_faces(feature)
    assert len(claimed) == 8
    assert len(view.constituent_faces(feature)) == 10
    assert all(view.face(face) for face in claimed)


def test_document_serializes_pairs_in_its_local_face_index_space():
    document = build_recognition_document(_open_shell())
    (feature,) = [f for f in document["features"] if f["family"] == "thin_wall_bodies"]
    record = feature["record"]
    indices = {
        index
        for pair in record["face_pairs"]
        for index in (pair["first_face"], pair["second_face"])
    }
    rim_indices = {index for region in record["rim_regions"] for index in region}
    assert indices == set(feature["defining_faces"])
    assert indices | rim_indices == set(feature["constituent_faces"])
    assert rim_indices == set(record["unpaired_faces"])
    json.dumps(document, allow_nan=False)


def test_solid_block_has_no_dominant_thin_wall():
    assert recognise_thin_wall_bodies(Box(100, 80, 40)) == []
    assert recognise_thin_wall_bodies(Box(100, 80, 3)) == []


def test_open_pipe_pairs_curved_skins_and_rim_faces():
    pipe = Cylinder(20, 100) - Cylinder(17, 100)
    (record,) = recognise_thin_wall_bodies(pipe)
    assert record.thickness == 3.0
    assert len(record.face_pairs) == 1
    assert len(record.unpaired_faces) == 2
    assert len(record.rim_regions) == 2


def test_unpaired_through_cut_is_heuristically_after_shell_and_not_an_opening():
    drilled = _open_shell() - Rot(0, 90, 0) * Cylinder(5, 120)
    (record,) = recognise_thin_wall_bodies(drilled)
    hint = record.history_hint
    assert hint.basis == "heuristic"
    assert len(hint.opening_rims) == 2
    assert len(hint.after_shell_cut_faces) == 2
    assert set(hint.after_shell_cut_faces).isdisjoint(
        {index for region in hint.opening_rims for index in region}
    )


@pytest.mark.slow
@pytest.mark.parametrize(
    ("case", "minimum_paired_fraction", "minimum_pairs"),
    (("cgb241", 0.99, 25), ("cgb207", 0.87, 45)),
)
def test_public_cadgenbench_shell_inputs_keep_imported_wall_evidence(
    case: str, minimum_paired_fraction: float, minimum_pairs: int
):
    source = Path(__file__).parent / "corpus" / "cadgenbench_inputs" / f"{case}.step"
    part = import_step_geometry(source)
    if case == "cgb241":
        aggregate = build_recognition_result(part)
        (record,) = aggregate.thin_wall_bodies
        assert aggregate.bosses == ()
        assert aggregate.risers == ()
        assert aggregate.plates == ()
        assert len(aggregate.holes) == 3
    else:
        (record,) = recognise_thin_wall_bodies(part)
    assert abs(record.thickness - 3.0) < 0.001
    assert record.paired_area_fraction >= minimum_paired_fraction
    assert len(record.face_pairs) >= minimum_pairs
    assert record.rim_regions
    assert record.history_hint.basis == "heuristic"
    assert record.history_hint.direction == "inward"
    if case == "cgb241":
        assert len(record.history_hint.outer_faces) == 25
        assert len(record.history_hint.inner_faces) == 25
        assert len(record.history_hint.before_shell_collar_pairs) == 6
        assert record.history_hint.opening_rims == record.rim_regions
