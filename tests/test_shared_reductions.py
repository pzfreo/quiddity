# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

"""ADR 0009: a reduction shared by more than one recogniser is total over its input.

`_recess_faces._planar_faces` is the reduction the record was written about. It used to drop any
planar face whose normal aligned with no principal axis, and slots, pockets and channels all
inherited that without saying so — an axis-aligned-walls restriction applying to three families
and documented on none, which no consumer could observe and no instrumentation could count,
because the faces never arrived.

These tests pin the property rather than the gate. Whether a family *uses* an oblique wall is
that family's business and may change; that the shared layer *hands it over* is the record's
requirement, and it is the thing that was wrong.

The distinction matters for what a failure here would mean. A family declining an oblique wall
is a documented capability limit. The shared reduction discarding one is the limit becoming
invisible, which is what took 3,163 labelled triangular-pocket faces to notice.
"""

from __future__ import annotations

from build123d import Box, BuildPart, BuildSketch, Plane, Polygon, Pos, Rot, extrude

from quiddity._recess_faces import _planar_faces
from tools._legacy_recognition import namespace

r = namespace()


def _triangular_pocket():
    """A block with a blind triangular recess: three walls, one of them oblique."""

    with BuildPart() as prism:
        with BuildSketch(Plane.XY):
            Polygon((-8, -6), (8, -6), (0, 8))
        extrude(amount=8)
    return Box(60, 40, 12) - Pos(0, 0, 3) * prism.part


def _oblique_walled_block():
    """A rectangular block with one corner sliced off, so a large planar wall runs oblique."""

    return Box(60, 40, 12) - Pos(30, 20, 0) * Rot(0, 0, 45) * Box(20, 20, 40)


def test_the_shared_reduction_hands_over_an_oblique_planar_wall():
    """The whole of ADR 0009, on the reduction it was written about.

    Before the record this returned only axis-aligned faces, so this assertion could not have
    been written — there was no entry to inspect. `axis is None` is the representation the
    record asks for: the attribute that does not apply is absent, and the face is still there.
    """

    for part in (_triangular_pocket(), _oblique_walled_block()):
        faces = _planar_faces(part)
        oblique = [face for face in faces if face.axis is None]

        assert oblique, "an oblique planar wall was dropped by the shared reduction"
        for face in oblique:
            assert face.normal is not None, "carried, so its normal is carried too"
            assert max(abs(component) for component in face.normal) < 0.999


def test_every_planar_face_of_the_part_survives_the_reduction():
    """Total, not merely tolerant of the one case that prompted the record.

    Counted rather than sampled: the reduction's declared domain is planar faces, so its output
    length is the part's planar-face count and nothing narrows it further.
    """

    part = _oblique_walled_block()
    planar = [face for face in part.faces() if face.geom_type.name.upper().startswith("PLANE")]
    assert len(_planar_faces(part)) == len(planar) > 0


def test_the_families_still_decline_what_they_cannot_pair():
    """The rejection moved; it did not disappear, and this is what says so.

    ADR 0009 does not ask a recogniser to handle oblique geometry — only that declining it be
    the recogniser's own visible act. The recess families pair walls sharing a normal axis, so
    a wall with no axis has nothing to pair against, and they report nothing here. When that
    changes it should change *because a family changed*, and this test is what will notice.
    """

    part = _triangular_pocket()
    assert _planar_faces(part), "the fixture must reach the families at all"
    assert any(face.axis is None for face in _planar_faces(part))

    assert r.recognise_pockets(part) == []
    assert r.recognise_slots(part) == []
    assert r.recognise_channels(part) == []
