# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Bounded three-plane, two-ended open support proof for legacy recess candidates."""

from __future__ import annotations

from dataclasses import dataclass
from itertools import product

from build123d import Face, Plane, Solid, Vector, Wire

from quiddity._adjacency import FaceGraph, FaceNode
from quiddity._effective_surfaces import EffectiveSurfaceQuery
from quiddity._recess_records import Channel, Pocket
from quiddity._section_passages import _end_slab, _probe_prism
from quiddity._sections import LocalFrame, PlanarSection, SectionVertex
from quiddity._support_apertures import proved_support_apertures
from quiddity._support_patches import covered_patch
from quiddity._volume_probe import material_fraction as _material_fraction


@dataclass(frozen=True)
class OpenChannelProof:
    axis: str
    run_interval: tuple[float, float]
    boundary: tuple[tuple[float, float], ...]
    aperture_faces: tuple[FaceNode, ...] = ()


def _bounds(graph: FaceGraph, node: FaceNode):
    points = [tuple(vertex) for vertex in graph.face(node).vertices()]
    return tuple((min(p[i] for p in points), max(p[i] for p in points)) for i in range(3))


def _supports(
    graph: FaceGraph,
    nodes: frozenset[FaceNode],
    bounds,
    axis,
    at,
    sign,
    *,
    surfaces: EffectiveSurfaceQuery | None = None,
    aperture_faces: set[FaceNode] | None = None,
) -> bool:
    """Require complete actual support or individually proved interior apertures."""
    transverse = [i for i in range(3) if i != axis]
    corners = list(product(*(bounds[i] for i in transverse)))
    points = []
    for a, b in (corners[0], corners[1], corners[3], corners[2]):
        xyz = [0.0, 0.0, 0.0]
        xyz[axis], xyz[transverse[0]], xyz[transverse[1]] = at, a, b
        points.append(Vector(*xyz))
    patch = Face(Wire.make_polygon((*points, points[0])))
    supports = []
    for node in nodes:
        normal = graph.normal(node) if graph.is_planar(node) else None
        if normal is None or normal[axis] * sign < 1 - 1e-8:
            continue
        limits = _bounds(graph, node)[axis]
        if max(abs(limit - at) for limit in limits) > 1e-6:
            continue
        supports.append(node)
    source_faces = tuple(graph.face(node) for node in supports)
    if covered_patch(patch, source_faces):
        return True
    if surfaces is None:
        return False
    apertures = tuple(
        proof
        for node in supports
        for proof in proved_support_apertures(graph, surfaces, node, patch)
    )
    if not covered_patch(patch, (*source_faces, *(proof.disk for proof in apertures))):
        return False
    if aperture_faces is not None:
        aperture_faces.update(proof.cylinder for proof in apertures)
    return True


def prove_open_channel(
    graph: FaceGraph,
    defining: frozenset[FaceNode],
    constituent: frozenset[FaceNode],
    record: Pocket | Channel,
    *,
    surfaces: EffectiveSurfaceQuery,
) -> OpenChannelProof | None:
    if isinstance(record, Pocket) and record.edge_anchored:
        return None
    owner = graph.common_valid_solid(constituent)
    if owner is None:
        return None
    w, d, run = map("xyz".index, (record.width_axis, record.depth_axis, record.long_axis))
    walls = {}
    for node in defining:
        normal = graph.normal(node) if graph.is_planar(node) else None
        if normal is None or abs(normal[w]) < 1 - 1e-8:
            continue
        sign = 1 if normal[w] > 0 else -1
        if sign in walls:
            return None  # ambiguous opposed-wall authority
        walls[sign] = _bounds(graph, node)
    if set(walls) != {-1, 1}:
        return None
    bounds = [
        (max(walls[1][i][0], walls[-1][i][0]), min(walls[1][i][1], walls[-1][i][1]))
        for i in range(3)
    ]
    bounds[w] = (sum(walls[1][w]) / 2, sum(walls[-1][w]) / 2)
    if any(high - low <= 1e-6 for low, high in bounds):
        return None
    floor = bounds[d][0 if record.open_sign == 1 else 1]
    mouth = bounds[d][1 if record.open_sign == 1 else 0]
    aperture_faces: set[FaceNode] = set()
    try:
        if not all(
            _supports(
                graph,
                constituent,
                bounds,
                axis,
                at,
                sign,
                surfaces=surfaces,
                aperture_faces=aperture_faces,
            )
            for axis, at, sign in (
                (w, bounds[w][0], 1),
                (w, bounds[w][1], -1),
                (d, floor, record.open_sign),
            )
        ):
            return None
        if graph.common_valid_solid((*constituent, *aperture_faces)) != owner:
            return None
        centre = tuple((low + high) / 2 for low, high in bounds)
        frame = LocalFrame.principal(record.long_axis, (centre[0], centre[1], centre[2]))
        local_axes = {0: (1, 2), 1: (2, 0), 2: (0, 1)}[run]
        half_u, half_v = ((bounds[i][1] - bounds[i][0]) / 2 for i in local_axes)
        section = PlanarSection(
            tuple(
                SectionVertex(point)
                for point in (
                    (-half_u, -half_v),
                    (half_u, -half_v),
                    (half_u, half_v),
                    (-half_u, half_v),
                )
            )
        )
        solid = graph.solid_shape(owner)
        span = bounds[run]
        thickness = max(2e-5, max(1.0, span[1] - span[0], half_u, half_v) * 1e-4)
        if _material_fraction(solid, _probe_prism(frame, span, section)) > 1e-9:
            return None
        if any(
            _material_fraction(solid, _end_slab(frame, end, sign, thickness, section)) > 1e-9
            for end, sign in ((span[0], -1), (span[1], 1))
        ):
            return None
        # The lateral opening is physical absence too, not a fourth unobserved support wall.
        lateral_bounds = list(bounds)
        lateral_bounds[d] = tuple(
            sorted((mouth + record.open_sign * 1e-6, mouth + record.open_sign * thickness))
        )
        origin = Vector(*(pair[0] for pair in lateral_bounds))
        dx, dy, dz = (hi - lo for lo, hi in lateral_bounds)
        probe = Solid.make_box(dx, dy, dz, plane=Plane(origin))
        if _material_fraction(solid, probe) > 1e-9:
            return None
    except (RuntimeError, TypeError, ValueError, ZeroDivisionError):
        return None
    transverse = [i for i in range(3) if i != run]
    chain = []
    for width, depth in (
        (bounds[w][0], mouth),
        (bounds[w][0], floor),
        (bounds[w][1], floor),
        (bounds[w][1], mouth),
    ):
        xyz = [0.0, 0.0, 0.0]
        xyz[w], xyz[d] = width, depth
        chain.append((xyz[transverse[0]], xyz[transverse[1]]))
    return OpenChannelProof(
        record.long_axis,
        (round(span[0], 3), round(span[1], 3)),
        tuple(chain),
        tuple(sorted(aperture_faces, key=lambda node: node.index)),
    )
