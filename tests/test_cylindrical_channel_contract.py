"""Kernel-free whole-domain validation of the bounded ADR-0022 end combination."""

import json
from dataclasses import replace

import pytest

from quiddity import (
    CylindricalEndSurface,
    OpenSectionProfile,
    PassageFrame,
    PassageSectionVertex,
    PlanarEndSurface,
    SectionEnd,
    SectionRecessEnds,
    SectionRecessGeometry,
)


def _profile(points=((0.47, -1), (-0.47, -1), (-0.47, 1), (0.47, 1))):
    boundary = tuple(PassageSectionVertex(point, 0) for point in points)
    boundary = min(boundary, tuple(reversed(boundary)))
    return OpenSectionProfile("open", boundary, (boundary[-1].point, boundary[0].point))


def _geometry(*, reverse=False, offset=0, interval=None, profile=None):
    cylinder = CylindricalEndSurface(
        "cylinder",
        (0, offset, -66 if reverse else 66),
        (1, 0),
        4,
        "positive" if reverse else "negative",
    )
    cylinder_end, planar_end = SectionEnd("open", cylinder), SectionEnd("open")
    ends = (
        SectionRecessEnds(cylinder_end, planar_end)
        if reverse
        else SectionRecessEnds(planar_end, cylinder_end)
    )
    return SectionRecessGeometry(
        "section_recess",
        PassageFrame((0, 0, 0), (0, 0, 1), (1, 0, 0), (0, 1, 0)),
        interval or ((-62, 0) if reverse else (0, 62)),
        profile or _profile(),
        ends,
    )


@pytest.mark.parametrize("reverse", [False, True])
def test_cylinder_branch_is_independent_of_open_end_order(reverse):
    geometry = _geometry(reverse=reverse)
    value = json.loads(json.dumps(geometry.to_dict()))
    curved = value["ends"]["low" if reverse else "high"]
    assert curved["condition"] == "open"
    assert curved["surface"]["branch"] == ("positive" if reverse else "negative")
    assert value["profile"]["closure"] == "open"


@pytest.mark.parametrize("reverse", [False, True])
@pytest.mark.parametrize("plane", [62, 62.02])
def test_interior_extremum_refuses_touching_and_crossing_even_with_increasing_interval(
    reverse, plane
):
    # Axis passes inside the domain but not through its centroid. The centroid
    # interval increases, yet the true interior extremum crosses/touches the plane.
    interval = (-62.031, -plane) if reverse else (plane, 62.031)
    with pytest.raises(ValueError, match="strictly separated"):
        _geometry(reverse=reverse, offset=0.5, interval=interval)


@pytest.mark.parametrize(
    "points",
    [
        ((0.47, -1), (-0.47, -1), (-0.47, 1)),
        ((0.47, -1), (-0.47, -1), (-0.47, 1), (0.4, 1)),
        ((0.47, -1), (-0.47, -1), (-0.3, 1), (0.64, 1)),
        ((0.47, 0), (-0.47, 0), (-0.47, 2), (0.47, 2)),
    ],
)
def test_general_open_chains_do_not_acquire_an_implicit_closing_wall(points):
    with pytest.raises(ValueError):
        _geometry(profile=_profile(points))


@pytest.mark.parametrize("change", ["capped", "sloped", "wrong_branch", "centroid"])
def test_other_channel_end_configurations_stay_refused(change):
    geometry = _geometry()
    if change == "capped":
        ends = replace(geometry.ends, low=SectionEnd("capped"))
    elif change == "sloped":
        ends = replace(geometry.ends, low=SectionEnd("open", PlanarEndSurface(gradient=(0.1, 0))))
    elif change == "wrong_branch":
        ends = replace(
            geometry.ends,
            high=SectionEnd("open", replace(geometry.ends.high.surface, branch="positive")),
        )
    else:
        with pytest.raises(ValueError, match="centroid"):
            replace(geometry, run_interval=(0, 62.127))
        return
    with pytest.raises(ValueError):
        replace(geometry, ends=ends)
