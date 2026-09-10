# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Nonplanar thread faces cannot create axial shoulder stations (#587)."""

from pathlib import Path

import pytest
from attribution_audit import attributed_run
from build123d import Box, Cylinder, GeomType, Pos, Rotation, Solid, import_step
from OCP.BRepBuilderAPI import BRepBuilderAPI_NurbsConvert, BRepBuilderAPI_Sewing
from OCP.TopoDS import TopoDS

from quiddity import recognise_turned_steps
from quiddity._adjacency import FaceGraph
from quiddity._candidates import FamilyId
from quiddity._claims import ClaimLedger
from quiddity._effective_surfaces import effective_faces_for_part

CORPUS = Path(__file__).parent / "corpus" / "cadgenbench"


def _signature(steps):
    return [(step.lo, step.hi, step.diameter) for step in steps]


@pytest.mark.skipif(not CORPUS.exists(), reason="Vendored STEP corpus is absent from sdist")
def test_exact_threaded_connector_has_no_face_centre_shoulders():
    part = import_step(CORPUS / "threaded_connector_109.step")
    steps = recognise_turned_steps(part)
    assert _signature(steps) == [
        (0, 57.42, 130.94),
        (57.42, 67.42, 122),
        (67.42, 87.42, 130.94),
        (87.42, 93.42, 122),
        (93.42, 113.42, 130.94),
        (113.42, 343.13, 122),
        (366.82, 420, 103.12),
    ]
    ledger = ClaimLedger(FaceGraph(part))
    assert recognise_turned_steps(part, ledger=ledger) == steps
    candidates = ledger.candidate_set(FamilyId.TURNED_STEPS).candidates
    # All eight original cylindrical crest patches must survive coalescing.
    assert len(ledger.defining_of(candidates[-1])) == 8


@pytest.mark.skipif(not CORPUS.exists(), reason="Vendored STEP corpus is absent from sdist")
def test_real_spool_retains_its_partial_profile():
    part = import_step(CORPUS / "flanged_spool_132.step")
    assert _signature(recognise_turned_steps(part)) == [(0, 5, 130), (5, 108, 70), (108, 113, 130)]
    assert float(part.bounding_box().size.Z) == pytest.approx(140)


@pytest.mark.parametrize("scale", [0.05, 1, 5])
def test_internal_annular_faces_do_not_split_one_outer_diameter(scale):
    part = Pos(0, 0, 50) * Cylinder(15, 100) + Pos(0, 0, 105) * Cylinder(8, 10)
    for z in (5, 15, 25):
        part -= Pos(0, 0, z) * (Cylinder(14, 1) - Cylinder(10, 1))
    part = Pos(91, -37, 48) * Rotation(0, 90, 0) * part.scale(scale)
    ledger, steps = attributed_run(part, FamilyId.TURNED_STEPS, recognise_turned_steps)
    assert [(s.axis, s.length, s.diameter) for s in steps] == [
        ("x", 100 * scale, 30 * scale),
        ("x", 10 * scale, 16 * scale),
    ]
    assert len(ledger.candidate_set(FamilyId.TURNED_STEPS).candidates) == 2


def test_equal_diameters_across_an_axial_gap_are_not_joined():
    part = (
        Pos(0, 0, 2.5) * Cylinder(15, 5)
        + Pos(0, 0, 17.5) * Cylinder(15, 5)
        + Pos(0, 0, 22.5) * Cylinder(8, 5)
        + Pos(0, 0, 10) * Box(2, 2, 20)
    )
    assert len(part.solids()) == 1
    assert _signature(recognise_turned_steps(part)) == [(0, 5, 30), (15, 20, 30), (20, 25, 16)]


def test_certified_nurbs_planes_still_establish_shoulders():
    native = Pos(0, 0, 20) * Cylinder(15, 40) + Pos(0, 0, 55) * Cylinder(8, 30)
    sewing = BRepBuilderAPI_Sewing(1e-6)
    for face in native.faces():
        sewing.Add(
            BRepBuilderAPI_NurbsConvert(face.wrapped, True).Shape()
            if face.geom_type == GeomType.PLANE
            else face.wrapped
        )
    sewing.Perform()
    nurbs = Solid(TopoDS.Shell_s(sewing.SewedShape()))
    assert nurbs.is_valid
    expected = [(0, 40, 30), (40, 70, 16)]
    assert _signature(recognise_turned_steps(nurbs)) == expected
    query = effective_faces_for_part(nurbs)
    assert _signature(recognise_turned_steps(nurbs, face_surfaces=query)) == expected
