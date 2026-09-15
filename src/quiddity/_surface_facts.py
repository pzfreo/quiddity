# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Closed analytic surface facts for one face, below every family module.

This is the graph-independent core that `inspection` publishes and `experimental_geometry`
wraps. It lives in a leaf so that the geometry facade, and the run context built on it, do
not reach any family module through the inspection facade's declared-feature readers.
"""

from __future__ import annotations

import math
from dataclasses import dataclass
from enum import Enum
from itertools import product
from typing import TypeAlias

from OCP.BRepAdaptor import BRepAdaptor_Surface as _BRepAdaptor_Surface
from OCP.BRepClass import BRepClass_FaceClassifier as _BRepClass_FaceClassifier
from OCP.gp import gp_Pnt2d as _gp_Pnt2d
from OCP.Standard import Standard_Failure as _Standard_Failure
from OCP.TopAbs import TopAbs_IN as _TopAbs_IN
from OCP.TopAbs import TopAbs_ON as _TopAbs_ON

from quiddity._adjacency import FaceGraph as _FaceGraph
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
