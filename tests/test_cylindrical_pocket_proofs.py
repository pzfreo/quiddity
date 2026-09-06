from __future__ import annotations

import json
import math

import pytest
from build123d import (
    Box,
    Compound,
    Cylinder,
    Face,
    Plane,
    Polygon,
    Pos,
    Rot,
    ShapeList,
    Solid,
    Vertex,
    Wire,
    export_step,
    extrude,
    import_step,
)

from quiddity import CylindricalEndSurface, build_section_recess_document
from quiddity._adjacency import FaceGraph
from quiddity._cylindrical_pockets import cylindrical_pocket_proofs
from quiddity._effective_surfaces import EffectiveSurfaceIndex
from quiddity._section_recess_geometry import _cylindrical_candidate


def _base(scale=1.0, offset=0.0):
    return Rot(0, 90, 0) * Cylinder(20 * scale, 80 * scale) - Pos(
        0, offset * scale, 14 * scale
    ) * Box(6 * scale, 24 * scale, 12 * scale)


def _proofs(part):
    graph = FaceGraph(part)
    return graph, cylindrical_pocket_proofs(graph, EffectiveSurfaceIndex(graph))


def _shape(value):
    return Compound(value) if isinstance(value, ShapeList) else value


def _reconstruct_json_pocket(geometry):
    """Consumer reconstruction from JSON alone, independent of producer helpers."""
    frame = geometry["frame"]
    ends = geometry["ends"]
    cylinder_index = next(
        i for i, name in enumerate(("low", "high")) if ends[name]["surface"]["type"] == "cylinder"
    )
    cylinder = ends[("low", "high")[cylinder_index]]["surface"]
    floor = geometry["run_interval"][1 - cylinder_index]
    points = [vertex["point"] for vertex in geometry["profile"]["boundary"]]
    direction = cylinder["axis_direction"]
    norm = math.hypot(*direction)
    a, b = (coordinate / norm for coordinate in direction)
    cx, cy, cz = cylinder["axis_point"]
    radius = cylinder["radius"]

    def world(u, v, s):
        return tuple(
            frame["origin"][i] + u * frame["u"][i] + v * frame["v"][i] + s * frame["run"][i]
            for i in range(3)
        )

    top = cz + (radius if cylinder["branch"] == "positive" else -radius)
    base = Face(Wire.make_polygon([world(u, v, floor) for u, v in points], close=True))
    sweep = Solid.extrude(base, tuple((top - floor) * x for x in frame["run"]))
    axial = [a * (u - cx) + b * (v - cy) for u, v in points]
    start, stop = min(axial) - radius, max(axial) + radius
    axis = tuple(a * frame["u"][i] + b * frame["v"][i] for i in range(3))
    stock = Solid.make_cylinder(
        radius, stop - start, Plane(origin=world(cx + a * start, cy + b * start, cz), z_dir=axis)
    )
    return _shape(sweep.intersect(stock))


def _json_boundary_samples(geometry):
    """Sample curved interiors as well as corners, without producer geometry code."""
    frame = geometry["frame"]
    ends = geometry["ends"]
    cylinder_index = next(
        i for i, name in enumerate(("low", "high")) if ends[name]["surface"]["type"] == "cylinder"
    )
    cylinder = ends[("low", "high")[cylinder_index]]["surface"]
    floor = geometry["run_interval"][1 - cylinder_index]
    points = [vertex["point"] for vertex in geometry["profile"]["boundary"]]
    a, b = cylinder["axis_direction"]
    norm = math.hypot(a, b)
    a, b = a / norm, b / norm
    cx, cy, cz = cylinder["axis_point"]
    sign = 1 if cylinder["branch"] == "positive" else -1

    def sample(u, v, fraction):
        q = -b * (u - cx) + a * (v - cy)
        roof = cz + sign * math.sqrt(cylinder["radius"] ** 2 - q**2)
        s = floor + fraction * (roof - floor)
        return tuple(
            frame["origin"][i] + u * frame["u"][i] + v * frame["v"][i] + s * frame["run"][i]
            for i in range(3)
        )

    fractions = (0, 0.25, 0.5, 0.75, 1)
    # These authored fixtures have four-sided profiles; interpolation traverses
    # the actual rotated rectangle rather than its enclosing axis-aligned box.
    assert len(points) == 4
    for t in fractions:
        for r in fractions:
            u, v = (
                (1 - t) * (1 - r) * points[0][i]
                + t * (1 - r) * points[1][i]
                + t * r * points[2][i]
                + (1 - t) * r * points[3][i]
                for i in range(2)
            )
            yield sample(u, v, 0)
            yield sample(u, v, 1)
    for first, second in zip(points, points[1:] + points[:1], strict=True):
        for t in fractions:
            u, v = ((1 - t) * first[i] + t * second[i] for i in range(2))
            for r in fractions:
                yield sample(u, v, r)


@pytest.mark.parametrize("scale", [0.1, 1.0, 10.0])
@pytest.mark.parametrize("offset", [0.0, 3.0])
@pytest.mark.parametrize("rotation", [Rot(), Rot(17, 31, 43), Rot(180, 0, 0)])
def test_original_sources_prove_complete_cylindrical_end(scale, offset, rotation):
    graph, proofs = _proofs(Pos(3, 7, 11) * rotation * _base(scale, offset))
    assert len(proofs) == 1
    proof = proofs[0]
    assert len(proof.walls) == 4
    assert proof.stock not in (*proof.walls, proof.floor)
    assert graph.common_valid_solid((proof.floor, *proof.walls, proof.stock)) == proof.owner
    assert proof.radius == pytest.approx(20 * scale)
    volume = 1544.4026611038823 if offset == 0 else 1503.322366310343
    assert proof.volume == pytest.approx(volume * scale**3, rel=1e-9)


@pytest.mark.parametrize("kind", ["side_breakout", "pierced_wall", "bridge", "mouth_bridge"])
def test_missing_support_and_obstructions_refused(kind):
    part = _base()
    if kind == "side_breakout":
        part = Rot(0, 90, 0) * Cylinder(20, 80) - Pos(0, 6, 14) * Box(6, 36, 12)
    elif kind == "pierced_wall":
        part -= Pos(0, 0, 12) * Rot(0, 90, 0) * Cylinder(1, 30)
    elif kind == "bridge":
        part += Pos(0, 0, 12) * Box(10, 1, 1)
    else:
        part += Pos(0, 0, 20) * Box(10, 1, 1)
    assert not _proofs(part)[1]
    document = build_section_recess_document(part)
    assert not any(
        isinstance(end.surface, CylindricalEndSurface)
        for record in document.occurrences
        for end in (record.geometry.ends.low, record.geometry.ends.high)
    )


def test_compound_ownership_and_step(tmp_path):
    part = Compound([_base(), Pos(100, 0, 0) * _base()])
    _, proofs = _proofs(part)
    assert len(proofs) == 2
    assert proofs[0].owner != proofs[1].owner
    assert set(proofs[0].walls).isdisjoint(proofs[1].walls)
    path = tmp_path / "curved-mouth.step"
    export_step(part, path)
    _, reread = _proofs(import_step(path))
    assert len(reread) == 2
    assert [p.volume for p in reread] == pytest.approx([p.volume for p in proofs])


@pytest.mark.parametrize("sides", [3, 6])
@pytest.mark.parametrize("rotation", [Rot(), Rot(17, 31, 43)])
def test_polygonal_cylindrical_pocket_keeps_complete_original_wall_ring(sides, rotation):
    points = [
        (8 * math.cos(2 * math.pi * i / sides), 8 * math.sin(2 * math.pi * i / sides))
        for i in range(sides)
    ]
    cutter = Pos(0, 0, 8) * extrude(Polygon(*points, align=None), amount=20)
    part = Pos(3, 7, 11) * rotation * (Rot(0, 90, 0) * Cylinder(20, 80) - cutter)
    document = build_section_recess_document(part)
    assert len(document.occurrences) == 1
    record = document.occurrences[0]
    assert record.classification.feature_kind == "pocket"
    assert record.classification.section_shape == {3: "triangular", 6: "hexagonal"}[sides]
    assert len(record.geometry.profile.boundary) == sides
    assert len(record.evidence.defining_faces) == sides
    assert len(record.evidence.constituent_faces) == sides + 1
    assert any(
        isinstance(end.surface, CylindricalEndSurface)
        for end in (record.geometry.ends.low, record.geometry.ends.high)
    )


@pytest.mark.parametrize("scale", [0.1, 1.0, 10.0])
@pytest.mark.parametrize("offset", [0.0, 3.0])
@pytest.mark.parametrize("rotation", [Rot(), Rot(17, 31, 43), Rot(180, 0, 0)])
def test_public_cylindrical_pocket_preserves_source_evidence(scale, offset, rotation):
    part = Pos(3, 7, 11) * rotation * _base(scale, offset)
    document = build_section_recess_document(part)
    assert document.schema_version == 3
    assert len(document.occurrences) == 1
    assert not document.refusals
    record = document.occurrences[0]
    assert record.classification.feature_kind == "pocket"
    assert record.classification.section_shape == "rectangular"
    assert len(record.evidence.defining_faces) == 4
    assert len(record.evidence.constituent_faces) == 5
    surfaces = (record.geometry.ends.low.surface, record.geometry.ends.high.surface)
    cylinders = [surface for surface in surfaces if isinstance(surface, CylindricalEndSurface)]
    assert len(cylinders) == 1
    assert cylinders[0].radius == pytest.approx(20 * scale)

    # No private proof, CAD object, or original face is available to this consumer.
    value = json.loads(json.dumps(record.to_dict()))
    reconstructed = _reconstruct_json_pocket(value["geometry"])
    stock = Pos(3, 7, 11) * rotation * (Rot(0, 90, 0) * Cylinder(20 * scale, 80 * scale))
    expected = _shape(stock.cut(part))
    assert reconstructed.is_valid
    assert abs(reconstructed.volume - expected.volume) <= expected.area * 0.002
    # Near-coincident rotated Boolean cuts are kernel-unstable. Check emitted
    # physical boundaries directly, including the curved face interior/crest.
    boundary = Compound(expected.faces())
    assert (
        max(
            boundary.distance_to(Vertex(point))
            for point in _json_boundary_samples(value["geometry"])
        )
        <= 0.002
    )


def test_tiny_source_axis_tilt_cannot_exceed_public_displacement_budget():
    part = Rot(0, 90 - math.degrees(5e-9), 0) * Cylinder(20, 1600000)
    part -= Pos(0, 0, 14) * Box(1200000, 24, 12)
    graph, proofs = _proofs(part)
    assert len(proofs) == 1
    with pytest.raises(ValueError, match="displacement limit"):
        _cylindrical_candidate(graph, proofs[0])
