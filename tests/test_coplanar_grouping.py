# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

"""Grouping coplanar faces by distance rather than by grid cell.

``levels`` and ``plates`` used to group faces with ``round(coord / tol) * tol`` as a dict key.
That asks which ``tol``-wide cell a coordinate lands in, not how far two coordinates are apart,
so it merged pairs almost ``tol`` apart and split pairs a hundredth apart depending only on
where the grid boundaries happened to fall.

Nothing in the golden corpus caught it: every fixture is modelled on round numbers, which sit
predictably within their cells. These tests use deliberately off-grid geometry, which is what
an imported STEP file actually looks like.
"""

from __future__ import annotations

import pytest
from build123d import Box, Pos

from quiddity import recognise_face_levels, recognise_plates
from quiddity._geometry import cluster_coordinates

TOL = 0.5


def _horizontal_face_zs(part) -> list[float]:
    return sorted({round(f.center().Z, 6) for f in part.faces() if abs(f.normal_at().Z) > 0.99})


def test_a_close_pair_is_never_split_while_a_distant_pair_merges():
    """The defect in one assertion: grid phase, not distance, used to decide.

    Four horizontal faces at -1.0, 1.0, 1.24 and 1.26. Under ``round(z / 0.5) * 0.5`` the pair
    0.24 apart shared a cell and merged, while the pair 0.02 apart straddled a boundary and
    split — and the face at 1.24 vanished into the level reported at 1.0.
    """

    part = Box(40, 40, 2) + Pos(0, 0, 1.25) * Box(10, 10, 0.02)
    assert _horizontal_face_zs(part) == [-1.0, 1.0, 1.24, 1.26]

    zs = [level.z for level in recognise_face_levels(part, tol=TOL)]

    # 1.24 and 1.26 belong to the same upper body and are 0.02 apart, so no grouping may
    # separate them...
    assert not (any(abs(z - 1.24) < 1e-9 for z in zs) and any(abs(z - 1.26) < 1e-9 for z in zs))
    # ...but the 0.24 air gap between separate bodies is not permission to merge their levels.
    assert zs == [-1.0, 1.0, 1.24]


@pytest.mark.parametrize("offset", [0.0, 0.13, 0.24, 0.26, 0.37, 0.49])
def test_grouping_does_not_depend_on_where_the_part_sits_relative_to_a_grid(offset):
    """Sliding the whole part by less than *tol* must not change how many levels it has.

    A grid key answers differently as the part slides across cell boundaries. Distance between
    faces is unchanged by a rigid translation, so the count must be too.
    """

    def levels_at(shift: float) -> int:
        # Faces at -1.0, 1.0, 1.3 and 2.2. The 1.0/1.3 pair is closer than tol, so whether it
        # groups is exactly what a grid key decides by phase and a distance test does not.
        part = (
            Box(40, 40, 2) + Pos(0, 0, 1.15) * Box(20, 20, 0.3) + Pos(0, 0, 1.75) * Box(8, 8, 0.9)
        )
        return len(recognise_face_levels(Pos(0, 0, shift) * part, tol=TOL))

    assert levels_at(offset) == levels_at(0.0) == 3


def test_plate_thickness_is_the_measured_thickness_not_a_multiple_of_the_tolerance():
    """``Plate.lo``/``hi`` used to carry the grid key, quantising a public record field.

    A 3.7 mm slab at an off-grid position reported its faces rounded to the nearest 0.5, so its
    thickness came back as 3.5 or 4.0 — an error of up to ``tol`` on the plate's only size.
    """

    # A 3.7 mm slab whose faces sit at -1.62 and 2.08 — neither a multiple of tol. The post
    # keeps the slab from being the part's own envelope, which is excluded by design.
    slab = Pos(0, 0, 0.23) * Box(60, 60, 3.7)
    part = slab + Pos(0, 0, 10.08) * Box(10, 10, 16)

    (plate,) = [p for p in recognise_plates(part, tol=TOL) if p.axis == "z"]

    assert plate.hi - plate.lo == pytest.approx(3.7, abs=1e-3)
    assert plate.lo == pytest.approx(-1.62, abs=1e-3)


def test_clusters_are_bounded_so_a_fine_staircase_does_not_chain_into_one_level():
    """Single-linkage would merge any chain of small gaps; bounded clusters cannot.

    Nine coordinates 0.4 apart are each within ``tol=0.5`` of their neighbour. Single linkage
    would return one cluster spanning 3.2; bounding each cluster at ``tol`` keeps them apart.
    """

    coordinates = [0.4 * step for step in range(9)]

    clusters = cluster_coordinates(coordinates, tol=TOL)

    assert len(clusters) > 1
    for cluster in clusters:
        members = [coordinates[index] for index in cluster]
        assert max(members) - min(members) <= TOL


def test_clustering_is_independent_of_the_order_faces_arrive_in():
    """OCCT traversal order is not a modelling fact, so it must not select the grouping."""

    coordinates = [5.0, 0.1, 5.2, 0.0, 10.0]

    forward = cluster_coordinates(coordinates, tol=TOL)
    reversed_input = cluster_coordinates(coordinates[::-1], tol=TOL)

    def values(clusters, source):
        return sorted(sorted(source[index] for index in cluster) for cluster in clusters)

    assert values(forward, coordinates) == values(reversed_input, coordinates[::-1])
