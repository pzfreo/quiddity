"""Experimental graph evidence and stable inspection compatibility coverage."""

from __future__ import annotations

import copy
import typing

import pytest
from build123d import (
    Box,
    Cylinder,
    GeomType,
    Pos,
    RegularPolygon,
    Rot,
    Torus,
    Vertex,
    export_step,
    extrude,
    fillet,
    import_step,
)
from OCP.BRep import BRep_Tool
from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.BRepBuilderAPI import BRepBuilderAPI_MakeFace
from OCP.Geom import Geom_RectangularTrimmedSurface
from OCP.GeomConvert import GeomConvert

import quiddity
from quiddity.experimental_geometry import (
    AnalyticSurface,
    BlendFact,
    BlendRef,
    BoundaryRef,
    CollapsedBridge,
    FaceInspection,
    FaceRef,
    GeometryGraph,
    GeometryProvenance,
    RefusedSurface,
    inspect_face,
)


def _blend_interrupted_boss():
    prism = extrude(RegularPolygon(20, 6), 30)
    vertical = [edge for edge in prism.edges() if abs(float(edge.tangent_at().Z)) > 0.99]
    return Box(100, 80, 10) + Pos(0, 0, 5) * fillet(vertical, 2)


def _cylinder_fact(part) -> tuple[GeometryGraph, object, AnalyticSurface]:
    graph = GeometryGraph(part)
    for ref in graph.faces:
        fact = graph.surface_fact(ref)
        if isinstance(fact, AnalyticSurface) and fact.kind.value == "cylinder":
            return graph, ref, fact
    raise AssertionError("fixture contains no cylindrical surface")


def _as_bspline_face(face):
    adaptor = BRepAdaptor_Surface(face.wrapped)
    trimmed = Geom_RectangularTrimmedSurface(
        BRep_Tool.Surface_s(face.wrapped),
        adaptor.FirstUParameter(),
        adaptor.LastUParameter(),
        adaptor.FirstVParameter(),
        adaptor.LastVParameter(),
    )
    bspline = GeomConvert.SurfaceToBSplineSurface_s(trimmed)
    return type(face)(
        BRepBuilderAPI_MakeFace(
            bspline,
            adaptor.FirstUParameter(),
            adaptor.LastUParameter(),
            adaptor.FirstVParameter(),
            adaptor.LastVParameter(),
            1e-7,
        ).Face()
    )


def test_facade_is_provisional_not_a_root_export() -> None:
    assert "GeometryGraph" not in quiddity.__all__
    assert not hasattr(quiddity, "GeometryGraph")


def test_projected_value_schemas_do_not_leak_private_runtime_types() -> None:
    projected = (
        FaceRef,
        BoundaryRef,
        BlendRef,
        AnalyticSurface,
        RefusedSurface,
        BlendFact,
        GeometryProvenance,
        CollapsedBridge,
        FaceInspection,
    )

    assert all("quiddity._" not in repr(typing.get_type_hints(value)) for value in projected)


def test_face_handles_are_run_local_and_copying_does_not_confer_authority() -> None:
    part = Box(10, 20, 30)
    graph = GeometryGraph(part)
    other = GeometryGraph(part)
    ref = graph.faces[0]

    assert graph.face(ref).wrapped.IsSame(part.faces()[0].wrapped)
    with pytest.raises(ValueError, match="foreign"):
        other.face(ref)
    with pytest.raises(ValueError, match="copied"):
        graph.face(copy.copy(ref))


def test_surface_fact_and_anchor_support_the_draftwright_fillet_workflow() -> None:
    graph, ref, fact = _cylinder_fact(Cylinder(8, 20))

    assert fact.parameters[6] == pytest.approx(8)
    assert max(abs(value) for value in fact.parameters[3:6]) == pytest.approx(1)
    anchor = graph.surface_anchor(ref)
    assert (anchor[0] ** 2 + anchor[1] ** 2) ** 0.5 == pytest.approx(8)
    assert Vertex(*anchor).distance_to(graph.face(ref)) < 1e-7


def test_standalone_face_inspection_exposes_no_graph_handle() -> None:
    face = Cylinder(8, 20).faces().filter_by(GeomType.CYLINDER)[0]

    inspection = inspect_face(face)

    assert isinstance(inspection.surface, AnalyticSurface)
    assert inspection.surface.parameters[6] == pytest.approx(8)
    assert inspection.anchor is not None
    assert Vertex(*inspection.anchor).distance_to(face) < 1e-7
    assert set(typing.get_type_hints(FaceInspection)) == {"surface", "anchor"}


def test_recovered_surface_retains_recovery_and_orientation_provenance() -> None:
    face = _as_bspline_face(max(Box(10, 5, 2).faces(), key=lambda item: item.area))
    graph = GeometryGraph(face)
    fact = graph.surface_fact(graph.faces[0])

    assert isinstance(fact, AnalyticSurface)
    assert fact.kind.value == "plane"
    assert fact.provenance.value == "recovered"
    assert fact.orientation.value == "recovered-unoriented"

    inspected = inspect_face(face)
    assert inspected.surface == fact
    assert inspected.anchor is not None


def test_unsupported_surface_is_a_closed_refusal_value() -> None:
    graph = GeometryGraph(Torus(8, 2))

    refusals = [graph.surface_fact(ref) for ref in graph.faces]
    assert any(isinstance(fact, RefusedSurface) for fact in refusals)
    assert all(isinstance(fact, (AnalyticSurface, RefusedSurface)) for fact in refusals)

    inspected = inspect_face(Torus(8, 2).faces()[0])
    assert isinstance(inspected.surface, RefusedSurface)


def test_blend_collapse_retains_complete_opaque_provenance() -> None:
    graph = GeometryGraph(_blend_interrupted_boss())
    chains = tuple(fact for fact in graph.blend_facts() if fact.side == "convex")

    assert len(chains) == 6
    bridges = graph.collapsed_bridges(tuple(chain.ref for chain in chains))
    assert len(bridges) == 6
    for chain in chains:
        bridge = next(
            item
            for item in bridges
            if frozenset(item.supports) == frozenset(chain.supports[0] | chain.supports[1])
        )
        assert bridge.provenance.faces == frozenset(
            (*chain.blend_faces, *chain.supports[0], *chain.supports[1])
        )
        assert sorted(map(id, bridge.provenance.boundary)) == sorted(map(id, chain.boundary))


@pytest.mark.parametrize("transform", [Pos(13, -7, 4), Rot(0, 0, 37), Pos(2, 3, 5) * Rot(0, 0, 91)])
def test_surface_and_blend_queries_survive_rigid_transforms(transform) -> None:
    graph = GeometryGraph(transform * _blend_interrupted_boss())

    assert len(tuple(fact for fact in graph.blend_facts() if fact.side == "convex")) == 6


def test_step_roundtrip_preserves_the_spike_contract(tmp_path) -> None:
    path = tmp_path / "blend-boss.step"
    export_step(_blend_interrupted_boss(), path)
    graph = GeometryGraph(import_step(path))

    assert len(tuple(fact for fact in graph.blend_facts() if fact.side == "convex")) == 6
