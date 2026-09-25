"""Circular face groups remain useful when their member is not a known feature."""

from __future__ import annotations

import hashlib
import json
import time
from pathlib import Path

import pytest
from build123d import Box, Compound, Cylinder, GeomType, Pos, Rectangle, Rot, import_step, loft

from quiddity import recognise_circular_face_patterns
from quiddity.document import build_recognition_document
from quiddity.evidence import build_recognition_evidence


def _swept_fins(count: int = 7, *, omit: int | None = None):
    blade = loft(
        [
            Pos(30, 0, 2) * Rectangle(22, 3),
            Rot(0, 0, 15) * Pos(30, 0, 20) * Rectangle(22, 3),
        ]
    )
    part = Cylinder(55, 4)
    for index in range(count):
        if index != omit:
            part += Rot(0, 0, 360 * index / count) * blade
    return part


@pytest.mark.parametrize("count", [5, 7, 9])
def test_swept_fins_publish_one_complete_circular_face_pattern(count: int) -> None:
    part = _swept_fins(count)
    (pattern,) = recognise_circular_face_patterns(part)

    assert pattern.count == count
    assert pattern.pitch_degrees == pytest.approx(360 / count)
    assert pattern.seed_index == 0
    assert pattern.axis_direction == pytest.approx((0, 0, 1))

    view = build_recognition_evidence(part)
    (feature,) = tuple(
        feature for feature in view.features if view.family(feature) == "circular_face_patterns"
    )
    instances = view.instance_faces(feature)
    assert len(instances) == pattern.count
    assert all(instances)
    assert len(frozenset().union(*instances)) == sum(len(group) for group in instances)
    assert frozenset().union(*instances) == view.constituent_faces(feature)

    document = build_recognition_document(part)
    (entry,) = tuple(
        entry for entry in document["features"] if entry["family"] == "circular_face_patterns"
    )
    assert len(entry["instances"]) == pattern.count
    assert [instance["seed"] for instance in entry["instances"]] == [True] + [False] * (count - 1)
    assert set(entry["excluded_faces"]) == set(range(len(part.faces()))) - set(
        entry["constituent_faces"]
    )


def test_incomplete_swept_fin_cycle_is_not_a_pattern() -> None:
    assert recognise_circular_face_patterns(_swept_fins(omit=3)) == []


def test_rotated_fins_retain_the_physical_axis_and_count() -> None:
    part = Rot(17, 23, 31) * _swept_fins()
    (pattern,) = recognise_circular_face_patterns(part)
    expected = Rot(17, 23, 31) * Cylinder(1, 1)
    cylinder = next(face for face in expected.faces() if face.geom_type is GeomType.CYLINDER)
    assert pattern.count == 7
    assert pattern.axis_direction == pytest.approx(
        tuple(float(value) for value in cylinder.axis_of_rotation.direction), abs=1e-8
    )


def test_document_exclusions_are_local_to_the_pattern_body() -> None:
    part = Compound(children=[_swept_fins(), Pos(300, 0, 0) * Box(10, 10, 10)])
    document = build_recognition_document(part)
    (pattern,) = tuple(
        entry for entry in document["features"] if entry["family"] == "circular_face_patterns"
    )
    excluded = set(pattern["excluded_faces"])
    seed_face = pattern["instances"][0]["face_indices"][0]
    owner = document["faces"][seed_face]["body_indices"]
    assert len(excluded) == 3
    assert all(document["faces"][index]["body_indices"] == owner for index in excluded)


@pytest.mark.slow
@pytest.mark.timeout(60)
def test_cgb203_impeller_repeats_split_bspline_face_groups() -> None:
    expected = json.loads(
        (Path(__file__).parent / "circular_face_pattern_expected.json").read_text(encoding="utf-8")
    )
    fixture = Path(__file__).parent / "corpus/cadgenbench_inputs/cgb203.step"
    if not fixture.exists():
        pytest.skip("vendored CADGenBench corpus is absent from the sdist")
    assert hashlib.sha256(fixture.read_bytes()).hexdigest() == expected["source_sha256"]
    part = import_step(str(fixture))
    assert len(part.faces()) == expected["face_count"]
    started = time.monotonic()
    (pattern,) = recognise_circular_face_patterns(part)
    assert time.monotonic() - started < 45
    assert pattern.count == expected["instance_count"]
    assert pattern.pitch_degrees == pytest.approx(expected["pitch_degrees"])
    assert pattern.axis_direction == pytest.approx(expected["axis_direction"])
    assert pattern.fit_error < expected["maximum_fit_error"]
