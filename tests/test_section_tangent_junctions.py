"""Independently authored tangent semicircle junctions, not corpus geometry."""

import math

import pytest

from quiddity._sections import SectionVertex, _validate_adjacent


@pytest.mark.parametrize("radius", [0.001, 0.1, 1, 10, 100, 1000])
@pytest.mark.parametrize("degrees", range(0, 180, 2))
@pytest.mark.parametrize("reverse", [False, True])
@pytest.mark.parametrize("inward", [-0.001, 0, 0.001])
def test_tangent_junction_survives_scale_rotation_and_reversal(radius, degrees, reverse, inward):
    angle = math.radians(degrees)

    def vertex(x, y, bulge=0):
        return SectionVertex(
            (
                radius * (x * math.cos(angle) - y * math.sin(angle)),
                radius * (x * math.sin(angle) + y * math.cos(angle)),
            ),
            bulge,
        )

    if reverse:
        vertices = (vertex(1, 0, -1), vertex(-1, 0), vertex(-1 + inward, -1))
    else:
        vertices = (vertex(-1 + inward, -1), vertex(-1, 0, 1), vertex(1, 0))
    if inward > 0:
        with pytest.raises(ValueError, match="away from their shared endpoint"):
            _validate_adjacent(*vertices)
    else:
        _validate_adjacent(*vertices)


@pytest.mark.parametrize("sweep_degrees", [-270, -180, -90, 90, 180, 270])
@pytest.mark.parametrize("radius", [0.001, 1, 1000])
@pytest.mark.parametrize("reverse", [False, True])
@pytest.mark.parametrize("length", [1, 1e-6])
def test_second_intersection_must_belong_to_both_finite_line_and_signed_arc(
    sweep_degrees, radius, reverse, length
):
    sweep = math.radians(sweep_degrees)
    bulge = math.tan(sweep / 4)
    shared = (radius, 0)
    end = (radius * math.cos(sweep), radius * math.sin(sweep))
    line_end = (radius * (1 - 1e-5 * length), radius * length)
    if reverse:
        vertices = (SectionVertex(end, -bulge), SectionVertex(shared), SectionVertex(line_end))
    else:
        vertices = (SectionVertex(line_end), SectionVertex(shared, bulge), SectionVertex(end))
    if sweep_degrees > 0 and length == 1:
        with pytest.raises(ValueError, match="away from their shared endpoint"):
            _validate_adjacent(*vertices)
    else:
        _validate_adjacent(*vertices)
