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
from quiddity._definitions import FIRST_RELEASE
from quiddity._record import Record
from quiddity._registry import DERIVED_DEFINITIONS, PHYSICAL_DEFINITIONS, Counted

ROOT = Path(__file__).parents[1]
TARGET = ROOT / "src" / "quiddity" / "capabilities.json"

# Recognisers, output records, aggregate membership and census keys are derived from the
# registry below. What stays by hand is what ADR 0005 makes a deliberate contract: the
# evidence a family publishes, and the records that are not its registry output. A new
# family sets `introduced`; the default is the first release of this distribution.
EVIDENCE: dict[str, dict[str, object]] = {}

# Records a family publishes beyond its registry output records: nested values, evidence
# records, consumer aggregates and projections, with their roles and aggregate membership.
EXTRA_RECORDS: dict[str, list[tuple[str, str, list[str]]]] = {}


def _family_id(entrypoint: str) -> str:
    """``recognise_x_y`` publishes as family ``x-y``; a family that breaks the rule fails at
    the EVIDENCE lookup below with the derived id in the message."""

    return entrypoint.removeprefix("recognise_").replace("_", "-")


def _is_module_declared(definition: object) -> bool:
    """Whether the family describes itself, rather than being a literal in `_registry`.

    A declaration's discoverer is written in the family module. A registry literal's comes from
    `simple()` in `_definitions`, or is an adapter defined in `_registry` itself.
    """

    discover = getattr(definition, "discover", None) or getattr(definition, "derive", None)
    return getattr(discover, "__module__", "") not in {
        "quiddity._definitions",
        "quiddity._registry",
    }


def _registry_families() -> dict[str, dict[str, object]]:
    """One FAMILIES entry per registry definition whose entry point the package exports."""

    exported = set(recognition.__all__)
    families: dict[str, dict[str, object]] = {}
    definitions = [
        (d.public_entrypoint, "part", d.record_types, d.result_field, d.census, d.evidence, d)
        for d in PHYSICAL_DEFINITIONS
    ] + [
        (d.public_entrypoint, "derived", d.record_types, d.result_field, d.census, d.evidence, d)
        for d in DERIVED_DEFINITIONS
    ]
    for (
        entrypoint,
        kind,
        record_types,
        result_field,
        census_spec,
        declared,
        definition,
    ) in definitions:
        if entrypoint not in exported:
            continue
        family_id = _family_id(entrypoint)
        if declared is None and _is_module_declared(definition):
            raise KeyError(
                f"{family_id} is declared in its own module but names no ManifestEvidence; "
                "move its EVIDENCE and EXTRA_RECORDS entries into the declaration"
            )
        if declared is not None:
            if family_id in EVIDENCE:
                raise KeyError(f"{family_id} declares its evidence; remove its EVIDENCE entry")
            evidence: dict[str, object] = {
                key: list(value) if isinstance(value, tuple) else value
                for key, value in (
                    ("goldens", declared.goldens),
                    ("golden_paths", declared.golden_paths),
                    ("tests", declared.tests),
                    ("introduced", declared.introduced),
                )
                if value
            }
        elif family_id in EVIDENCE:
            evidence = EVIDENCE[family_id]
        else:
            raise KeyError(f"{family_id} is in the registry but has no EVIDENCE entry")
        if declared is not None:
            # Symmetric with the evidence rule above: declaring makes the family the owner, so an
            # empty `extra_records` means it has none, not that the table should still be read.
            if family_id in EXTRA_RECORDS:
                raise KeyError(f"{family_id} declares its extra records; remove its entry")
            extra = [(name, role, list(fields)) for name, role, fields in declared.extra_records]
        else:
            extra = EXTRA_RECORDS.get(family_id, [])
        overridden = {name for name, _role, _membership in extra}
        records = [
            (record.__name__, "output", [f"RecognitionResult.{result_field}"])
            for record in record_types
            if record.__name__ not in overridden
        ] + list(extra)
        census = census_spec.key if isinstance(census_spec, Counted) else None
        families[family_id] = {
            "recognisers": [(entrypoint, kind)],
            "records": records,
            "census": census,
            **evidence,
        }
    unknown = (set(EVIDENCE) | set(EXTRA_RECORDS)) - families.keys()
    if unknown:
        raise KeyError(f"no exported registry family for {sorted(unknown)}")
    return families


FAMILIES = _registry_families()

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
                [f"tests/golden/{name}/expected.json" for name in spec.get("goldens", [])]
                + spec.get("golden_paths", [])
            ),
            "id": family_id,
            "introduced_in": spec.get("introduced", FIRST_RELEASE),
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
