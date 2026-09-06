"""Independent contract tests for the supported F7 inspection API."""

from __future__ import annotations

import copy
import dataclasses
import importlib
import inspect
import json
import math
import subprocess
import sys
import types
import typing
from enum import Enum
from pathlib import Path

import pytest
from build123d import (
    Align,
    Box,
    Cone,
    Cylinder,
    GeomType,
    Polygon,
    Pos,
    Rot,
    Sphere,
    Torus,
    Vertex,
)
from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.TopAbs import TopAbs_OUT

import quiddity as recognition
import quiddity.experimental_geometry as experimental
import quiddity.inspection as inspection

ROOT = Path(__file__).parents[1]
MANIFEST = ROOT / "src" / "quiddity" / "inspection_api.json"

EXPECTED_KINDS = {
    "AnalyticSurface": "dataclass",
    "BevelReject": "exception",
    "FaceInspection": "dataclass",
    "OrientationCapability": "enum",
    "RefusedSurface": "dataclass",
    "SurfaceFact": "type-alias",
    "SurfaceKind": "enum",
    "SurfaceProvenance": "enum",
    "SurfaceRefusalReason": "enum",
    "classify_bevel": "function",
    "cone_rims": "function",
    "floor_face_anchor": "function",
    "inspect_face": "function",
    "read_double_d_tool": "function",
}

EXPECTED_ALIASES = {
    "AnalyticSurface": ["quiddity.experimental_geometry.AnalyticSurface"],
    "BevelReject": [
        "quiddity.BevelReject",
        "quiddity.chamfers.BevelReject",
    ],
    "FaceInspection": ["quiddity.experimental_geometry.FaceInspection"],
    "OrientationCapability": ["quiddity.experimental_geometry.OrientationCapability"],
    "RefusedSurface": ["quiddity.experimental_geometry.RefusedSurface"],
    "SurfaceFact": ["quiddity.experimental_geometry.SurfaceFact"],
    "SurfaceKind": ["quiddity.experimental_geometry.SurfaceKind"],
    "SurfaceProvenance": ["quiddity.experimental_geometry.SurfaceProvenance"],
    "SurfaceRefusalReason": ["quiddity.experimental_geometry.SurfaceRefusalReason"],
    "classify_bevel": [
        "quiddity.chamfers.classify_bevel",
        "quiddity.classify_bevel",
    ],
    "cone_rims": [
        "quiddity.cone_rims",
        "quiddity.countersinks.cone_rims",
    ],
    "floor_face_anchor": [
        "quiddity.floor_face_anchor",
        "quiddity.grooves.floor_face_anchor",
    ],
    "inspect_face": ["quiddity.experimental_geometry.inspect_face"],
    "read_double_d_tool": ["quiddity.profiled_bores.read_double_d_tool"],
}

EXPECTED_SURFACE_PARAMETERS = {
    "cone": [
        ("apex_x", "model-length"),
        ("apex_y", "model-length"),
        ("apex_z", "model-length"),
        ("axis_x", "unitless"),
        ("axis_y", "unitless"),
        ("axis_z", "unitless"),
        ("signed_semi_angle", "radian"),
    ],
    "cylinder": [
        ("axis_point_x", "model-length"),
        ("axis_point_y", "model-length"),
        ("axis_point_z", "model-length"),
        ("axis_x", "unitless"),
        ("axis_y", "unitless"),
        ("axis_z", "unitless"),
        ("radius", "model-length"),
    ],
    "plane": [
        ("normal_x", "unitless"),
        ("normal_y", "unitless"),
        ("normal_z", "unitless"),
        ("offset", "model-length"),
    ],
    "sphere": [
        ("centre_x", "model-length"),
        ("centre_y", "model-length"),
        ("centre_z", "model-length"),
        ("radius", "model-length"),
    ],
}

EXPECTED_BEVEL_REASONS = ["nonplanar", "degenerate", "aligned", "compound"]

EXPECTED_DOUBLE_D_RETURN_MEMBERS = [
    {"name": "axis", "type": "str", "unit": None, "values": ["x", "y", "z"]},
    {
        "name": "major_diameter",
        "type": "float",
        "unit": "model-length",
        "values": None,
    },
    {
        "name": "across_flats",
        "type": "float",
        "unit": "model-length",
        "values": None,
    },
    {
        "name": "origin",
        "type": "tuple[float,float,float]",
        "unit": "model-length",
        "values": None,
    },
    {"name": "depth", "type": "float", "unit": "model-length", "values": None},
    {
        "name": "profile_direction",
        "type": "tuple[float,float,float]",
        "unit": "unitless",
        "values": None,
    },
]


def _symbols() -> list[dict[str, object]]:
    manifest = inspection.inspection_api_manifest()
    return typing.cast(list[dict[str, object]], manifest["api"]["symbols"])


def _resolve(qualified_name: str) -> object:
    module_name, _, name = qualified_name.rpartition(".")
    return getattr(importlib.import_module(module_name), name)


def _manifest_contract(manifest: dict[str, object], name: str) -> dict[str, object]:
    api = typing.cast(dict[str, object], manifest["api"])
    symbols = typing.cast(list[dict[str, object]], api["symbols"])
    symbol = next(item for item in symbols if item["name"] == name)
    return typing.cast(dict[str, object], symbol["contract"])


def _assert_required_consumer_contract(manifest: dict[str, object]) -> None:
    bevel = _manifest_contract(manifest, "BevelReject")
    assert bevel["attributes"] == [
        {"name": "reason", "type": "str", "values": EXPECTED_BEVEL_REASONS}
    ]
    double_d = _manifest_contract(manifest, "read_double_d_tool")
    assert double_d["returns"] == {
        "kind": "tuple",
        "members": EXPECTED_DOUBLE_D_RETURN_MEMBERS,
    }


def _type_name(annotation: object) -> str:
    if annotation is type(None):
        return "null"
    if annotation in {bool, float, int, str}:
        return typing.cast(type, annotation).__name__
    if inspect.isclass(annotation) and annotation.__module__.startswith("quiddity"):
        return annotation.__name__
    origin = typing.get_origin(annotation)
    args = typing.get_args(annotation)
    if origin in {typing.Union, types.UnionType}:
        return "|".join(
            sorted({_type_name(arg) for arg in args}, key=lambda item: (item == "null", item))
        )
    if origin is tuple:
        return (
            "tuple[" + ",".join("..." if arg is Ellipsis else _type_name(arg) for arg in args) + "]"
        )
    raise TypeError(annotation)


def test_manifest_query_is_deterministic_isolated_and_separately_versioned() -> None:
    first = inspection.inspection_api_manifest()
    expected = json.loads(MANIFEST.read_text(encoding="utf-8"))

    assert first == inspection.inspection_api_manifest() == expected
    assert inspection.inspection_api_manifest_json() == MANIFEST.read_text(encoding="utf-8")
    assert first["format"] == inspection.INSPECTION_API_FORMAT
    assert first["format_version"] == inspection.INSPECTION_API_FORMAT_VERSION
    assert first["package"] == {
        "name": "quiddity",
        "version": recognition.__version__,
    }
    assert "inspection" not in recognition.capability_manifest()
    typing.cast(dict[str, object], first["api"])["symbols"] = []
    assert _symbols(), "callers must not mutate the installed contract"
    with pytest.raises(inspection.InspectionApiManifestError, match="unsupported requested"):
        inspection.inspection_api_manifest(format_version=2)
    with pytest.raises(inspection.InspectionApiManifestError, match="unsupported requested"):
        inspection.inspection_api_manifest_json(format_version=2)
    with pytest.raises(inspection.InspectionApiManifestError, match="unsupported requested"):
        inspection.inspection_api_manifest(format_version=True)


def test_manifest_roster_and_runtime_contracts_are_derived_independently() -> None:
    declared = {typing.cast(str, item["name"]): item for item in _symbols()}
    assert {name: item["kind"] for name, item in declared.items()} == EXPECTED_KINDS
    assert {name: item["aliases"] for name, item in declared.items()} == EXPECTED_ALIASES

    for name, kind in EXPECTED_KINDS.items():
        value = getattr(inspection, name)
        contract = typing.cast(dict[str, object], declared[name]["contract"])
        if kind == "dataclass":
            hints = typing.get_type_hints(value)
            parameters = value.__dataclass_params__
            assert contract == {
                "fields": [
                    {"name": field.name, "type": _type_name(hints[field.name])}
                    for field in dataclasses.fields(value)
                ],
                "frozen": parameters.frozen,
                "slots": getattr(parameters, "slots", "__slots__" in value.__dict__),
            }
        elif kind == "enum":
            assert inspect.isclass(value) and issubclass(value, Enum)
            assert contract == {
                "members": [{"name": member.name, "value": member.value} for member in value]
            }
        elif kind == "exception":
            assert inspect.isclass(value) and issubclass(value, ValueError)
            reason = typing.get_type_hints(value)["reason"]
            assert typing.get_origin(reason) is typing.Literal
            assert contract == {
                "attributes": [
                    {
                        "name": "reason",
                        "type": "str",
                        "values": list(typing.get_args(reason)),
                    }
                ],
                "base": "ValueError",
            }
        elif kind == "function":
            expected: dict[str, object] = {"signature": str(inspect.signature(value))}
            if name == "read_double_d_tool":
                expected["returns"] = {
                    "kind": "tuple",
                    "members": EXPECTED_DOUBLE_D_RETURN_MEMBERS,
                }
            assert contract == expected
        else:
            assert kind == "type-alias"
            assert set(typing.get_args(value)) == {
                inspection.AnalyticSurface,
                inspection.RefusedSurface,
            }
            assert contract == {"definition": "AnalyticSurface|RefusedSurface"}


def test_manifest_freezes_each_surface_parameter_layout_and_unit() -> None:
    api = inspection.inspection_api_manifest()["api"]
    layouts = {
        kind: [(item["name"], item["unit"]) for item in layout]
        for kind, layout in api["surface_parameters"].items()
    }

    assert layouts == EXPECTED_SURFACE_PARAMETERS
    assert set(layouts) == {kind.value for kind in inspection.SurfaceKind}


def test_manifest_freezes_bevel_reasons_and_double_d_tuple_semantics() -> None:
    manifest = inspection.inspection_api_manifest()
    _assert_required_consumer_contract(manifest)
    for reason in EXPECTED_BEVEL_REASONS:
        error = inspection.BevelReject(reason)
        assert error.reason == reason
        assert str(error) == reason


@pytest.mark.parametrize(
    "mutate",
    [
        lambda manifest: typing.cast(
            list[dict[str, object]],
            _manifest_contract(manifest, "BevelReject")["attributes"],
        )[0].update({"values": ["nonplanar", "degenerate", "aligned"]}),
        lambda manifest: typing.cast(
            list[dict[str, object]],
            _manifest_contract(manifest, "BevelReject")["attributes"],
        )[0].update({"values": [*EXPECTED_BEVEL_REASONS, "future"]}),
        lambda manifest: typing.cast(
            list[dict[str, object]],
            typing.cast(
                dict[str, object],
                _manifest_contract(manifest, "read_double_d_tool")["returns"],
            )["members"],
        ).reverse(),
        lambda manifest: typing.cast(
            list[dict[str, object]],
            typing.cast(
                dict[str, object],
                _manifest_contract(manifest, "read_double_d_tool")["returns"],
            )["members"],
        )[0].update({"name": "principal_axis"}),
        lambda manifest: typing.cast(
            list[dict[str, object]],
            typing.cast(
                dict[str, object],
                _manifest_contract(manifest, "read_double_d_tool")["returns"],
            )["members"],
        )[1].update({"type": "int"}),
        lambda manifest: typing.cast(
            list[dict[str, object]],
            typing.cast(
                dict[str, object],
                _manifest_contract(manifest, "read_double_d_tool")["returns"],
            )["members"],
        )[1].update({"unit": "unitless"}),
    ],
)
def test_consumer_gate_rejects_semantic_contract_drift(mutate) -> None:
    manifest = inspection.inspection_api_manifest()
    mutate(manifest)
    with pytest.raises(AssertionError):
        _assert_required_consumer_contract(manifest)


@pytest.mark.parametrize(
    ("rotation", "expected_axis"),
    [(Rot(), "z"), (Rot(0, 90, 0), "x"), (Rot(90, 0, 0), "y")],
)
def test_double_d_manifest_member_order_reconstructs_runtime_read(
    rotation, expected_axis: str
) -> None:
    centre = (Align.CENTER, Align.CENTER, Align.CENTER)
    tool = rotation * (Cylinder(5, 20, align=centre) & Box(7.2, 20, 40, align=centre))
    values = inspection.read_double_d_tool(tool)
    named = dict(
        zip(
            [member["name"] for member in EXPECTED_DOUBLE_D_RETURN_MEMBERS],
            values,
            strict=True,
        )
    )

    assert named["axis"] == expected_axis
    assert named["major_diameter"] == pytest.approx(10.0)
    assert named["across_flats"] == pytest.approx(7.2)
    assert named["origin"] == pytest.approx((0.0, 0.0, 0.0))
    assert named["depth"] == pytest.approx(20.0)
    profile_direction = typing.cast(tuple[float, float, float], named["profile_direction"])
    assert math.dist(profile_direction, (0.0, 0.0, 0.0)) == pytest.approx(1.0)
    assert profile_direction["xyz".index(expected_axis)] == pytest.approx(0.0)


def test_surface_parameter_layouts_reconstruct_the_native_primitives() -> None:
    plane_face = max(
        Box(7, 9, 11).faces().filter_by(GeomType.PLANE),
        key=lambda face: face.center().Z,
    )
    cylinder_face = (Pos(4, 5, 0) * Cylinder(3, 8)).faces().filter_by(GeomType.CYLINDER)[0]
    cone_face = (Pos(2, 3, 0) * Cone(5, 2, 10)).faces().filter_by(GeomType.CONE)[0]
    sphere_face = (Pos(2, 3, 4) * Sphere(5)).faces().filter_by(GeomType.SPHERE)[0]

    facts = {}
    for face in (plane_face, cylinder_face, cone_face, sphere_face):
        fact = inspection.inspect_face(face).surface
        assert isinstance(fact, inspection.AnalyticSurface)
        names = [name for name, _unit in EXPECTED_SURFACE_PARAMETERS[fact.kind.value]]
        facts[fact.kind] = dict(zip(names, fact.parameters, strict=True))

    plane = facts[inspection.SurfaceKind.PLANE]
    normal = (plane["normal_x"], plane["normal_y"], plane["normal_z"])
    assert math.dist(normal, (0.0, 0.0, 0.0)) == pytest.approx(1.0)
    for vertex in plane_face.vertices():
        point = tuple(vertex)
        assert sum(a * b for a, b in zip(point, normal, strict=True)) == pytest.approx(
            plane["offset"]
        )

    cylinder = facts[inspection.SurfaceKind.CYLINDER]
    axis_point = tuple(cylinder[f"axis_point_{axis}"] for axis in "xyz")
    axis = tuple(cylinder[f"axis_{axis}"] for axis in "xyz")
    assert axis_point == pytest.approx((4.0, 5.0, 0.0))
    assert axis == pytest.approx((0.0, 0.0, 1.0))
    assert cylinder["radius"] == pytest.approx(3.0)

    cone = facts[inspection.SurfaceKind.CONE]
    native_cone = BRepAdaptor_Surface(cone_face.wrapped).Cone()
    apex = native_cone.Apex()
    raw_axis = native_cone.Axis().Direction()
    api_axis = tuple(cone[f"axis_{axis}"] for axis in "xyz")
    direction_sign = math.copysign(
        1.0,
        sum(
            api * raw
            for api, raw in zip(
                api_axis,
                (float(raw_axis.X()), float(raw_axis.Y()), float(raw_axis.Z())),
                strict=True,
            )
        ),
    )
    assert tuple(cone[f"apex_{axis}"] for axis in "xyz") == pytest.approx(
        (float(apex.X()), float(apex.Y()), float(apex.Z()))
    )
    assert math.dist(api_axis, (0.0, 0.0, 0.0)) == pytest.approx(1.0)
    assert cone["signed_semi_angle"] == pytest.approx(
        direction_sign * float(native_cone.SemiAngle())
    )

    sphere = facts[inspection.SurfaceKind.SPHERE]
    assert tuple(sphere[f"centre_{axis}"] for axis in "xyz") == pytest.approx((2.0, 3.0, 4.0))
    assert sphere["radius"] == pytest.approx(5.0)


def test_every_manifested_compatibility_alias_preserves_exact_identity() -> None:
    for symbol in _symbols():
        primary = _resolve(typing.cast(str, symbol["qualified_name"]))
        assert primary is getattr(inspection, typing.cast(str, symbol["name"]))
        for alias in typing.cast(list[str], symbol["aliases"]):
            assert _resolve(alias) is primary, alias


def test_only_standalone_inspection_graduated_from_the_experimental_facade() -> None:
    for name in (
        "AnalyticSurface",
        "FaceInspection",
        "OrientationCapability",
        "RefusedSurface",
        "SurfaceFact",
        "SurfaceKind",
        "SurfaceProvenance",
        "SurfaceRefusalReason",
        "inspect_face",
    ):
        assert getattr(experimental, name) is getattr(inspection, name)

    assert not hasattr(inspection, "GeometryGraph")
    assert not hasattr(recognition, "GeometryGraph")
    assert "GeometryGraph" not in inspection.__all__
    assert not {
        "AnalyticSurfaceFact",
        "EffectiveSurfaceIndex",
        "FaceGraph",
        "RefusedSurfaceFact",
    } & set(vars(inspection))


def test_supported_module_all_is_the_exact_roster_plus_manifest_protocol() -> None:
    manifest_protocol = {
        "INSPECTION_API_FORMAT",
        "INSPECTION_API_FORMAT_VERSION",
        "InspectionApiManifest",
        "InspectionApiManifestError",
        "inspection_api_manifest",
        "inspection_api_manifest_json",
        "validate_inspection_api_manifest",
    }

    assert set(inspection.__all__) == set(EXPECTED_KINDS) | manifest_protocol


def test_stable_inspect_face_returns_native_fact_anchor_and_closed_refusal() -> None:
    cylinder = Cylinder(8, 20).faces().filter_by(GeomType.CYLINDER)[0]
    inspected = inspection.inspect_face(cylinder)

    assert isinstance(inspected.surface, inspection.AnalyticSurface)
    assert inspected.surface.kind is inspection.SurfaceKind.CYLINDER
    assert inspected.surface.provenance is inspection.SurfaceProvenance.NATIVE
    assert inspected.surface.parameters[6] == pytest.approx(8)
    assert inspected.anchor is not None
    assert Vertex(*inspected.anchor).distance_to(cylinder) < 1e-7

    refused = inspection.inspect_face(Torus(8, 2).faces()[0])
    assert isinstance(refused.surface, inspection.RefusedSurface)
    assert refused.surface.reason in set(inspection.SurfaceRefusalReason)


def test_inspection_anchor_does_not_fall_inside_an_inner_trim() -> None:
    part = Box(20, 20, 2) - Pos(0, 0, -1) * Cylinder(3, 4)
    face = max(part.faces().filter_by(GeomType.PLANE), key=lambda item: item.center().Z)

    inspected = inspection.inspect_face(face)

    assert inspected.anchor is not None
    assert Vertex(*inspected.anchor).distance_to(face) < 1e-7
    assert Vertex(*inspected.anchor).distance_to(Vertex(0, 0, inspected.anchor[2])) > 0.0


def test_inspection_anchor_is_in_or_on_a_concave_trim() -> None:
    face = Polygon((0, 0), (4, 0), (4, 1), (1, 1), (1, 4), (0, 4)).face()

    inspected = inspection.inspect_face(face)

    assert inspected.anchor is not None
    assert Vertex(*inspected.anchor).distance_to(face) < 1e-7


def test_inspection_anchor_falls_back_to_a_proved_outer_boundary(monkeypatch) -> None:
    class OutsideClassifier:
        def __init__(self, *_args) -> None:
            pass

        def State(self):
            return TopAbs_OUT

    monkeypatch.setattr(inspection, "_BRepClass_FaceClassifier", OutsideClassifier)
    face = max(Box(7, 9, 11).faces().filter_by(GeomType.PLANE), key=lambda item: item.center().Z)

    anchor = inspection.inspect_face(face).anchor

    assert anchor is not None
    assert Vertex(*anchor).distance_to(face.outer_wire()) < 1e-7


def test_inspection_omits_anchor_when_surface_bounds_are_invalid(monkeypatch) -> None:
    class InvalidBounds:
        def __init__(self, _wrapped) -> None:
            pass

        def FirstUParameter(self) -> float:
            return math.nan

        def LastUParameter(self) -> float:
            return 1.0

        def FirstVParameter(self) -> float:
            return 0.0

        def LastVParameter(self) -> float:
            return 1.0

    face = Cylinder(3, 8).faces().filter_by(GeomType.CYLINDER)[0]
    monkeypatch.setattr(inspection, "_BRepAdaptor_Surface", InvalidBounds)

    assert inspection.inspect_face(face).anchor is None


def test_committed_manifest_is_the_deterministic_generator_output() -> None:
    subprocess.run(
        [sys.executable, "tools/generate_inspection_api_manifest.py", "--check"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
        timeout=30,
    )


@pytest.mark.parametrize(
    ("mutate", "message"),
    [
        (lambda value: value.clear(), "missing required"),
        (lambda value: value.update({"future": 1}), "unknown fields"),
        (lambda value: value.update({"format": "other"}), "document kind"),
        (lambda value: value.update({"format_version": 2}), "format version"),
        (lambda value: value.update({"format_version": True}), "format version"),
        (lambda value: value.update({"package": None}), "package must be"),
        (lambda value: value["package"].pop("version"), "package identity"),
        (lambda value: value["package"].update({"name": "other"}), "package identity"),
        (lambda value: value["package"].update({"version": "next"}), "semantic"),
        (lambda value: value.update({"api": None}), "api must be"),
        (lambda value: value["api"].pop("major"), "missing required"),
        (lambda value: value["api"].update({"major": 2}), "API major"),
        (lambda value: value["api"].update({"major": True}), "API major"),
        (lambda value: value["api"].update({"namespace": "other"}), "namespace"),
        (lambda value: value["api"].pop("surface_parameters"), "missing required"),
        (
            lambda value: value["api"].update({"surface_parameters": {}}),
            "four supported",
        ),
        (
            lambda value: value["api"]["surface_parameters"].update({"plane": []}),
            "non-empty",
        ),
        (
            lambda value: value["api"]["surface_parameters"]["plane"].__setitem__(0, None),
            "must be an object",
        ),
        (
            lambda value: value["api"]["surface_parameters"]["plane"][0].update({"unit": "pixels"}),
            "unit",
        ),
        (
            lambda value: value["api"]["surface_parameters"]["plane"][0].pop("unit"),
            "missing required",
        ),
        (
            lambda value: value["api"]["surface_parameters"]["plane"][0].update(
                {"name": "not valid"}
            ),
            "name",
        ),
        (
            lambda value: value["api"]["surface_parameters"]["plane"].append(
                copy.deepcopy(value["api"]["surface_parameters"]["plane"][0])
            ),
            "names must be unique",
        ),
        (lambda value: value["api"].update({"symbols": []}), "non-empty"),
        (lambda value: value["api"]["symbols"].__setitem__(0, None), "must be an object"),
        (lambda value: value["api"]["symbols"][0].pop("kind"), "missing required"),
        (lambda value: value["api"]["symbols"][0].update({"future": 1}), "unknown fields"),
        (lambda value: value["api"]["symbols"][0].update({"name": "not valid"}), "name"),
        (
            lambda value: value["api"]["symbols"][0].update({"qualified_name": "other"}),
            "qualified_name",
        ),
        (lambda value: value["api"]["symbols"][0].update({"kind": "record"}), "kind"),
        (lambda value: value["api"]["symbols"][0].update({"kind": []}), "kind"),
        (
            lambda value: value["api"]["symbols"][0].update({"introduced_in": "future"}),
            "semantic",
        ),
        (
            lambda value: value["api"]["symbols"][0].update({"introduced_in": "9.0.0"}),
            "introduced after",
        ),
        (lambda value: value["api"]["symbols"][0].update({"aliases": {}}), "aliases"),
        (
            lambda value: value["api"]["symbols"][0].update({"aliases": ["quiddity.bad"] * 2}),
            "aliases",
        ),
        (
            lambda value: value["api"]["symbols"][1].update(
                {"aliases": [value["api"]["symbols"][0]["aliases"][0]]}
            ),
            "one owner",
        ),
        (
            lambda value: value["api"]["symbols"][0].update(
                {"aliases": [value["api"]["symbols"][1]["qualified_name"]]}
            ),
            "collide with primary",
        ),
        (lambda value: value["api"]["symbols"][0].update({"contract": None}), "contract"),
        (
            lambda value: value["api"]["symbols"][0]["contract"].update({"future": 1}),
            "unknown fields",
        ),
        (
            lambda value: value["api"]["symbols"][0].update({"contract": {"fields": []}}),
            "incomplete",
        ),
        (
            lambda value: value["api"]["symbols"][0]["contract"]["fields"].__setitem__(0, None),
            "must be an object",
        ),
        (
            lambda value: value["api"]["symbols"][0]["contract"].update({"frozen": "yes"}),
            "contract is invalid",
        ),
        (
            lambda value: value["api"]["symbols"][0]["contract"]["fields"][0].pop("type"),
            "missing required",
        ),
        (
            lambda value: value["api"]["symbols"][0]["contract"]["fields"][0].update({"type": ""}),
            "is invalid",
        ),
        (
            lambda value: value["api"]["symbols"][0]["contract"]["fields"].append(
                copy.deepcopy(value["api"]["symbols"][0]["contract"]["fields"][0])
            ),
            "field names must be unique",
        ),
        (
            lambda value: next(item for item in value["api"]["symbols"] if item["kind"] == "enum")[
                "contract"
            ].update({"members": []}),
            "non-empty array",
        ),
        (
            lambda value: next(item for item in value["api"]["symbols"] if item["kind"] == "enum")[
                "contract"
            ]["members"].__setitem__(0, None),
            "must be an object",
        ),
        (
            lambda value: next(item for item in value["api"]["symbols"] if item["kind"] == "enum")[
                "contract"
            ]["members"][0].pop("value"),
            "missing required",
        ),
        (
            lambda value: next(item for item in value["api"]["symbols"] if item["kind"] == "enum")[
                "contract"
            ]["members"][0].update({"name": "not valid"}),
            "is invalid",
        ),
        (
            lambda value: next(item for item in value["api"]["symbols"] if item["kind"] == "enum")[
                "contract"
            ]["members"].append(
                copy.deepcopy(
                    next(item for item in value["api"]["symbols"] if item["kind"] == "enum")[
                        "contract"
                    ]["members"][0]
                )
            ),
            "enum names and values must be unique",
        ),
        (lambda value: value["api"]["symbols"].reverse(), "name-sorted"),
        (
            lambda value: value["api"]["symbols"].append(
                copy.deepcopy(value["api"]["symbols"][-1])
            ),
            "unique",
        ),
    ],
)
def test_validator_fails_closed_on_malformed_documents(mutate, message: str) -> None:
    manifest = inspection.inspection_api_manifest()
    mutate(manifest)
    with pytest.raises(inspection.InspectionApiManifestError, match=message):
        inspection.validate_inspection_api_manifest(manifest)


def test_validator_rejects_non_objects_and_invalid_scalar_contracts() -> None:
    with pytest.raises(inspection.InspectionApiManifestError, match="JSON object"):
        inspection.validate_inspection_api_manifest(None)

    manifest = inspection.inspection_api_manifest()
    function = next(item for item in manifest["api"]["symbols"] if item["kind"] == "function")
    function["contract"]["signature"] = ""
    with pytest.raises(inspection.InspectionApiManifestError, match="signature is invalid"):
        inspection.validate_inspection_api_manifest(manifest)

    manifest = inspection.inspection_api_manifest()
    _manifest_contract(manifest, "BevelReject")["base"] = ""
    with pytest.raises(inspection.InspectionApiManifestError, match="base is invalid"):
        inspection.validate_inspection_api_manifest(manifest)

    manifest = inspection.inspection_api_manifest()
    _manifest_contract(manifest, "SurfaceFact")["definition"] = ""
    with pytest.raises(inspection.InspectionApiManifestError, match="contract value"):
        inspection.validate_inspection_api_manifest(manifest)


@pytest.mark.parametrize(
    ("mutate", "message"),
    [
        (
            lambda contract: contract.update({"attributes": []}),
            "attributes must be a non-empty array",
        ),
        (
            lambda contract: typing.cast(list[object], contract["attributes"]).__setitem__(0, None),
            "must be an object",
        ),
        (
            lambda contract: typing.cast(list[dict[str, object]], contract["attributes"])[0].pop(
                "type"
            ),
            "missing required fields",
        ),
        (
            lambda contract: typing.cast(list[dict[str, object]], contract["attributes"])[0].update(
                {"values": ["aligned", "aligned"]}
            ),
            "is invalid",
        ),
        (
            lambda contract: typing.cast(list[dict[str, object]], contract["attributes"]).append(
                copy.deepcopy(typing.cast(list[dict[str, object]], contract["attributes"])[0])
            ),
            "attribute names must be unique",
        ),
    ],
)
def test_validator_rejects_malformed_exception_attributes(mutate, message: str) -> None:
    manifest = inspection.inspection_api_manifest()
    mutate(_manifest_contract(manifest, "BevelReject"))
    with pytest.raises(inspection.InspectionApiManifestError, match=message):
        inspection.validate_inspection_api_manifest(manifest)


@pytest.mark.parametrize(
    ("mutate", "message"),
    [
        (lambda contract: contract.pop("returns"), "contract is incomplete"),
        (
            lambda contract: contract.update({"returns": None}),
            "returns must be an object",
        ),
        (
            lambda contract: typing.cast(dict[str, object], contract["returns"]).update(
                {"kind": "record"}
            ),
            "must define a tuple",
        ),
        (
            lambda contract: typing.cast(dict[str, object], contract["returns"]).update(
                {"members": []}
            ),
            "members must be a non-empty array",
        ),
        (
            lambda contract: typing.cast(
                list[object],
                typing.cast(dict[str, object], contract["returns"])["members"],
            ).__setitem__(0, None),
            "must be an object",
        ),
        (
            lambda contract: typing.cast(
                list[dict[str, object]],
                typing.cast(dict[str, object], contract["returns"])["members"],
            )[0].pop("type"),
            "missing required fields",
        ),
        (
            lambda contract: typing.cast(
                list[dict[str, object]],
                typing.cast(dict[str, object], contract["returns"])["members"],
            )[0].update({"unit": "degrees"}),
            "is invalid",
        ),
        (
            lambda contract: typing.cast(
                list[dict[str, object]],
                typing.cast(dict[str, object], contract["returns"])["members"],
            )[0].update({"values": ["x", "x"]}),
            "is invalid",
        ),
        (
            lambda contract: typing.cast(
                list[dict[str, object]],
                typing.cast(dict[str, object], contract["returns"])["members"],
            ).append(
                copy.deepcopy(
                    typing.cast(
                        list[dict[str, object]],
                        typing.cast(dict[str, object], contract["returns"])["members"],
                    )[0]
                )
            ),
            "return member names must be unique",
        ),
    ],
)
def test_validator_rejects_malformed_function_return_contracts(mutate, message: str) -> None:
    manifest = inspection.inspection_api_manifest()
    mutate(_manifest_contract(manifest, "read_double_d_tool"))
    with pytest.raises(inspection.InspectionApiManifestError, match=message):
        inspection.validate_inspection_api_manifest(manifest)


def test_manifest_loader_rejects_a_resource_for_another_package_version(monkeypatch) -> None:
    manifest = inspection.inspection_api_manifest()
    manifest["package"]["version"] = "0.4.5"

    class Resource:
        def joinpath(self, _name: str):
            return self

        def read_text(self, *, encoding: str) -> str:
            assert encoding == "utf-8"
            return json.dumps(manifest)

    monkeypatch.setattr(inspection, "files", lambda _package: Resource())
    with pytest.raises(inspection.InspectionApiManifestError, match="does not match installed"):
        inspection.inspection_api_manifest()
