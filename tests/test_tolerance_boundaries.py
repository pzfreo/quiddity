# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

"""What happens *at* a threshold, on geometry built to sit either side of it.

``test_length_tolerance`` covers the shared tolerance arithmetic — that it is monotone,
floored and scale-following. This covers the other half: a recogniser's answer changing where
ADR 0008 says it changes, measured on a solid rather than argued from the constant.

The subject is the line between a **drafted wall** and a **chamfer**, which is the boundary a
consumer is most likely to walk into by accident. Moulded and cast parts carry draft on every
vertical face, typically 1° to 3°; this package calls a face axis-aligned while its normal's
largest component stays at or above ``AXIS_ALIGNED_COS``, so the line falls at 8.11° and an
ordinary draft angle is comfortably a wall. The fixtures here are the same block chamfered by
the same 4 mm leg, with only the second leg changed to move the angle across that line.

The angles are derived from the constant rather than written down. A test that hard-codes 8.11
passes for the wrong reason after someone edits the threshold; these fail, which is what the
boundary is for.
"""

from __future__ import annotations

import math

from build123d import Axis, Box, chamfer

from quiddity import recognise_chamfers
from quiddity._geometry import AXIS_ALIGNED_COS

#: The tilt from an axis at which a face stops counting as aligned with it: 8.11°.
BOUNDARY = math.degrees(math.acos(AXIS_ALIGNED_COS))
#: Far enough either side to clear kernel noise, close enough that nothing else changes.
MARGIN = 0.15


def _drafted(degrees: float, scale: float = 1.0):
    """A block with one edge chamfered, its slant leaning *degrees* from the vertical wall."""

    part = Box(60 * scale, 40 * scale, 12 * scale)
    edge = part.edges().filter_by(Axis.Y).sort_by(Axis.X)[0]
    return chamfer(edge, length=4.0 * scale, length2=4.0 * scale * math.tan(math.radians(degrees)))


def test_a_face_just_inside_the_draft_boundary_is_a_wall_and_just_outside_is_a_chamfer():
    """The flip, and the record carrying the angle it was built with.

    Below the boundary the slant is a drafted wall and nothing reports it. Above it, one
    chamfer, and its ``angle`` is the angle the fixture was cut at — so the boundary is not
    merely where output appears but where output becomes *right*.
    """

    assert recognise_chamfers(_drafted(BOUNDARY - MARGIN)) == []

    found = recognise_chamfers(_drafted(BOUNDARY + MARGIN))
    assert len(found) == 1
    assert found[0].angle == round(BOUNDARY + MARGIN, 2)


def test_the_draft_boundary_does_not_move_with_the_size_of_the_part():
    """A direction cosine is dimensionless, so the same draft decides the same way at any size.

    Modelled a hundred times larger, the fixture is a 6 m block; the angles are unchanged and
    so is the answer. This is the property ADR 0008 asks a dimensionless gate to have, and the
    reason the threshold is stated as a cosine rather than as a millimetre offset.
    """

    assert recognise_chamfers(_drafted(BOUNDARY - MARGIN, scale=100.0)) == []
    assert len(recognise_chamfers(_drafted(BOUNDARY + MARGIN, scale=100.0))) == 1


def test_small_chamfers_follow_geometry_unless_the_caller_sets_a_size_floor():
    """Recognition has no default callout-size policy; an explicit ``tol`` remains literal."""

    tiny = 0.05
    short_leg = 4.0 * tiny * math.tan(math.radians(BOUNDARY + MARGIN))
    assert short_leg < 0.3, "the premise: this fixture must be under the caller's example floor"

    assert recognise_chamfers(_drafted(BOUNDARY - MARGIN, scale=tiny)) == []
    assert len(recognise_chamfers(_drafted(BOUNDARY + MARGIN, scale=tiny))) == 1
    assert recognise_chamfers(_drafted(BOUNDARY + MARGIN, scale=tiny), tol=0.3) == []
