"""Authored half-publication-grid translations must not abort recognition."""

from dataclasses import replace

import pytest
from build123d import Box, Pos, RegularPolygon, extrude

from quiddity import build_section_recess_document
from quiddity.passages import Passage, _same_legacy_passage_geometry


@pytest.mark.parametrize("sides", [3, 4, 6])
@pytest.mark.parametrize("shift", [-100.0005, -0.0005, 0.00049, 0.0005, 0.00051, 100.0005])
def test_translated_polygonal_passage_survives_centroid_rounding_ties(sides, shift):
    part = Pos(shift, shift, shift) * (
        Box(40, 40, 20.001) - extrude(RegularPolygon(3.12345, sides), 40, both=True)
    )
    document = build_section_recess_document(part)
    assert len(document.occurrences) == 1
    assert document.occurrences[0].classification.feature_kind == "passage"
    assert document.refusals == ()


@pytest.mark.parametrize("sign", [-1, 1])
def test_only_source_anchored_adjacent_grid_ties_are_equivalent(sign):
    left = Passage("z", 4, 10, (0, 0, 0), ((-1, -1), (1, -1), (1, 1), (-1, 1)))
    right = replace(left, at=(sign * 0.001, 0, 0))
    source = (sign * 0.0005, 0, 0)
    assert not _same_legacy_passage_geometry(left, right)
    assert _same_legacy_passage_geometry(left, right, exact_at=source)
    assert not _same_legacy_passage_geometry(left, right, exact_at=(sign * 0.00049, 0, 0))
    assert not _same_legacy_passage_geometry(
        left, replace(right, at=(sign * 0.002, 0, 0)), exact_at=(sign * 0.001, 0, 0)
    )
    for change in (
        {"axis": "x"},
        {"sides": 3},
        {"length": 10.001},
        {"section": ((-2, -1), (1, -1), (1, 1), (-2, 1))},
    ):
        assert not _same_legacy_passage_geometry(left, replace(right, **change), exact_at=source)
