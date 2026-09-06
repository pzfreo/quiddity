# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Exact area-union proof for physical support patches."""

from build123d import Face, Shape, ShapeList


def covered_patch(patch: Face, supports: tuple[Face, ...]) -> bool:
    """Require full support without double-counting overlapping source faces."""
    remaining: list[Shape] = [patch]
    for support in supports:
        fragments: list[Shape] = []
        for fragment in remaining:
            difference = fragment.cut(support)
            if isinstance(difference, ShapeList):
                fragments.extend(difference)
            elif difference is not None:
                fragments.append(difference)
        remaining = fragments
        if sum(fragment.area for fragment in remaining) <= patch.area * 1e-9:
            return True
    return False
