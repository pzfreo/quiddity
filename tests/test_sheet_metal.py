"""Developable sheet geometry and bend-plan regression for #748."""

from __future__ import annotations

import json
from pathlib import Path

import pytest
from build123d import Box, BuildLine, BuildSketch, Line, Pos, ThreePointArc, extrude, make_face

from quiddity import (
    build_recognition_result,
    import_step_geometry,
    recognise_sheet_metal_bodies,
)
from quiddity.document import build_recognition_document
from quiddity.evidence import build_recognition_evidence


def _formed_bracket():
    """A 4 mm sheet with a 90 degree bend of inner radius 3 mm."""
    with BuildLine() as outline:
        Line((-40, 7), (0, 7))
        ThreePointArc((0, 7), (4.949747468, 4.949747468), (7, 0))
        Line((7, 0), (7, -40))
        Line((7, -40), (3, -40))
        Line((3, -40), (3, 0))
        ThreePointArc((3, 0), (2.121320344, 2.121320344), (0, 3))
        Line((0, 3), (-40, 3))
        Line((-40, 3), (-40, 7))
    with BuildSketch() as section:
        make_face(outline.line)
    return extrude(section.sketch, amount=100)


def test_formed_bracket_has_reconstructible_bend_plan():
    bracket = _formed_bracket()
    (sheet,) = recognise_sheet_metal_bodies(bracket, k_factor=0.4)
    assert sheet.thickness == pytest.approx(4)
    assert len(sheet.flanges) == 2
    assert len(sheet.bends) == 1
    assert sheet.bends[0].angle_degrees == pytest.approx(90)
    assert sheet.bends[0].inner_radius == pytest.approx(3)
    assert sheet.bends[0].neutral_radius == pytest.approx(4.6)
    assert sheet.bends[0].bend_allowance == pytest.approx(4.6 * 3.141592653589793 / 2)
    assert len(sheet.cut_edge_faces) == 4
    assert sheet.flat_pattern.k_factor == 0.4
    assert sheet.flat_pattern.valid_blank
    assert sheet.flat_pattern.overlap_witnesses == ()
    assert sheet.flat_pattern.tree_bends == (0,)
    assert len(sheet.flat_pattern.flat_faces) == 2
    assert len(sheet.flat_pattern.bend_strips) == 1
    assert len(sheet.formed_features) == 0
    flat_area = sum(
        abs(
            (strip.corners[1][0] - strip.corners[0][0])
            * (strip.corners[3][1] - strip.corners[0][1])
            - (strip.corners[1][1] - strip.corners[0][1])
            * (strip.corners[3][0] - strip.corners[0][0])
        )
        for strip in sheet.flat_pattern.bend_strips
    )
    assert flat_area == pytest.approx(sheet.bends[0].bend_allowance * 100)
    assert set(sheet.first_side_faces).isdisjoint(sheet.second_side_faces)
    expected = json.loads(Path(__file__).with_name("sheet_metal_expected.json").read_text())
    assert {
        "thickness": round(sheet.thickness, 6),
        "flanges": len(sheet.flanges),
        "bends": len(sheet.bends),
        "cut_edge_faces": len(sheet.cut_edge_faces),
        "paired_area_fraction": round(sheet.paired_area_fraction, 6),
    } == expected

    aggregate = build_recognition_result(bracket)
    assert len(aggregate.sheet_metal_bodies) == 1
    assert aggregate.sheet_metal_bodies[0].thickness == pytest.approx(4)
    view = build_recognition_evidence(bracket)
    feature = next(ref for ref in view.features if view.family(ref) == "sheet_metal_bodies")
    assert view.record(feature) == aggregate.sheet_metal_bodies[0]
    assert view.defining_faces(feature)
    document = build_recognition_document(bracket)
    assert any(item["family"] == "sheet_metal_bodies" for item in document["features"])
    json.dumps(document, allow_nan=False)


def test_plain_solid_and_unsupported_k_factor_are_refused():
    assert recognise_sheet_metal_bodies(Box(40, 20, 10)) == []
    with pytest.raises(ValueError, match="k_factor"):
        recognise_sheet_metal_bodies(_formed_bracket(), k_factor=-0.1)


def test_blind_pocket_cannot_be_explained_as_a_sheet_cut_or_form():
    pocketed = _formed_bracket() - Pos(-20, 6.75, 50) * Box(10, 1.5, 10)
    assert recognise_sheet_metal_bodies(pocketed) == []


@pytest.mark.slow
def test_ttt_hanger_step_has_main_blank_and_formed_tabs():
    source = Path(__file__).parent / "corpus" / "ttt_inputs" / "sm-hanger.step"
    part = import_step_geometry(source)
    (sheet,) = recognise_sheet_metal_bodies(part)
    assert sheet.thickness == pytest.approx(4, abs=1e-5)
    assert len(sheet.flanges) == 9
    assert len(sheet.bends) == 8
    assert sheet.formed_features == ()
    assert sheet.flat_pattern is not None
    assert len(sheet.flat_pattern.tree_bends) == 8
    assert len(sheet.flat_pattern.flat_faces) == 11
    assert len(sheet.flat_pattern.bend_strips) == 12
    assert {66, 79} <= {face.source_face for face in sheet.flat_pattern.flat_faces}
    assert not sheet.flat_pattern.valid_blank
    assert sheet.flat_pattern_status == "overlap"
    assert sheet.flat_pattern.overlap_witnesses
    assert all(item.area > 0 for item in sheet.flat_pattern.overlap_witnesses)
    main_face = next(face for face in sheet.flat_pattern.flat_faces if face.source_face == 64)
    area = sum(
        abs(
            (main_face.vertices[b][0] - main_face.vertices[a][0])
            * (main_face.vertices[c][1] - main_face.vertices[a][1])
            - (main_face.vertices[b][1] - main_face.vertices[a][1])
            * (main_face.vertices[c][0] - main_face.vertices[a][0])
        )
        / 2
        for a, b, c in main_face.triangles
    )
    assert area == pytest.approx(part.faces()[64].area, rel=0.002)


@pytest.mark.slow
def test_cgb245_rounded_cut_edges_keep_flange_and_bend_evidence():
    part = import_step_geometry(
        Path(__file__).parent / "corpus" / "cadgenbench_inputs" / "cgb245.step"
    )
    (sheet,) = recognise_sheet_metal_bodies(part)
    assert sheet.thickness == pytest.approx(2, abs=1e-4)
    assert len(sheet.flanges) >= 20
    assert len(sheet.bends) >= 20
    assert sum(item.kind == "rounded_cut" for item in sheet.edge_treatments) >= 62
    assert all(
        item.face in sheet.cut_edge_faces or item.kind.endswith("corner")
        for item in sheet.edge_treatments
    )
    assert sheet.flat_pattern is None
    assert sheet.flat_pattern_status == "non_tree"
    document = build_recognition_document(part)
    assert any(item["family"] == "sheet_metal_bodies" for item in document["features"])
    json.dumps(document, allow_nan=False)
