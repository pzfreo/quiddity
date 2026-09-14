import json
import sys
from pathlib import Path

ROOT = Path(__file__).parents[1]
GOLDEN_ROOT = ROOT / "tests" / "golden"
sys.path.insert(0, str(ROOT / "tools"))

from golden_support import CANONICALIZER_VERSION, canonical_json  # noqa: E402

PUBLIC_RECOGNISERS = {
    "recognise_angled_steps",
    "recognise_blends",
    "recognise_paired_ramp_steps",
    "recognise_passages",
    "recognise_prismatic_pockets",
    "recognise_through_steps",
    "recognise_bosses",
    "recognise_chamfers",
    "recognise_channels",
    "recognise_circular_blind_steps",
    "recognise_countersinks",
    "recognise_double_d_bores",
    "recognise_face_levels",
    "recognise_fillets",
    "recognise_flats",
    "recognise_grooves",
    "recognise_gusset_ribs",
    "recognise_gusset_rib_patterns",
    "recognise_hole_patterns",
    "recognise_holes",
    "recognise_plates",
    "recognise_pocket_patterns",
    "recognise_pockets",
    "recognise_polygonal_bosses",
    "recognise_polygonal_stock",
    "recognise_rectangular_pads",
    "recognise_rectangular_blind_slots",
    "recognise_repeating_radial_profiles",
    "recognise_round_bottom_blind_slots",
    "recognise_risers",
    "recognise_slot_patterns",
    "recognise_slots",
    "recognise_turned_steps",
}
SUBSTRATES = {
    "analyse_cylinders",
    "feature_diameters",
    "full_cylinders",
    "step_level_records",
}


def _goldens():
    for path in sorted(GOLDEN_ROOT.glob("*/expected.json")):
        yield path, json.loads(path.read_text(encoding="utf-8"))


def test_checked_in_goldens_are_canonical_and_bound_to_the_pinned_source():
    paths = []

    for path, golden in _goldens():
        paths.append(path)
        assert path.read_text(encoding="utf-8") == canonical_json(golden)
        assert golden["canonicalizer_version"] == CANONICALIZER_VERSION
        assert golden["fixture"] == path.parent.name
        # A golden says where it came from; it is no longer pinned to a Draftwright commit,
        # because the corpus has fixtures this package originated and they have no such source.
        assert golden.get("source", {}).get("repository") or golden.get("provenance", {}).get(
            "source_repository"
        )
        assert set(golden["recognition"]["individual"]) == PUBLIC_RECOGNISERS
        assert set(golden["recognition"]["substrates"]) == SUBSTRATES
        assert set(golden["recognition"]["aggregate"]) == {"prismatic", "rotational"}
        assert isinstance(golden["recognition"]["feature_census"], dict)

    assert paths


def test_corpus_exercises_every_public_recogniser_with_positive_evidence():
    exercised = {name: False for name in PUBLIC_RECOGNISERS}

    for _, golden in _goldens():
        for name, records in golden["recognition"]["individual"].items():
            exercised[name] |= bool(records)

    assert {name for name, seen in exercised.items() if not seen} == set()


def test_canonical_goldens_contain_no_object_representations_or_environment_paths():
    forbidden = (
        '"solid_idx"',
        "<build123d.",
        "<OCP.",
        " object at 0x",
        "/tmp/",
        "\\\\",
    )

    for path, _ in _goldens():
        text = path.read_text(encoding="utf-8")
        assert not any(token in text for token in forbidden), path
