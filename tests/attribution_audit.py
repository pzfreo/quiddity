"""Shared F5b oracle for physical-family attribution fixtures."""

from __future__ import annotations

from collections.abc import Callable, Mapping, Sequence
from math import atan2, degrees, hypot
from typing import Any

from build123d import GeomType
from OCP.BRepAdaptor import BRepAdaptor_Surface

from quiddity._adjacency import FaceGraph
from quiddity._bevel import classify_bevel
from quiddity._candidates import FamilyId
from quiddity._claims import ClaimLedger
from quiddity._cylinder_substrate import analyse_cylinders
from quiddity._geometry import _coaxial_axis_lines
from quiddity.countersinks import cone_rims


def assert_ring_role(ledger, candidate, record) -> None:
    """Derive the exact planar ring faces independently from the serialized section."""

    axis = "xyz".index(record.axis)
    others = [index for index in range(3) if index != axis]
    run_length = getattr(record, "depth", getattr(record, "length", None))
    section = record.section

    def on_boundary(node) -> bool:
        if not ledger.graph.is_planar(node):
            return False
        normal = ledger.graph.normal(node)
        if normal is None or abs(normal[axis]) > 1e-6:
            return False
        bounds = ledger.graph.bounds(node)
        expected_low = record.at[axis] - run_length / 2
        expected_high = record.at[axis] + run_length / 2
        if (
            abs(bounds[axis][0] - expected_low) > 0.002
            or abs(bounds[axis][1] - expected_high) > 0.002
        ):
            return False
        center = ledger.graph.face(node).center()
        point = (
            (center.X, center.Y, center.Z)[others[0]],
            (center.X, center.Y, center.Z)[others[1]],
        )
        for at, start in enumerate(section):
            end = section[(at + 1) % len(section)]
            dx, dy = end[0] - start[0], end[1] - start[1]
            cross = abs(dx * (point[1] - start[1]) - dy * (point[0] - start[0]))
            dot = (point[0] - start[0]) * dx + (point[1] - start[1]) * dy
            length = hypot(dx, dy)
            if cross <= 0.002 * length and -0.002 <= dot <= length * length + 0.002:
                return True
        return False

    expected = frozenset(node for node in ledger.graph.nodes if on_boundary(node))
    assert expected
    assert len(expected) == record.sides == len(record.section)
    assert ledger.defining_of(candidate) == expected


def assert_turned_step_role(part, ledger, candidate, record) -> None:
    """Derive all cylindrical bands that establish one turned rung."""

    inventory = (*analyse_cylinders(part)[0], *analyse_cylinders(part)[1])
    axis_bands = [item for item in inventory if item["external"] and item["axis"] == record.axis]
    main = max(axis_bands, key=lambda item: item["diameter"])
    midpoint = 0.5 * (record.lo + record.hi)

    coaxial = [
        item
        for item in axis_bands
        if _coaxial_axis_lines(
            main["axis_xyz"],
            main["dir_xyz"],
            item["axis_xyz"],
            item["dir_xyz"],
            tol=1e-6,
        )
    ]
    eligible = [item for item in coaxial if item["s_lo"] <= midpoint <= item["s_hi"]]
    if not eligible:
        # ADR 0008 freezes the 0.7 mm allowance and its per-band half-width cap.
        eligible = [
            item
            for item in coaxial
            if item["s_lo"] - min(0.7, (item["s_hi"] - item["s_lo"]) / 2)
            <= midpoint
            <= item["s_hi"] + min(0.7, (item["s_hi"] - item["s_lo"]) / 2)
        ]
    widest = max(item["diameter"] for item in eligible)
    assert abs(widest - record.diameter) <= 1e-6

    def establishes(item) -> bool:
        return item in eligible and abs(item["diameter"] - widest) <= 1e-6

    expected = frozenset(
        ledger.graph.require_node(item["face"]) for item in inventory if establishes(item)
    )
    assert expected
    assert ledger.defining_of(candidate) == expected


def assert_chamfer_role(ledger, candidate, record) -> None:
    """Derive the exact bevel or cone that establishes a Chamfer."""

    defining = ledger.defining_of(candidate)
    assert len(defining) == 1
    (node,) = defining
    face = ledger.graph.face(node)
    center = face.center()
    assert tuple(round(value, 3) for value in (center.X, center.Y, center.Z)) == record.at
    axis = "xyz".index(record.axis)
    if record.turned:
        surface = BRepAdaptor_Surface(face.wrapped)
        assert face.geom_type == GeomType.CONE
        rims = cone_rims(face)
        assert rims is not None
        minor, major, _ = rims
        direction = surface.Cone().Axis().Direction()
        components = (direction.X(), direction.Y(), direction.Z())
        major_center = (major.arc_center.X, major.arc_center.Y, major.arc_center.Z)
        minor_center = (minor.arc_center.X, minor.arc_center.Y, minor.arc_center.Z)
        axial = abs(sum((major_center[at] - minor_center[at]) * components[at] for at in range(3)))
        radial = major.radius - minor.radius
        assert abs(abs(components[axis]) - 1.0) <= 1e-6
        assert (round(max(axial, radial), 3), round(min(axial, radial), 3)) == (
            record.leg1,
            record.leg2,
        )
        assert round(degrees(atan2(min(axial, radial), max(axial, radial))), 2) == (record.angle)
    else:
        edge_i, _normal, _span, leg_hi, leg_lo = classify_bevel(face)
        assert edge_i == axis
        assert (round(leg_hi, 3), round(leg_lo, 3)) == (record.leg1, record.leg2)
        assert round(degrees(atan2(leg_lo, leg_hi)), 2) == record.angle


def assert_angled_step_role(ledger, candidate, record) -> None:
    """Derive the oblique slant and all serialized dimensions from its face."""

    defining = ledger.defining_of(candidate)
    assert len(defining) == 1
    (node,) = defining
    face = ledger.graph.face(node)
    edge_i, _normal, span, leg_hi, leg_lo = classify_bevel(face)
    center = face.center()
    assert edge_i == "xyz".index(record.axis)
    assert (round(leg_hi, 3), round(leg_lo, 3)) == (record.leg1, record.leg2)
    assert round(degrees(atan2(leg_lo, leg_hi)), 2) == record.angle
    assert round(span[edge_i][1] - span[edge_i][0], 3) == record.length
    assert tuple(round(value, 3) for value in (center.X, center.Y, center.Z)) == record.at


def assert_groove_role(ledger, candidate, record) -> None:
    """Derive the one cylindrical floor band from the record dimensions."""

    axis = "xyz".index(record.axis)

    def establishes(node) -> bool:
        face = ledger.graph.face(node)
        if face.geom_type != GeomType.CYLINDER:
            return False
        bounds = ledger.graph.bounds(node)
        if abs((bounds[axis][1] - bounds[axis][0]) - record.width) > 1e-6:
            return False
        bounds = ledger.graph.bounds(node)
        center = tuple(0.5 * (low + high) for low, high in bounds)
        if any(abs(center[at] - record.at[at]) > 1e-6 for at in range(3)):
            return False
        cylinder = BRepAdaptor_Surface(face.wrapped).Cylinder()
        direction = cylinder.Axis().Direction()
        components = (direction.X(), direction.Y(), direction.Z())
        return (
            abs(abs(components[axis]) - 1.0) <= 1e-6
            and abs(2 * cylinder.Radius() - record.diameter) <= 1e-6
        )

    expected = frozenset(node for node in ledger.graph.nodes if establishes(node))
    assert len(expected) == 1
    assert ledger.defining_of(candidate) == expected


def attributed_run(
    part,
    family: FamilyId,
    recognise: Callable[..., Sequence],
    *,
    kwargs: Mapping[str, Any] | None = None,
):
    """Prove writer parity and the identity/provenance lifecycle for one real fixture."""

    call_kwargs = dict(kwargs or {})
    plain = tuple(recognise(part, **call_kwargs))
    ledger = ClaimLedger(FaceGraph(part))
    measured = tuple(recognise(part, ledger=ledger, **call_kwargs))
    assert [type(record) for record in measured] == [type(record) for record in plain]
    assert measured == plain
    assert [record.to_dict() for record in measured] == [record.to_dict() for record in plain]

    candidates = ledger.candidate_set(family).candidates
    assert len(candidates) == len(measured)
    for candidate, record in zip(candidates, measured, strict=True):
        assert candidate.family is family
        assert candidate.record is record
        defining = ledger.defining_of(candidate)
        assert defining
        assert ledger.graph.common_valid_solid(defining) is not None
        if family in {FamilyId.PRISMATIC_POCKETS, FamilyId.PASSAGES}:
            assert_ring_role(ledger, candidate, record)
        if family is FamilyId.TURNED_STEPS:
            assert_turned_step_role(part, ledger, candidate, record)
        if family is FamilyId.GROOVES:
            assert_groove_role(ledger, candidate, record)
        if family is FamilyId.CHAMFERS:
            assert_chamfer_role(ledger, candidate, record)
        if family is FamilyId.ANGLED_STEPS:
            assert_angled_step_role(ledger, candidate, record)
    return ledger, list(measured)


def unattributed_run(
    part,
    family: FamilyId,
    recognise: Callable[..., Sequence],
    *,
    kwargs: Mapping[str, Any] | None = None,
):
    """Prove a negative fixture issues neither output nor an orphan family Candidate."""

    ledger = ClaimLedger(FaceGraph(part))
    assert recognise(part, ledger=ledger, **dict(kwargs or {})) == []
    assert ledger.candidate_set(family).candidates == ()
    return ledger
