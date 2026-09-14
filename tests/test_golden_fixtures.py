import importlib.util
import sys
from pathlib import Path

from build123d import Shape

ROOT = Path(__file__).parents[1]
GOLDEN_ROOT = ROOT / "tests" / "golden"
sys.path.insert(0, str(ROOT))

EXPECTED_CASES = {
    "angled_blind_step",
    "blind_pockets_and_pocket_patterns",
    "bolt_circle_and_rectangular_grid",
    "chamfers_fillets_and_flats",
    "circular_blind_step",
    "counterbored_and_countersunk_holes",
    "double_d_bore",
    "hexagonal_passage",
    "gusset_rib_patterns",
    "gusset_ribs",
    "interrupted_and_cross_bores",
    "open_channels",
    "oriented_slots",
    "paired_ramp_step",
    "rectangular_blind_slot",
    "rectangular_through_step",
    "plates_pads_levels_and_slanted_steps",
    "polygonal_boss",
    "polygonal_stock",
    "repeating_radial_profile",
    "round_bottom_blind_slot",
    "simple_through_hole",
    "small_convex_blends",
    "slanted_counterbore",
    "slanted_steps",
    "straight_and_obround_slots",
    "toroidal_blend_compound",
    "toroidal_blend_internal",
    "toroidal_blends_turned",
    "traversal_order",
    "triangular_and_hex_pockets",
    "turned_steps_and_grooves",
}


def _load(path):
    spec = importlib.util.spec_from_file_location(f"fixture_{path.parent.name}", path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_complete_synthetic_fixture_corpus_is_executable_and_provenanced():
    fixture_paths = sorted(GOLDEN_ROOT.glob("*/fixture.py"))

    assert {path.parent.name for path in fixture_paths} == EXPECTED_CASES
    for path in fixture_paths:
        fixture = _load(path)
        assert fixture.PROVENANCE["creator"] == "Paul Fremantle"
        # The provenance block is licensing attribution and is still required. Which repository
        # a fixture came from is not: most were relicensed from Draftwright at a pinned commit,
        # families originated here were not, and asserting the former of all of them stopped
        # describing the corpus once it grew its own.
        assert fixture.PROVENANCE["source_repository"]
        assert fixture.PROVENANCE["source_path"].startswith("tests/test_")
        assert fixture.PROVENANCE["license"] == "Apache-2.0"
        shape = fixture.build_fixture()
        assert isinstance(shape, Shape)
        assert shape.is_valid
        for variant in getattr(fixture, "equivalent_fixtures", lambda: {})().values():
            assert isinstance(variant, Shape)
            assert variant.is_valid
