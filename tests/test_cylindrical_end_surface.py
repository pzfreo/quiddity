import json

import pytest

from quiddity._cylindrical_end_surface import CylindricalEndSurface

POLYGON = ((-3.0, -12.0), (3.0, -12.0), (3.0, 12.0), (-3.0, 12.0))


def _surface(**changes):
    return CylindricalEndSurface(
        **{
            "type": "cylinder",
            "axis_point": (0.0, 0.0, 0.0),
            "axis_direction": (1.0, 0.0),
            "radius": 20.0,
            "branch": "positive",
            **changes,
        }
    )


def test_exact_issue_541_height_and_interior_crest():
    surface = _surface()
    assert surface.height((0, 0)) == 20
    assert surface.height((0, 12)) == 16
    assert surface.polygon_height_bounds(POLYGON) == (16, 20)
    assert json.loads(json.dumps(surface.to_dict()))["type"] == "cylinder"


def test_negative_branch_and_offset_domain():
    assert _surface(branch="negative").polygon_height_bounds(POLYGON) == (-20, -16)
    surface = _surface(axis_point=(0, -3, 7))
    low, high = surface.polygon_height_bounds(POLYGON)
    assert low == pytest.approx(7 + (400 - 225) ** 0.5)
    assert high == 27


@pytest.mark.parametrize("point", [(0, 20), (0, 21), (0, float("nan"))])
def test_invalid_or_tangent_domain_refused(point):
    with pytest.raises(ValueError):
        _surface().height(point)


@pytest.mark.parametrize(
    "change",
    [
        {"radius": 0},
        {"radius": True},
        {"radius": float("inf")},
        {"axis_direction": (0, 0)},
        {"axis_direction": (-1, 0)},
        {"axis_point": (1, 0, 0)},
        {"branch": "nearest"},
        {"type": "sphere"},
        {"radius": 1.0000001},
    ],
)
def test_noncanonical_or_invalid_parameters_refused(change):
    with pytest.raises(ValueError):
        _surface(**change)
