import json
from pathlib import Path

import pytest
from build123d import (
    Align,
    Box,
    BuildPart,
    BuildSketch,
    Compound,
    Plane,
    Polygon,
    Pos,
    Rot,
    SlotOverall,
    extrude,
)

from quiddity._adjacency import FaceGraph
from quiddity._candidates import FamilyId
from quiddity._claims import ClaimLedger
from quiddity._dispositions import Outcome, ReasonCode
from quiddity.edge_open_circular_recesses import (
    EdgeOpenCircularPocket,
    OpenCircularSection,
    OpenCircularSectionSegment,
    recognise_edge_open_circular_pockets,
)
from quiddity.result import _take_inventory
from tools._legacy_recognition import (
    build_raw_recognition_result,
)


def _open_circular_pocket(*, x: float = 16, y: float = 10, depth: float = 8):
    with BuildPart() as stock_builder:
        with BuildSketch(Plane.XY):
            Polygon((-30, -20), (30, -20), (30, 10), (20, 20), (-30, 20))
        extrude(amount=12)
    with BuildPart() as cutter_builder:
        with BuildSketch(Plane.XY.offset(12 - depth)):
            SlotOverall(30, 10)
        extrude(amount=depth + 2)
    return stock_builder.part - Pos(x, y, 0) * cutter_builder.part


def _section() -> OpenCircularSection:
    segments = (
        OpenCircularSectionSegment("arc", (3.0, 0.0), (4.0, 1.0), (3.0, 1.0), 1.0, 1.5707963),
        OpenCircularSectionSegment("line", (4.0, 1.0), (4.0, 5.0)),
        OpenCircularSectionSegment("arc", (4.0, 5.0), (2.0, 5.0), (3.0, 5.0), 1.0, 3.1415927),
        OpenCircularSectionSegment("line", (2.0, 5.0), (2.0, 2.0)),
    )
    return OpenCircularSection(segments, ((2.0, 2.0), (3.0, 0.0)))


def test_record_serializes_only_the_physical_chain_and_explicit_gap() -> None:
    record = EdgeOpenCircularPocket("z", (4.0, 12.0), 1, _section())

    assert json.loads(json.dumps(record.to_dict())) == {
        "axis": "z",
        "run_interval": [4.0, 12.0],
        "open_sign": 1,
        "section": {
            "segments": [
                {
                    "kind": "arc",
                    "start": [3.0, 0.0],
                    "end": [4.0, 1.0],
                    "center": [3.0, 1.0],
                    "radius": 1.0,
                    "sweep": 1.5707963,
                },
                {
                    "kind": "line",
                    "start": [4.0, 1.0],
                    "end": [4.0, 5.0],
                    "center": None,
                    "radius": None,
                    "sweep": None,
                },
                {
                    "kind": "arc",
                    "start": [4.0, 5.0],
                    "end": [2.0, 5.0],
                    "center": [3.0, 5.0],
                    "radius": 1.0,
                    "sweep": 3.1415927,
                },
                {
                    "kind": "line",
                    "start": [2.0, 5.0],
                    "end": [2.0, 2.0],
                    "center": None,
                    "radius": None,
                    "sweep": None,
                },
            ],
            "opening": [[2.0, 2.0], [3.0, 0.0]],
        },
    }


def test_public_section_refuses_contradictory_arc_geometry() -> None:
    segments = list(_section().segments)
    arc = segments[0]
    with pytest.raises(ValueError, match="arc sweep must connect"):
        OpenCircularSectionSegment("arc", arc.start, arc.end, arc.center, arc.radius, -arc.sweep)


def test_public_section_refuses_unequal_radii_and_missing_intact_end() -> None:
    segments = list(_section().segments)
    intact = segments[2]
    segments[2] = OpenCircularSectionSegment(
        "arc",
        (4.0, 5.0),
        (2.0, 5.0),
        (3.0, 5.0),
        1.0001,
        intact.sweep,
    )
    with pytest.raises(ValueError, match="one equal radius"):
        OpenCircularSection(tuple(segments), (segments[-1].end, segments[0].start))

    segments = list(_section().segments)
    segments[2] = OpenCircularSectionSegment(
        "arc", (4.0, 5.0), (3.0, 6.0), (3.0, 5.0), 1.0, 1.5707963
    )
    segments[3] = OpenCircularSectionSegment("line", (3.0, 6.0), (2.0, 2.0))
    with pytest.raises(ValueError, match="exactly one intact semicircle"):
        OpenCircularSection(tuple(segments), (segments[-1].end, segments[0].start))


def test_recognises_an_authored_partial_arc_without_fabricating_its_closure() -> None:
    (found,) = recognise_edge_open_circular_pockets(_open_circular_pocket())

    assert found.axis == "z"
    assert found.run_interval == (4.0, 12.0)
    assert found.open_sign == 1
    arcs = [segment for segment in found.section.segments if segment.kind == "arc"]
    assert len(arcs) == 2
    assert sorted(round(abs(segment.sweep or 0), 6) for segment in arcs) == [0.927295, 3.141593]
    assert found.section.opening == (
        found.section.segments[-1].end,
        found.section.segments[0].start,
    )


def test_authored_open_circular_payload_matches_independent_golden() -> None:
    expected = json.loads(
        (Path(__file__).parent / "edge_open_circular_pocket_expected.json").read_text()
    )
    actual = json.loads(
        json.dumps(
            [
                record.to_dict()
                for record in recognise_edge_open_circular_pockets(_open_circular_pocket())
            ]
        )
    )

    assert actual == expected


def test_accepted_open_circular_recess_projects_to_unified_contract() -> None:
    result = build_raw_recognition_result(_open_circular_pocket())

    projected = [
        record
        for record in result.section_recesses
        if record.classification.feature_kind == "edge_open_recess"
    ]
    assert len(result.edge_open_circular_pockets) == len(projected) == 1
    (record,) = projected
    assert record.classification.section_shape == "obround"
    assert record.geometry.profile.closure == "open"
    assert any(vertex.bulge != 0.0 for vertex in record.geometry.profile.boundary)


@pytest.mark.parametrize(("x", "y"), ((14, 12), (16, 10), (18, 8)))
def test_several_authored_interruption_shapes_are_supported(x: float, y: float) -> None:
    assert len(recognise_edge_open_circular_pockets(_open_circular_pocket(x=x, y=y))) == 1


def test_closed_through_and_wholly_erased_end_shapes_are_not_this_family() -> None:
    stock = Box(60, 40, 12, align=(Align.CENTER, Align.CENTER, Align.MIN))
    with BuildPart() as cutter_builder:
        with BuildSketch(Plane.XY.offset(4)):
            SlotOverall(30, 10)
        extrude(amount=10)
    assert recognise_edge_open_circular_pockets(stock - cutter_builder.part) == []

    through = stock - Pos(0, 0, -1) * extrude(Plane.XY * SlotOverall(30, 10), 14)
    assert recognise_edge_open_circular_pockets(through) == []


def test_axis_and_rigid_motion_covariance_preserve_the_physical_profile() -> None:
    part = _open_circular_pocket()
    (baseline,) = recognise_edge_open_circular_pockets(part)
    (rotated,) = recognise_edge_open_circular_pockets(Rot(90, 0, 0) * part)

    assert baseline.axis != rotated.axis
    assert baseline.run_interval[1] - baseline.run_interval[0] == pytest.approx(
        rotated.run_interval[1] - rotated.run_interval[0]
    )
    assert baseline.section == rotated.section


def test_equal_occurrences_on_separate_solids_keep_separate_ownership() -> None:
    first = _open_circular_pocket()
    part = Compound([first, Pos(100, 0, 0) * _open_circular_pocket()])
    ledger = ClaimLedger(FaceGraph(part))

    found = recognise_edge_open_circular_pockets(part, ledger=ledger)

    assert len(found) == 2
    assert len(ledger.claims) == 2
    assert all(len(claim.defining) == 4 for claim in ledger.claims)
    candidates = ledger.candidate_set_for(FamilyId.EDGE_OPEN_CIRCULAR_POCKETS, found)
    assert all(len(candidate.evidence.constituent) == 5 for candidate in candidates.candidates)
    assert all(
        ledger.graph.common_valid_solid(candidate.evidence.constituent) is not None
        for candidate in candidates.candidates
    )


def test_complete_open_circular_contract_supersedes_its_partial_pocket_fragment() -> None:
    product = _take_inventory(_open_circular_pocket())

    (open_pocket,) = product.reconciliation.for_family(FamilyId.EDGE_OPEN_CIRCULAR_POCKETS)
    (fragment,) = product.reconciliation.for_family(FamilyId.POCKETS)
    assert open_pocket.outcome is Outcome.ACCEPTED
    assert fragment.outcome is Outcome.REJECTED
    assert fragment.reason is ReasonCode.POCKET_SUPERSEDED_BY_EDGE_OPEN_CIRCULAR_POCKET
    assert fragment.related == (open_pocket.candidate,)
    assert len(product._legacy_result.edge_open_circular_pockets) == 1
    assert product._legacy_result.pockets == ()
