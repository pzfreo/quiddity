# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle


import copy
import importlib.util
import json
import math
from pathlib import Path

import pytest
from build123d import (
    Box,
    BuildPart,
    BuildSketch,
    Plane,
    Polygon,
    Pos,
    export_step,
    extrude,
    import_step,
)

import quiddity._sections as section_module
from quiddity._section_adapters import (
    occurrence_to_passage,
    occurrence_to_prismatic_pocket,
    passage_to_occurrence,
    prismatic_pocket_to_occurrence,
)
from quiddity._sections import (
    BodyRef,
    BodyRefIssuer,
    LocalFrame,
    PlanarSection,
    SectionEnds,
    SectionOccurrence,
    SectionVertex,
    occurrence_geometry_dict,
    section_vertex_dict,
    validate_occurrence,
)
from quiddity.passages import Passage, recognise_passages
from quiddity.prismatic_pockets import PrismaticPocket, recognise_prismatic_pockets
from tools._legacy_recognition import (
    build_recognition_result,
)


def _square() -> tuple[tuple[float, float], ...]:
    return ((-2.0, -1.0), (2.0, -1.0), (2.0, 1.0), (-2.0, 1.0))


@pytest.mark.parametrize("axis", ["x", "y", "z"])
def test_passage_private_section_round_trip_is_exact(axis: str) -> None:
    axis_index = "xyz".index(axis)
    transverse = [index for index in range(3) if index != axis_index]
    at = [0.0, 0.0, 0.0]
    at[axis_index] = 7.0
    # Section coordinates use the record's two transverse axes.
    at[transverse[0]] = 0.0
    at[transverse[1]] = 0.0
    record = Passage(axis, 4, 6.0, tuple(at), _square())  # type: ignore[arg-type]
    issuer = BodyRefIssuer()
    body = issuer.issue(signature="body-a")

    occurrence = passage_to_occurrence(record, body_ref=body, body_refs=issuer)

    assert occurrence.ends == SectionEnds(False, False)
    assert occurrence_to_passage(occurrence, body_refs=issuer) == record
    assert occurrence_to_passage(occurrence, body_refs=issuer).to_dict() == record.to_dict()


@pytest.mark.parametrize("open_sign", [-1, 1])
def test_blind_end_topology_preserves_both_open_signs(open_sign: int) -> None:
    record = PrismaticPocket("z", 4, 5.0, open_sign, (0.0, 0.0, 4.0), _square())
    issuer = BodyRefIssuer()
    body = issuer.issue()

    occurrence = prismatic_pocket_to_occurrence(record, body_ref=body, body_refs=issuer)

    assert occurrence.ends == SectionEnds(open_sign == 1, open_sign == -1)
    assert occurrence_to_prismatic_pocket(occurrence, body_refs=issuer) == record


def test_body_references_are_run_local_and_revalidated() -> None:
    first, second = BodyRefIssuer(), BodyRefIssuer()
    body = first.issue(signature="same")
    other = second.issue(signature="same")
    record = Passage("z", 4, 2.0, (0.0, 0.0, 0.0), _square())

    with pytest.raises(ValueError, match="not issued"):
        passage_to_occurrence(record, body_ref=other, body_refs=first)

    forged = object.__new__(BodyRef)
    object.__setattr__(forged, "signature", body.signature)
    object.__setattr__(forged, "_issuer", body._issuer)
    with pytest.raises(ValueError, match="not issued"):
        passage_to_occurrence(record, body_ref=forged, body_refs=first)

    occurrence = passage_to_occurrence(record, body_ref=body, body_refs=first)
    object.__setattr__(body, "signature", "changed")
    with pytest.raises(ValueError, match="mutated"):
        occurrence_to_passage(occurrence, body_refs=first)


def test_duplicate_nonempty_body_signatures_fail_within_one_run() -> None:
    issuer = BodyRefIssuer()
    first = issuer.issue(signature="coincident")
    second = issuer.issue()
    assert first is not second
    with pytest.raises(ValueError, match="unambiguous"):
        issuer.issue(signature="coincident")

    for copied in (copy.copy(first), copy.deepcopy(first)):
        with pytest.raises(ValueError, match="not issued"):
            issuer.validate(copied)


def test_coincident_bodies_keep_distinct_run_occurrences() -> None:
    issuer = BodyRefIssuer()
    record = Passage("z", 4, 2.0, (0.0, 0.0, 0.0), _square())
    first = passage_to_occurrence(record, body_ref=issuer.issue(), body_refs=issuer)
    second = passage_to_occurrence(record, body_ref=issuer.issue(), body_refs=issuer)
    assert first is not second
    assert first.body is not second.body


def test_proposal_projection_is_primitive_only_and_omits_run_identity() -> None:
    issuer = BodyRefIssuer()
    body = issuer.issue(signature="private")
    record = Passage("z", 4, 2.0, (0.0, 0.0, 0.0), _square())
    occurrence = passage_to_occurrence(record, body_ref=body, body_refs=issuer)

    projected = occurrence_geometry_dict(occurrence, body_refs=issuer)

    assert "body" not in projected
    assert projected["ends"] == {"low_capped": False, "high_capped": False}
    assert json.loads(json.dumps(projected)) == projected


def test_schema_proposal_pins_the_normative_consumer_contract() -> None:
    proposal = (Path(__file__).parents[1] / "docs" / "planar-section-schema-proposal.md").read_text(
        encoding="utf-8"
    )
    for contract in (
        "world(t, x, y) = origin + t * run + x * u + y * v",
        "counter-clockwise in the right-handed `(u, v)` plane",
        "must not re-orthonormalize",
        "Euclidean norm within `1e-6`",
        "dot product is at most `2e-6`",
        "at most `3e-6`",
        "largest absolute **serialized** component",
        "finite JSON number and not a boolean",
        "analytic serialized centroid must be within `0.0008 mm`",
        "abs(dot(origin, run)) <= 0.000868 mm + 1e-6 * norm(origin)",
        "capability-manifest `schema_version`",
    ):
        assert contract in proposal


def test_proposal_projection_rejects_interval_collapse_and_amplified_frame_rounding() -> None:
    issuer = BodyRefIssuer()
    section = PlanarSection(tuple(SectionVertex(point) for point in _square()))
    principal = LocalFrame.principal("z", (0.0, 0.0, 0.0))
    collapsed = SectionOccurrence(
        issuer.issue(), principal, (0.0, 0.0004), section, SectionEnds(False, False)
    )
    with pytest.raises(ValueError, match="interval collapses"):
        occurrence_geometry_dict(collapsed, body_refs=issuer)

    oblique = LocalFrame.canonical((1.0, 2.0, 3.0), (0.0, 0.0, 0.0))
    long = SectionOccurrence(
        issuer.issue(), oblique, (-1_000_000.0, 1_000_000.0), section, SectionEnds(False, False)
    )
    with pytest.raises(ValueError, match="moves its geometry"):
        occurrence_geometry_dict(long, body_refs=issuer)

    major_raw = PlanarSection(
        (
            SectionVertex((-1.0, 0.0), 5097.0924455409795),
            SectionVertex((1.0, 0.0), 1 / 5097.0924455409795),
        )
    )
    major_centroid = major_raw.centroid
    major = PlanarSection(
        tuple(
            SectionVertex(
                (
                    vertex.point[0] - major_centroid[0],
                    vertex.point[1] - major_centroid[1],
                ),
                vertex.bulge,
            )
            for vertex in major_raw.boundary
        )
    )
    underestimated = SectionOccurrence(
        issuer.issue(),
        LocalFrame.canonical(
            (0.8606375331162682, 0.9646329473090614, 0.9046959845122367),
            (0.0, 0.0, 0.0),
        ),
        (-1.0, 1.0),
        major,
        SectionEnds(False, False),
    )
    with pytest.raises(ValueError, match="moves its geometry"):
        occurrence_geometry_dict(underestimated, body_refs=issuer)


def test_section_negative_zero_is_canonicalized() -> None:
    section = PlanarSection(
        (
            SectionVertex((-0.0, -0.0)),
            SectionVertex((1.0, -0.0)),
            SectionVertex((-0.0, 1.0)),
        )
    )
    encoded = json.dumps([section_vertex_dict(vertex) for vertex in section.boundary])
    assert "-0.0" not in encoded

    near_zero = PlanarSection(
        (
            SectionVertex((-1e-7, 0.0)),
            SectionVertex((1.0, 0.0)),
            SectionVertex((1.0, 1.0)),
        )
    )
    assert "-0.0" not in json.dumps([section_vertex_dict(vertex) for vertex in near_zero.boundary])

    issuer = BodyRefIssuer()
    occurrence = SectionOccurrence(
        issuer.issue(),
        LocalFrame.principal("z", (0.0, 0.0, 0.0)),
        (-0.0004, 1.0),
        PlanarSection(tuple(SectionVertex(point) for point in _square())),
        SectionEnds(False, False),
    )
    assert "-0.0" not in json.dumps(occurrence_geometry_dict(occurrence, body_refs=issuer))


def test_two_arc_circle_has_exact_arc_area_and_centroid() -> None:
    circle = PlanarSection(
        (
            SectionVertex((-1.0, 0.0), -1.0),
            SectionVertex((1.0, 0.0), -1.0),
        )
    )
    assert circle.area == pytest.approx(math.pi)
    assert circle.centroid == pytest.approx((0.0, 0.0), abs=1e-12)


def test_asymmetric_arc_loop_uses_circular_segment_centroid() -> None:
    half_disk = PlanarSection((SectionVertex((-1.0, 0.0), -1.0), SectionVertex((1.0, 0.0))))
    assert half_disk.area == pytest.approx(math.pi / 2)
    assert half_disk.centroid == pytest.approx((0.0, 4 / (3 * math.pi)), abs=1e-12)


@pytest.mark.parametrize("offset", [1e9, 1e12])
def test_area_and_centroid_are_stable_under_large_translation(offset: float) -> None:
    polygon = PlanarSection(tuple(SectionVertex((offset + x, offset + y)) for x, y in _square()))
    arc = PlanarSection(
        (
            SectionVertex((offset - 1.0, offset), -1.0),
            SectionVertex((offset + 1.0, offset)),
        )
    )
    assert polygon.area == pytest.approx(8.0)
    assert polygon.centroid == pytest.approx((offset, offset), abs=1e-6)
    assert arc.area == pytest.approx(math.pi / 2)
    assert arc.centroid == pytest.approx((offset, offset + 4 / (3 * math.pi)), abs=1e-4)


def test_mixed_line_arc_centroid_is_translation_invariant() -> None:
    """Line and arc moment contributions must use one compatible Green field."""

    boundary = (
        SectionVertex((-6.0, -3.0)),
        SectionVertex((6.0, -3.0), 1.0),
        SectionVertex((6.0, 3.0)),
        SectionVertex((-6.0, 3.0), 1.0),
    )
    translated = tuple(
        SectionVertex((vertex.point[0] + 17.0, vertex.point[1] - 11.0), vertex.bulge)
        for vertex in boundary
    )

    assert PlanarSection(boundary).centroid == pytest.approx((0.0, 0.0), abs=1e-12)
    assert PlanarSection(translated).centroid == pytest.approx((17.0, -11.0), abs=1e-12)


def test_equivalent_arc_subdivision_preserves_area_and_centroid() -> None:
    half = math.tan(math.pi / 8)
    two = PlanarSection((SectionVertex((-1.0, 0.0), -1.0), SectionVertex((1.0, 0.0), -1.0)))
    four = PlanarSection(
        (
            SectionVertex((-1.0, 0.0), -half),
            SectionVertex((0.0, 1.0), -half),
            SectionVertex((1.0, 0.0), -half),
            SectionVertex((0.0, -1.0), -half),
        )
    )
    assert four.area == pytest.approx(two.area, abs=1e-12)
    assert four.centroid == pytest.approx(two.centroid, abs=1e-12)


def test_reversed_mixed_loop_has_same_canonical_boundary() -> None:
    original = (
        SectionVertex((0.0, 0.0), 0.5),
        SectionVertex((2.0, 0.0), 0.0),
        SectionVertex((2.0, 2.0), 0.0),
        SectionVertex((0.0, 2.0), 0.0),
    )
    reversed_loop = (
        SectionVertex((0.0, 0.0), 0.0),
        SectionVertex((0.0, 2.0), 0.0),
        SectionVertex((2.0, 2.0), 0.0),
        SectionVertex((2.0, 0.0), -0.5),
    )
    assert PlanarSection(original) == PlanarSection(reversed_loop)


def test_major_arc_and_primitive_projection_are_preserved() -> None:
    section = PlanarSection(
        (
            SectionVertex((-1.0, 0.0), -2.0),
            SectionVertex((1.0, 0.0), -0.5),
        )
    )
    projected = tuple(section_vertex_dict(vertex) for vertex in section.boundary)
    bulges = tuple(item["bulge"] for item in projected)
    assert all(isinstance(value, float) for value in bulges)
    assert any(abs(value) > 1 for value in bulges if isinstance(value, float))
    json.dumps(projected)


def test_major_arc_projection_uses_a_whole_curve_displacement_bound() -> None:
    with pytest.raises(ValueError, match="moves its boundary"):
        PlanarSection(
            (
                SectionVertex((2.9043848088529707, 0.6959148088332885), 4.861127260347701),
                SectionVertex((-6.906520869503443, -0.4239503385675043)),
            )
        )


def test_tiny_arc_that_serializes_as_zero_fails_closed() -> None:
    with pytest.raises(ValueError, match="collapse a nonzero arc"):
        PlanarSection(
            (
                SectionVertex((0.0, 0.0), 1e-13),
                SectionVertex((1.0, 0.0)),
                SectionVertex((0.0, 1.0)),
            )
        )


def test_self_crossing_line_loop_fails_closed() -> None:
    with pytest.raises(ValueError, match="simple|shared endpoint"):
        PlanarSection(
            tuple(
                SectionVertex(point) for point in ((0.0, 0.0), (2.0, 2.0), (0.0, 2.0), (2.0, 0.0))
            )
        )


@pytest.mark.parametrize(
    "boundary",
    [
        ((0.0, 0.0, 1.0), (2.0, 0.0, 0.0), (0.0, 1.0, 0.0), (2.0, 1.0, 0.0)),
        ((-2.0, 0.0, 1.0), (2.0, 0.0, 0.0), (0.0, -2.0, 1.0), (0.0, 2.0, 0.0)),
    ],
    ids=("line-arc", "arc-arc"),
)
def test_mixed_boundary_intersections_fail_closed(
    boundary: tuple[tuple[float, float, float], ...],
) -> None:
    with pytest.raises(ValueError, match="simple|shared endpoint"):
        PlanarSection(tuple(SectionVertex((x, y), bulge) for x, y, bulge in boundary))


def test_legacy_projection_refuses_arcs_and_inconsistent_records() -> None:
    issuer = BodyRefIssuer()
    body = issuer.issue()
    frame = LocalFrame.principal("z", (0.0, 0.0, 0.0))
    arc = PlanarSection((SectionVertex((-1.0, 0.0), -1.0), SectionVertex((1.0, 0.0), -1.0)))
    occurrence = SectionOccurrence(body, frame, (-1.0, 1.0), arc, SectionEnds(False, False))
    with pytest.raises(ValueError, match="cannot represent arc"):
        occurrence_to_passage(occurrence, body_refs=issuer)

    inconsistent = Passage("z", 4, 2.0, (3.0, 0.0, 0.0), _square())
    with pytest.raises(ValueError, match="centre disagrees"):
        passage_to_occurrence(inconsistent, body_ref=body, body_refs=issuer)


@pytest.mark.parametrize(
    "record, message",
    [
        (Passage("q", 4, 2.0, (0.0, 0.0, 0.0), _square()), "axis"),
        (Passage("z", 4, 0.0, (0.0, 0.0, 0.0), _square()), "span"),
        (Passage("z", 3, 2.0, (0.0, 0.0, 0.0), _square()), "side count"),
        (Passage("z", 4, 2.0, (math.nan, 0.0, 0.0), _square()), "centre"),
        (
            Passage(
                "z",
                4,
                2.0,
                (0.0, 0.0, 0.0),
                ((-2.0, -1.0, 0.0), (2.0, -1.0), (2.0, 1.0), (-2.0, 1.0)),  # type: ignore[arg-type]
            ),
            "finite pairs",
        ),
    ],
)
def test_invalid_hand_built_passages_fail_closed(record: Passage, message: str) -> None:
    issuer = BodyRefIssuer()
    with pytest.raises(ValueError, match=message):
        passage_to_occurrence(record, body_ref=issuer.issue(), body_refs=issuer)


def test_invalid_hand_built_pocket_open_sign_fails_closed() -> None:
    issuer = BodyRefIssuer()
    record = PrismaticPocket("z", 4, 2.0, 0, (0.0, 0.0, 0.0), _square())
    with pytest.raises(ValueError, match="open_sign"):
        prismatic_pocket_to_occurrence(record, body_ref=issuer.issue(), body_refs=issuer)


@pytest.mark.parametrize(
    "operation, message",
    [
        (lambda: LocalFrame.canonical((0.0, 0.0, 0.0), (0.0, 0.0, 0.0)), "nonzero"),
        (
            lambda: LocalFrame(
                (math.inf, 0.0, 0.0),
                (1.0, 0.0, 0.0),
                (0.0, 1.0, 0.0),
                (0.0, 0.0, 1.0),
            ),
            "finite",
        ),
        (
            lambda: LocalFrame(
                (0.0, 0.0, 0.0),
                (2.0, 0.0, 0.0),
                (0.0, 1.0, 0.0),
                (0.0, 0.0, 1.0),
            ),
            "unit length",
        ),
        (
            lambda: LocalFrame(
                (0.0, 0.0, 0.0),
                (1.0, 0.0, 0.0),
                (1.0, 0.0, 0.0),
                (0.0, 0.0, 1.0),
            ),
            "orthogonal",
        ),
        (
            lambda: LocalFrame(
                (0.0, 0.0, 0.0),
                (1.0, 0.0, 0.0),
                (0.0, 1.0, 0.0),
                (0.0, 0.0, -1.0),
            ),
            "right handed",
        ),
        (lambda: LocalFrame.principal("q", (0.0, 0.0, 0.0)), "axis"),
        (lambda: LocalFrame.principal("z", (math.nan, 0.0, 0.0)), "finite"),
        (lambda: LocalFrame.canonical((0.0, 0.0, 1.0), (math.nan, 0.0, 0.0)), "finite"),
        (lambda: SectionVertex((math.nan, 0.0)), "finite"),
    ],
)
def test_private_frame_and_vertex_values_fail_closed(operation, message: str) -> None:
    with pytest.raises(ValueError, match=message):
        operation()


def test_private_section_shape_and_serialization_refusals_are_closed() -> None:
    with pytest.raises(ValueError, match="at least two"):
        PlanarSection((SectionVertex((0.0, 0.0)),))
    with pytest.raises(ValueError, match="distinct"):
        PlanarSection(
            (
                SectionVertex((0.0, 0.0)),
                SectionVertex((0.0, 0.0)),
                SectionVertex((1.0, 0.0)),
            )
        )
    with pytest.raises(ValueError, match="collapse distinct vertices"):
        PlanarSection(
            (
                SectionVertex((0.0, 0.0)),
                SectionVertex((0.0004, 0.0004)),
                SectionVertex((1.0, 0.0)),
            )
        )
    with pytest.raises(ValueError, match="nonzero area"):
        section_module._moments(
            (
                SectionVertex((0.0, 0.0)),
                SectionVertex((1.0, 0.0)),
                SectionVertex((2.0, 0.0)),
            )
        )
    with pytest.raises(ValueError, match="arc endpoints"):
        section_module._arc(SectionVertex((0.0, 0.0), 1.0), SectionVertex((0.0, 0.0)))


@pytest.mark.parametrize(
    "a, b, c, d",
    [
        ((0.0, 0.0), (2.0, 0.0), (1.0, 0.0), (1.0, 1.0)),
        ((0.0, 0.0), (2.0, 0.0), (1.0, 1.0), (1.0, 0.0)),
        ((0.0, 0.0), (2.0, 0.0), (-1.0, -1.0), (1.0, 1.0)),
        ((0.0, 0.0), (2.0, 0.0), (1.0, -1.0), (3.0, 1.0)),
    ],
)
def test_line_intersection_includes_each_collinear_endpoint_case(a, b, c, d) -> None:
    assert section_module._line_intersection(a, b, c, d)


def test_arc_intersection_helpers_cover_disjoint_concentric_and_crossing_cases() -> None:
    upper = section_module._Arc((0.0, 0.0), 1.0, 0.0, math.pi)
    shifted = section_module._Arc((1.0, 0.0), 1.0, 0.0, math.pi)
    distant = section_module._Arc((4.0, 0.0), 1.0, 0.0, math.pi)
    concentric = section_module._Arc((0.0, 0.0), 2.0, 0.0, math.pi)

    assert not section_module._point_on_arc((3.0, 0.0), upper)
    assert section_module._line_arc_points((-2.0, 3.0), (2.0, 3.0), upper) == ()
    assert section_module._arc_arc_points(upper, concentric) == ()
    assert section_module._arc_arc_points(upper, distant) == ()
    assert section_module._arc_arc_intersection(upper, shifted)


def test_adjacent_line_and_arc_cannot_meet_again_away_from_shared_endpoint() -> None:
    with pytest.raises(ValueError, match="away from"):
        section_module._validate_adjacent(
            SectionVertex((-2.0, 0.0)),
            SectionVertex((2.0, 0.0), 1.0),
            SectionVertex((-2.0, 0.0)),
        )
    with pytest.raises(ValueError, match="away from"):
        section_module._validate_adjacent(
            SectionVertex((-2.0, 0.0), 1.0),
            SectionVertex((2.0, 0.0)),
            SectionVertex((-2.0, 0.0)),
        )
    with pytest.raises(ValueError, match="overlap|backtrack"):
        section_module._validate_adjacent(
            SectionVertex((0.0, 0.0)),
            SectionVertex((1.0, 0.0)),
            SectionVertex((0.0, 0.0)),
        )


def test_occurrence_body_and_end_value_invariants_fail_closed() -> None:
    with pytest.raises(ValueError, match="booleans"):
        SectionEnds(1, False)  # type: ignore[arg-type]
    with pytest.raises(ValueError, match="both ends"):
        SectionEnds(True, True)
    issuer = BodyRefIssuer()
    with pytest.raises(ValueError, match="nonempty string"):
        issuer.issue(signature="")
    with pytest.raises(ValueError, match="finite and increasing"):
        SectionOccurrence(
            issuer.issue(),
            LocalFrame.principal("z", (0.0, 0.0, 0.0)),
            (1.0, 1.0),
            PlanarSection(tuple(SectionVertex(point) for point in _square())),
            SectionEnds(False, False),
        )


def test_equivalent_noncanonical_occurrence_encodings_fail_closed() -> None:
    issuer = BodyRefIssuer()
    body = issuer.issue()
    ends = SectionEnds(False, False)
    offset_section = PlanarSection(tuple(SectionVertex((x + 2.0, y - 3.0)) for x, y in _square()))
    with pytest.raises(ValueError, match="origin-centred"):
        SectionOccurrence(
            body,
            LocalFrame.principal("z", (-2.0, 3.0, 0.0)),
            (-1.0, 1.0),
            offset_section,
            ends,
        )

    centred = PlanarSection(tuple(SectionVertex(point) for point in _square()))
    with pytest.raises(ValueError, match="perpendicular to its run"):
        SectionOccurrence(
            body,
            LocalFrame(
                origin=(0.0, 0.0, 2.0),
                run=(0.0, 0.0, 1.0),
                u=(1.0, 0.0, 0.0),
                v=(0.0, 1.0, 0.0),
            ),
            (-3.0, -1.0),
            centred,
            ends,
        )


def test_rotated_in_plane_frame_gauge_fails_closed() -> None:
    issuer = BodyRefIssuer()
    rotated_section = PlanarSection(
        tuple(
            SectionVertex(point) for point in ((-1.0, -2.0), (1.0, -2.0), (1.0, 2.0), (-1.0, 2.0))
        )
    )
    with pytest.raises(ValueError, match="canonical run and in-plane basis"):
        SectionOccurrence(
            issuer.issue(),
            LocalFrame(
                origin=(0.0, 0.0, 0.0),
                run=(0.0, 0.0, 1.0),
                u=(0.0, 1.0, 0.0),
                v=(-1.0, 0.0, 0.0),
            ),
            (-1.0, 1.0),
            rotated_section,
            SectionEnds(False, False),
        )


def test_dominant_axis_tie_is_decided_from_the_serialized_run() -> None:
    issuer = BodyRefIssuer()
    section = PlanarSection(tuple(SectionVertex(point) for point in _square()))
    projected = []
    for run in ((1.0 + 1e-8, 0.0, 1.0), (1.0, 0.0, 1.0 + 1e-8)):
        occurrence = SectionOccurrence(
            issuer.issue(),
            LocalFrame.canonical(run, (0.0, 0.0, 0.0)),
            (-1.0, 1.0),
            section,
            SectionEnds(False, False),
        )
        projected.append(occurrence_geometry_dict(occurrence, body_refs=issuer)["frame"])
    assert projected[0] == projected[1]


def test_occurrence_projection_revalidates_canonical_placement() -> None:
    issuer = BodyRefIssuer()
    occurrence = SectionOccurrence(
        issuer.issue(),
        LocalFrame.principal("z", (0.0, 0.0, 0.0)),
        (-1.0, 1.0),
        PlanarSection(tuple(SectionVertex(point) for point in _square())),
        SectionEnds(False, False),
    )
    object.__setattr__(
        occurrence,
        "frame",
        LocalFrame(
            origin=(0.0, 0.0, 1.0),
            run=(0.0, 0.0, 1.0),
            u=(1.0, 0.0, 0.0),
            v=(0.0, 1.0, 0.0),
        ),
    )
    with pytest.raises(ValueError, match="perpendicular to its run"):
        occurrence_geometry_dict(occurrence, body_refs=issuer)


@pytest.mark.parametrize("target", ["proposal", "passage", "pocket"])
def test_every_occurrence_reader_rejects_a_mutated_nonfinite_interval(target: str) -> None:
    issuer = BodyRefIssuer()
    body = issuer.issue()
    if target == "pocket":
        occurrence = prismatic_pocket_to_occurrence(
            PrismaticPocket("z", 4, 2.0, 1, (0.0, 0.0, 0.0), _square()),
            body_ref=body,
            body_refs=issuer,
        )
    else:
        occurrence = passage_to_occurrence(
            Passage("z", 4, 2.0, (0.0, 0.0, 0.0), _square()),
            body_ref=body,
            body_refs=issuer,
        )
    object.__setattr__(occurrence, "run_interval", (math.nan, 1.0))

    with pytest.raises(ValueError, match="finite and increasing"):
        if target == "proposal":
            occurrence_geometry_dict(occurrence, body_refs=issuer)
        elif target == "passage":
            occurrence_to_passage(occurrence, body_refs=issuer)
        else:
            occurrence_to_prismatic_pocket(occurrence, body_refs=issuer)


@pytest.mark.parametrize("target", ["proposal", "passage", "pocket"])
def test_every_occurrence_reader_rejects_a_mutated_offset_section(target: str) -> None:
    issuer = BodyRefIssuer()
    body = issuer.issue()
    if target == "pocket":
        occurrence = prismatic_pocket_to_occurrence(
            PrismaticPocket("z", 4, 2.0, -1, (0.0, 0.0, 0.0), _square()),
            body_ref=body,
            body_refs=issuer,
        )
    else:
        occurrence = passage_to_occurrence(
            Passage("z", 4, 2.0, (0.0, 0.0, 0.0), _square()),
            body_ref=body,
            body_refs=issuer,
        )
    object.__setattr__(
        occurrence,
        "section",
        PlanarSection(tuple(SectionVertex((x + 1.0, y)) for x, y in _square())),
    )

    with pytest.raises(ValueError, match="origin-centred"):
        if target == "proposal":
            occurrence_geometry_dict(occurrence, body_refs=issuer)
        elif target == "passage":
            occurrence_to_passage(occurrence, body_refs=issuer)
        else:
            occurrence_to_prismatic_pocket(occurrence, body_refs=issuer)


@pytest.mark.parametrize(
    "mutation, message",
    [
        ("frame-type", "LocalFrame"),
        ("section-type", "PlanarSection"),
        ("section-content", "invalid section"),
        ("section-order", "not canonical or was mutated"),
        ("ends-type", "SectionEnds"),
    ],
)
def test_occurrence_read_validation_rejects_mutated_nested_values(
    mutation: str, message: str
) -> None:
    issuer = BodyRefIssuer()
    occurrence = passage_to_occurrence(
        Passage("z", 4, 2.0, (0.0, 0.0, 0.0), _square()),
        body_ref=issuer.issue(),
        body_refs=issuer,
    )
    if mutation == "frame-type":
        object.__setattr__(occurrence, "frame", object())
    elif mutation == "section-type":
        object.__setattr__(occurrence, "section", object())
    elif mutation == "section-content":
        object.__setattr__(occurrence.section, "boundary", (object(),))
    elif mutation == "section-order":
        boundary = occurrence.section.boundary
        object.__setattr__(occurrence.section, "boundary", boundary[1:] + boundary[:1])
    else:
        object.__setattr__(occurrence, "ends", object())

    with pytest.raises(ValueError, match=message):
        occurrence_geometry_dict(occurrence, body_refs=issuer)


def test_occurrence_validator_rejects_the_wrong_value_type() -> None:
    with pytest.raises(ValueError, match="SectionOccurrence"):
        validate_occurrence(object(), body_refs=BodyRefIssuer())  # type: ignore[arg-type]


def test_reverse_legacy_projection_refuses_wrong_ends_and_free_axis_frame() -> None:
    issuer = BodyRefIssuer()
    body = issuer.issue()
    section = PlanarSection(tuple(SectionVertex(point) for point in _square()))
    principal = LocalFrame.principal("z", (0.0, 0.0, 0.0))
    with pytest.raises(ValueError, match="two open ends"):
        occurrence_to_passage(
            SectionOccurrence(body, principal, (-1.0, 1.0), section, SectionEnds(True, False)),
            body_refs=issuer,
        )
    with pytest.raises(ValueError, match="exactly one capped end"):
        occurrence_to_prismatic_pocket(
            SectionOccurrence(body, principal, (-1.0, 1.0), section, SectionEnds(False, False)),
            body_refs=issuer,
        )
    with pytest.raises(ValueError, match="principal-axis"):
        occurrence_to_passage(
            SectionOccurrence(
                body,
                LocalFrame.canonical((1.0, 2.0, 3.0), (0.0, 0.0, 0.0)),
                (-1.0, 1.0),
                section,
                SectionEnds(False, False),
            ),
            body_refs=issuer,
        )


def test_legacy_centroid_double_rounding_keeps_exact_record_projection() -> None:
    record = Passage(
        "z",
        3,
        2.0,
        (0.0, 0.0, 0.0),
        ((0.0, 0.0), (0.001, 0.0), (0.0, 0.001)),
    )
    issuer = BodyRefIssuer()
    occurrence = passage_to_occurrence(record, body_ref=issuer.issue(), body_refs=issuer)
    assert occurrence.section.centroid == pytest.approx((0.0, 0.0), abs=1e-12)
    assert occurrence_to_passage(occurrence, body_refs=issuer) == record


def test_legacy_centroid_accepts_published_serialization_displacement() -> None:
    record = PrismaticPocket(
        "x",
        3,
        3.114,
        1,
        (1.557, 9.413, 16.738),
        ((8.616, 17.431), (9.0, 15.552), (10.624, 17.233)),
    )
    issuer = BodyRefIssuer()

    occurrence = prismatic_pocket_to_occurrence(record, body_ref=issuer.issue(), body_refs=issuer)

    assert occurrence.section.centroid == pytest.approx((0.0, 0.0), abs=1e-12)


def test_free_axis_frame_sign_and_dominant_tie_are_deterministic() -> None:
    positive = LocalFrame.canonical((1.0, 1.0, 1.0), (2.0, 3.0, 4.0))
    reversed_run = LocalFrame.canonical((-1.0, -1.0, -1.0), (2.0, 3.0, 4.0))
    assert positive == reversed_run
    assert positive.run[2] > 0  # Z wins the exact dominant-component tie.


def test_scale_preserves_normalized_section_shape() -> None:
    base = PlanarSection(tuple(SectionVertex(point) for point in _square()))
    scaled = PlanarSection(
        tuple(SectionVertex((point[0] * 1000, point[1] * 1000)) for point in _square())
    )
    assert scaled.area == pytest.approx(base.area * 1_000_000)
    assert scaled.centroid == pytest.approx((base.centroid[0] * 1000, base.centroid[1] * 1000))


def test_cyclic_traversal_produces_the_same_section() -> None:
    vertices = tuple(SectionVertex(point) for point in _square())
    assert PlanarSection(vertices) == PlanarSection(vertices[2:] + vertices[:2])


def _mat_vec(
    matrix: tuple[tuple[float, float, float], ...], point: tuple[float, float, float]
) -> tuple[float, float, float]:
    values = tuple(sum(row[index] * point[index] for index in range(3)) for row in matrix)
    return (float(values[0]), float(values[1]), float(values[2]))


def _sample_section_world(
    section: PlanarSection, frame: LocalFrame, *, samples: int = 17
) -> tuple[tuple[float, float, float], ...]:
    points: list[tuple[float, float, float]] = []
    for index, vertex in enumerate(section.boundary):
        following = section.boundary[(index + 1) % len(section.boundary)]
        if vertex.bulge == 0.0:
            local = tuple(
                (
                    vertex.point[0] + fraction * (following.point[0] - vertex.point[0]),
                    vertex.point[1] + fraction * (following.point[1] - vertex.point[1]),
                )
                for fraction in (step / samples for step in range(samples))
            )
        else:
            dx = following.point[0] - vertex.point[0]
            dy = following.point[1] - vertex.point[1]
            chord = math.hypot(dx, dy)
            offset = chord * (1 - vertex.bulge**2) / (4 * vertex.bulge)
            centre = (
                (vertex.point[0] + following.point[0]) / 2 - dy * offset / chord,
                (vertex.point[1] + following.point[1]) / 2 + dx * offset / chord,
            )
            radius = chord * (1 + vertex.bulge**2) / (4 * abs(vertex.bulge))
            start = math.atan2(vertex.point[1] - centre[1], vertex.point[0] - centre[0])
            sweep = 4 * math.atan(vertex.bulge)
            local = tuple(
                (
                    centre[0] + radius * math.cos(start + sweep * step / samples),
                    centre[1] + radius * math.sin(start + sweep * step / samples),
                )
                for step in range(samples)
            )
        points.extend(
            (
                frame.origin[0] + frame.u[0] * point[0] + frame.v[0] * point[1],
                frame.origin[1] + frame.u[1] * point[0] + frame.v[1] * point[1],
                frame.origin[2] + frame.u[2] * point[0] + frame.v[2] * point[1],
            )
            for point in local
        )
    return tuple(points)


@pytest.mark.parametrize(
    "matrix",
    [
        (
            (1.0, 0.0, 0.0),
            (0.0, math.cos(0.61), -math.sin(0.61)),
            (0.0, math.sin(0.61), math.cos(0.61)),
        ),
        ((-1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0)),
    ],
    ids=("rotation", "mirror"),
)
def test_transformed_section_reconstructs_the_transformed_world_geometry(
    matrix: tuple[tuple[float, float, float], ...],
) -> None:
    source_frame = LocalFrame.principal("z", (3.0, 4.0, 0.0))
    source = PlanarSection(tuple(SectionVertex(point) for point in _square()))
    world: tuple[tuple[float, float, float], ...] = tuple(
        (
            source_frame.origin[0]
            + source_frame.u[0] * vertex.point[0]
            + source_frame.v[0] * vertex.point[1],
            source_frame.origin[1]
            + source_frame.u[1] * vertex.point[0]
            + source_frame.v[1] * vertex.point[1],
            source_frame.origin[2]
            + source_frame.u[2] * vertex.point[0]
            + source_frame.v[2] * vertex.point[1],
        )
        for vertex in source.boundary
    )
    transformed = tuple(_mat_vec(matrix, point) for point in world)
    run = _mat_vec(matrix, source_frame.run)
    centroid = tuple(
        sum(point[index] for point in transformed) / len(transformed) for index in range(3)
    )
    frame = LocalFrame.canonical(run, centroid)  # type: ignore[arg-type]
    local = tuple(
        SectionVertex(
            (
                sum((point[i] - frame.origin[i]) * frame.u[i] for i in range(3)),
                sum((point[i] - frame.origin[i]) * frame.v[i] for i in range(3)),
            )
        )
        for point in transformed
    )
    section = PlanarSection(local)
    reconstructed = tuple(
        tuple(
            frame.origin[index]
            + frame.u[index] * vertex.point[0]
            + frame.v[index] * vertex.point[1]
            for index in range(3)
        )
        for vertex in section.boundary
    )

    def point_key(point: tuple[float, ...]) -> tuple[float, ...]:
        return tuple(round(component, 9) for component in point)

    for actual, expected in zip(
        sorted(reconstructed, key=point_key),
        sorted(transformed, key=point_key),
        strict=True,
    ):
        assert actual == pytest.approx(expected, abs=1e-9)


@pytest.mark.parametrize(
    "matrix",
    [
        (
            (1.0, 0.0, 0.0),
            (0.0, math.cos(0.61), -math.sin(0.61)),
            (0.0, math.sin(0.61), math.cos(0.61)),
        ),
        ((-1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0)),
    ],
    ids=("rotation", "mirror"),
)
def test_transformed_arc_section_preserves_world_geometry(matrix) -> None:
    frame = LocalFrame.principal("z", (2.0, 3.0, 0.0))
    source = PlanarSection((SectionVertex((-1.0, 0.0), -1.0), SectionVertex((1.0, 0.0), -1.0)))
    world = tuple(
        (
            frame.origin[0] + frame.u[0] * vertex.point[0] + frame.v[0] * vertex.point[1],
            frame.origin[1] + frame.u[1] * vertex.point[0] + frame.v[1] * vertex.point[1],
            frame.origin[2] + frame.u[2] * vertex.point[0] + frame.v[2] * vertex.point[1],
        )
        for vertex in source.boundary
    )
    transformed = tuple(_mat_vec(matrix, point) for point in world)
    transformed_run = _mat_vec(matrix, frame.run)
    transformed_centroid = _mat_vec(matrix, frame.origin)
    placed = LocalFrame.canonical(transformed_run, transformed_centroid)
    transformed_u, transformed_v = _mat_vec(matrix, frame.u), _mat_vec(matrix, frame.v)
    a = sum(transformed_u[index] * placed.u[index] for index in range(3))
    b = sum(transformed_v[index] * placed.u[index] for index in range(3))
    c = sum(transformed_u[index] * placed.v[index] for index in range(3))
    d = sum(transformed_v[index] * placed.v[index] for index in range(3))
    orientation = a * d - b * c
    bulge_sign = 1.0 if orientation > 0 else -1.0
    local = tuple(
        SectionVertex(
            (
                sum((point[i] - placed.origin[i]) * placed.u[i] for i in range(3)),
                sum((point[i] - placed.origin[i]) * placed.v[i] for i in range(3)),
            ),
            vertex.bulge * bulge_sign,
        )
        for point, vertex in zip(transformed, source.boundary, strict=True)
    )
    transformed_section = PlanarSection(local)
    assert transformed_section.area == pytest.approx(source.area)
    assert transformed_section.centroid == pytest.approx((0.0, 0.0), abs=1e-9)
    expected_curve = tuple(
        _mat_vec(matrix, point) for point in _sample_section_world(source, frame)
    )
    actual_curve = _sample_section_world(transformed_section, placed)
    assert (
        max(min(math.dist(point, other) for other in actual_curve) for point in expected_curve)
        < 1e-9
    )
    assert (
        max(min(math.dist(point, other) for other in expected_curve) for point in actual_curve)
        < 1e-9
    )


def _step_round_trip(part, path: Path):
    export_step(part, str(path))
    return import_step(str(path))


def test_step_records_pass_through_private_adapters_byte_identically(tmp_path: Path) -> None:
    with BuildPart() as cutter:
        with BuildSketch(Plane.XY):
            Polygon((-8, -6), (8, -6), (0, 8))
        extrude(amount=60, both=True)
    assert cutter.part is not None
    passage_part = Box(60, 40, 20) - cutter.part

    with BuildPart() as pocket_cutter:
        with BuildSketch(Plane.XY):
            Polygon((-8, -6), (8, -6), (0, 8))
        extrude(amount=14)
    assert pocket_cutter.part is not None
    pocket_part = Box(60, 40, 20) - Pos(0, 0, 2) * pocket_cutter.part

    (passage,) = recognise_passages(_step_round_trip(passage_part, tmp_path / "passage.step"))
    (pocket,) = recognise_prismatic_pockets(_step_round_trip(pocket_part, tmp_path / "pocket.step"))
    issuer = BodyRefIssuer()
    passage_body, pocket_body = issuer.issue(), issuer.issue()

    projected_passage = occurrence_to_passage(
        passage_to_occurrence(passage, body_ref=passage_body, body_refs=issuer),
        body_refs=issuer,
    )
    projected_pocket = occurrence_to_prismatic_pocket(
        prismatic_pocket_to_occurrence(pocket, body_ref=pocket_body, body_refs=issuer),
        body_refs=issuer,
    )
    assert projected_passage.to_dict() == passage.to_dict()
    assert projected_pocket.to_dict() == pocket.to_dict()


def test_all_existing_golden_polygonal_recess_records_round_trip() -> None:
    golden_root = Path(__file__).parent / "golden"
    checked = 0
    for fixture_path in sorted(golden_root.glob("*/fixture.py")):
        spec = importlib.util.spec_from_file_location(
            f"section_adapter_{fixture_path.parent.name}", fixture_path
        )
        assert spec is not None and spec.loader is not None
        fixture = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(fixture)
        part = fixture.build_fixture()
        prismatic_result = build_recognition_result(part, rotational=False)
        rotational_result = build_recognition_result(part, rotational=True)
        collections = (
            (recognise_passages(part), recognise_prismatic_pockets(part)),
            (prismatic_result.passages, prismatic_result.prismatic_pockets),
            (rotational_result.passages, rotational_result.prismatic_pockets),
        )
        issuer = BodyRefIssuer()
        for passages, pockets in collections:
            for passage in passages:
                occurrence = passage_to_occurrence(
                    passage, body_ref=issuer.issue(), body_refs=issuer
                )
                assert (
                    occurrence_to_passage(occurrence, body_refs=issuer).to_dict()
                    == passage.to_dict()
                )
                checked += 1
            for pocket in pockets:
                occurrence = prismatic_pocket_to_occurrence(
                    pocket, body_ref=issuer.issue(), body_refs=issuer
                )
                assert (
                    occurrence_to_prismatic_pocket(occurrence, body_refs=issuer).to_dict()
                    == pocket.to_dict()
                )
                checked += 1
    assert checked == 25
