"""Body-level constant-wall evidence from solid geometry."""

from __future__ import annotations

import json
from pathlib import Path

import pytest
from build123d import Box, Compound, Cylinder, Pos, Rot

from quiddity import (
    UnpairedWallFace,
    WallFacePair,
    build_recognition_result,
    import_step_geometry,
    recognise_thin_wall_bodies,
)
from quiddity.document import build_recognition_document
from quiddity.evidence import build_recognition_evidence


def _open_shell():
    return Box(100, 80, 40) - Box(94, 74, 40)


@pytest.mark.timeout(120)
def test_open_shell_with_two_thicknesses_reports_each_wall_pair() -> None:
    shell = Box(100, 80, 40) - Pos(0, 0, 4) * Box(96, 76, 40)

    (record,) = recognise_thin_wall_bodies(shell)

    assert record.thickness == 2.0
    assert len(record.face_pairs) == 5
    assert {round(pair.thickness, 6) for pair in record.face_pairs} == {2.0, 4.0}
    assert record.paired_area_fraction > 0.85
    assert len(record.rim_regions) == 1
    assert record.history_hint.direction == "inward"
    assert len(record.history_hint.outer_faces) == len(record.history_hint.inner_faces) == 5
    document = build_recognition_document(shell)
    (feature,) = [f for f in document["features"] if f["family"] == "thin_wall_bodies"]
    assert {pair["thickness"] for pair in feature["record"]["face_pairs"]} == {2.0, 4.0}


def test_wall_pair_rejects_nonphysical_thickness() -> None:
    with pytest.raises(ValueError, match="positive and finite"):
        WallFacePair(0, 1, 0.0)
    with pytest.raises(ValueError, match="positive and finite"):
        WallFacePair(0, 1, float("nan"))


def test_unpaired_wall_face_uses_closed_labels_and_source_indices() -> None:
    with pytest.raises(ValueError, match="closed kind"):
        UnpairedWallFace(0, "unknown")
    with pytest.raises(ValueError, match="nonnegative"):
        UnpairedWallFace(-1, "cut_edge")


def test_open_shell_exposes_wall_pairs_and_leaves_mouth_faces_unpaired():
    shell = _open_shell()
    (record,) = recognise_thin_wall_bodies(shell)

    assert record.thickness == 3.0
    assert {pair.thickness for pair in record.face_pairs} == {3.0}
    assert len(record.face_pairs) == 4
    assert len(record.unpaired_faces) == 2
    assert len(record.rim_regions) == 2
    assert {item.kind for item in record.unpaired_face_classes} == {"cut_edge"}
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
    classes = {item.face: item.kind for item in record.unpaired_face_classes}
    assert all(classes[index] == "cut_edge" for index in hint.after_shell_cut_faces)


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
    assert all(
        pair.thickness is not None and abs(pair.thickness - 3.0) < 0.02
        for pair in record.face_pairs
    )
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


@pytest.mark.slow
@pytest.mark.timeout(120)
def test_cgb207_joint_rounds_are_paired_and_every_remainder_is_classified() -> None:
    source = Path(__file__).parent / "corpus/cadgenbench_inputs/cgb207.step"
    part = import_step_geometry(source)
    (record,) = recognise_thin_wall_bodies(part)
    pairs = {frozenset((pair.first_face, pair.second_face)) for pair in record.face_pairs}
    assert record.paired_area_fraction >= 0.894
    assert len(record.face_pairs) >= 58
    assert len(record.unpaired_faces) <= 170
    assert frozenset((153, 242)) in pairs  # concentric cylindrical joint
    assert frozenset((127, 198)) in pairs  # concentric toroidal joint
    assert frozenset((129, 200)) in pairs
    assert {item.face for item in record.unpaired_face_classes} == set(record.unpaired_faces)
    assert {item.kind for item in record.unpaired_face_classes} <= {
        "cut_edge",
        "joint_blend",
        "non_wall_feature",
    }
    assert {item.kind for item in record.unpaired_face_classes} == {
        "cut_edge",
        "joint_blend",
        "non_wall_feature",
    }
