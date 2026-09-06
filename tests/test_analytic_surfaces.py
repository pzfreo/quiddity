import pytest
from build123d import Axis, Box, Cylinder
from OCP.BRepAdaptor import BRepAdaptor_Surface

import quiddity._adjacency as adjacency_module
import quiddity._analytic_surfaces as analytic_module
import quiddity._effective_surfaces as effective_module
from quiddity._analytic_surfaces import (
    SurfaceKind,
    equivalent_parameters,
    native_primitive,
    validated_parameters,
)


def _parameters(face, kind: SurfaceKind) -> tuple[float, ...]:
    adaptor = BRepAdaptor_Surface(face.wrapped)
    return validated_parameters(kind, native_primitive(adaptor, kind))


def test_f1_and_f2_share_one_canonical_parameter_authority() -> None:
    assert effective_module.validated_parameters is validated_parameters
    assert adjacency_module.validated_parameters is validated_parameters
    assert effective_module.native_primitive is native_primitive
    assert adjacency_module.native_primitive is native_primitive


def test_coplanar_native_faces_are_equivalent_after_in_plane_translation() -> None:
    face = max(Box(10, 5, 2).faces(), key=lambda item: item.area)
    moved = face.translate((7, -3, 0))

    assert equivalent_parameters(
        SurfaceKind.PLANE,
        _parameters(face, SurfaceKind.PLANE),
        _parameters(moved, SurfaceKind.PLANE),
        local=5.0,
    )


def test_cylinder_equivalence_ignores_seam_rotation_but_not_radius() -> None:
    face = max(Cylinder(5, 12).faces(), key=lambda item: item.area)
    moved_seam = face.rotate(Axis.Z, 37)
    left = _parameters(face, SurfaceKind.CYLINDER)
    right = _parameters(moved_seam, SurfaceKind.CYLINDER)

    assert equivalent_parameters(SurfaceKind.CYLINDER, left, right, local=10.0)
    changed_radius = (*right[:-1], right[-1] + 1e-3)
    assert not equivalent_parameters(SurfaceKind.CYLINDER, left, changed_radius, local=10.0)


@pytest.mark.parametrize(
    ("kind", "left", "right", "expected"),
    [
        (SurfaceKind.PLANE, (0, 0, 1, 0), (0, 0, -1, 0), True),
        (SurfaceKind.PLANE, (0, 0, 1, 0), (0, 1, 0, 0), False),
        (SurfaceKind.CONE, (0, 0, 0, 0, 0, 1, 0.2), (0, 0, 0, 0, 0, 1, 0.21), False),
        (SurfaceKind.SPHERE, (0, 0, 0, 5), (0, 0, 0, 5.000001), True),
        (SurfaceKind.SPHERE, (0, 0, 0, 5), (0, 0, 0.01, 5), False),
    ],
)
def test_analytic_equivalence_has_closed_primitive_specific_rules(
    kind: SurfaceKind,
    left: tuple[float, ...],
    right: tuple[float, ...],
    expected: bool,
) -> None:
    assert equivalent_parameters(kind, left, right, local=1.0) is expected


@pytest.mark.parametrize("local", [0.0, -1.0, float("nan")])
def test_analytic_equivalence_refuses_invalid_local_scale(local: float) -> None:
    with pytest.raises(ValueError, match="finite positive"):
        equivalent_parameters(SurfaceKind.PLANE, (0, 0, 1, 0), (0, 0, 1, 0), local=local)


@pytest.mark.parametrize(
    ("kind", "parameters", "message"),
    [
        (SurfaceKind.PLANE, (float("nan"), 0, 1, 0), "finite"),
        (SurfaceKind.CYLINDER, (0, 0, 0, 2, 0, 0, 5), "unit length"),
        (SurfaceKind.SPHERE, (0, 0, 0, 0), "positive"),
        (SurfaceKind.CONE, (0, 0, 0, 0, 0, 1, 0), "strictly between"),
    ],
)
def test_analytic_parameter_domains_fail_closed(monkeypatch, kind, parameters, message) -> None:
    monkeypatch.setattr(analytic_module, "_primitive_parameters", lambda *_: parameters)
    with pytest.raises(ValueError, match=message):
        validated_parameters(kind, object())
