"""Core evidence on authored solids and the public CADGenBench editing input."""

from __future__ import annotations

import gzip
import hashlib
from pathlib import Path

import pytest
from build123d import Box, Cylinder, Pos, Rot, Sphere, import_step

from quiddity import build_recognition_result, recognise_interior_voids
from quiddity._interior_void_grid import expanded_air_components, sample_void_grid

CORPUS = Path(__file__).parent / "corpus" / "cadgenbench_inputs"


def test_closed_and_small_port_spherical_cores() -> None:
    block = Box(100, 100, 100)
    closed = block - Sphere(25)
    ported = closed - Pos(0, 0, 35) * Cylinder(5, 30)

    closed_records = recognise_interior_voids(closed)
    ported_records = recognise_interior_voids(ported)
    assert len(closed_records) == len(ported_records) == 1
    assert closed_records[0].openings == ()
    assert len(ported_records[0].openings) == 1
    assert ported_records[0].void_faces
    assert set(ported_records[0].void_faces).isdisjoint(ported_records[0].openings[0])
    assert 50_000 < ported_records[0].estimated_volume < 70_000
    assert ported_records[0].volume_method == "six_axis_grid"
    assert ported_records[0].estimated_volume == pytest.approx(
        ported_records[0].enclosed_samples * ported_records[0].grid_pitch ** 3
    )
    assert len(build_recognition_result(ported).interior_voids) == 1


def test_open_pockets_and_through_bores_are_not_cores() -> None:
    block = Box(100, 80, 80)
    oblique_pocket = block - Pos(0, 0, 30) * Rot(35, 25, 17) * Cylinder(10, 35)
    assert recognise_interior_voids(oblique_pocket) == []
    assert recognise_interior_voids(block - Cylinder(10, 100)) == []
    grid = sample_void_grid(oblique_pocket)
    assert grid is not None and grid.components  # six-axis enclosure alone is insufficient
    assert expanded_air_components(grid) == frozenset()


def test_cgb243_core_is_original_body_evidence(tmp_path: Path) -> None:
    compressed = CORPUS / "cgb243.step.gz"
    raw = gzip.decompress(compressed.read_bytes())
    assert hashlib.sha256(raw).hexdigest() == (
        "4d57202dd151423930d469be8d5d76c508aae127dbf7c5530790cdc23fe629f0"
    )
    source = tmp_path / "cgb243.step"
    source.write_bytes(raw)
    body = import_step(str(source))
    assert len(body.faces()) == 588
    direct = recognise_interior_voids(body)
    assert tuple(direct) == build_recognition_result(body).interior_voids
    assert len(direct) == 1
    record = direct[0]
    assert record.body_index == 0
    assert len(record.void_faces) >= 2
    assert record.openings
    assert 50_000 < record.estimated_volume < 65_000
    assert all(index < len(body.faces()) for index in record.void_faces)
    assert all(index < len(body.faces()) for group in record.openings for index in group)


def test_cgb207_thin_shell_has_no_core() -> None:
    body = import_step(str(CORPUS / "cgb207.step"))
    assert recognise_interior_voids(body) == []
