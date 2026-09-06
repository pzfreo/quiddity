# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Public access to the installed, versioned recognition capability manifest."""

from __future__ import annotations

import argparse
import copy
import json
import re
from importlib.resources import files
from typing import Any, TypeAlias, cast

from quiddity import __version__

CAPABILITY_FORMAT = "quiddity-capabilities"
CAPABILITY_FORMAT_VERSION = 2
JSONValue: TypeAlias = None | bool | int | float | str | list["JSONValue"] | dict[str, "JSONValue"]
CapabilityManifest: TypeAlias = dict[str, JSONValue]
_FAMILY_ID = re.compile(r"^[a-z][a-z0-9]*(?:[.-][a-z0-9]+)*$")
_ROLES = {"aggregate", "evidence", "nested", "output", "projection"}
_STATUSES = {"deferred", "supported", "unsupported"}
_UNITS = {"deg", "mm", "none", "rad", "unit-vector"}
_RECORD_TYPE = re.compile(r"^record:[A-Z][A-Za-z0-9]*$")
_VERSION = re.compile(r"^(\d+)\.(\d+)\.(\d+)(?:[.+-][A-Za-z0-9.-]+)?$")


class CapabilityManifestError(ValueError):
    """The installed manifest is missing, stale, or uses an unsupported format."""


def _keys(value: dict[str, Any], allowed: set[str], context: str) -> None:
    unknown = sorted(set(value) - allowed)
    if unknown:
        raise CapabilityManifestError(f"{context} has unknown fields: {', '.join(unknown)}")


def _paths(value: object, context: str, *, allow_empty: bool = False) -> None:
    if not isinstance(value, list) or (not value and not allow_empty):
        raise CapabilityManifestError(f"{context} must be a non-empty path list")
    for path in value:
        if (
            not isinstance(path, str)
            or not path
            or path.startswith("/")
            or ".." in path.split("/")
            or "\\" in path
        ):
            raise CapabilityManifestError(f"{context} contains an invalid source-relative path")
    if value != sorted(set(value)):
        raise CapabilityManifestError(f"{context} must contain unique, sorted paths")


def _version(value: object, context: str) -> tuple[int, int, int]:
    if not isinstance(value, str) or not (match := _VERSION.fullmatch(value)):
        raise CapabilityManifestError(f"{context} must be a semantic package version")
    major, minor, patch = match.groups()
    return int(major), int(minor), int(patch)


def _split_union(value: str) -> list[str]:
    parts = []
    depth = 0
    start = 0
    for index, char in enumerate(value):
        if char == "[":
            depth += 1
        elif char == "]":
            depth -= 1
            if depth < 0:
                return []
        elif char == "|" and depth == 0:
            parts.append(value[start:index])
            start = index + 1
    if depth:
        return []
    parts.append(value[start:])
    return parts


def _valid_type(value: object) -> bool:
    if not isinstance(value, str) or not value or any(char.isspace() for char in value):
        return False
    union = _split_union(value)
    if len(union) > 1:
        return all(_valid_type(member) for member in union) and len(union) == len(set(union))
    if value in {"bool", "float", "int", "null", "str"} or _RECORD_TYPE.fullmatch(value):
        return True
    if value.startswith("list[") and value.endswith("]"):
        return _valid_type(value[5:-1])
    if value.startswith("tuple[") and value.endswith("]"):
        inner = value[6:-1]
        depth = 0
        comma = -1
        for index, char in enumerate(inner):
            if char == "[":
                depth += 1
            elif char == "]":
                depth -= 1
            elif char == "," and depth == 0:
                comma = index
        count = inner[comma + 1 :]
        return comma > 0 and count.isdigit() and int(count) > 0 and _valid_type(inner[:comma])
    return False


def _validate_aliases(
    aliases: object,
    family_ids: set[str],
    record_names: set[str],
    package_version: tuple[int, int, int],
) -> None:
    if not isinstance(aliases, list):
        raise CapabilityManifestError("aliases must be an array")
    keys = []
    replacements: dict[tuple[str, str], str] = {}
    for index, alias in enumerate(aliases):
        context = f"aliases[{index}]"
        if not isinstance(alias, dict):
            raise CapabilityManifestError(f"{context} must be an object")
        _keys(
            alias,
            {"deprecated_in", "kind", "old", "rationale", "remove_in", "replacement"},
            context,
        )
        if set(alias) != {
            "deprecated_in",
            "kind",
            "old",
            "rationale",
            "remove_in",
            "replacement",
        }:
            raise CapabilityManifestError(f"{context} is missing required fields")
        if alias.get("kind") not in {"family", "record"}:
            raise CapabilityManifestError(f"{context}.kind must be family or record")
        if not all(isinstance(alias.get(key), str) and alias[key] for key in alias):
            raise CapabilityManifestError(f"{context} values must be non-empty strings")
        key = (alias["kind"], alias["old"])
        keys.append(key)
        replacements[key] = alias["replacement"]
        current_names = family_ids if alias["kind"] == "family" else record_names
        if alias["old"] in current_names:
            raise CapabilityManifestError(f"{context} reuses a current name")
        deprecated = _version(alias["deprecated_in"], f"{context}.deprecated_in")
        removal = _version(alias["remove_in"], f"{context}.remove_in")
        if deprecated >= removal:
            raise CapabilityManifestError(f"{context} removal must follow deprecation")
        if package_version >= removal:
            raise CapabilityManifestError(f"{context} has passed its removal version")
    if keys != sorted(keys) or len(keys) != len(set(keys)):
        raise CapabilityManifestError("aliases must be unique and sorted by kind then old name")
    for start in replacements:
        seen = set()
        current = start
        while current in replacements:
            if current in seen:
                raise CapabilityManifestError(f"alias cycle includes {start[1]!r}")
            seen.add(current)
            current = (start[0], replacements[current])
        current_names = family_ids if start[0] == "family" else record_names
        if current[1] not in current_names:
            raise CapabilityManifestError(f"alias {start[1]!r} replaces an unknown {start[0]}")


def _validate_record(record: object, family_id: str, index: int) -> str:
    context = f"family {family_id!r} record[{index}]"
    if not isinstance(record, dict):
        raise CapabilityManifestError(f"{context} must be an object")
    allowed = {
        "aggregate_membership",
        "aggregate_membership_rationale",
        "fields",
        "name",
        "qualified_name",
        "role",
        "schema_version",
    }
    _keys(record, allowed, context)
    required = allowed - {"aggregate_membership_rationale"}
    if not required <= record.keys():
        raise CapabilityManifestError(f"{context} is missing required fields")
    name = record["name"]
    if not isinstance(name, str) or not name:
        raise CapabilityManifestError(f"{context}.name must be a non-empty string")
    if record["qualified_name"] != f"quiddity.{name}":
        raise CapabilityManifestError(f"{context} has a non-public qualified name")
    if record["role"] not in _ROLES:
        raise CapabilityManifestError(f"{context} has unknown role {record['role']!r}")
    if type(record["schema_version"]) is not int or record["schema_version"] < 1:
        raise CapabilityManifestError(f"{context}.schema_version must be a positive integer")
    membership = record["aggregate_membership"]
    if not isinstance(membership, list) or not all(
        isinstance(path, str) and path.startswith("RecognitionResult.") for path in membership
    ):
        raise CapabilityManifestError(f"{context}.aggregate_membership is invalid")
    if membership != sorted(set(membership)):
        raise CapabilityManifestError(f"{context}.aggregate_membership must be unique and sorted")
    rationale = record.get("aggregate_membership_rationale")
    if not membership and (not isinstance(rationale, str) or not rationale):
        raise CapabilityManifestError(f"{context} needs an empty-membership rationale")
    if membership and rationale is not None:
        raise CapabilityManifestError(f"{context} has a rationale despite aggregate membership")
    fields = record["fields"]
    if not isinstance(fields, dict) or not fields or list(fields) != sorted(fields):
        raise CapabilityManifestError(f"{context}.fields must be non-empty and name-sorted")
    for field_name, field in fields.items():
        field_context = f"{context}.{field_name}"
        if not isinstance(field, dict):
            raise CapabilityManifestError(f"{field_context} must be an object")
        _keys(field, {"required", "type", "units"}, field_context)
        if set(field) != {"required", "type", "units"}:
            raise CapabilityManifestError(f"{field_context} is missing schema data")
        if not isinstance(field["required"], bool):
            raise CapabilityManifestError(f"{field_context}.required must be boolean")
        if not _valid_type(field["type"]):
            raise CapabilityManifestError(
                f"{field_context}.type is outside capability type grammar"
            )
        if field["units"] not in _UNITS:
            raise CapabilityManifestError(f"{field_context} has unknown units {field['units']!r}")
    return name


def validate_capability_manifest(manifest: object) -> None:
    """Validate format-2 document structure without trusting its capability claims.

    Package CI separately derives runtime and archive inventories; this function is the
    supported fail-closed schema check useful to callers before they join consumer declarations.
    """
    if not isinstance(manifest, dict):
        raise CapabilityManifestError("capability manifest must be a JSON object")
    _keys(manifest, {"aliases", "families", "format", "format_version", "package"}, "manifest")
    if set(manifest) != {"aliases", "families", "format", "format_version", "package"}:
        raise CapabilityManifestError("capability manifest is missing required top-level fields")
    if manifest["format"] != CAPABILITY_FORMAT:
        raise CapabilityManifestError(
            f"unsupported capability document kind {manifest['format']!r}"
        )
    if (
        type(manifest["format_version"]) is not int
        or manifest["format_version"] != CAPABILITY_FORMAT_VERSION
    ):
        raise CapabilityManifestError(
            f"unsupported capability format version {manifest['format_version']!r}"
        )
    package = manifest["package"]
    if not isinstance(package, dict):
        raise CapabilityManifestError("package must be an object")
    _keys(package, {"name", "version"}, "package")
    if set(package) != {"name", "version"} or package["name"] != "quiddity":
        raise CapabilityManifestError("package identity must be quiddity with a version")
    if not isinstance(package["version"], str) or not package["version"]:
        raise CapabilityManifestError("package.version must be a non-empty string")
    package_version = _version(package["version"], "package.version")
    families = manifest["families"]
    if not isinstance(families, list) or not families:
        raise CapabilityManifestError("families must be a non-empty array")
    ids = []
    record_names = []
    entry_points = []
    family_allowed = {
        "census_name",
        "census_output",
        "census_rationale",
        "documentation",
        "golden_evidence",
        "id",
        "introduced_in",
        "rationale",
        "recognisers",
        "records",
        "status",
        "test_evidence",
    }
    for index, family in enumerate(families):
        context = f"families[{index}]"
        if not isinstance(family, dict):
            raise CapabilityManifestError(f"{context} must be an object")
        _keys(family, family_allowed, context)
        required = family_allowed - {"census_rationale", "rationale"}
        if not required <= family.keys():
            raise CapabilityManifestError(f"{context} is missing required fields")
        family_id = family["id"]
        if not isinstance(family_id, str) or not _FAMILY_ID.fullmatch(family_id):
            raise CapabilityManifestError(f"{context}.id is invalid")
        ids.append(family_id)
        status = family["status"]
        if status not in _STATUSES:
            raise CapabilityManifestError(f"family {family_id!r} has unknown status {status!r}")
        introduced = _version(family["introduced_in"], f"family {family_id!r}.introduced_in")
        if introduced > package_version:
            raise CapabilityManifestError(f"family {family_id!r} is introduced after this package")
        recognisers = family["recognisers"]
        records = family["records"]
        if not isinstance(recognisers, list):
            raise CapabilityManifestError(f"family {family_id!r}.recognisers must be an array")
        if not isinstance(records, list):
            raise CapabilityManifestError(f"family {family_id!r}.records must be an array")
        if status == "supported" and (not recognisers or not records):
            raise CapabilityManifestError(f"supported family {family_id!r} needs runtime entries")
        if status != "supported":
            if recognisers or records or family["golden_evidence"]:
                raise CapabilityManifestError(
                    f"reserved family {family_id!r} claims runtime support"
                )
            if not isinstance(family.get("rationale"), str) or not family["rationale"]:
                raise CapabilityManifestError(f"reserved family {family_id!r} needs rationale")
        family_entries = []
        for recogniser in recognisers:
            if not isinstance(recogniser, dict):
                raise CapabilityManifestError(f"family {family_id!r} recogniser must be an object")
            _keys(
                recogniser,
                {
                    "entry_point",
                    "kind",
                    "ledger_state",
                    "remove_in",
                    "replacement",
                    "role",
                },
                f"family {family_id!r} recogniser",
            )
            base_keys = {"entry_point", "kind", "role"}
            compatibility_keys = {"ledger_state", "remove_in", "replacement"}
            expected_keys = (
                base_keys | compatibility_keys
                if recogniser.get("role") == "compatibility"
                else base_keys
            )
            if set(recogniser) != expected_keys or recogniser["kind"] not in {
                "derived",
                "part",
            }:
                raise CapabilityManifestError(f"family {family_id!r} recogniser is invalid")
            if recogniser["role"] not in {"compatibility", "derived", "physical"}:
                raise CapabilityManifestError(f"family {family_id!r} recogniser role is invalid")
            if recogniser["role"] == "compatibility":
                if recogniser["ledger_state"] != "unavailable":
                    raise CapabilityManifestError(
                        f"family {family_id!r} compatibility ledger state is invalid"
                    )
                _version(recogniser["remove_in"], f"family {family_id!r} remove_in")
                if not (
                    isinstance(recogniser["replacement"], str)
                    and recogniser["replacement"].startswith("quiddity.recognise_")
                ):
                    raise CapabilityManifestError(
                        f"family {family_id!r} compatibility replacement is invalid"
                    )
            entry = recogniser["entry_point"]
            if not isinstance(entry, str) or not entry.startswith("quiddity.recognise_"):
                raise CapabilityManifestError(f"family {family_id!r} entry point is not public")
            family_entries.append(entry)
            entry_points.append(entry)
        if family_entries != sorted(family_entries):
            raise CapabilityManifestError(f"family {family_id!r} recognisers are not sorted")
        family_records = [
            _validate_record(record, family_id, i) for i, record in enumerate(records)
        ]
        if family_records != sorted(family_records):
            raise CapabilityManifestError(f"family {family_id!r} records are not sorted")
        record_names.extend(family_records)
        if family["census_name"] is None:
            if (
                not isinstance(family.get("census_rationale"), str)
                or not family["census_rationale"]
            ):
                raise CapabilityManifestError(f"family {family_id!r} needs census_rationale")
            if family["census_output"] is not None:
                raise CapabilityManifestError(f"family {family_id!r} has census output without key")
        elif not isinstance(family["census_name"], str) or not family["census_name"]:
            raise CapabilityManifestError(f"family {family_id!r}.census_name is invalid")
        elif not isinstance(family["census_output"], str) or not family["census_output"].startswith(
            "RecognitionResult."
        ):
            raise CapabilityManifestError(f"family {family_id!r}.census_output is invalid")
        for key in ("documentation", "golden_evidence", "test_evidence"):
            _paths(family[key], f"family {family_id!r}.{key}", allow_empty=status != "supported")
    if ids != sorted(ids) or len(ids) != len(set(ids)):
        raise CapabilityManifestError("family IDs must be unique and sorted")
    if len(record_names) != len(set(record_names)):
        raise CapabilityManifestError("record names must have exactly one primary family")
    if len(entry_points) != len(set(entry_points)):
        raise CapabilityManifestError("recognisers must belong to exactly one family")
    _validate_aliases(manifest["aliases"], set(ids), set(record_names), package_version)


def _load_manifest() -> dict[str, Any]:
    resource = files("quiddity").joinpath("capabilities.json")
    manifest = cast(dict[str, Any], json.loads(resource.read_text(encoding="utf-8")))
    validate_capability_manifest(manifest)
    package = manifest.get("package")
    if not isinstance(package, dict) or package.get("version") != __version__:
        actual = package.get("version") if isinstance(package, dict) else None
        raise CapabilityManifestError(
            f"capability manifest version {actual!r} does not match installed package "
            f"version {__version__!r}; rebuild the distribution with a current manifest"
        )
    return manifest


def capability_manifest(*, format_version: int = CAPABILITY_FORMAT_VERSION) -> CapabilityManifest:
    """Return an isolated JSON-compatible copy of the installed capability manifest.

    Consumers must request a format version they understand. Unknown versions fail closed rather
    than silently treating future families or states as unsupported geometry.
    """
    if format_version != CAPABILITY_FORMAT_VERSION:
        raise CapabilityManifestError(
            f"unsupported requested capability format version {format_version!r}; "
            f"installed package exposes version {CAPABILITY_FORMAT_VERSION}"
        )
    return cast(CapabilityManifest, copy.deepcopy(_load_manifest()))


def capability_manifest_json(*, format_version: int = CAPABILITY_FORMAT_VERSION) -> str:
    """Return the installed manifest as deterministic, newline-terminated JSON."""
    return (
        json.dumps(capability_manifest(format_version=format_version), indent=2, sort_keys=True)
        + "\n"
    )


def main(argv: list[str] | None = None) -> int:
    """Print the installed manifest for downstream CI and command-line inspection."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--format-version",
        type=int,
        default=CAPABILITY_FORMAT_VERSION,
        help="manifest schema major understood by the caller",
    )
    args = parser.parse_args(argv)
    print(capability_manifest_json(format_version=args.format_version), end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
