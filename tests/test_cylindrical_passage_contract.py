"""Kernel-free validation of the bounded ADR-0023 passage combination."""

import json
from dataclasses import replace

import pytest

from quiddity import (
    ClosedSectionProfile,
    CylindricalEndSurface,
    PassageFrame,
    PassageSectionVertex,
    PlanarEndSurface,
    SectionEnd,
    SectionRecessEnds,
    SectionRecessGeometry,
)

PROFILES = (
    ((-2, -1), (2, -1), (0, 2)),
    ((-2, -2), (2, -2), (2, 2), (-2, 2)),
    ((-2, 0), (-1, -2), (1, -2), (2, 0), (1, 2), (-1, 2)),
)


def geometry(points=PROFILES[0], *, reverse=False, offset=0, radius=8, interval=None):
    cylinder = CylindricalEndSurface(
        "cylinder", (0, offset, 0), (1, 0), radius, "positive" if reverse else "negative"
    )
    plane, curved = SectionEnd("open"), SectionEnd("open", cylinder)
    return SectionRecessGeometry(
        "section_recess",
        PassageFrame((0, 0, 0), (0, 0, 1), (1, 0, 0), (0, 1, 0)),
        interval or ((8, 20) if reverse else (-20, -8)),
        ClosedSectionProfile("closed", tuple(PassageSectionVertex(p, 0) for p in points)),
        SectionRecessEnds(curved, plane) if reverse else SectionRecessEnds(plane, curved),
    )


@pytest.mark.parametrize("points", PROFILES)
@pytest.mark.parametrize("reverse", [False, True])
def test_closed_polygon_with_two_open_ends_has_explicit_internal_cylinder(points, reverse):
    value = json.loads(json.dumps(geometry(points, reverse=reverse).to_dict()))
    assert value["profile"]["closure"] == "closed"
    assert {end["condition"] for end in value["ends"].values()} == {"open"}
    surface = value["ends"]["low" if reverse else "high"]["surface"]
    assert surface["type"] == "cylinder"
    assert surface["branch"] == ("positive" if reverse else "negative")


@pytest.mark.parametrize("reverse", [False, True])
@pytest.mark.parametrize("plane", [8, 7.99])
def test_interior_extremum_cannot_touch_or_cross_planar_end(reverse, plane):
    # The shifted axis crosses the profile interior, not its centroid. The
    # centroid interval increases but is insufficient to prove separation.
    interval = (7.984, plane) if reverse else (-plane, -7.984)
    with pytest.raises(ValueError, match="strictly separated"):
        geometry(reverse=reverse, offset=0.5, interval=interval)


@pytest.mark.parametrize("change", ["branch", "sloped", "cap", "centroid", "both_cylinders"])
def test_other_end_combinations_do_not_become_supported(change):
    value = geometry()
    if change == "branch":
        high = replace(value.ends.high, surface=replace(value.ends.high.surface, branch="positive"))
        ends = replace(value.ends, high=high)
    elif change == "sloped":
        ends = replace(value.ends, low=SectionEnd("open", PlanarEndSurface(gradient=(0.1, 0))))
    elif change == "cap":
        ends = replace(value.ends, low=SectionEnd("capped"))
    elif change == "both_cylinders":
        ends = replace(value.ends, low=value.ends.high)
    else:
        with pytest.raises(ValueError, match="centroid"):
            replace(value, run_interval=(-20, -7.9))
        return
    with pytest.raises(ValueError):
        replace(value, ends=ends)


def test_concave_closed_polygon_is_outside_initial_contract():
    points = ((-3, -2), (3, -2), (1, 0), (3, 2), (-3, 2), (-1, 0))
    with pytest.raises(ValueError, match="convex polygon"):
        geometry(points)


@pytest.mark.parametrize("radius", [1.99, 2])
def test_boundary_outside_or_tangent_to_cylinder_domain_is_refused(radius):
    with pytest.raises(ValueError):
        geometry(PROFILES[1], radius=radius)
