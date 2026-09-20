# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Supported geometry reads shared by declared and recognised features.

This is the deliberately narrow F7 API from ADR 0010.  It publishes only the five
consumer-proven inspection operations; graph identity, adjacency, blend collapse
and recognition evidence remain private or experimental.

``experimental_geometry.inspect_face`` remains an identity-preserving compatibility
alias.  New consumers should import the supported names from this module.

The analytic surface-fact core lives in the ``_surface_facts`` leaf and is republished here
under this module's name, so the geometry facade can use it without importing the family
readers below.
"""

from __future__ import annotations

import copy
import json
import re
from functools import partial
from importlib.resources import files
from typing import Any, TypeAlias, cast

from quiddity._bevel import BevelReject, classify_bevel
from quiddity._manifest import check_keys as _check_keys
from quiddity._manifest import check_object as _check_object
from quiddity._manifest import parse_version as _parse_version
from quiddity._surface_facts import (
    AnalyticSurface,
    FaceInspection,
    OrientationCapability,
    RefusedSurface,
    SurfaceFact,
    SurfaceKind,
    SurfaceProvenance,
    SurfaceRefusalReason,
    inspect_face,
)
from quiddity.countersinks import cone_rims
from quiddity.grooves import floor_face_anchor
from quiddity.profiled_bores import read_double_d_tool

INSPECTION_API_FORMAT = "quiddity-inspection-api"
INSPECTION_API_FORMAT_VERSION = 1
_INSPECTION_API_MAJOR = 1
_INSPECTION_NAMESPACE = "quiddity.inspection"
_SYMBOL = re.compile(r"^[A-Za-z][A-Za-z0-9_]*$")
_QUALIFIED = re.compile(r"^quiddity(?:\.[A-Za-z][A-Za-z0-9_]*)+$")
_KINDS = {"dataclass", "enum", "exception", "function", "type-alias"}
_PARAMETER_KINDS = {"plane", "cylinder", "cone", "sphere"}
_PARAMETER_UNITS = {"model-length", "radian", "unitless"}
_RETURN_UNITS = {"model-length", "unitless"}

InspectionApiManifest: TypeAlias = dict[str, Any]


class InspectionApiManifestError(ValueError):
    """The installed inspection API manifest is missing, stale, or unsupported."""


_keys = partial(_check_keys, error=InspectionApiManifestError)
_version = partial(_parse_version, error=InspectionApiManifestError)
_object = partial(_check_object, error=InspectionApiManifestError)


def _validate_package(package: object) -> tuple[int, int, int]:
    """Return the package version the document claims to have been produced by."""

    if not isinstance(package, dict):
        raise InspectionApiManifestError("package must be an object")
    _keys(package, {"name", "version"}, "package")
    if set(package) != {"name", "version"} or package["name"] != "quiddity":
        raise InspectionApiManifestError("package identity must be quiddity with a version")
    return _version(package["version"], "package.version")


def _validate_surface_parameters(surface_parameters: object) -> None:
    """Check the parameter layout published for each of the four analytic surface kinds."""

    if not isinstance(surface_parameters, dict) or set(surface_parameters) != _PARAMETER_KINDS:
        raise InspectionApiManifestError(
            "api.surface_parameters must define the four supported surface kinds"
        )
    for surface_kind, layout in surface_parameters.items():
        context = f"api.surface_parameters.{surface_kind}"
        if not isinstance(layout, list) or not layout:
            raise InspectionApiManifestError(f"{context} must be a non-empty array")
        parameter_names: list[str] = []
        for index, parameter in enumerate(layout):
            item_context = f"{context}[{index}]"
            _object(parameter, {"name", "unit"}, item_context)
            parameter_name = parameter["name"]
            if not isinstance(parameter_name, str) or not _SYMBOL.fullmatch(parameter_name):
                raise InspectionApiManifestError(f"{item_context}.name is invalid")
            if not isinstance(parameter["unit"], str) or parameter["unit"] not in _PARAMETER_UNITS:
                raise InspectionApiManifestError(f"{item_context}.unit is invalid")
            parameter_names.append(parameter_name)
        if len(parameter_names) != len(set(parameter_names)):
            raise InspectionApiManifestError(f"{context} names must be unique")


def _validate_dataclass_contract(contract: dict[str, Any], context: str) -> None:
    fields = contract["fields"]
    if (
        not isinstance(fields, list)
        or not fields
        or type(contract["frozen"]) is not bool
        or type(contract["slots"]) is not bool
    ):
        raise InspectionApiManifestError(f"{context}.contract is invalid")
    field_names: list[str] = []
    for field_index, field in enumerate(fields):
        field_context = f"{context}.contract.fields[{field_index}]"
        _object(field, {"name", "type"}, field_context)
        if (
            not isinstance(field["name"], str)
            or not _SYMBOL.fullmatch(field["name"])
            or not isinstance(field["type"], str)
            or not field["type"]
        ):
            raise InspectionApiManifestError(f"{field_context} is invalid")
        field_names.append(field["name"])
    if len(field_names) != len(set(field_names)):
        raise InspectionApiManifestError(f"{context}.contract field names must be unique")


def _validate_enum_contract(contract: dict[str, Any], context: str) -> None:
    members = contract["members"]
    if not isinstance(members, list) or not members:
        raise InspectionApiManifestError(f"{context}.contract.members must be a non-empty array")
    member_names: list[str] = []
    member_values: list[str] = []
    for member_index, member in enumerate(members):
        member_context = f"{context}.contract.members[{member_index}]"
        _object(member, {"name", "value"}, member_context)
        if (
            not isinstance(member["name"], str)
            or not _SYMBOL.fullmatch(member["name"])
            or not isinstance(member["value"], str)
            or not member["value"]
        ):
            raise InspectionApiManifestError(f"{member_context} is invalid")
        member_names.append(member["name"])
        member_values.append(member["value"])
    if len(member_names) != len(set(member_names)) or len(member_values) != len(set(member_values)):
        raise InspectionApiManifestError(f"{context}.contract enum names and values must be unique")


def _validate_exception_contract(contract: dict[str, Any], context: str) -> None:
    if not isinstance(contract["base"], str) or not contract["base"]:
        raise InspectionApiManifestError(f"{context}.contract.base is invalid")
    attributes = contract["attributes"]
    if not isinstance(attributes, list) or not attributes:
        raise InspectionApiManifestError(f"{context}.contract.attributes must be a non-empty array")
    attribute_names: list[str] = []
    for attribute_index, attribute in enumerate(attributes):
        attribute_context = f"{context}.contract.attributes[{attribute_index}]"
        _object(attribute, {"name", "type", "values"}, attribute_context)
        values = attribute["values"]
        if (
            not isinstance(attribute["name"], str)
            or not _SYMBOL.fullmatch(attribute["name"])
            or not isinstance(attribute["type"], str)
            or not attribute["type"]
            or not isinstance(values, list)
            or not values
            or not all(isinstance(value, str) and value for value in values)
            or len(values) != len(set(values))
        ):
            raise InspectionApiManifestError(f"{attribute_context} is invalid")
        attribute_names.append(attribute["name"])
    if len(attribute_names) != len(set(attribute_names)):
        raise InspectionApiManifestError(f"{context}.contract attribute names must be unique")


def _validate_function_contract(contract: dict[str, Any], context: str) -> None:
    if not isinstance(contract["signature"], str) or not contract["signature"]:
        raise InspectionApiManifestError(f"{context}.contract.signature is invalid")
    if "returns" not in contract:
        return
    returns = contract["returns"]
    if not isinstance(returns, dict):
        raise InspectionApiManifestError(f"{context}.contract.returns must be an object")
    _keys(returns, {"kind", "members"}, f"{context}.contract.returns")
    if set(returns) != {"kind", "members"} or returns["kind"] != "tuple":
        raise InspectionApiManifestError(f"{context}.contract.returns must define a tuple")
    members = returns["members"]
    if not isinstance(members, list) or not members:
        raise InspectionApiManifestError(
            f"{context}.contract.returns.members must be a non-empty array"
        )
    return_member_names: list[str] = []
    for member_index, member in enumerate(members):
        member_context = f"{context}.contract.returns.members[{member_index}]"
        _object(member, {"name", "type", "unit", "values"}, member_context)
        unit = member["unit"]
        values = member["values"]
        if (
            not isinstance(member["name"], str)
            or not _SYMBOL.fullmatch(member["name"])
            or not isinstance(member["type"], str)
            or not member["type"]
            or (unit is not None and (not isinstance(unit, str) or unit not in _RETURN_UNITS))
            or (
                values is not None
                and (
                    not isinstance(values, list)
                    or not values
                    or not all(isinstance(value, str) and value for value in values)
                    or len(values) != len(set(values))
                )
            )
        ):
            raise InspectionApiManifestError(f"{member_context} is invalid")
        return_member_names.append(member["name"])
    if len(return_member_names) != len(set(return_member_names)):
        raise InspectionApiManifestError(f"{context}.contract return member names must be unique")


def _validate_contract(symbol: dict[str, Any], context: str) -> None:
    """Check the per-kind contract body, which is the half of a symbol that varies."""

    name = symbol["name"]
    kind = symbol["kind"]
    contract = symbol["contract"]
    if not isinstance(contract, dict) or not contract:
        raise InspectionApiManifestError(f"{context}.contract must be a non-empty object")
    expected_contract = {
        "dataclass": {"fields", "frozen", "slots"},
        "enum": {"members"},
        "exception": {"attributes", "base"},
        "function": ({"returns", "signature"} if name == "read_double_d_tool" else {"signature"}),
        "type-alias": {"definition"},
    }[kind]
    _keys(contract, expected_contract, f"{context}.contract")
    if set(contract) != expected_contract:
        raise InspectionApiManifestError(f"{context}.contract is incomplete")
    if kind == "dataclass":
        _validate_dataclass_contract(contract, context)
    elif kind == "enum":
        _validate_enum_contract(contract, context)
    elif kind == "exception":
        _validate_exception_contract(contract, context)
    elif kind == "function":
        _validate_function_contract(contract, context)
    else:
        (contract_value,) = contract.values()
        if not isinstance(contract_value, str) or not contract_value:
            raise InspectionApiManifestError(f"{context}.contract value is invalid")


def _validate_symbol(
    symbol: object, index: int, package_version: tuple[int, int, int]
) -> tuple[str, str, list[str]]:
    """Return the ``(name, qualified_name, aliases)`` of one validated symbol entry."""

    context = f"api.symbols[{index}]"
    symbol = _object(
        symbol,
        {"aliases", "contract", "introduced_in", "kind", "name", "qualified_name"},
        context,
    )
    name = symbol["name"]
    if not isinstance(name, str) or not _SYMBOL.fullmatch(name):
        raise InspectionApiManifestError(f"{context}.name is invalid")
    if symbol["qualified_name"] != f"{_INSPECTION_NAMESPACE}.{name}":
        raise InspectionApiManifestError(f"{context}.qualified_name is invalid")
    kind = symbol["kind"]
    if not isinstance(kind, str) or kind not in _KINDS:
        raise InspectionApiManifestError(f"{context}.kind is invalid")
    introduced = _version(symbol["introduced_in"], f"{context}.introduced_in")
    if introduced > package_version:
        raise InspectionApiManifestError(f"{context} is introduced after this package")
    aliases = symbol["aliases"]
    if (
        not isinstance(aliases, list)
        or not all(
            isinstance(alias, str)
            and _QUALIFIED.fullmatch(alias)
            and alias != symbol["qualified_name"]
            for alias in aliases
        )
        or aliases != sorted(set(aliases))
    ):
        raise InspectionApiManifestError(f"{context}.aliases is invalid")
    _validate_contract(symbol, context)
    return name, symbol["qualified_name"], aliases


def validate_inspection_api_manifest(manifest: object) -> None:
    """Validate the closed format-1 inspection API document."""

    if not isinstance(manifest, dict):
        raise InspectionApiManifestError("inspection API manifest must be a JSON object")
    _keys(manifest, {"api", "format", "format_version", "package"}, "manifest")
    if set(manifest) != {"api", "format", "format_version", "package"}:
        raise InspectionApiManifestError("inspection API manifest is missing required fields")
    if manifest["format"] != INSPECTION_API_FORMAT:
        raise InspectionApiManifestError(
            f"unsupported inspection document kind {manifest['format']!r}"
        )
    if (
        type(manifest["format_version"]) is not int
        or manifest["format_version"] != INSPECTION_API_FORMAT_VERSION
    ):
        raise InspectionApiManifestError(
            f"unsupported inspection format version {manifest['format_version']!r}"
        )

    package_version = _validate_package(manifest["package"])

    api = manifest["api"]
    if not isinstance(api, dict):
        raise InspectionApiManifestError("api must be an object")
    api_fields = {"major", "namespace", "surface_parameters", "symbols"}
    _keys(api, api_fields, "api")
    if set(api) != api_fields:
        raise InspectionApiManifestError("api is missing required fields")
    if type(api["major"]) is not int or api["major"] != _INSPECTION_API_MAJOR:
        raise InspectionApiManifestError(f"unsupported inspection API major {api['major']!r}")
    if api["namespace"] != _INSPECTION_NAMESPACE:
        raise InspectionApiManifestError("inspection API namespace is invalid")
    _validate_surface_parameters(api["surface_parameters"])

    symbols = api["symbols"]
    if not isinstance(symbols, list) or not symbols:
        raise InspectionApiManifestError("api.symbols must be a non-empty array")

    names: list[str] = []
    qualified_names: list[str] = []
    all_aliases: list[str] = []
    for index, symbol in enumerate(symbols):
        name, qualified_name, aliases = _validate_symbol(symbol, index, package_version)
        names.append(name)
        qualified_names.append(qualified_name)
        all_aliases.extend(aliases)
    if names != sorted(names) or len(names) != len(set(names)):
        raise InspectionApiManifestError("inspection API symbols must be unique and name-sorted")
    if len(all_aliases) != len(set(all_aliases)) or set(all_aliases) & set(qualified_names):
        raise InspectionApiManifestError(
            "inspection API aliases must have one owner and not collide with primary symbols"
        )


def _load_inspection_api_manifest() -> InspectionApiManifest:
    resource = files("quiddity").joinpath("inspection_api.json")
    manifest = cast(InspectionApiManifest, json.loads(resource.read_text(encoding="utf-8")))
    validate_inspection_api_manifest(manifest)
    from quiddity import __version__

    package = cast(dict[str, object], manifest["package"])
    if package["version"] != __version__:
        raise InspectionApiManifestError(
            f"inspection API manifest version {package['version']!r} does not match installed "
            f"package version {__version__!r}"
        )
    return manifest


def inspection_api_manifest(
    *, format_version: int = INSPECTION_API_FORMAT_VERSION
) -> InspectionApiManifest:
    """Return an isolated copy of the installed inspection API contract."""

    if type(format_version) is not int or format_version != INSPECTION_API_FORMAT_VERSION:
        raise InspectionApiManifestError(
            f"unsupported requested inspection format version {format_version!r}"
        )
    return copy.deepcopy(_load_inspection_api_manifest())


def inspection_api_manifest_json(*, format_version: int = INSPECTION_API_FORMAT_VERSION) -> str:
    """Return the installed inspection API contract as canonical JSON."""

    return (
        json.dumps(inspection_api_manifest(format_version=format_version), indent=2, sort_keys=True)
        + "\n"
    )


__all__ = [
    "INSPECTION_API_FORMAT",
    "INSPECTION_API_FORMAT_VERSION",
    "AnalyticSurface",
    "BevelReject",
    "FaceInspection",
    "InspectionApiManifest",
    "InspectionApiManifestError",
    "OrientationCapability",
    "RefusedSurface",
    "SurfaceFact",
    "SurfaceKind",
    "SurfaceProvenance",
    "SurfaceRefusalReason",
    "classify_bevel",
    "cone_rims",
    "floor_face_anchor",
    "inspect_face",
    "inspection_api_manifest",
    "inspection_api_manifest_json",
    "read_double_d_tool",
    "validate_inspection_api_manifest",
]
