"""Public cutover checks independent of the private detector-baseline tooling."""

import dataclasses
import json

import pytest
from build123d import Box, Compound, Pos, Rot

import quiddity as public
from quiddity.evidence import build_recognition_evidence
from tests.golden.blind_pockets_and_pocket_patterns.fixture import build_fixture as pocket_grid
from tests.golden.plates_pads_levels_and_slanted_steps.fixture import build_fixture as channels

RETIRED = {
    "Pocket",
    "PocketArray",
    "PocketGrid",
    "PrismaticPocket",
    "Channel",
    "Passage",
    "RectangularBlindSlot",
    "RoundBottomBlindSlot",
    "EdgeOpenCircularPocket",
    "EdgeOpenPrismaticRecess",
    "recognise_pockets",
    "recognise_pocket_patterns",
    "recognise_prismatic_pockets",
    "recognise_channels",
    "recognise_passages",
    "recognise_section_passages",
    "recognise_rectangular_blind_slots",
    "recognise_round_bottom_blind_slots",
    "recognise_edge_open_circular_pockets",
    "recognise_edge_open_prismatic_recesses",
}
OLD_FIELDS = {
    "pockets",
    "pocket_patterns",
    "prismatic_pockets",
    "channels",
    "passages",
    "section_passages",
    "rectangular_blind_slots",
    "round_bottom_blind_slots",
    "edge_open_circular_pockets",
    "edge_open_prismatic_recesses",
}


def test_retired_public_records_functions_and_result_fields_are_gone():
    assert not RETIRED.intersection(public.__all__)
    assert not any(hasattr(public, name) for name in RETIRED)
    result = public.build_raw_recognition_result(channels())
    assert type(result) is public.RecognitionResult
    assert not OLD_FIELDS.intersection(field.name for field in dataclasses.fields(result))
    assert not any(hasattr(result, name) for name in OLD_FIELDS)


@pytest.mark.parametrize("rotation", [Rot(), Rot(90, 0, 0), Rot(0, 90, 0), Rot(180, 0, 0)])
def test_both_authored_partial_support_channels_have_truthful_open_topology(rotation):
    part = Pos(123, -57, 91) * rotation * channels()
    document = public.build_section_recess_document(part)
    assert document.schema_version == 2
    assert document.refusals == ()
    assert len(document.occurrences) == 2
    for occurrence in document.occurrences:
        assert occurrence.classification.feature_kind == "channel"
        assert occurrence.classification.section_shape == "rectangular"
        assert occurrence.geometry.profile.closure == "open"
        assert len(occurrence.geometry.profile.boundary) == 4
        assert (
            occurrence.geometry.ends.low.condition
            == occurrence.geometry.ends.high.condition
            == "open"
        )
    assert sorted(
        round(item.geometry.run_interval[1] - item.geometry.run_interval[0], 3)
        for item in document.occurrences
    ) == [18, 50]
    json.dumps(document.to_dict(), allow_nan=False)


def test_patterns_reference_unified_occurrences_without_embedding_old_pockets():
    part = pocket_grid()
    document = public.build_section_recess_document(part)
    assert len(document.occurrences) == 6
    assert document.refusals == ()
    (pattern,) = document.patterns
    assert isinstance(pattern, public.SectionRecessGrid)
    assert set(pattern.members) == set(range(6))
    assert not hasattr(pattern, "pockets")


@pytest.mark.parametrize("rotation", [Rot(), Rot(90, 0, 0), Rot(0, 90, 0), Rot(0, 0, 90)])
def test_grid_directions_reconstruct_the_source_lattice_in_each_plane(rotation):
    document = public.build_section_recess_document(rotation * pocket_grid())
    (pattern,) = document.patterns
    expected = [rotation * Pos(x, y, 7) for x in (-25, 25) for y in (-25, 0, 25)]
    rebuilt = [
        tuple(
            pattern.center[i]
            + (row - (pattern.rows - 1) / 2) * pattern.row_pitch * pattern.row_direction[i]
            + (col - (pattern.cols - 1) / 2) * pattern.col_pitch * pattern.col_direction[i]
            for i in range(3)
        )
        for row in range(pattern.rows)
        for col in range(pattern.cols)
    ]
    for location in expected:
        assert any(point == pytest.approx(tuple(location.position), abs=0.002) for point in rebuilt)


def test_linear_pattern_references_each_occurrence_in_order():
    part = Box(100, 50, 20)
    for x in (-25, 0, 25):
        part -= Pos(x, 0, 7) * Box(12, 10, 6)
    document = public.build_section_recess_document(part)
    (pattern,) = document.patterns
    assert isinstance(pattern, public.SectionRecessArray)
    assert set(pattern.members) == {0, 1, 2}
    assert pattern.pitch == 25
    assert pattern.direction == (1, 0, 0)


def test_public_evidence_and_census_use_the_unified_inventory_once():
    part = pocket_grid()
    view = build_recognition_evidence(part)
    recesses = [
        view.record(ref)
        for ref in view.features
        if isinstance(view.record(ref), public.SectionRecess)
    ]
    assert tuple(recesses) == view.result.section_recesses
    assert not any(type(view.record(ref)).__name__ in RETIRED for ref in view.features)
    counts = public.feature_census(part)
    assert counts["section_recess"] == 6
    assert not {"pocket", "prismatic_pocket", "passage", "channel"}.intersection(counts)


def test_unproved_old_summary_has_explicit_face_referenced_refusal():
    # Suspended material leaves the old corner detector's summary valid, but violates the
    # stronger constant-section contract. Its evidence must not disappear without explanation.
    part = Box(60, 40, 12) - Pos(25, 15, 4) * Box(20, 20, 8)
    part += Pos(-20, -10, 9) * Box(5, 5, 6)
    part += Pos(0, 0, 13) * Box(60, 40, 2)
    part += Pos(20, 10, 8) * Box(3, 3, 8)
    document = public.build_section_recess_document(part)
    assert document.occurrences == ()
    (refusal,) = document.refusals
    assert refusal.reason == "unsupported_support_geometry"
    assert refusal.body == 0
    assert len(refusal.evidence.defining_faces) == 3
    assert set(refusal.evidence.constituent_faces) <= set(range(len(document.faces)))
    view = build_recognition_evidence(part)
    refused = [
        feature
        for feature in view.features
        if isinstance(view.record(feature), public.SectionRecessRefusal)
    ]
    assert len(refused) == 1
    assert len(view.constituent_faces(refused[0])) == len(refusal.evidence.constituent_faces)


def test_identical_channels_on_separate_bodies_stay_separate():
    document = public.build_section_recess_document(
        Compound([channels(), Pos(200, 0, 0) * channels()])
    )
    assert len(document.occurrences) == 4
    assert {record.body for record in document.occurrences} == {0, 1}


@pytest.mark.parametrize("members", [(0,), (0, 0), (-1, 1), (True, 1), [0, 1], ([], 1)])
def test_pattern_member_indices_are_immutable_distinct_integers(members):
    with pytest.raises(ValueError):
        public.SectionRecessArray(members, 10, (1, 0, 0))


@pytest.mark.parametrize("pitch", [0, -1, float("nan"), float("inf"), True, "10"])
def test_array_refuses_invalid_pitch(pitch):
    with pytest.raises(ValueError):
        public.SectionRecessArray((0, 1), pitch, (1, 0, 0))


def test_pattern_and_refusal_contracts_validate_without_geometry_kernel():
    array = public.SectionRecessArray((0, 1), 10, (1, 0, 0))
    with pytest.raises(ValueError, match="unit length"):
        dataclasses.replace(array, direction=(2, 0, 0))
    grid = public.SectionRecessGrid((0, 1, 2, 3), 2, 2, 10, 10, (0, 1, 0), (1, 0, 0), (0, 0, 0))
    for changes in (
        {"rows": 1},
        {"cols": True},
        {"members": (0, 1)},
        {"row_pitch": -1},
        {"row_direction": (float("inf"), 0, 0)},
        {"row_direction": (2, 0, 0)},
        {"row_direction": (1, 0, 0)},
        {"center": (0, 0)},
    ):
        with pytest.raises(ValueError):
            dataclasses.replace(grid, **changes)
    evidence = public.SectionRecessEvidence((0,), (0,))
    refusal = public.SectionRecessRefusal(0, "unsupported_support_geometry", evidence)
    for changes in ({"body": -1}, {"body": True}, {"reason": "unknown"}, {"evidence": None}):
        with pytest.raises(ValueError):
            dataclasses.replace(refusal, **changes)
    document = public.SectionRecessDocument(
        2,
        "result",
        (public.SectionRecessBodyRef(0),),
        (public.SectionRecessFaceRef(0),),
        (),
        (refusal,),
    )
    for changes in (
        {"schema_version": 1},
        {"schema_version": 2.0},
        {"reference_scope": "global"},
        {"bodies": ()},
        {"faces": ()},
        {"refusals": [refusal]},
        {"patterns": (array,)},
        {"patterns": (object(),)},
    ):
        with pytest.raises(ValueError):
            dataclasses.replace(document, **changes)
