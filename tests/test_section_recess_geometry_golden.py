"""Consumer-facing JSON geometry for each migrated proof, not legacy dimension records."""

import json
from pathlib import Path

import pytest
from build123d import Box, Pos

from quiddity import build_section_recess_document
from tests.test_edge_open_circular_recesses import _open_circular_pocket
from tests.test_edge_open_prismatic_recesses import _edge_open_hexagon
from tests.test_rectangular_blind_slots import _slot as rectangular_slot
from tests.test_round_bottom_slots import _slot as round_bottom_slot
from tests.test_section_recesses import _blind_pocket, _polygonal_cutter, _polygonal_pocket

TRIANGLE = ((-4.0, -3.0), (4.0, -3.0), (0.0, 5.0))
CASES = {
    "obround_pocket": _blind_pocket,
    "polygonal_pocket": lambda: _polygonal_pocket(TRIANGLE),
    "polygonal_passage": lambda: (
        Box(60, 50, 12) - Pos(0, 0, -7) * _polygonal_cutter(TRIANGLE, depth=14)
    ),
    "edge_open_polygonal": _edge_open_hexagon,
    "edge_open_circular": _open_circular_pocket,
    "rectangular_blind_slot": rectangular_slot,
    "round_bottom_blind_slot": round_bottom_slot,
}


def snapshot(part):
    document = build_section_recess_document(part)
    # Source topology ordering is not portable between import/kernel versions. Freeze geometry
    # and evidence cardinalities here; run-local index validity is tested on the actual document.
    rows = [
        {
            "geometry": record.geometry.to_dict(),
            "classification": record.classification.to_dict(),
            "defining_face_count": len(record.evidence.defining_faces),
            "constituent_face_count": len(record.evidence.constituent_faces),
        }
        for record in document.occurrences
    ]
    return json.loads(json.dumps(sorted(rows, key=lambda row: json.dumps(row, sort_keys=True))))


@pytest.mark.parametrize("name", CASES)
def test_unified_json_geometry_matches_reviewed_golden(name):
    expected = json.loads(
        Path(__file__).with_name("section_recess_geometry_expected.json").read_text()
    )
    assert snapshot(CASES[name]()) == expected[name]
