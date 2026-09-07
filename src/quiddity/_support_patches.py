# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Exact area-union proof for physical support patches."""

from build123d import Face, Shape, ShapeList

from quiddity._typing import Bounds


def _separated(one: Bounds, other: Bounds) -> bool:
    """Whether the two boxes are strictly apart on some axis, so nothing can touch.

    Only ever asked of *conservative* boxes (``optimal=False`` is a superset of the shape),
    so a ``True`` here proves the shapes themselves are disjoint. Boxes that merely touch,
    and a box with no live ``Bnd_Box`` behind it (a null or empty shape, whose ``min``/``max``
    both collapse to the origin -- a live but degenerate shape has a real box and does not
    take this branch), answer ``False``: the caller then does the work it would have done
    anyway, which is the side of the test that cannot be wrong.
    """

    if one.wrapped is None or other.wrapped is None:
        return False
    return (
        one.min.X > other.max.X
        or other.min.X > one.max.X
        or one.min.Y > other.max.Y
        or other.min.Y > one.max.Y
        or one.min.Z > other.max.Z
        or other.min.Z > one.max.Z
    )


def covered_patch(patch: Face, supports: tuple[Face, ...]) -> bool:
    """Require full support without double-counting overlapping source faces.

    Cutting a fragment by a support it cannot reach is a no-op the kernel pays full price for:
    the swept-prism proof in :mod:`quiddity._section_recess_geometry` proves every face of a
    candidate against every wall plus the source and the cap, so a candidate with *W* walls
    costs about ``(W + 2)²`` Booleans of which nearly all are between faces that never meet.
    A fragment whose conservative bounding box is strictly apart from the support's is left
    exactly as it is, because ``Face.cut`` by a disjoint tool returns *the fragment itself* --
    ``BOPAlgo`` records no image for a shape nothing interfered with, so
    ``patch.cut(far).wrapped.IsEqual(patch.wrapped)`` is ``True``: same ``TShape``, location
    and orientation. Skipping the call therefore leaves the remaining-area sum below and every
    later cut looking at exactly the shape they would have seen, not at a numerically equal
    rebuild. That is a rejection, not a reordering: the supports are still consumed in the
    order given and the remaining-area threshold is still evaluated after each one, because
    the fragments that exist at each step -- and hence the number the threshold sees -- depend
    on that order.
    """

    remaining: list[Shape] = [patch]
    # One conservative box per fragment, computed on first need and carried across supports
    # for the fragments a support leaves alone. ``None`` marks a fragment not yet boxed.
    boxes: list[Bounds | None] = [None]
    for support in supports:
        support_box = support.bounding_box(optimal=False)
        fragments: list[Shape] = []
        fragment_boxes: list[Bounds | None] = []
        for index, fragment in enumerate(remaining):
            box = boxes[index]
            if box is None:
                box = fragment.bounding_box(optimal=False)
            if _separated(box, support_box):
                fragments.append(fragment)
                fragment_boxes.append(box)
                continue
            difference = fragment.cut(support)
            if isinstance(difference, ShapeList):
                fragments.extend(difference)
                fragment_boxes.extend([None] * len(difference))
            elif difference is not None:
                fragments.append(difference)
                fragment_boxes.append(None)
        remaining = fragments
        boxes = fragment_boxes
        if sum(fragment.area for fragment in remaining) <= patch.area * 1e-9:
            return True
    return False
