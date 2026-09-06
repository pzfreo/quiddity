"""Public geometry validation must not depend on which detector built the record."""

import json

import pytest

import quiddity
import quiddity._section_recess as records
import quiddity.section_recesses as facade
from quiddity import (
    ClosedSectionProfile,
    OpenSectionProfile,
    PassageFrame,
    PassageSectionVertex,
    PlanarEndSurface,
    SectionEnd,
    SectionRecessEnds,
    SectionRecessGeometry,
)


def _open(points, bulges=None):
    bulges = bulges or [0.0] * len(points)
    vertices = tuple(PassageSectionVertex(p, b) for p, b in zip(points, bulges, strict=True))
    return OpenSectionProfile("open", vertices, (vertices[-1].point, vertices[0].point))


def test_record_extraction_preserves_public_class_identity():
    for name in facade.__all__:
        value = getattr(facade, name)
        if isinstance(value, type):
            assert getattr(quiddity, name) is getattr(records, name) is value
            expected_module = (
                "quiddity._cylindrical_end_surface"
                if name == "CylindricalEndSurface"
                else "quiddity._section_recess"
            )
            assert value.__module__ == expected_module


@pytest.mark.parametrize(
    "points,bulges",
    [
        ([(-1, -1), (1, 1), (-1, 1), (1, -1)], None),
        ([(0, 0), (2, 0), (1, 0)], None),
        ([(-1, 0), (1, 0), (0, -2)], [1.0, 0.0, 0.0]),
        ([(-1, 0), (1, 0), (0, -2), (0, 1)], [1.0, 0.0, 0.0, 0.0]),
        ([(-1, 0), (1, 0), (0, -1)], [1.0, -0.414213562373, 0.0]),
    ],
)
def test_open_physical_chain_rejects_crossings_and_backtracking(points, bulges):
    with pytest.raises(ValueError, match="simple|overlap|shared endpoint"):
        _open(points, bulges)


@pytest.mark.parametrize(
    "points,bulges",
    [
        ([(0, 0), (1, 0)], None),
        ([(0, 0), (1, 0), (2, 0)], None),
        ([(-1, 0), (1, 0)], [1.0, 0.0]),
        ([(-1, 0), (1, 0), (2, 0)], [1.0, 0.0, 0.0]),
        # Its synthetic closing chord intersects the chain; it is not physical geometry.
        ([(0, 0), (2, 0), (0, 2), (2, 2)], None),
    ],
)
def test_valid_open_chain_does_not_validate_a_fabricated_wall(points, bulges):
    profile = _open(points, bulges)
    data = json.loads(json.dumps(profile.to_dict()))
    rebuilt = _open(
        [tuple(v["point"]) for v in data["boundary"]], [v["bulge"] for v in data["boundary"]]
    )
    assert rebuilt == profile


def _geometry(profile, low=(0.0, 0.0), high=(0.0, 0.0), span=1.0):
    return SectionRecessGeometry(
        "section_recess",
        PassageFrame((0, 0, 0), (0, 0, 1), (1, 0, 0), (0, 1, 0)),
        (0.0, span),
        profile,
        SectionRecessEnds(
            SectionEnd("capped", PlanarEndSurface(gradient=low)),
            SectionEnd("open", PlanarEndSurface(gradient=high)),
        ),
    )


def _square():
    return ClosedSectionProfile(
        "closed", tuple(PassageSectionVertex(p, 0.0) for p in [(-1, -1), (1, -1), (1, 1), (-1, 1)])
    )


@pytest.mark.parametrize("slope", [1.0, 2.0, -1.0, -2.0])
@pytest.mark.parametrize("closed", [False, True])
def test_end_planes_cannot_cross_or_touch(slope, closed):
    profile = _square() if closed else _open([(-1, 0), (1, 0)])
    with pytest.raises(ValueError, match="termination planes"):
        _geometry(profile, low=(slope, 0.0))


@pytest.mark.parametrize("slope", [0.0, 0.999999, -0.999999])
def test_separated_sloped_ends_remain_valid(slope):
    assert _geometry(_square(), low=(slope, 0.0)).run_interval == (0.0, 1.0)


@pytest.mark.parametrize("bulge,gradient", [(1.0, (0.0, 1.0)), (-1.0, (0.0, -1.0))])
def test_arc_interior_extremum_is_checked_even_when_endpoints_are_separated(bulge, gradient):
    profile = _open([(-1, 0), (1, 0)], [bulge, 0.0])
    with pytest.raises(ValueError, match="termination planes"):
        _geometry(profile, high=gradient)
    assert _geometry(profile, high=gradient, span=1.001)


def test_parallel_sloped_arc_ends_remain_valid():
    profile = _open([(-1, 0), (1, 0)], [1.0, 0.0])
    assert _geometry(profile, low=(2.0, 3.0), high=(2.0, 3.0))


@pytest.mark.parametrize(
    "bulge,gradient,contact_span",
    [
        (2.0, (-0.6, 0.8), 0.45),
        (-2.0, (-0.6, -0.8), 6.85),
    ],
)
def test_translated_major_arc_extremum_with_oblique_gradient(bulge, gradient, contact_span):
    # Centres (3, 3.25)/(3, 4.75), radius 1.25; extrema lie inside each major arc.
    profile = _open([(2, 4), (4, 4)], [bulge, 0.0])
    with pytest.raises(ValueError, match="termination planes"):
        _geometry(profile, high=gradient, span=round(contact_span - 0.001, 3))
    assert _geometry(profile, high=gradient, span=round(contact_span + 0.001, 3))


def test_closed_arc_extrema_and_geometry_json_round_trip():
    profile = ClosedSectionProfile(
        "closed",
        (
            PassageSectionVertex((-1, 0), 1.0),
            PassageSectionVertex((1, 0), 1.0),
        ),
    )
    with pytest.raises(ValueError, match="termination planes"):
        _geometry(profile, high=(0.0, 1.0))
    geometry = _geometry(profile, high=(0.0, 1.0), span=1.001)
    data = json.loads(json.dumps(geometry.to_dict()))
    rebuilt = SectionRecessGeometry(
        data["type"],
        PassageFrame(**{key: tuple(value) for key, value in data["frame"].items()}),
        tuple(data["run_interval"]),
        ClosedSectionProfile(
            "closed",
            tuple(
                PassageSectionVertex(tuple(v["point"]), v["bulge"])
                for v in data["profile"]["boundary"]
            ),
        ),
        SectionRecessEnds(
            *(
                SectionEnd(
                    data["ends"][end]["condition"],
                    PlanarEndSurface(gradient=tuple(data["ends"][end]["surface"]["gradient"])),
                )
                for end in ("low", "high")
            )
        ),
    )
    assert rebuilt == geometry
