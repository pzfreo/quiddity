#!/usr/bin/env python3
"""Build the committed capability manifest deterministically from reviewed metadata.

This is a maintenance tool, not the validator. CI independently derives exports, dataclass
schemas, aggregate membership, census keys, and archive contents in
``tests/test_capability_manifest.py`` and compares those facts with the committed document.
"""

from __future__ import annotations

import argparse
import dataclasses
import inspect
import json
import types
import typing
from pathlib import Path

import quiddity as recognition
from quiddity._record import Record

ROOT = Path(__file__).parents[1]
TARGET = ROOT / "src" / "quiddity" / "capabilities.json"

FAMILIES = {
    # Quiddity 0.2.0 is the second alpha and starts this distribution's version history.
    # All inherited families arrive together; future additions can override `introduced`.
    "angled-steps": {
        "recognisers": [("recognise_angled_steps", "part")],
        "records": [("AngledStep", "output", ["RecognitionResult.angled_steps"])],
        "census": "angled_step",
        "goldens": ["angled_blind_step"],
        "introduced": "0.2.0",
        "tests": ["tests/test_angled_steps.py"],
    },
    "section-recesses": {
        "recognisers": [("recognise_section_recesses", "part")],
        "records": [
            ("ClosedSectionProfile", "nested", []),
            ("CylindricalEndSurface", "nested", []),
            ("PlanarEndSurface", "nested", []),
            ("PlanarEndTerm", "nested", []),
            ("PlanarEnvelopeEndSurface", "nested", []),
            ("OpenSectionProfile", "nested", []),
            ("SectionEnd", "nested", []),
            ("SectionRecess", "output", ["RecognitionResult.section_recesses"]),
            ("SectionRecessBodyRef", "nested", []),
            ("SectionRecessClassification", "nested", []),
            ("SectionRecessDocument", "aggregate", []),
            ("SectionRecessEnds", "nested", []),
            ("SectionRecessEvidence", "nested", []),
            ("SectionRecessFaceRef", "nested", []),
            ("SectionRecessGeometry", "nested", []),
            ("PassageFrame", "nested", []),
            ("PassageSection", "nested", []),
            ("PassageSectionVertex", "nested", []),
            ("SectionRecessRefusal", "projection", ["RecognitionResult.section_recess_refusals"]),
            ("SectionRecessArray", "projection", ["RecognitionResult.section_recess_patterns"]),
            ("SectionRecessGrid", "projection", ["RecognitionResult.section_recess_patterns"]),
        ],
        "census": "section_recess",
        "goldens": [],
        "golden_paths": [
            "tests/section_recess_expected.json",
            "tests/section_recess_geometry_expected.json",
        ],
        "introduced": "0.2.0",
        "tests": [
            "tests/test_section_recesses.py",
            "tests/test_section_recess_geometry_golden.py",
            "tests/test_section_recess_migration.py",
            "tests/test_section_adapter_rounding.py",
            "tests/test_corner_section.py",
            "tests/test_section_recess_cutover.py",
            "tests/test_open_channel_section.py",
        ],
    },
    "paired-ramp-steps": {
        "recognisers": [("recognise_paired_ramp_steps", "part")],
        "records": [("PairedRampStep", "output", ["RecognitionResult.paired_ramp_steps"])],
        "census": "paired_ramp_step",
        "goldens": ["paired_ramp_step"],
        "introduced": "0.2.0",
        "tests": ["tests/test_paired_ramp_steps.py"],
    },
    "through-steps": {
        "recognisers": [("recognise_through_steps", "part")],
        "records": [("ThroughStep", "output", ["RecognitionResult.through_steps"])],
        "census": "through_step",
        "goldens": ["rectangular_through_step"],
        "introduced": "0.2.0",
        "tests": ["tests/test_through_steps.py"],
    },
    "circular-blind-steps": {
        "recognisers": [("recognise_circular_blind_steps", "part")],
        "records": [("CircularBlindStep", "output", ["RecognitionResult.circular_blind_steps"])],
        "census": "circular_blind_step",
        "goldens": ["circular_blind_step"],
        "introduced": "0.2.0",
        "tests": ["tests/test_circular_blind_steps.py"],
    },
    "bosses": {
        "recognisers": [("recognise_bosses", "part")],
        "records": [("BossRecord", "output", ["RecognitionResult.bosses"])],
        "census": "boss",
        "goldens": ["simple_through_hole", "turned_steps_and_grooves"],
    },
    "blends": {
        "recognisers": [("recognise_blends", "part")],
        "records": [
            ("Blend", "output", ["RecognitionResult.blends"]),
            ("CircularBlendPath", "nested", ["RecognitionResult.blends.path"]),
            ("StraightBlendPath", "nested", ["RecognitionResult.blends.path"]),
        ],
        "census": "blend",
        "goldens": [
            "small_convex_blends",
            "toroidal_blend_compound",
            "toroidal_blend_internal",
            "toroidal_blends_turned",
        ],
        "introduced": "0.2.0",
        "tests": ["tests/test_blends.py", "tests/test_blend_view.py"],
    },
    "chamfers": {
        "recognisers": [("recognise_chamfers", "part")],
        "records": [("Chamfer", "output", ["RecognitionResult.chamfers"])],
        "census": "chamfer",
        "goldens": ["chamfers_fillets_and_flats"],
        "tests": ["tests/test_turned_chamfers.py"],
    },
    "countersinks": {
        "recognisers": [("recognise_countersinks", "part")],
        "records": [
            (
                "CounterSink",
                "output",
                ["RecognitionResult.countersinks", "RecognitionResult.holes.csink"],
            )
        ],
        "census": "countersink",
        "goldens": ["counterbored_and_countersunk_holes"],
    },
    "double-d-bores": {
        "recognisers": [("recognise_double_d_bores", "part")],
        "records": [("DoubleDBore", "output", ["RecognitionResult.double_d_bores"])],
        "census": None,
        "goldens": ["double_d_bore"],
    },
    "face-levels": {
        "recognisers": [("recognise_face_levels", "part")],
        "records": [("FaceLevel", "evidence", ["RecognitionResult.step_levels"])],
        "census": None,
        "goldens": ["plates_pads_levels_and_slanted_steps", "slanted_steps"],
    },
    "fillets": {
        "recognisers": [("recognise_fillets", "part")],
        "records": [("Fillet", "output", ["RecognitionResult.fillets"])],
        "census": "fillet",
        "goldens": ["chamfers_fillets_and_flats"],
        "tests": ["tests/test_turned_chamfers.py"],
    },
    "flats": {
        "recognisers": [("recognise_flats", "part")],
        "records": [("Flat", "output", ["RecognitionResult.flats"])],
        "census": "flat",
        "goldens": ["chamfers_fillets_and_flats"],
    },
    "grooves": {
        "recognisers": [("recognise_grooves", "part")],
        "records": [("Groove", "output", ["RecognitionResult.grooves"])],
        "census": "groove",
        "goldens": ["turned_steps_and_grooves"],
    },
    "hole-patterns": {
        "recognisers": [("recognise_hole_patterns", "derived")],
        "records": [
            ("BoltCircle", "output", ["RecognitionResult.hole_patterns"]),
            ("LinearArray", "output", ["RecognitionResult.hole_patterns"]),
            ("RectGrid", "output", ["RecognitionResult.hole_patterns"]),
        ],
        "census": "hole_pattern",
        "goldens": ["bolt_circle_and_rectangular_grid"],
    },
    "holes": {
        "recognisers": [("recognise_holes", "part")],
        "records": [
            (
                "CounterBore",
                "nested",
                ["RecognitionResult.holes.cbore", "RecognitionResult.holes.spotface"],
            ),
            ("HoleRecord", "output", ["RecognitionResult.holes"]),
            ("HoleSpec", "evidence", []),
        ],
        "census": "hole",
        "goldens": ["simple_through_hole", "counterbored_and_countersunk_holes"],
    },
    "plates": {
        "recognisers": [("recognise_plates", "part")],
        "records": [("Plate", "output", ["RecognitionResult.plates"])],
        "census": "plate",
        "goldens": ["plates_pads_levels_and_slanted_steps"],
        "tests": ["tests/test_channel_plate_body_identity.py"],
    },
    "polygonal-bosses": {
        "recognisers": [("recognise_polygonal_bosses", "part")],
        "records": [("PolygonalBoss", "output", ["RecognitionResult.polygonal_bosses"])],
        "census": None,
        "goldens": ["polygonal_boss"],
    },
    "polygonal-stock": {
        "recognisers": [("recognise_polygonal_stock", "part")],
        "records": [("PolygonalStock", "output", ["RecognitionResult.polygonal_stock"])],
        "census": None,
        "goldens": ["polygonal_stock"],
    },
    "rectangular-pads": {
        "recognisers": [("recognise_rectangular_pads", "part")],
        "records": [("RaisedPad", "output", ["RecognitionResult.pads"])],
        "census": None,
        "goldens": ["plates_pads_levels_and_slanted_steps"],
        "tests": [
            "docs/benchmarks/nurbs-conversion-sweep.json",
            "tests/test_nurbs_conversion_sweep.py",
            "tests/test_pad_attribution.py",
        ],
    },
    "repeating-radial-profiles": {
        "recognisers": [("recognise_repeating_radial_profiles", "part")],
        "records": [
            (
                "RepeatingRadialProfile",
                "evidence",
                ["RecognitionResult.repeating_radial_profiles"],
            )
        ],
        "census": None,
        "goldens": ["repeating_radial_profile", "traversal_order"],
    },
    "risers": {
        "recognisers": [("recognise_risers", "part")],
        "records": [
            ("RiserEvidence", "evidence", ["RecognitionResult.risers"]),
            ("StepShoulder", "projection", []),
        ],
        "census": None,
        "goldens": ["plates_pads_levels_and_slanted_steps", "slanted_steps"],
    },
    "slot-patterns": {
        "recognisers": [("recognise_slot_patterns", "derived")],
        "records": [
            ("SlotArray", "output", ["RecognitionResult.slot_patterns"]),
            ("SlotGrid", "output", ["RecognitionResult.slot_patterns"]),
        ],
        "census": None,
        "goldens": ["straight_and_obround_slots"],
    },
    "oriented-slots": {
        "recognisers": [("recognise_oriented_slots", "part")],
        "records": [
            ("OrientedSlot", "output", ["RecognitionResult.oriented_slots"]),
            ("PassageEnds", "nested", []),
            ("SectionPassage", "nested", []),
        ],
        "census": "oriented_slot",
        "goldens": [],
        "golden_paths": ["tests/golden/oriented_slots/contract.json"],
        "introduced": "0.2.0",
        "tests": ["tests/test_oriented_slots.py"],
    },
    "oriented-slot-patterns": {
        "recognisers": [("recognise_oriented_slot_patterns", "derived")],
        "records": [
            (
                "OrientedSlotArray",
                "output",
                ["RecognitionResult.oriented_slot_patterns"],
            ),
            (
                "OrientedSlotGrid",
                "output",
                ["RecognitionResult.oriented_slot_patterns"],
            ),
        ],
        "census": None,
        "goldens": [],
        "golden_paths": ["tests/golden/oriented_slots/contract.json"],
        "introduced": "0.2.0",
        "tests": ["tests/test_oriented_slots.py"],
    },
    "slots": {
        "recognisers": [("recognise_slots", "part")],
        "records": [("Slot", "output", ["RecognitionResult.slots"])],
        "census": "slot",
        "goldens": ["straight_and_obround_slots"],
    },
    "turned-steps": {
        "recognisers": [("recognise_turned_steps", "part")],
        "records": [
            ("TurnedProfile", "aggregate", []),
            ("TurnedProfileKey", "nested", []),
            ("TurnedStep", "output", ["RecognitionResult.turned_steps"]),
        ],
        "census": "step",
        "goldens": ["turned_steps_and_grooves"],
    },
}

RECORD_SCHEMA_VERSIONS = {
    "SectionEnd": 2,
    "SectionRecessEnds": 2,
    "SectionRecessGeometry": 2,
    "SectionRecess": 2,
    "SectionRecessDocument": 3,
    "Channel": 2,
    "Blend": 3,
    "Chamfer": 2,
    "FaceLevel": 2,
    "Fillet": 2,
    "Groove": 2,
    "RaisedPad": 2,
    "PassageEnds": 2,
    "Plate": 2,
    "PassageSection": 2,
    "RiserEvidence": 3,
    "TurnedProfile": 2,
    "TurnedProfileKey": 2,
    "TurnedStep": 2,
    "ThroughStep": 2,
    "SectionPassage": 2,
}

NO_MEMBERSHIP_RATIONALE = {
    "ClosedSectionProfile": "Nested only in SectionRecessGeometry.",
    "OpenSectionProfile": "Nested only in SectionRecessGeometry.",
    "SectionEnd": "Nested only in SectionRecessEnds.",
    "CylindricalEndSurface": "Native cylindrical branch nested only in SectionEnd.",
    "PlanarEndSurface": "Planar boundary nested only in SectionEnd.",
    "PlanarEndTerm": "Absolute local-run plane term nested only in PlanarEnvelopeEndSurface.",
    "PlanarEnvelopeEndSurface": "Observed two-plane min/max boundary nested only in SectionEnd.",
    "SectionRecessBodyRef": "Nested only in SectionRecessDocument.",
    "SectionRecessClassification": "Nested only in SectionRecess.",
    "SectionRecessDocument": "Public JSON envelope built outside RecognitionResult.",
    "SectionRecessEnds": "Nested only in SectionRecessGeometry.",
    "SectionRecessEvidence": "Nested only in SectionRecess.",
    "SectionRecessFaceRef": "Nested only in SectionRecessDocument.",
    "SectionRecessGeometry": "Nested only in SectionRecess.",
    "HoleSpec": (
        "Derived grouping key; it is computed from HoleRecord and is not retained by "
        "RecognitionResult."
    ),
    "StepShoulder": "Pure consumer projection from RiserEvidence plus a caller-supplied level set.",
    "TurnedProfile": "Consumer aggregate built on demand from RecognitionResult.turned_steps.",
    "TurnedProfileKey": (
        "Nested physical-profile membership retained by Groove, TurnedStep and TurnedProfile."
    ),
    "PassageEnds": "Nested only in SectionPassage; retained to preserve explicit end topology.",
    "SectionPassage": "Nested source geometry of OrientedSlot; no standalone aggregate family.",
    "PassageFrame": "Shared placement of SectionRecessGeometry and OrientedSlot source geometry.",
    "PassageSection": (
        "Shared closed boundary of ClosedSectionProfile and OrientedSlot source geometry."
    ),
    "PassageSectionVertex": "Nested only in PassageSection.",
    "OpenPolygonalSection": "Nested only in EdgeOpenPrismaticRecess.",
    "OpenSectionOpening": "Nested only in OpenPolygonalSection.",
    "OpenCircularSection": "Nested only in EdgeOpenCircularPocket.",
    "OpenCircularSectionSegment": "Nested only in OpenCircularSection.",
}


def _union_type(args: tuple[object, ...]) -> str:
    rendered = sorted({_type_name(arg) for arg in args}, key=lambda value: (value == "null", value))
    return "|".join(rendered)


def _type_name(annotation: object) -> str:
    if annotation is type(None):
        return "null"
    if annotation in {bool, int, float, str}:
        return typing.cast(type, annotation).__name__
    if inspect.isclass(annotation) and issubclass(typing.cast(type, annotation), Record):
        return f"record:{typing.cast(type, annotation).__name__}"
    origin = typing.get_origin(annotation)
    args = typing.get_args(annotation)
    if origin in {typing.Union, types.UnionType}:
        return _union_type(args)
    if origin is tuple:
        if len(args) == 2 and args[1] is Ellipsis:
            return f"list[{_type_name(args[0])}]"
        rendered = [_type_name(arg) for arg in args]
        if len(set(rendered)) == 1:
            return f"tuple[{rendered[0]},{len(rendered)}]"
        return f"list[{_union_type(args)}]"
    raise TypeError(f"unsupported manifest annotation: {annotation!r}")


def _units(field: dataclasses.Field, annotation: object) -> str:
    name = field.name
    rendered = _type_name(annotation)
    if name == "sweep":
        return "rad"
    if name in {"angle", "included_angle"}:
        return "deg"
    if name in {
        "axis_direction",
        "depth_direction",
        "flat_direction",
        "flat_directions",
        "long_direction",
        "row_direction",
        "col_direction",
        "normal",
        "run",
        "u",
        "v",
        "width_direction",
    }:
        return "unit-vector"
    if name == "direction" and rendered.startswith("tuple[float,3]"):
        return "unit-vector"
    if name == "axis" and rendered.startswith("tuple[float,3]"):
        return "unit-vector"
    if (
        name
        in {
            "bulge",
            "constituent_faces",
            "members",
            "defining_faces",
            "gradient",
            "low_gradient",
            "high_gradient",
        }
        or rendered in {"bool", "int", "str"}
        or rendered.startswith("record:")
    ):
        return "none"
    if rendered.startswith("list[record:") or name in {
        "body_key",
        "cbore",
        "csink",
        "holes",
        "pockets",
        "sector_signature",
        "slots",
        "spotface",
        "steps",
    }:
        return "none"
    return "mm"


def _record(name: str, role: str, membership: list[str]) -> dict[str, object]:
    record_type = getattr(recognition, name)
    hints = typing.get_type_hints(record_type)
    fields = {}
    for field in sorted(dataclasses.fields(record_type), key=lambda item: item.name):
        annotation = hints[field.name]
        fields[field.name] = {
            "required": field.default is dataclasses.MISSING
            and field.default_factory is dataclasses.MISSING,
            "type": _type_name(annotation),
            "units": _units(field, annotation),
        }
    result: dict[str, object] = {
        "aggregate_membership": membership,
        "fields": fields,
        "name": name,
        "qualified_name": f"quiddity.{name}",
        "role": role,
        "schema_version": RECORD_SCHEMA_VERSIONS.get(name, 1),
    }
    if not membership:
        result["aggregate_membership_rationale"] = NO_MEMBERSHIP_RATIONALE[name]
    return result


def build_manifest() -> dict[str, object]:
    families = []
    for family_id, spec in sorted(FAMILIES.items()):
        census = spec["census"]
        records = [_record(*record) for record in sorted(spec["records"])]
        census_output = None
        if census is not None:
            outputs = [record for record in records if record["role"] == "output"]
            direct = sorted(
                {
                    path
                    for output in outputs
                    for path in output["aggregate_membership"]
                    if path.count(".") == 1
                }
            )
            if len(direct) != 1:
                raise ValueError(f"{family_id} needs exactly one census output")
            census_output = direct[0]
        family: dict[str, object] = {
            "census_name": census,
            "census_output": census_output,
            "documentation": ["docs/capabilities.md#proven-recognition-capability"],
            "golden_evidence": sorted(
                [f"tests/golden/{name}/expected.json" for name in spec["goldens"]]
                + spec.get("golden_paths", [])
            ),
            "id": family_id,
            "introduced_in": spec.get("introduced", "0.2.0"),
            "recognisers": [
                (
                    {
                        "entry_point": f"quiddity.{name}",
                        "kind": kind,
                        "role": role,
                    }
                    | (
                        {
                            "ledger_state": "unavailable",
                            "remove_in": "1.0.0",
                            "replacement": ("quiddity.recognise_section_passages"),
                        }
                        if role == "compatibility"
                        else {}
                    )
                )
                for recogniser in spec["recognisers"]
                for name, kind, role in (
                    recogniser
                    if len(recogniser) == 3
                    else (*recogniser, "derived" if recogniser[1] == "derived" else "physical"),
                )
            ],
            "records": records,
            "status": "supported",
            "test_evidence": sorted(
                [
                    "tests/test_capability_claims.py",
                    "tests/test_recogniser_contract.py",
                    *spec.get("tests", []),
                ]
            ),
        }
        if census is None:
            family["census_rationale"] = (
                "Geometry evidence is deliberately absent from the feature census; "
                "the census is not a completeness denominator."
            )
        families.append(family)
    return {
        "aliases": [],
        "families": families,
        "format": "quiddity-capabilities",
        "format_version": 2,
        "package": {"name": "quiddity", "version": recognition.__version__},
    }


def rendered_manifest() -> str:
    return json.dumps(build_manifest(), indent=2, sort_keys=True) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true", help="fail if the committed JSON differs")
    parser.add_argument("--write", action="store_true", help="replace the committed JSON")
    args = parser.parse_args()
    rendered = rendered_manifest()
    if args.check:
        if not TARGET.exists() or TARGET.read_text(encoding="utf-8") != rendered:
            parser.error(f"{TARGET.relative_to(ROOT)} is stale; run this tool with --write")
        return 0
    if args.write:
        TARGET.write_text(rendered, encoding="utf-8")
        return 0
    print(rendered, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
