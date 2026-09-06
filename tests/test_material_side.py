# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

"""Which side of a face the material is on, checked where the corpus cannot check it.

Four places in this package answer that question, by one convention specialised to three
surface types: ``_recess_faces._outward_normal`` for planes, ``_recess_faces``'s concave test and
``_cylinder_substrate``'s ``external`` flag for cylinders, and ``_hole_features``' convexity
test for spheres. All four read a face's ``FORWARD``/``REVERSED`` orientation *against* its
surface frame's handedness, because either alone is not enough: mirroring a solid makes the
frame left-handed and flips both, so a test on orientation alone inverts on a mirrored part.

**The corpus cannot falsify that.** Surveyed over all 72 vendored parts: ``REVERSED`` faces
number 3,061 of 4,407 and every part carries some, but **left-handed surface frames number 6 of
3,853, on 1 of 72 parts**. The handedness factor -- the whole reason the convention is a
product of two terms rather than one -- is effectively untested by every corpus this project
has. A disagreement count over them would come back near zero and mean nothing.

So the geometry is generated rather than found, and checked against an **independent oracle**
rather than against the other implementations. Stepping off a face along its claimed outward
normal must leave the solid, and stepping against it must stay inside; that is a different
mechanism from every implementation under test, so it can say which is *right* where
cross-comparison could only say they agree.

Epic 0002 item 1. The migration this precedes is only safe if the convention it moves is
correct, which is what these establish.
"""

from __future__ import annotations

import pytest
from build123d import Box, Cylinder, Plane, Pos, Torus, mirror
from OCP.BRep import BRep_Tool
from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.BRepClass3d import BRepClass3d_SolidClassifier
from OCP.BRepMesh import BRepMesh_IncrementalMesh
from OCP.GeomAbs import GeomAbs_Cylinder
from OCP.gp import gp_Pnt
from OCP.TopAbs import TopAbs_IN, TopAbs_Orientation, TopAbs_OUT
from OCP.TopLoc import TopLoc_Location

import quiddity as r
from quiddity._adjacency import FaceGraph, frame_points_outward

#: How far off the face to probe. Small against every fixture's thinnest wall (3 mm), so a
#: probe that lands outside the solid did so because the normal points that way and not
#: because it stepped clean through the material.
_STEP = 0.02


def _plain():
    return Box(30, 20, 10)


def _through():
    return Box(30, 20, 10) - Pos(0, 0, 2) * Box(12, 8, 8)


def _blind():
    return Box(30, 20, 10) - Pos(0, 0, 3) * Box(12, 8, 6)


def _bore():
    return Box(40, 40, 20) - Cylinder(6, 30)


def _boss():
    return Box(40, 40, 6) + Pos(0, 0, 3) * Cylinder(8, 10)


def _cells(part):
    """``{(is_forward, frame_is_right_handed): count}`` over the part's planar faces."""

    cells: dict[tuple[bool, bool], int] = {}
    for face in part.faces():
        try:
            direct = BRepAdaptor_Surface(face.wrapped).Plane().Position().Direct()
        except Exception:  # noqa: BLE001 -- not a plane, and only planes are counted here
            continue
        forward = face.wrapped.Orientation() == TopAbs_Orientation.TopAbs_FORWARD
        cells[(forward, direct)] = cells.get((forward, direct), 0) + 1
    return cells


def _cylinder_frames(part):
    """Whether each cylindrical face's frame is right-handed, in face order."""

    out = []
    for face in part.faces():
        surface = BRepAdaptor_Surface(face.wrapped)
        if surface.GetType() == GeomAbs_Cylinder:
            out.append(surface.Cylinder().Position().Direct())
    return out


def _points_on(face, limit=4):
    """Centroids of the face's own triangles.

    Four is enough because every face probed here is planar, so its normal is the same
    everywhere on it and one point would do; the rest are insurance against a degenerate
    triangle. A curved face would need sampling proportional to its curvature.

    Not ``face.center()``: the centroid of a face with a hole in it lies *in the hole*, so a
    probe from there reports a violation that is the fixture's fault rather than the code's.
    An earlier version of this test did exactly that and produced one false positive per
    cavity fixture. A triangle's centroid is on the face by construction.
    """

    location = TopLoc_Location()
    triangulation = BRep_Tool.Triangulation_s(face.wrapped, location)
    if triangulation is None:
        return []
    transform = location.Transformation()
    nodes = [
        triangulation.Node(i).Transformed(transform) for i in range(1, triangulation.NbNodes() + 1)
    ]
    out = []
    for i in range(1, min(triangulation.NbTriangles(), limit) + 1):
        a, b, c = triangulation.Triangle(i).Get()
        out.append(
            tuple(
                sum(getattr(nodes[k - 1], axis)() for k in (a, b, c)) / 3
                for axis in ("X", "Y", "Z")
            )
        )
    return out


def _violations(part):
    """Probes where the claimed outward normal disagrees with the solid classifier.

    Asked of `FaceGraph.normal`, which is the material-side normal -- `normal_at` already
    applies the orientation correction, so the frame-derived spelling this package kept beside
    it was the same answer by another route. These probes are what establish that.
    """

    BRepMesh_IncrementalMesh(part.wrapped, 0.5, False, 0.5, True)
    classifier = BRepClass3d_SolidClassifier(part.wrapped)
    graph = FaceGraph(part)
    bad, probes = [], 0
    for face in part.faces():
        normal = graph.normal(graph.require_node(face))
        if normal is None:
            continue
        for point in _points_on(face):
            for sign, expected in ((1.0, TopAbs_OUT), (-1.0, TopAbs_IN)):
                classifier.Perform(
                    gp_Pnt(*(p + sign * _STEP * n for p, n in zip(point, normal, strict=True))),
                    1e-9,
                )
                probes += 1
                if classifier.State() != expected:
                    way = "outward" if sign > 0 else "inward"
                    bad.append((tuple(round(n, 3) for n in normal), way))
    return bad, probes


@pytest.mark.parametrize(
    "build, name",
    [
        (_plain, "plain block"),
        (_through, "block with a through cavity"),
        (_blind, "block with a blind pocket"),
    ],
)
def test_the_outward_normal_agrees_with_the_solid_on_a_mirrored_part(build, name):
    """The cell the corpus has 6 faces of, on 1 part of 72.

    Mirroring makes every surface frame left-handed and flips every orientation, so a
    convention reading orientation alone would invert here and pass everywhere else. Checked in
    both directions -- outward leaves the solid, inward stays in it -- because a normal that is
    simply *wrong* rather than inverted fails only one of the two.
    """

    part = mirror(build(), about=Plane.YZ)
    left_handed = sum(count for (_fwd, direct), count in _cells(part).items() if not direct)
    assert left_handed, f"{name}: the fixture must actually produce left-handed frames"

    bad, probes = _violations(part)
    assert probes, "no probe ran, so this asserts nothing"
    assert bad == [], f"{name}: {len(bad)} of {probes} probes disagree with the solid: {bad[:3]}"


def test_the_same_holds_unmirrored_so_the_mirror_is_the_variable():
    """The control. Without it a mirrored-only pass could mean the probe is simply lenient."""

    for build in (_plain, _through, _blind):
        part = build()
        assert all(direct for (_fwd, direct) in _cells(part)), "the control must be right-handed"
        bad, probes = _violations(part)
        assert probes and bad == [], f"{len(bad)} of {probes} probes disagree"


def test_the_fixtures_reach_all_four_cells_between_them():
    """Otherwise the suite above proves less than its docstrings claim.

    ``REVERSED`` and left-handed are independent factors and the convention is their product,
    so a set of fixtures covering three cells would leave the interesting one untested -- which
    is exactly the corpus's position.
    """

    reached = set()
    for build in (_plain, _through, _blind):
        reached |= set(_cells(build()))
        reached |= set(_cells(mirror(build(), about=Plane.YZ)))
    assert reached == {(True, True), (False, True), (True, False), (False, False)}


@pytest.mark.parametrize("build, expected", [(_bore, False), (_boss, True)])
def test_a_cylinder_knows_which_side_its_material_is_on_when_mirrored(build, expected):
    """The second surface type, through the public substrate that carries the flag.

    ``external`` distinguishes a boss OD from a bore, and is read the same way: orientation
    against frame handedness. Mirroring flips both, so the flag must not move.

    The handedness assertion is not decoration. Without it this passes whether or not the
    mirrored fixture reaches a left-handed *cylinder* frame, and a fixture that quietly stopped
    reaching it would leave the mirror doing no work -- the same vacuity the planar tests above
    guard against, which this one was missing.
    """

    mirrored = mirror(build(), about=Plane.YZ)
    assert _cylinder_frames(build()) == [True], "the control must be right-handed"
    assert _cylinder_frames(mirrored) == [False], "the mirror must flip the cylinder's frame"

    plain = r.analyse_cylinders(build())[0]
    flipped = r.analyse_cylinders(mirrored)[0]

    assert plain and flipped, "the fixture must yield a Z cylinder both ways"
    assert [c["external"] for c in plain] == [expected] * len(plain)
    assert [c["external"] for c in flipped] == [expected] * len(flipped), (
        "mirroring inverted the material side"
    )


def test_a_surface_with_no_readable_frame_answers_none_rather_than_guessing():
    """The third answer, and it has to be a distinct one.

    `frame_points_outward` reads a frame off a plane, a cylinder or a sphere. A torus has one
    too, but not one this convention is defined against, and a spline has none at all. Returning
    a bool there would be a guess dressed as a fact, and every caller would inherit it: the
    recess families would silently place material on one side of a blend, and nothing downstream
    could tell that from a real answer.

    None is what makes absence a caller's decision. `FaceGraph.outward_normal` turns it into
    "not a planar face"; `_cylinder_substrate` never asks, because it has already filtered to
    cylinders.
    """

    (torus,) = Torus(12, 3).faces()
    assert frame_points_outward(torus) is None

    # And the readable ones do not answer None, or the assertion above is about nothing.
    for face in Box(10, 10, 10).faces():
        assert frame_points_outward(face) is not None
