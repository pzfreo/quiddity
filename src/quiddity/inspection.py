# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Supported geometry reads shared by declared and recognised features.

This is the deliberately narrow F7 API from ADR 0010.  It publishes only the five
consumer-proven inspection operations; graph identity, adjacency, blend collapse,
recognition evidence and correspondence remain private or experimental.

``experimental_geometry.inspect_face`` remains an identity-preserving compatibility
alias.  New consumers should import the supported names from this module.
"""

from __future__ import annotations

import copy
import json
import math
import re
from dataclasses import dataclass
from enum import Enum
from importlib.resources import files
from itertools import product
from typing import Any, TypeAlias, cast

from OCP.BRepAdaptor import BRepAdaptor_Surface as _BRepAdaptor_Surface
from OCP.BRepClass import BRepClass_FaceClassifier as _BRepClass_FaceClassifier
from OCP.gp import gp_Pnt2d as _gp_Pnt2d
from OCP.Standard import Standard_Failure as _Standard_Failure
from OCP.TopAbs import TopAbs_IN as _TopAbs_IN
from OCP.TopAbs import TopAbs_ON as _TopAbs_ON

from quiddity._adjacency import FaceGraph as _FaceGraph
from quiddity._bevel import BevelReject, classify_bevel
from quiddity._effective_surfaces import (
    AnalyticSurfaceFact as _AnalyticSurfaceFact,
)
from quiddity._effective_surfaces import (
    EffectiveSurfaceIndex as _EffectiveSurfaceIndex,
)
from quiddity._effective_surfaces import (
    RefusedSurfaceFact as _RefusedSurfaceFact,
)
from quiddity._typing import FaceLike
from quiddity.countersinks import cone_rims
from quiddity.grooves import floor_face_anchor
from quiddity.profiled_bores import read_double_d_tool

INSPECTION_API_FORMAT = "quiddity-inspection-api"
INSPECTION_API_FORMAT_VERSION = 1
_INSPECTION_API_MAJOR = 1
_INSPECTION_NAMESPACE = "quiddity.inspection"
_VERSION = re.compile(r"^(\d+)\.(\d+)\.(\d+)(?:[.+-][A-Za-z0-9.-]+)?$")
_SYMBOL = re.compile(r"^[A-Za-z][A-Za-z0-9_]*$")
_QUALIFIED = re.compile(r"^quiddity(?:\.[A-Za-z][A-Za-z0-9_]*)+$")
_KINDS = {"dataclass", "enum", "exception", "function", "type-alias"}
_PARAMETER_KINDS = {"plane", "cylinder", "cone", "sphere"}
_PARAMETER_UNITS = {"model-length", "radian", "unitless"}
_RETURN_UNITS = {"model-length", "unitless"}

InspectionApiManifest: TypeAlias = dict[str, Any]


class InspectionApiManifestError(ValueError):
    """The installed inspection API manifest is missing, stale, or unsupported."""


class SurfaceKind(Enum):
    PLANE = "plane"
    CYLINDER = "cylinder"
    CONE = "cone"
    SPHERE = "sphere"


class SurfaceProvenance(Enum):
    NATIVE = "native"
    RECOVERED = "recovered"


class OrientationCapability(Enum):
    NATIVE_ORIENTED = "native-oriented"
    RECOVERED_UNORIENTED = "recovered-unoriented"


class SurfaceRefusalReason(Enum):
    UNSUPPORTED_KIND = "unsupported-kind"
    UNSUPPORTED_TORUS_RECOVERY = "unsupported-torus-recovery"
    FIT_UNAVAILABLE = "fit-unavailable"
    INVALID_INPUT = "invalid-input"
    INVALID_RESULT = "invalid-result"
    RESIDUAL_EXCEEDED = "residual-exceeded"
    AMBIGUOUS_PRIMITIVE = "ambiguous-primitive"
    UNSUPPORTED_OCCT_CONTRACT = "unsupported-occt-contract"


@dataclass(frozen=True, slots=True)
class AnalyticSurface:
    """One native or bounded-recovered analytic surface fact."""

    kind: SurfaceKind
    provenance: SurfaceProvenance
    orientation: OrientationCapability
    parameters: tuple[float, ...]
    requested_tolerance: float
    kernel_reported_gap: float


@dataclass(frozen=True, slots=True)
class RefusedSurface:
    """A closed reason why a face has no supported analytic fact."""

    reason: SurfaceRefusalReason


SurfaceFact: TypeAlias = AnalyticSurface | RefusedSurface


@dataclass(frozen=True, slots=True)
class FaceInspection:
    """One face's closed analytic result and optional point on its trimmed surface."""

    surface: SurfaceFact
    anchor: tuple[float, float, float] | None


def _project_surface_fact(fact: _AnalyticSurfaceFact | _RefusedSurfaceFact) -> SurfaceFact:
    if isinstance(fact, _RefusedSurfaceFact):
        return RefusedSurface(SurfaceRefusalReason(fact.reason.value))
    return AnalyticSurface(
        SurfaceKind(fact.kind.value),
        SurfaceProvenance(fact.provenance.value),
        OrientationCapability(fact.orientation.value),
        fact.parameters,
        fact.requested_tolerance,
        fact.kernel_reported_gap,
    )


def _surface_anchor(face: FaceLike) -> tuple[float, float, float]:
    """Return a point proved in/on the original trimmed face, or refuse.

    The midpoint of a face's rectangular UV bounds can lie in an inner wire or outside a
    concave outer wire.  Prefer the closest deterministic interior grid point, then fall
    back to an outer-boundary midpoint, which is still on the trimmed face.
    """

    try:
        surface = _BRepAdaptor_Surface(face.wrapped)
        u_bounds = (float(surface.FirstUParameter()), float(surface.LastUParameter()))
        v_bounds = (float(surface.FirstVParameter()), float(surface.LastVParameter()))
        if not all(math.isfinite(value) for value in (*u_bounds, *v_bounds)):
            raise ValueError("surface parameter bounds are not finite")

        fractions = (0.5, 0.25, 0.75, 0.125, 0.375, 0.625, 0.875)
        samples = sorted(
            product(fractions, repeat=2),
            key=lambda item: (
                (item[0] - 0.5) ** 2 + (item[1] - 0.5) ** 2,
                item,
            ),
        )
        for u_fraction, v_fraction in samples:
            u = u_bounds[0] + u_fraction * (u_bounds[1] - u_bounds[0])
            v = v_bounds[0] + v_fraction * (v_bounds[1] - v_bounds[0])
            classifier = _BRepClass_FaceClassifier(face.wrapped, _gp_Pnt2d(u, v), 1e-7)
            if classifier.State() not in {_TopAbs_IN, _TopAbs_ON}:
                continue
            point = surface.Value(u, v)
            return (float(point.X()), float(point.Y()), float(point.Z()))

        u_mid = 0.5 * (u_bounds[0] + u_bounds[1])
        v_mid = 0.5 * (v_bounds[0] + v_bounds[1])
        target = surface.Value(u_mid, v_mid)
        target_xyz = (float(target.X()), float(target.Y()), float(target.Z()))
        candidates: list[tuple[float, float, float]] = []
        for edge in face.outer_wire().edges():
            point = edge.position_at(0.5)
            candidate = (float(point.X), float(point.Y), float(point.Z))
            if all(math.isfinite(value) for value in candidate):
                candidates.append(candidate)
        if not candidates:
            raise ValueError("surface outer boundary has no finite midpoint")
        return min(
            candidates,
            key=lambda point: (
                sum(
                    (value - target_value) ** 2
                    for value, target_value in zip(point, target_xyz, strict=True)
                ),
                point,
            ),
        )
    except (AttributeError, _Standard_Failure, RuntimeError, ValueError) as error:
        raise ValueError("surface anchor is unavailable") from error


def inspect_face(face: FaceLike) -> FaceInspection:
    """Return a bounded analytic fact and optional on-surface anchor for one face.

    The call is graph-independent for its consumer: no graph handle or topology identity
    enters or leaves the API.  Internally it uses the same run-owned effective-surface
    authority as aggregate recognition.  Unsupported, ambiguous or unbounded geometry is
    returned as :class:`RefusedSurface`; anchor failure is represented by ``None``.
    """

    graph = _FaceGraph(face)
    node = graph.require_node(face)
    surface = _project_surface_fact(_EffectiveSurfaceIndex(graph).fact(node))
    try:
        anchor = _surface_anchor(face)
    except ValueError:
        anchor = None
    return FaceInspection(surface, anchor)


def _keys(value: dict[str, Any], allowed: set[str], context: str) -> None:
    unknown = sorted(set(value) - allowed)
    if unknown:
        raise InspectionApiManifestError(f"{context} has unknown fields: {', '.join(unknown)}")


def _version(value: object, context: str) -> tuple[int, int, int]:
    if not isinstance(value, str) or not (match := _VERSION.fullmatch(value)):
        raise InspectionApiManifestError(f"{context} must be a semantic package version")
    return cast(tuple[int, int, int], tuple(int(item) for item in match.groups()))


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

    package = manifest["package"]
    if not isinstance(package, dict):
        raise InspectionApiManifestError("package must be an object")
    _keys(package, {"name", "version"}, "package")
    if set(package) != {"name", "version"} or package["name"] != "quiddity":
        raise InspectionApiManifestError("package identity must be quiddity with a version")
    package_version = _version(package["version"], "package.version")

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
    surface_parameters = api["surface_parameters"]
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
            if not isinstance(parameter, dict):
                raise InspectionApiManifestError(f"{item_context} must be an object")
            _keys(parameter, {"name", "unit"}, item_context)
            if set(parameter) != {"name", "unit"}:
                raise InspectionApiManifestError(f"{item_context} is missing required fields")
            parameter_name = parameter["name"]
            if not isinstance(parameter_name, str) or not _SYMBOL.fullmatch(parameter_name):
                raise InspectionApiManifestError(f"{item_context}.name is invalid")
            if not isinstance(parameter["unit"], str) or parameter["unit"] not in _PARAMETER_UNITS:
                raise InspectionApiManifestError(f"{item_context}.unit is invalid")
            parameter_names.append(parameter_name)
        if len(parameter_names) != len(set(parameter_names)):
            raise InspectionApiManifestError(f"{context} names must be unique")
    symbols = api["symbols"]
    if not isinstance(symbols, list) or not symbols:
        raise InspectionApiManifestError("api.symbols must be a non-empty array")

    names: list[str] = []
    qualified_names: list[str] = []
    all_aliases: list[str] = []
    for index, symbol in enumerate(symbols):
        context = f"api.symbols[{index}]"
        if not isinstance(symbol, dict):
            raise InspectionApiManifestError(f"{context} must be an object")
        required = {
            "aliases",
            "contract",
            "introduced_in",
            "kind",
            "name",
            "qualified_name",
        }
        _keys(symbol, required, context)
        if set(symbol) != required:
            raise InspectionApiManifestError(f"{context} is missing required fields")
        name = symbol["name"]
        if not isinstance(name, str) or not _SYMBOL.fullmatch(name):
            raise InspectionApiManifestError(f"{context}.name is invalid")
        names.append(name)
        if symbol["qualified_name"] != f"{_INSPECTION_NAMESPACE}.{name}":
            raise InspectionApiManifestError(f"{context}.qualified_name is invalid")
        qualified_names.append(symbol["qualified_name"])
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
        all_aliases.extend(aliases)
        contract = symbol["contract"]
        if not isinstance(contract, dict) or not contract:
            raise InspectionApiManifestError(f"{context}.contract must be a non-empty object")
        expected_contract = {
            "dataclass": {"fields", "frozen", "slots"},
            "enum": {"members"},
            "exception": {"attributes", "base"},
            "function": (
                {"returns", "signature"} if name == "read_double_d_tool" else {"signature"}
            ),
            "type-alias": {"definition"},
        }[kind]
        _keys(contract, expected_contract, f"{context}.contract")
        if set(contract) != expected_contract:
            raise InspectionApiManifestError(f"{context}.contract is incomplete")
        if kind == "dataclass":
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
                if not isinstance(field, dict):
                    raise InspectionApiManifestError(f"{field_context} must be an object")
                _keys(field, {"name", "type"}, field_context)
                if set(field) != {"name", "type"}:
                    raise InspectionApiManifestError(f"{field_context} is missing required fields")
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
        elif kind == "enum":
            members = contract["members"]
            if not isinstance(members, list) or not members:
                raise InspectionApiManifestError(
                    f"{context}.contract.members must be a non-empty array"
                )
            member_names: list[str] = []
            member_values: list[str] = []
            for member_index, member in enumerate(members):
                member_context = f"{context}.contract.members[{member_index}]"
                if not isinstance(member, dict):
                    raise InspectionApiManifestError(f"{member_context} must be an object")
                _keys(member, {"name", "value"}, member_context)
                if set(member) != {"name", "value"}:
                    raise InspectionApiManifestError(f"{member_context} is missing required fields")
                if (
                    not isinstance(member["name"], str)
                    or not _SYMBOL.fullmatch(member["name"])
                    or not isinstance(member["value"], str)
                    or not member["value"]
                ):
                    raise InspectionApiManifestError(f"{member_context} is invalid")
                member_names.append(member["name"])
                member_values.append(member["value"])
            if len(member_names) != len(set(member_names)) or len(member_values) != len(
                set(member_values)
            ):
                raise InspectionApiManifestError(
                    f"{context}.contract enum names and values must be unique"
                )
        elif kind == "exception":
            if not isinstance(contract["base"], str) or not contract["base"]:
                raise InspectionApiManifestError(f"{context}.contract.base is invalid")
            attributes = contract["attributes"]
            if not isinstance(attributes, list) or not attributes:
                raise InspectionApiManifestError(
                    f"{context}.contract.attributes must be a non-empty array"
                )
            attribute_names: list[str] = []
            for attribute_index, attribute in enumerate(attributes):
                attribute_context = f"{context}.contract.attributes[{attribute_index}]"
                if not isinstance(attribute, dict):
                    raise InspectionApiManifestError(f"{attribute_context} must be an object")
                _keys(attribute, {"name", "type", "values"}, attribute_context)
                if set(attribute) != {"name", "type", "values"}:
                    raise InspectionApiManifestError(
                        f"{attribute_context} is missing required fields"
                    )
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
                raise InspectionApiManifestError(
                    f"{context}.contract attribute names must be unique"
                )
        elif kind == "function":
            if not isinstance(contract["signature"], str) or not contract["signature"]:
                raise InspectionApiManifestError(f"{context}.contract.signature is invalid")
            if "returns" not in contract:
                continue
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
                if not isinstance(member, dict):
                    raise InspectionApiManifestError(f"{member_context} must be an object")
                _keys(member, {"name", "type", "unit", "values"}, member_context)
                if set(member) != {"name", "type", "unit", "values"}:
                    raise InspectionApiManifestError(f"{member_context} is missing required fields")
                unit = member["unit"]
                values = member["values"]
                if (
                    not isinstance(member["name"], str)
                    or not _SYMBOL.fullmatch(member["name"])
                    or not isinstance(member["type"], str)
                    or not member["type"]
                    or (unit is not None and unit not in _RETURN_UNITS)
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
                raise InspectionApiManifestError(
                    f"{context}.contract return member names must be unique"
                )
        else:
            (contract_value,) = contract.values()
            if not isinstance(contract_value, str) or not contract_value:
                raise InspectionApiManifestError(f"{context}.contract value is invalid")
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
