# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

"""How the solid turns where two faces meet — the half of an attributed graph this package lacked.

Nodes carried facts about a face. Nothing said what happened *between* two of them, so a
recogniser needing that inferred it at the point of use, or did not ask. `FaceGraph.arc` is that
attribute, and these are the tests that say it is right.

**Symmetry is the assertion that needs no known answer**, and it is the one that caught the two
real errors here. A dihedral is a property of the pair, so `arc(a, b)` and `arc(b, a)` must agree
however the faces are handed in; when they did not, the cause was a sign convention rather than
a hard geometry case, and no amount of counting expected convex edges would have localised it.

The counts below are worth reading carefully, because the obvious expectation is wrong. A blind
pocket has **16** convex arcs and 8 concave, not 12 and 12: its *mouth* edges are convex, because
the material forms a 90-degree wedge where the top face meets a wall, not the 270-degree one the
wall-to-wall and wall-to-floor edges have.
"""

from __future__ import annotations

from collections import Counter
from pathlib import Path

import pytest
from build123d import (
    Axis,
    Box,
    Compound,
    Cone,
    Cylinder,
    Edge,
    Face,
    Plane,
    Pos,
    Rot,
    Shell,
    Solid,
    Wire,
    export_step,
    fillet,
    import_step,
)
from OCP.BRepBuilderAPI import BRepBuilderAPI_MakeFace, BRepBuilderAPI_Sewing
from OCP.Geom import Geom_BezierSurface
from OCP.gp import gp_Pnt
from OCP.TColgp import TColgp_Array2OfPnt
from OCP.TopoDS import TopoDS

from quiddity._adjacency import (
    FaceEdges,
    FaceGraph,
    _SmoothSideObservation,
    is_any_smooth,
    is_opposed_nonsmooth,
    same_arc_kind,
)
from quiddity._geometry import SMOOTH_ARC_GAP
from tests.golden._common import load_fixture

_CORPUS = Path(__file__).parent / "corpus" / "mfcadpp"


def _arcs(part):
    """Every arc of *part*, counted by classification."""

    graph = FaceGraph(part)
    found: Counter = Counter()
    for a in graph.nodes:
        for b in graph.neighbours(a):
            if b.index > a.index:
                found[graph.arc(a, b)] += 1
    return found


def test_closed_arc_helpers_never_infer_a_turn_from_absence() -> None:
    assert is_opposed_nonsmooth("convex", "concave")
    assert is_opposed_nonsmooth("concave", "convex")
    assert not is_opposed_nonsmooth("convex", "unknown")
    assert not is_opposed_nonsmooth("concave", None)
    assert same_arc_kind("convex", "convex")
    assert same_arc_kind(None, None)
    assert same_arc_kind("unknown", "unknown")
    assert not same_arc_kind("unknown", None)


def _plain():
    return Box(20, 20, 20)


def _blind_pocket():
    return Box(40, 40, 20) - Pos(0, 0, 6) * Box(12, 12, 12)


def _through_passage():
    return Box(40, 40, 20) - Box(12, 12, 40)


def _bore():
    return Box(40, 40, 20) - Cylinder(6, 40)


def _countersink():
    return Box(40, 40, 20) - Pos(0, 0, 5) * Cone(9, 4, 12)


def _filleted():
    return fillet(Box(40, 20, 10).edges().filter_by(Axis.Z), radius=3)


@pytest.mark.parametrize(
    "build",
    [_plain, _blind_pocket, _through_passage, _bore, _countersink, _filleted],
)
def test_an_arc_reads_the_same_from_either_face(build):
    """A dihedral belongs to the pair, so the order the faces are handed in cannot matter.

    This is the strongest assertion in the file because it needs no expected answer, and it is
    what localised both real errors during development: flipping the edge direction for a
    ``REVERSED`` face double-corrected what `normal_at` already handled, and it showed up here
    as exactly half a box's edges disagreeing with themselves.
    """

    graph = FaceGraph(build())
    for a in graph.nodes:
        for b in graph.neighbours(a):
            assert graph.arc(a, b) == graph.arc(b, a)


def test_every_edge_of_a_plain_box_is_convex():
    """The sign check. Symmetry alone would survive a global sign error; this will not."""

    assert _arcs(_plain()) == {"convex": 12}


def test_a_pocket_is_concave_where_it_wraps_and_convex_where_it_opens():
    """16 and 8, not 12 and 12 — the mouth edges are convex and the obvious count is wrong.

    Four box verticals, four top perimeter, four bottom perimeter and **four pocket mouths** are
    convex: at a mouth the material is a 90-degree wedge between the top face and the wall. The
    eight concave are the four wall-to-wall verticals and the four wall-to-floor, where the
    material wraps 270 degrees around the edge.
    """

    assert _arcs(_blind_pocket()) == {"convex": 16, "concave": 8}


def test_a_through_void_is_concave_only_along_its_corners():
    """The passage has no floor, so its four concave arcs are the wall-to-wall verticals alone.

    Its two mouths are convex for the same reason a pocket's is, which is why this differs from
    the pocket by exactly the four wall-to-floor arcs.
    """

    assert _arcs(_through_passage()) == {"convex": 20, "concave": 4}


def test_a_curved_face_classifies_and_a_conical_one_does_too():
    """The case a whole-face normal cannot serve, and the reason the attribute reads per point.

    A cone's normal differs everywhere on it, so an arc against one has to be read where the
    faces meet. A groove's conical lead-in is exactly this shape, and it is the geometry
    ADR 0004's amendment is about seeing across.
    """

    assert _arcs(_bore()) == {"convex": 14}
    assert _arcs(_countersink()) == {"convex": 13, "concave": 1}


def test_a_tangential_blend_reads_as_smooth():
    """The half with live consumers: seeing *through* a blend and *across* a split face.

    A fillet meets each neighbour tangentially, so those arcs are neither convex nor concave —
    there is no corner. Four rounded corners, two neighbours each, is eight.

    This is also the assertion that caught the attribute reading normals at the wrong place.
    `normal_at` ignores the point it is handed — asked at 0, 90 and 180 degrees around a
    cylinder it returns one vector three times — so an earlier version read each patch's middle
    and every one of these came back convex, with the tangency invisible.
    """

    assert _arcs(_filleted()) == {"convex": 16, "smooth": 8}


def _smooth_pairs(graph: FaceGraph):
    return [
        (a, b)
        for a in graph.nodes
        for b in graph.neighbours(a)
        if b.index > a.index and is_any_smooth(graph.arc(a, b))
    ]


def test_external_and_internal_rounds_have_opposite_smooth_sides() -> None:
    external = FaceGraph(_filleted())
    external_pairs = _smooth_pairs(external)
    assert len(external_pairs) == 8
    assert {external.smooth_side(a, b) for a, b in external_pairs} == {"convex"}

    cutter = fillet(Box(12, 12, 20).edges().filter_by(Axis.Z), radius=2)
    internal = FaceGraph(Box(40, 40, 10) - Pos(0, 0, -5) * cutter)
    internal_pairs = _smooth_pairs(internal)
    assert len(internal_pairs) == 8
    assert {internal.smooth_side(a, b) for a, b in internal_pairs} == {"concave"}


def _split_native_solids():
    rectangle = Solid.make_loft(
        [Wire.make_rect(10, 8, Plane.XY.offset(z)) for z in (0, 5, 10)], ruled=True
    )
    cylinder = Solid.make_loft(
        [Wire.make_circle(5, Plane.XY.offset(z)) for z in (0, 5, 10)], ruled=True
    )
    cone = Solid.make_loft(
        [Wire.make_circle(radius, Plane.XY.offset(z)) for radius, z in ((5, 0), (4, 5), (3, 10))],
        ruled=True,
    )
    lower = Solid.make_sphere(5, angle1=-90, angle2=0)
    upper = Solid.make_sphere(5, angle1=0, angle2=90)
    sphere = Solid(
        Shell(
            [
                max(lower.faces(), key=lambda face: face.area),
                max(upper.faces(), key=lambda face: face.area),
            ]
        )
    )
    return rectangle, cylinder, cone, sphere


@pytest.mark.parametrize("part", _split_native_solids())
def test_equivalent_native_surface_splits_are_neutral(part) -> None:
    graph = FaceGraph(part)
    pairs = _smooth_pairs(graph)

    assert pairs
    assert {graph.smooth_side(a, b) for a, b in pairs} == {"neutral"}


def test_tangent_higher_order_bezier_is_not_a_neutral_continuation(monkeypatch) -> None:
    """Equal tangent and boundary curvature do not prove the surfaces continue.

    The Bezier height is ``u**4``: at ``u == 0`` it shares a plane's position, tangent and
    second derivative, but bends away immediately inside the patch.  Neutrality is therefore
    authorized only by the native-analytic equivalence seam, never by sampled D2 equality.
    """

    poles = TColgp_Array2OfPnt(1, 5, 1, 2)
    for u in range(1, 6):
        for v in range(1, 3):
            poles.SetValue(
                u,
                v,
                gp_Pnt((u - 1) / 4, v - 1, 1.0 if u == 5 else 0.0),
            )
    curved = Face(BRepBuilderAPI_MakeFace(Geom_BezierSurface(poles), 1e-7).Face())
    plane = Face.make_rect(1, 1, Plane(origin=(-0.5, 0.5, 0)))
    sewing = BRepBuilderAPI_Sewing(1e-6)
    sewing.Add(curved.wrapped)
    sewing.Add(plane.wrapped)
    sewing.Perform()
    graph = FaceGraph(Shell(TopoDS.Shell_s(sewing.SewedShape())))
    a, b = graph.nodes
    (edge,) = graph.shared_edges(a, b)

    assert is_any_smooth(graph.arc(a, b))
    assert not graph._native_continuation(a, b, local=1.0)
    observations = [
        graph._normal_curvature(node, edge, fraction)
        for node in (a, b)
        for fraction in (0.25, 0.5, 0.75)
    ]
    assert all(observation is not None for observation in observations)
    assert all(abs(observation[0]) < 1e-12 for observation in observations if observation)
    assert {observation[1] for observation in observations if observation} == {False, True}

    # The shell deliberately has no material-side authority. Bypass only that already-tested
    # ownership gate so this adversary reaches the real shared-edge D2 reducer.
    monkeypatch.setattr(graph, "_eligible_side_edge", lambda *_: True)
    assert graph.smooth_side(a, b) == "unproven"


def test_native_continuation_primitive_failure_is_unproven(monkeypatch) -> None:
    graph = FaceGraph(_split_native_solids()[1])
    a, b = _smooth_pairs(graph)[0]
    monkeypatch.setattr(
        "quiddity._adjacency.validated_parameters",
        lambda *_: (_ for _ in ()).throw(ValueError("invalid primitive")),
    )

    assert not graph._native_continuation(a, b, local=1.0)


def test_split_native_side_survives_step_round_trip(tmp_path) -> None:
    cylinder = _split_native_solids()[1]
    path = tmp_path / "split-cylinder.step"
    assert export_step(cylinder, path)
    graph = FaceGraph(import_step(path))
    pairs = _smooth_pairs(graph)

    assert pairs
    assert {graph.smooth_side(a, b) for a, b in pairs} == {"neutral"}


@pytest.mark.parametrize(
    ("part", "expected"),
    [
        (_filleted(), "convex"),
        (
            Box(40, 40, 10)
            - Pos(0, 0, -5) * fillet(Box(12, 12, 20).edges().filter_by(Axis.Z), radius=2),
            "concave",
        ),
    ],
)
def test_sided_rounds_survive_step_round_trip(tmp_path, part, expected) -> None:
    path = tmp_path / f"{expected}-round.step"
    assert export_step(part, path)
    graph = FaceGraph(import_step(path))
    pairs = _smooth_pairs(graph)

    assert pairs
    assert {graph.smooth_side(a, b) for a, b in pairs} == {expected}


def test_open_smooth_join_remains_unproven_after_step_round_trip(tmp_path) -> None:
    source = FaceGraph(_filleted())
    a, b = _smooth_pairs(source)[0]
    path = tmp_path / "open-smooth-shell.step"
    assert export_step(Shell([source.face(a), source.face(b)]), path)
    graph = FaceGraph(import_step(path))
    pairs = _smooth_pairs(graph)

    assert len(pairs) == 1
    assert graph.smooth_side(*pairs[0]) == "unproven"


@pytest.mark.parametrize(
    "part",
    [
        _filleted().mirror(Plane.YZ),
        _filleted().rotate(Axis((0, 0, 0), (1, 1, 0)), 37),
        _filleted().scale(10),
    ],
)
def test_smooth_convex_side_is_rigid_transform_and_scale_invariant(part) -> None:
    graph = FaceGraph(part)
    pairs = _smooth_pairs(graph)

    assert len(pairs) == 8
    assert {graph.smooth_side(a, b) for a, b in pairs} == {"convex"}


def test_smooth_side_is_symmetric_and_cached_once_per_edge(monkeypatch) -> None:
    graph = FaceGraph(_filleted())
    a, b = _smooth_pairs(graph)[0]
    calls = []
    original = graph._derive_smooth_side_edge

    def counted(left, right, edge):
        calls.append(edge)
        return original(left, right, edge)

    monkeypatch.setattr(graph, "_derive_smooth_side_edge", counted)
    first = graph.smooth_side(a, b)
    assert first in ("convex", "concave", "neutral", "unproven")
    assert graph.smooth_side(b, a) == first
    assert len(calls) == len(graph.shared_edges(a, b))

    edge = graph.shared_edges(a, b)[0]
    cached = graph._smooth_side_edge(a, b, edge)
    assert graph._smooth_side_edge(b, a, edge) is cached


def test_invalid_local_side_scale_fails_closed(monkeypatch) -> None:
    graph = FaceGraph(_filleted())
    a, b = _smooth_pairs(graph)[0]
    monkeypatch.setattr(graph, "_eligible_side_edge", lambda *_: True)

    class InvalidLength:
        length = float("nan")

    assert graph._derive_smooth_side_edge(a, b, InvalidLength()).result == "unproven"


def test_smooth_side_is_independent_of_fresh_face_and_edge_order() -> None:
    part = _filleted()

    class ReorderedPart:
        def faces(self):
            return list(reversed(part.faces()))

        def solids(self):
            return part.solids()

    class ReversedFaceEdges(FaceEdges):
        def of(self, face):
            return list(reversed(super().of(face)))

    baseline = FaceGraph(part)
    reordered = FaceGraph(ReorderedPart(), face_edges=ReversedFaceEdges())

    def keyed_sides(graph):
        found = {}
        pairs = _smooth_pairs(graph)
        for a, b in pairs:
            faces = tuple(
                sorted(
                    (
                        round(graph.face(node).center().X, 9),
                        round(graph.face(node).center().Y, 9),
                        round(graph.face(node).center().Z, 9),
                        round(float(graph.face(node).area), 9),
                    )
                    for node in (a, b)
                )
            )
            edges = tuple(
                sorted(
                    (
                        round(edge.center().X, 9),
                        round(edge.center().Y, 9),
                        round(edge.center().Z, 9),
                        round(float(edge.length), 9),
                    )
                    for edge in graph.shared_edges(a, b)
                )
            )
            found[(faces, edges)] = graph.smooth_side(a, b)
        assert len(found) == len(pairs), "the test signature must identify each pair uniquely"
        return found

    assert keyed_sides(baseline) == keyed_sides(reordered)


def test_duplicate_solid_ownership_cannot_authorize_material_side() -> None:
    part = _filleted()

    class AmbiguousOwnership:
        def faces(self):
            return part.faces()

        def solids(self):
            solid = part.solids()[0]
            return [solid, solid]

    graph = FaceGraph(AmbiguousOwnership())
    pairs = _smooth_pairs(graph)
    assert pairs
    assert {graph.smooth_side(a, b) for a, b in pairs} == {"unproven"}


@pytest.mark.parametrize("failure", ["part", "solid"])
def test_ownership_kernel_failures_are_side_unproven(failure) -> None:
    part = _filleted()

    class BrokenSolid:
        @property
        def is_valid(self):
            raise RuntimeError("BRepCheck failed")

    class BrokenOwnership:
        def faces(self):
            return part.faces()

        def solids(self):
            if failure == "part":
                raise RuntimeError("solid traversal failed")
            return [BrokenSolid()]

    graph = FaceGraph(BrokenOwnership())
    a, b = _smooth_pairs(graph)[0]
    assert graph.smooth_side(a, b) == "unproven"


def test_a_disconnected_second_solid_does_not_poison_owned_sides() -> None:
    graph = FaceGraph(Compound(children=[_filleted(), Pos(100, 0, 0) * Box(5, 5, 5)]))
    pairs = _smooth_pairs(graph)

    assert len(pairs) == 8
    assert {graph.smooth_side(a, b) for a, b in pairs} == {"convex"}


def test_open_topods_solid_cannot_authorize_material_side() -> None:
    source = FaceGraph(_filleted())
    a, b = _smooth_pairs(source)[0]
    open_shell = Shell([source.face(a), source.face(b)])
    open_solid = Solid(open_shell)

    class OpenOwnership:
        def faces(self):
            return [source.face(a), source.face(b)]

        def solids(self):
            return [open_solid]

    assert not open_solid.is_valid
    graph = FaceGraph(OpenOwnership())
    left, right = graph.nodes
    assert is_any_smooth(graph.arc(left, right))
    assert graph.smooth_side(left, right) == "unproven"


def test_periodic_self_seam_never_becomes_a_pair_side() -> None:
    graph = FaceGraph(Solid.make_sphere(5))
    (node,) = graph.nodes

    assert graph.edges(node), "the sphere must retain its periodic representation edge"
    assert graph.neighbours(node) == ()
    edge = graph.edges(node)[0]
    assert graph._normal_curvature(node, edge, 0.0) is None
    assert graph._normal_curvature(node, edge, 1.0) is None


def test_normal_curvature_ignores_the_input_edge_wrapper_orientation() -> None:
    graph = FaceGraph(_filleted())
    a, b = _smooth_pairs(graph)[0]
    edge = graph.shared_edges(a, b)[0]
    reversed_edge = Edge(TopoDS.Edge_s(edge.wrapped.Reversed()))

    for node in (a, b):
        forward = graph._normal_curvature(node, edge, 0.5)
        backward = graph._normal_curvature(node, reversed_edge, 0.5)
        assert backward == pytest.approx(forward)


def test_edge_orientation_lookup_refuses_a_foreign_edge() -> None:
    graph = FaceGraph(_plain())
    foreign = Box(3, 3, 3).edges()[0]
    with pytest.raises(ValueError, match="absent from its original face"):
        graph._edge_reversed_in_face(graph.nodes[0], foreign)


def test_normal_curvature_kernel_failure_is_unproven(monkeypatch) -> None:
    graph = FaceGraph(_filleted())
    a, b = _smooth_pairs(graph)[0]
    edge = graph.shared_edges(a, b)[0]
    monkeypatch.setattr(
        "quiddity._adjacency.BRepAdaptor_Curve",
        lambda *_: (_ for _ in ()).throw(RuntimeError("curve adaptor failed")),
    )

    assert graph._normal_curvature(a, edge, 0.5) is None


def test_non_manifold_three_face_edge_is_side_unproven() -> None:
    faces = [
        Face.make_rect(1, 1, Plane(origin=(-0.5, 0.5, 0))),
        Face.make_rect(1, 1, Plane(origin=(0.5, 0.5, 0))),
        Face.make_rect(
            1,
            1,
            Plane(origin=(0, 0.5, 0.5), x_dir=(0, 1, 0), z_dir=(1, 0, 0)),
        ),
    ]
    sewing = BRepBuilderAPI_Sewing(1e-6)
    sewing.SetNonManifoldMode(True)
    for face in faces:
        sewing.Add(face.wrapped)
    sewing.Perform()
    graph = FaceGraph(Shell(TopoDS.Shell_s(sewing.SewedShape())))
    pair = next((a, b) for a, b in _smooth_pairs(graph))
    (edge,) = graph.shared_edges(*pair)
    assert len(graph._edge_face_map()[edge]) == 3

    # Bypass only the separately tested solid-ownership gate to isolate the real three-face edge.
    graph._face_solids = ((0,),) * len(graph.nodes)
    graph._closed_solids = frozenset({0})
    assert graph.smooth_side(*pair) == "unproven"


def test_open_faces_can_be_legacy_smooth_but_side_unproven() -> None:
    source = FaceGraph(_filleted())
    a, b = _smooth_pairs(source)[0]
    graph = FaceGraph(Compound(children=[source.face(a), source.face(b)]))
    left, right = graph.nodes

    assert graph.arc(left, right) == "smooth"
    assert graph.smooth_side(left, right) == "unproven"
    assert is_any_smooth(graph.arc(left, right))


@pytest.mark.parametrize(
    ("left", "right", "expected"),
    [
        ((0.0, True), (-0.25, False), "convex"),
        ((0.0, True), (0.25, False), "concave"),
        ((-0.1, False), (-0.2, False), "convex"),
        ((0.1, False), (0.2, False), "concave"),
        ((0.1, False), (0.1, False), "unproven"),
        ((0.5e-6, False), (-0.5e-6, False), "unproven"),
        ((-0.25, False), (0.25, False), "unproven"),
        ((0.0, False), (0.0, False), "unproven"),
    ],
)
def test_smooth_side_reducer_is_total_and_fail_closed(monkeypatch, left, right, expected) -> None:
    """Exercise closed sign states absent from the checked-in geometry matrix.

    The frozen semantic-fixture scan found 36 regular plane/curve samples and zero curved/curved
    same-sign or opposite-sign samples. Real external/internal rounds own the available production
    signs; these exact curvature observations own only the otherwise unavailable reducer states.
    """

    graph = FaceGraph(_filleted())
    a, b = _smooth_pairs(graph)[0]
    edge = graph.shared_edges(a, b)[0]
    answers = {a: left, b: right}
    monkeypatch.setattr(graph, "_normal_curvature", lambda node, *_: answers[node])

    assert graph._smooth_side_sample(a, b, edge, 0.5, 1.0) == expected


def test_disagreeing_samples_and_shared_edges_are_side_unproven(monkeypatch) -> None:
    """Pin reductions that the development geometry cannot currently instantiate.

    A sweep of every checked-in semantic fixture contains no smooth pair sharing several edges,
    so manufacturing two frozen edge observations is the bounded way to prove disagreement is
    fail-closed. Real one-edge convex/concave/neutral joins and real nonsmooth multi-edge pairs
    are covered separately; this test owns only the otherwise unobserved reducer states.
    """

    graph = FaceGraph(_filleted())
    a, b = _smooth_pairs(graph)[0]
    edge = graph.shared_edges(a, b)[0]
    sample_answers = iter(("convex", "concave", "convex"))
    monkeypatch.setattr(graph, "_eligible_side_edge", lambda *_: True)
    monkeypatch.setattr(graph, "_native_continuation", lambda *_args, **_kwargs: False)
    monkeypatch.setattr(graph, "_smooth_side_sample", lambda *_: next(sample_answers))
    assert graph._derive_smooth_side_edge(a, b, edge).result == "unproven"

    graph._smooth_sides.clear()
    monkeypatch.setattr(graph, "shared_edges", lambda *_: ("first", "second"))
    observations = iter(
        (
            _SmoothSideObservation(("convex",) * 3, "convex"),
            _SmoothSideObservation(("concave",) * 3, "concave"),
        )
    )
    monkeypatch.setattr(graph, "_smooth_side_edge", lambda *_: next(observations))
    assert graph.smooth_side(a, b) == "unproven"


def test_failed_differential_enrichment_never_rewrites_legacy_smooth(monkeypatch) -> None:
    graph = FaceGraph(_filleted())
    a, b = _smooth_pairs(graph)[0]
    monkeypatch.setattr(graph, "_normal_curvature", lambda *_: None)

    assert graph.arc(a, b) == "smooth"
    assert graph.smooth_side(a, b) == "unproven"
    assert b in graph.smooth_region(a)


def test_non_smooth_and_foreign_side_queries_fail_intentionally() -> None:
    graph = FaceGraph(_plain())
    a, b = graph.nodes[0], graph.neighbours(graph.nodes[0])[0]
    assert graph.arc(a, b) == "convex"
    assert graph.smooth_side(a, b) is None

    foreign = FaceGraph(_plain()).nodes[0]
    with pytest.raises(ValueError, match="not issued"):
        graph.smooth_side(a, foreign)


def test_a_smooth_region_is_maximal_immutable_and_cached_for_each_member():
    """The gAAG view normalises tangent patches once without changing the base graph."""

    graph = FaceGraph(_filleted())
    a, b = next(
        (a, b) for a in graph.nodes for b in graph.neighbours(a) if graph.arc(a, b) == "smooth"
    )

    region = graph.smooth_region(a)
    assert {a, b} <= region
    assert graph.smooth_region(b) is region
    assert all(
        neighbour in region
        for member in region
        for neighbour in graph.neighbours(member)
        if graph.arc(member, neighbour) == "smooth"
    )


def test_faces_that_do_not_meet_have_no_arc():
    """None, rather than a guess. Two faces with no shared edge have no dihedral to report."""

    graph = FaceGraph(_plain())
    opposite = [
        (a, b)
        for a in graph.nodes
        for b in graph.nodes
        if b.index > a.index and b not in graph.neighbours(a)
    ]
    assert opposite, "a box has opposite faces, or this asserts nothing"
    assert all(graph.arc(a, b) is None for a, b in opposite)


def _slanted_counterbore():
    """A checked-in fixture carrying face pairs that meet along **two** edges.

    Not contrived: swept over the golden corpus, four fixtures have such a pair and this one has
    two, one all-convex and one all-concave. A pair sharing several edges is the case an
    edge-at-a-time classifier answers by accident.
    """

    return load_fixture(
        Path(__file__).parent / "golden" / "slanted_counterbore" / "fixture.py"
    ).build_fixture()


def test_a_pair_meeting_along_several_edges_is_classified_from_all_of_them():
    """The semantic: an arc belongs to the *pair*, and every shared edge has to agree.

    An earlier version read `shared_edges(...)[0]` and so depended on a traversal order
    `neighbours` does not promise. Here the edges do agree, which is why requiring agreement is
    free today -- and why the first pair that disagrees will be loud rather than silently
    order-dependent.
    """

    graph = FaceGraph(_slanted_counterbore())
    multi = [
        (a, b)
        for a in graph.nodes
        for b in graph.neighbours(a)
        if b.index > a.index and len(graph.shared_edges(a, b)) > 1
    ]
    assert multi, "this fixture must still carry a multi-edge pair, or the test proves nothing"

    for a, b in multi:
        per_edge = {graph._classify_at(a, b, edge) for edge in graph.shared_edges(a, b)}
        assert len(per_edge) == 1, "the fixture's edges agree; a disagreement needs its own case"
        assert graph.arc(a, b) == per_edge.pop()


def test_the_answer_does_not_depend_on_the_order_the_shared_edges_come_back_in():
    """Permutation, not just count -- the drift `strict=True` cannot see.

    Classifying every edge and requiring agreement makes this true by construction, which is the
    point: reversing the edge list cannot change a set of size one.
    """

    graph = FaceGraph(_slanted_counterbore())
    for a in graph.nodes:
        for b in graph.neighbours(a):
            edges = graph.shared_edges(a, b)
            if len(edges) < 2:
                continue
            forward = {graph._classify_at(a, b, edge) for edge in edges}
            backward = {graph._classify_at(a, b, edge) for edge in reversed(edges)}
            assert forward == backward


def test_a_shallow_corner_is_not_smooth():
    """The false positive this attribute must not produce, and the reason the gap was measured.

    Seeing *through* a join that is really a shallow step would merge two faces a recogniser
    needs kept apart. The threshold was once 1.8 degrees, chosen on an assumption about kernel
    noise that measurement disproved -- tangencies are *exact*. A one-degree ramp is nowhere near
    smooth and must not read as it.
    """

    ramp = Box(60, 40, 10) - Pos(0, 0, 5) * Rot(0, 1, 0) * Box(80, 60, 10)
    graph = FaceGraph(ramp)
    kinds = {graph.arc(a, b) for a in graph.nodes for b in graph.neighbours(a) if b.index > a.index}
    assert "smooth" not in kinds

    # And the gap it would have to close is far outside the threshold, by orders of magnitude.
    assert SMOOTH_ARC_GAP < 1e-6, "a shallow corner sits at ~1e-4; the gap must stay well below"


def test_the_classification_is_the_same_at_any_scale():
    """Angles and signs only, so a part a thousand times bigger is the same part to an arc."""

    small = Box(2, 2, 2) - Pos(0, 0, 0.6) * Box(1.2, 1.2, 1.2)
    large = Box(2000, 2000, 2000) - Pos(0, 0, 600) * Box(1200, 1200, 1200)
    assert _arcs(small) == _arcs(large)


@pytest.mark.skipif(
    not (_CORPUS / "MANIFEST.json").is_file(),
    reason="the vendored MFCAD++ subset is excluded from the sdist",
)
def test_imported_step_geometry_classifies_without_unknowns():
    """Generated solids are the easy case; imported B-Rep is what the package is for.

    `unknown` is a real answer and not a failure, but a corpus of ordinary machined parts should
    not need it -- if it starts appearing here, some geometry has stopped being readable and
    that is worth knowing before a recogniser depends on the attribute.
    """

    from build123d import import_step

    models = sorted(_CORPUS.glob("*.step"))[:5]
    assert models, "the vendored corpus must be present for this test to mean anything"
    for path in models:
        graph = FaceGraph(import_step(str(path)))
        for a in graph.nodes:
            for b in graph.neighbours(a):
                assert graph.arc(a, b) in ("convex", "concave", "smooth")


def test_a_repeated_query_does_not_recompute_the_kernel_work():
    """The graph is a run-local memo everywhere else, and an arc is its most expensive fact.

    Surface projection and two first-derivative evaluations per shared edge is not something to
    repeat once several recognisers traverse the same arcs.
    """

    graph = FaceGraph(_plain())
    a = graph.nodes[0]
    b = graph.neighbours(a)[0]

    calls = []
    original = graph._classify_arc
    graph._classify_arc = lambda x, y: (calls.append((x, y)), original(x, y))[1]  # type: ignore[method-assign]

    first = graph.arc(a, b)
    assert len(calls) == 1
    assert graph.arc(a, b) == first
    assert graph.arc(b, a) == first
    assert len(calls) == 1, "the cache must be symmetric, not one entry per ordering"


def test_shared_edges_that_disagree_give_no_single_answer():
    """The rule that makes a multi-edge pair safe, tested as logic rather than hunted as geometry.

    No disagreeing pair has been observed -- a sweep of the checked-in STEP corpus found 49
    pairs sharing two edges and none whose edges classify differently -- so this cannot be
    reached by choosing a fixture. It is still the branch that stops an arc silently depending
    on which edge came back first, which is what the previous implementation did.

    Injecting the disagreement tests the aggregation and nothing else, which is the part that
    was written rather than measured.
    """

    graph = FaceGraph(_plain())
    a = graph.nodes[0]
    b = graph.neighbours(a)[0]
    assert graph.arc(a, b) == "convex", "the real answer, before it is overridden"

    answers = iter(("convex", "concave"))
    graph._arcs.clear()
    graph._classify_at = lambda *_: next(answers)  # type: ignore[method-assign]
    graph.shared_edges = lambda *_: ("first", "second")  # type: ignore[method-assign]

    assert graph.arc(a, b) == "unknown"


def test_a_warm_cache_still_refuses_another_graph_s_nodes():
    """The cache must not become a way past the graph's provenance check.

    `FaceNode` carries only an index, so a node from another graph of an identical part looks
    the same. Every read the graph offers resolves through `owns`, and an arc keyed straight
    off `.index` would have skipped that -- but only on a *hit*, because the miss path validates
    inside `shared_edges`. So the bug appears for the second caller onward and never the first,
    which is why this test warms the cache before it proves anything.

    Answering for the wrong solid is the failure `require_node` and `claims_of` are both written
    to refuse; a cache is not a licence to stop refusing it.
    """

    part, twin = _plain(), _plain()
    graph, other = FaceGraph(part), FaceGraph(twin)
    a = graph.nodes[0]
    b = graph.neighbours(a)[0]

    assert graph.arc(a, b) == "convex", "warm the cache with a pair this graph does own"

    foreign_a = other.nodes[a.index]
    foreign_b = other.nodes[b.index]
    with pytest.raises(ValueError, match="not issued by this graph"):
        graph.arc(foreign_a, foreign_b)
