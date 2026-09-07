"""Consumer contracts for exact, body-owned outer profile supports (#579)."""

import copy
import json
import math
import pickle
from dataclasses import replace

import pytest
from build123d import (
    Axis,
    Box,
    Compound,
    Cylinder,
    Edge,
    Face,
    GeomType,
    Pos,
    RegularPolygon,
    Rot,
    Solid,
    Wire,
    export_step,
    extrude,
    fillet,
)

from quiddity import (
    FramedRecognitionEvidence,
    build_framed_recognition_evidence,
    import_step_geometry,
)
from quiddity._outer_profile_geometry import _read_profile
from quiddity.evidence import (
    OuterProfileRefusalReason as Reason,
)
from quiddity.evidence import (
    PlanarOuterProfileEvidence,
    ProfileArc,
    ProfileLine,
    RefusedPlanarOuterProfile,
    build_recognition_evidence,
)


def triangle(radius=5, scale=1):
    part = extrude(RegularPolygon(50 * scale, 3), 6 * scale)
    return fillet(part.edges().filter_by(Axis.Z), radius * scale) if radius else part


def cap(view, sign=1):
    return next(ref for ref in view.faces if view.face(ref).normal_at().Z * sign > 0.999)


def bound(view, ref):
    value = view.planar_outer_profile(ref)
    assert isinstance(value, PlanarOuterProfileEvidence), value
    return value


def assert_support_correspondence(view, issued, supports=None):
    """Independent source-edge checks; no producer geometry helper is used."""
    supports = issued.profile.supports if supports is None else supports
    outer = view.face(issued.face).outer_wire().edges()
    assert len(supports) == len(outer)
    source_edges = [view.profile_edge(issued, at) for at in range(len(supports))]
    assert all(any(e.wrapped.IsSame(raw.wrapped) for raw in outer) for e in source_edges)
    assert all(
        not a.wrapped.IsSame(b.wrapped)
        for at, a in enumerate(source_edges)
        for b in source_edges[at + 1 :]
    )
    for support, edge in zip(supports, source_edges, strict=True):
        endpoints = (tuple(edge.position_at(0)), tuple(edge.position_at(1)))
        assert (
            min(
                max(math.dist(support.start, a), math.dist(support.end, b))
                for a, b in (endpoints, endpoints[::-1])
            )
            < 1e-6
        )
        if isinstance(support, ProfileLine):
            assert edge.geom_type == GeomType.LINE
            assert math.dist(support.start, support.end) == pytest.approx(edge.length, abs=1e-6)
            source_direction = tuple(
                (endpoints[1][i] - endpoints[0][i]) / edge.length for i in range(3)
            )
            assert (
                min(
                    math.dist(support.direction, source_direction),
                    math.dist(support.direction, tuple(-v for v in source_direction)),
                )
                <= 2e-8
            )
        else:
            assert edge.geom_type == GeomType.CIRCLE
            assert support.center == pytest.approx(tuple(edge.arc_center), abs=1e-6)
            assert support.radius == pytest.approx(edge.radius, abs=1e-6)
            assert abs(support.sweep) * support.radius == pytest.approx(edge.length, abs=1e-6)
            radial = tuple(support.start[i] - support.center[i] for i in range(3))
            n = issued.profile.normal
            crossed = (
                n[1] * radial[2] - n[2] * radial[1],
                n[2] * radial[0] - n[0] * radial[2],
                n[0] * radial[1] - n[1] * radial[0],
            )
            middle = tuple(
                support.center[i]
                + math.cos(support.sweep / 2) * radial[i]
                + math.sin(support.sweep / 2) * crossed[i]
                for i in range(3)
            )
            assert math.dist(middle, tuple(edge.position_at(0.5))) < 1e-6


@pytest.mark.parametrize("radius", [0, 5])
@pytest.mark.parametrize("scale", [0.1, 1, 10])
@pytest.mark.parametrize("sign", [-1, 1])
def test_sharp_and_rounded_profiles_have_known_directed_supports(radius, scale, sign):
    part = triangle(radius, scale)
    view = build_recognition_evidence(part)
    issued = bound(view, cap(view, sign))
    profile = issued.profile
    assert profile.normal == pytest.approx((0, 0, sign))
    assert profile.inner_loop_count == 0
    assert len(profile.supports) == (6 if radius else 3)
    lines = [s for s in profile.supports if isinstance(s, ProfileLine)]
    assert len(lines) == 3
    assert all(math.hypot(*line.direction) == pytest.approx(1) for line in lines)
    assert all(
        math.dist(s.start, s.end)
        == pytest.approx((50 * math.sqrt(3) - 2 * radius * math.sqrt(3)) * scale)
        for s in lines
    )
    for at, s in enumerate(profile.supports):
        assert s.end == profile.supports[(at + 1) % len(profile.supports)].start
        if isinstance(s, ProfileArc):
            assert s.radius == pytest.approx(radius * scale)
            assert s.sweep == pytest.approx(2 * math.pi / 3)
            assert isinstance(profile.supports[(at - 1) % len(profile.supports)], ProfileLine)
            assert isinstance(profile.supports[(at + 1) % len(profile.supports)], ProfileLine)
    assert issued.face in issued.body_faces <= view.faces
    assert len(issued.body_faces) == len(part.faces())
    assert_support_correspondence(view, issued)
    encoded = json.loads(json.dumps(profile.to_dict()))
    assert encoded["schema_version"] == 1 and encoded["boundary_kind"] == "outer"
    assert [s["kind"] for s in encoded["supports"]].count("line") == 3


def test_rounded_transition_supplies_an_explicit_virtual_intersection():
    view = build_recognition_evidence(triangle())
    p = bound(view, cap(view)).profile
    vertices = []
    for at, arc in enumerate(p.supports):
        if not isinstance(arc, ProfileArc):
            continue
        left = p.supports[at - 1]
        right = p.supports[(at + 1) % len(p.supports)]
        # Intersect the independently selected outgoing/incoming line extensions.
        a, b = left.direction, right.direction
        delta = tuple(right.start[i] - left.end[i] for i in range(3))
        t = (delta[0] * b[1] - delta[1] * b[0]) / (a[0] * b[1] - a[1] * b[0])
        vertex = tuple(left.end[i] + t * a[i] for i in range(3))
        assert t > 0
        assert sum((vertex[i] - right.start[i]) * b[i] for i in range(3)) < 0
        vertices.append(vertex)
    assert len(vertices) == 3
    assert sorted(math.hypot(v[0], v[1]) for v in vertices) == pytest.approx([50] * 3)
    assert not any(any(math.dist(v, s.start) < 1e-6 for s in p.supports) for v in vertices)


def test_inner_loop_is_counted_but_never_becomes_outer_adjacency():
    part = triangle() - Pos(0, 0, 3) * Cylinder(6, 20)
    view = build_recognition_evidence(part)
    issued = bound(view, cap(view))
    assert issued.profile.inner_loop_count == 1
    assert len(issued.profile.supports) == 6
    assert_support_correspondence(view, issued)
    assert all(
        not view.profile_edge(issued, i).wrapped.IsSame(inner.wrapped)
        for i in range(6)
        for wire in view.face(issued.face).inner_wires()
        for inner in wire.edges()
    )


def test_concave_and_nonplanar_profiles_refuse_with_named_reasons():
    part = Box(50, 50, 6) - Pos(15, 15, 0) * Box(30, 30, 20)
    view = build_recognition_evidence(part)
    assert view.planar_outer_profile(cap(view)).reason is Reason.CONCAVE_PROFILE
    view = build_recognition_evidence(triangle())
    curved = next(ref for ref in view.faces if view.face(ref).geom_type == GeomType.CYLINDER)
    assert view.planar_outer_profile(curved).reason is Reason.NOT_PLANAR


def test_mixed_freeform_wire_is_not_replaced_with_straight_supports():
    wire = Wire(
        [
            Edge.make_line((-20, 0, 0), (20, 0, 0)),
            Edge.make_line((20, 0, 0), (20, 10, 0)),
            Edge.make_spline([(20, 10, 0), (0, 20, 0), (-20, 10, 0)]),
            Edge.make_line((-20, 10, 0), (-20, 0, 0)),
        ]
    )
    view = build_recognition_evidence(extrude(Face(wire), 6))
    assert view.planar_outer_profile(cap(view)).reason is Reason.UNSUPPORTED_CURVE


@pytest.mark.parametrize("placement", [Pos(), Pos(100, 0, 0), Pos(50, 0, 0)])
@pytest.mark.parametrize("reverse", [False, True])
def test_equal_and_touching_bodies_never_share_profile_identity(placement, reverse):
    first = Box(50, 50, 6)
    second = placement * copy.deepcopy(first)
    children = [first, second]
    view = build_recognition_evidence(Compound(children[::-1] if reverse else children))
    profiles = [bound(view, ref) for ref in view.faces if view.face(ref).normal_at().Z > 0.999]
    assert len(profiles) == 2
    assert profiles[0].body_faces.isdisjoint(profiles[1].body_faces)
    assert all(len(p.body_faces) == 6 for p in profiles)
    assert all(len(p.profile.supports) == 4 for p in profiles)
    for p in profiles:
        assert_support_correspondence(view, p)


def test_shared_topology_and_unowned_face_refuse_instead_of_guessing_body():
    first = Box(50, 50, 6)
    # Distinct solid occurrences sharing the exact shell, not a duplicate child
    # which OCCT's unique-subshape enumeration legitimately collapses to one solid.
    second = Solid(first.shells()[0])
    part = Compound([first, second])
    assert len(part.solids()) == 2
    view = build_recognition_evidence(part)
    assert view.planar_outer_profile(cap(view)).reason is Reason.AMBIGUOUS_BODY
    face = first.faces().sort_by(Axis.Z)[-1]
    view = build_recognition_evidence(face)
    assert view.planar_outer_profile(cap(view)).reason is Reason.AMBIGUOUS_BODY


@pytest.mark.parametrize("rotation", [Rot(17, 31, 43), Rot(180, 0, 0)])
def test_raw_rigid_motion_preserves_profile_supports_and_source_binding(rotation):
    placement = Pos(123.0004, -57.0004, 91.0004) * rotation
    view = build_recognition_evidence(placement * triangle())
    normal = tuple((rotation * Face.make_rect(1, 1)).normal_at())
    ref = next(r for r in view.faces if tuple(view.face(r).normal_at()) == pytest.approx(normal))
    issued = bound(view, ref)
    assert len(issued.profile.supports) == 6
    assert_support_correspondence(view, issued)
    base = build_recognition_evidence(triangle())
    expected = bound(base, cap(base)).profile
    expected_points = [tuple((placement * Pos(*s.start)).position) for s in expected.supports]
    assert all(
        any(math.dist(s.start, p) < 1e-6 for p in expected_points) for s in issued.profile.supports
    )


def test_framed_profile_maps_to_exact_caller_face_without_another_run(monkeypatch):
    import quiddity.frames as result_module

    original = result_module._take_inventory
    calls = []

    def counted(*args, **kwargs):
        calls.append(1)
        return original(*args, **kwargs)

    monkeypatch.setattr(result_module, "_take_inventory", counted)
    part = Pos(19, 29, 37) * Rot(17, 31, 43) * triangle()
    view = build_framed_recognition_evidence(part)
    assert isinstance(view, FramedRecognitionEvidence)
    profiles = [
        bound(view, r)
        for r in view.faces
        if len(view.face(r).outer_wire().edges()) == 6 and view.face(r).geom_type == GeomType.PLANE
    ]
    assert len(profiles) == 2 and len(calls) == 1
    for p in profiles:
        assert view.planar_outer_profile(p.face) is p
        assert_support_correspondence(view, p)
        caller = view.caller_face(p.face)
        assert any(caller.wrapped.IsSame(f.wrapped) for f in part.faces())
        # The public frame is the explicit local-to-caller transform.
        expected = [view.frame.to_world(s.start) for s in p.profile.supports]
        vertices = [tuple(v.center()) for v in caller.outer_wire().vertices()]
        assert all(any(math.dist(point, v) < 1e-6 for v in vertices) for point in expected)
    assert len(calls) == 1


def test_profile_queries_are_lazy_cached_and_leave_the_existing_inventory_unchanged(monkeypatch):
    calls = []
    from quiddity import _outer_profile_geometry as geometry

    original = geometry._read_profile
    monkeypatch.setattr(
        geometry, "_read_profile", lambda face: (calls.append(face), original(face))[1]
    )
    view = build_recognition_evidence(triangle())
    assert calls == []
    result, features, association = view.result, view.features, view.association
    p = bound(view, cap(view))
    assert view.planar_outer_profile(p.face) is p
    assert len(calls) == 1
    assert view.result is result and view.features is features and view.association is association


def test_profile_reference_authority_and_index_guards():
    first = build_recognition_evidence(triangle())
    second = build_recognition_evidence(triangle())
    p = bound(first, cap(first))
    with pytest.raises(ValueError, match="foreign"):
        second.profile_edge(p, 0)
    with pytest.raises(ValueError, match="foreign"):
        second.planar_outer_profile(p.face)
    with pytest.raises(TypeError):
        PlanarOuterProfileEvidence()
    with pytest.raises(TypeError):
        first.profile_edge(p, True)
    with pytest.raises(TypeError):
        first.profile_edge(p.profile, 0)
    for index in (-1, 6):
        with pytest.raises(IndexError):
            first.profile_edge(p, index)
    with pytest.raises(TypeError):
        copy.copy(p)
    with pytest.raises(TypeError):
        pickle.dumps(p)
    forged = object.__new__(PlanarOuterProfileEvidence)
    object.__setattr__(forged, "_PlanarOuterProfileEvidence__face", p.face)
    with pytest.raises(ValueError, match="forged"):
        first.profile_edge(forged, 0)


def test_removing_or_changing_support_evidence_fails_source_correspondence():
    view = build_recognition_evidence(triangle())
    p = bound(view, cap(view))
    assert_support_correspondence(view, p)
    with pytest.raises(AssertionError):
        assert_support_correspondence(view, p, p.profile.supports[:-1])
    changed = list(p.profile.supports)
    at = next(i for i, s in enumerate(changed) if isinstance(s, ProfileLine))
    changed[at] = replace(changed[at], start=tuple(v + 1 for v in changed[at].start))
    with pytest.raises(AssertionError):
        assert_support_correspondence(view, p, changed)
    changed = list(p.profile.supports)
    at = next(i for i, s in enumerate(changed) if isinstance(s, ProfileArc))
    changed[at] = replace(changed[at], sweep=-changed[at].sweep)
    with pytest.raises(AssertionError):
        assert_support_correspondence(view, p, changed)


def test_opposite_normal_and_reversed_wire_keep_physical_support_correspondence():
    face = triangle().faces().filter_by(GeomType.PLANE).sort_by(Axis.Z)[-1]
    before = _read_profile(face)
    reversed_face = Face(face.wrapped.Reversed())
    after = _read_profile(reversed_face)
    assert not isinstance(before, RefusedPlanarOuterProfile)
    assert not isinstance(after, RefusedPlanarOuterProfile)
    assert after[0].normal == pytest.approx(tuple(-v for v in before[0].normal))
    assert all(
        any(
            math.dist(s.start, t.end) < 1e-6 and math.dist(s.end, t.start) < 1e-6
            for t in after[0].supports
        )
        for s in before[0].supports
    )


def test_six_line_flange_with_alternating_arc_radii_and_holes():
    part = extrude(RegularPolygon(60, 6), 6)
    corners = sorted(
        (tuple(e.center()) for e in part.edges().filter_by(Axis.Z)),
        key=lambda p: math.atan2(p[1], p[0]),
    )
    for at, point in enumerate(corners):
        edge = min(
            part.edges().filter_by(Axis.Z), key=lambda e: math.dist(tuple(e.center()), point)
        )
        part = fillet(edge, 3 if at % 2 else 8)
    for degrees in (0, 120, 240):
        part -= Pos(
            25 * math.cos(math.radians(degrees)), 25 * math.sin(math.radians(degrees)), 3
        ) * Cylinder(3, 20)
    view = build_recognition_evidence(part)
    issued = bound(view, cap(view))
    assert len(issued.profile.supports) == 12
    assert issued.profile.inner_loop_count == 3
    assert sum(isinstance(s, ProfileLine) for s in issued.profile.supports) == 6
    assert sorted(
        s.radius for s in issued.profile.supports if isinstance(s, ProfileArc)
    ) == pytest.approx([3, 3, 3, 8, 8, 8])
    assert_support_correspondence(view, issued)


def test_step_reimport_preserves_geometry_with_fresh_source_binding(tmp_path):
    path = tmp_path / "rounded-profile.step"
    original = triangle()
    assert export_step(original, path)
    before = build_recognition_evidence(original)
    after = build_recognition_evidence(import_step_geometry(path))
    old = bound(before, cap(before))
    new = bound(after, cap(after))
    assert len(new.profile.supports) == 6
    assert_support_correspondence(after, new)
    assert all(
        any(math.dist(s.start, t.start) < 1e-6 for t in new.profile.supports)
        for s in old.profile.supports
    )
    with pytest.raises(ValueError, match="foreign"):
        after.profile_edge(old, 0)


@pytest.mark.parametrize("change", ["plane", "normal", "radius", "sweep", "open", "unsupported"])
def test_profile_schema_rejects_incoherent_hand_built_geometry(change):
    view = build_recognition_evidence(triangle())
    p = bound(view, cap(view)).profile
    with pytest.raises((ValueError, TypeError)):
        if change == "plane":
            replace(p, origin=(0, 0, 7))
        elif change == "normal":
            replace(p, normal=(0, 0, 0))
        else:
            supports = list(p.supports)
            if change == "open":
                supports.pop()
            elif change == "unsupported":
                supports.append(object())
            else:
                at = next(i for i, s in enumerate(supports) if isinstance(s, ProfileArc))
                arc = supports[at]
                supports[at] = replace(
                    arc,
                    **(
                        {"radius": arc.radius + 1}
                        if change == "radius"
                        else {"sweep": arc.sweep + 0.1}
                    ),
                )
            replace(p, supports=tuple(supports))


@pytest.mark.parametrize("displacement", [3e-5, 5e-7])
def test_loose_vertex_tolerance_cannot_invent_a_different_line_support(displacement):
    from OCP.BRep import BRep_Builder
    from OCP.gp import gp_Pnt

    part = Box(20, 20, 6)
    face = part.faces().sort_by(Axis.Z)[-1]
    vertex = face.vertices()[0]
    point = vertex.center()
    # Authored OCCT-valid tolerance mismatch: the vertex moves, its edge curve does not.
    BRep_Builder().UpdateVertex(
        vertex.wrapped, gp_Pnt(point.X + displacement, point.Y, point.Z), 1e-4
    )
    assert part.is_valid
    view = build_recognition_evidence(part)
    assert view.planar_outer_profile(cap(view)).reason is Reason.INVALID_BOUNDARY


def test_nist_import_never_publishes_displaced_vertex_as_a_trimmed_support():
    view = build_recognition_evidence(
        import_step_geometry("tests/corpus/nist/nist_ftc_08_asme1_rc.stp")
    )
    accepted = 0
    refused = 0
    for ref in view.faces:
        inspected = view.planar_outer_profile(ref)
        if isinstance(inspected, PlanarOuterProfileEvidence):
            assert_support_correspondence(view, inspected)
            accepted += 1
        elif inspected.reason is Reason.INVALID_BOUNDARY:
            refused += 1
    assert accepted and refused
