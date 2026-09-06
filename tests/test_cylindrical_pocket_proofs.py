from __future__ import annotations

import math

import pytest
from build123d import Box, Compound, Cylinder, Pos, Rot, export_step, import_step

from quiddity import CylindricalEndSurface, build_section_recess_document
from quiddity._adjacency import FaceGraph
from quiddity._cylindrical_pockets import cylindrical_pocket_proofs
from quiddity._effective_surfaces import EffectiveSurfaceIndex
from quiddity._section_recess_geometry import _cylindrical_candidate


def _base(scale=1.0, offset=0.0):
    return Rot(0, 90, 0) * Cylinder(20 * scale, 80 * scale) - Pos(
        0, offset * scale, 14 * scale
    ) * Box(6 * scale, 24 * scale, 12 * scale)


def _proofs(part):
    graph = FaceGraph(part)
    return graph, cylindrical_pocket_proofs(graph, EffectiveSurfaceIndex(graph))


@pytest.mark.parametrize("scale", [0.1, 1.0, 10.0])
@pytest.mark.parametrize("offset", [0.0, 3.0])
@pytest.mark.parametrize("rotation", [Rot(), Rot(17, 31, 43), Rot(180, 0, 0)])
def test_original_sources_prove_complete_cylindrical_end(scale, offset, rotation):
    graph, proofs = _proofs(Pos(3, 7, 11) * rotation * _base(scale, offset))
    assert len(proofs) == 1
    proof = proofs[0]
    assert len(proof.walls) == 4
    assert proof.stock not in (*proof.walls, proof.floor)
    assert graph.common_valid_solid((proof.floor, *proof.walls, proof.stock)) == proof.owner
    assert proof.radius == pytest.approx(20 * scale)
    volume = 1544.4026611038823 if offset == 0 else 1503.322366310343
    assert proof.volume == pytest.approx(volume * scale**3, rel=1e-9)


@pytest.mark.parametrize("kind", ["side_breakout", "pierced_wall", "bridge", "mouth_bridge"])
def test_missing_support_and_obstructions_refused(kind):
    part = _base()
    if kind == "side_breakout":
        part = Rot(0, 90, 0) * Cylinder(20, 80) - Pos(0, 6, 14) * Box(6, 36, 12)
    elif kind == "pierced_wall":
        part -= Pos(0, 0, 12) * Rot(0, 90, 0) * Cylinder(1, 30)
    elif kind == "bridge":
        part += Pos(0, 0, 12) * Box(10, 1, 1)
    else:
        part += Pos(0, 0, 20) * Box(10, 1, 1)
    assert not _proofs(part)[1]


def test_compound_ownership_and_step(tmp_path):
    part = Compound([_base(), Pos(100, 0, 0) * _base()])
    _, proofs = _proofs(part)
    assert len(proofs) == 2
    assert proofs[0].owner != proofs[1].owner
    assert set(proofs[0].walls).isdisjoint(proofs[1].walls)
    path = tmp_path / "curved-mouth.step"
    export_step(part, path)
    _, reread = _proofs(import_step(path))
    assert len(reread) == 2
    assert [p.volume for p in reread] == pytest.approx([p.volume for p in proofs])


@pytest.mark.parametrize("scale", [0.1, 1.0, 10.0])
@pytest.mark.parametrize("offset", [0.0, 3.0])
@pytest.mark.parametrize("rotation", [Rot(), Rot(17, 31, 43), Rot(180, 0, 0)])
def test_public_cylindrical_pocket_preserves_source_evidence(scale, offset, rotation):
    part = Pos(3, 7, 11) * rotation * _base(scale, offset)
    document = build_section_recess_document(part)
    assert document.schema_version == 3
    assert len(document.occurrences) == 1
    assert not document.refusals
    record = document.occurrences[0]
    assert record.classification.feature_kind == "pocket"
    assert record.classification.section_shape == "rectangular"
    assert len(record.evidence.defining_faces) == 4
    assert len(record.evidence.constituent_faces) == 5
    surfaces = (record.geometry.ends.low.surface, record.geometry.ends.high.surface)
    cylinders = [surface for surface in surfaces if isinstance(surface, CylindricalEndSurface)]
    assert len(cylinders) == 1
    assert cylinders[0].radius == pytest.approx(20 * scale)


def test_tiny_source_axis_tilt_cannot_exceed_public_displacement_budget():
    part = Rot(0, 90 - math.degrees(5e-9), 0) * Cylinder(20, 1600000)
    part -= Pos(0, 0, 14) * Box(1200000, 24, 12)
    graph, proofs = _proofs(part)
    assert len(proofs) == 1
    with pytest.raises(ValueError, match="displacement limit"):
        _cylindrical_candidate(graph, proofs[0])
