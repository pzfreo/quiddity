"""ADR 0002 recogniser-contract tests, ported from the pinned Draftwright baseline.

Enforces the uniform contract mechanically (epic #584 WP3):

- **Immutable records** — every recogniser returns frozen dataclasses.
- **Uniform serialization** — each record has ``.to_dict()`` (the :class:`Record`
  mixin) that yields a *JSON-serializable* nested dict. This is the invariant with
  teeth: a leaked build123d / OCP object would make ``json.dumps`` raise, so the
  test proves the "geometry-only records, no build123d type leaks out" rule.
- **Signature shape** — a part-based recogniser takes ``part`` then keyword-only
  args; a derived recogniser takes a single positional inventory.
"""

from __future__ import annotations

import dataclasses
import inspect
import json
from math import cos, pi, sin

import pytest
from build123d import (
    Align,
    Axis,
    Box,
    BuildPart,
    BuildSketch,
    Cylinder,
    Plane,
    Polygon,
    Pos,
    RegularPolygon,
    Rot,
    chamfer,
    extrude,
    fillet,
)

from quiddity import (
    BoltCircle,
    BossRecord,
    Chamfer,
    CounterSink,
    DoubleDBore,
    FaceEdges,
    FaceLevel,
    Fillet,
    Flat,
    Groove,
    HoleRecord,
    LinearArray,
    PairedRampStep,
    Plate,
    PolygonalBoss,
    PolygonalStock,
    RaisedPad,
    RectGrid,
    RepeatingRadialProfile,
    Slot,
    SlotArray,
    SlotGrid,
    StepShoulder,
    ThroughStep,
    TurnedStep,
    analyse_cylinders,
    project_step_shoulders,
    recognise_angled_steps,
    recognise_bosses,
    recognise_chamfers,
    recognise_countersinks,
    recognise_double_d_bores,
    recognise_face_levels,
    recognise_fillets,
    recognise_flats,
    recognise_grooves,
    recognise_hole_patterns,
    recognise_holes,
    recognise_paired_ramp_steps,
    recognise_plates,
    recognise_polygonal_bosses,
    recognise_polygonal_stock,
    recognise_rectangular_pads,
    recognise_repeating_radial_profiles,
    recognise_risers,
    recognise_slot_patterns,
    recognise_slots,
    recognise_through_steps,
    recognise_turned_steps,
)
from quiddity._record import Record
from tools._legacy_recognition import (
    Channel,
    Pocket,
    PocketArray,
    PocketGrid,
    recognise_channels,
    recognise_passages,
    recognise_pocket_patterns,
    recognise_pockets,
)

# Every record class a recogniser returns. The coverage test asserts the drive-parts
# below actually emit one of each — so a record type that silently stops being produced
# (or a new recogniser added without contract coverage) fails the test loudly, rather
# than slipping through a bare count. (CounterBore/HoleSpec/TurnedProfile are sub-records
# / aggregates, not recogniser returns, so they are exercised nested, not listed here.)
_EXPECTED_RECORD_TYPES = {
    HoleRecord,
    DoubleDBore,
    CounterSink,
    BossRecord,
    BoltCircle,
    LinearArray,
    RectGrid,
    Chamfer,
    Channel,
    Fillet,
    Flat,
    Groove,
    Slot,
    SlotArray,
    SlotGrid,
    Pocket,
    PocketArray,
    PocketGrid,
    PolygonalBoss,
    PolygonalStock,
    RaisedPad,
    Plate,
    FaceLevel,
    StepShoulder,
    TurnedStep,
    RepeatingRadialProfile,
    PairedRampStep,
    ThroughStep,
}


def _csk_plate():
    from build123d import Cone

    plate = Box(90, 60, 12)
    for x, y in [(-30, -15), (5, 12), (30, -8)]:
        plate -= Pos(x, y, 0) * Cylinder(3, 12)
        plate -= Pos(x, y, 4) * Cone(3, 7, 4)
    return plate


def _stepped():
    return Box(80, 40, 10) + Pos(-20, 0, 10) * Box(40, 40, 12)


def _double_d_plate():
    centre = (Align.CENTER, Align.CENTER, Align.CENTER)
    cutter = Cylinder(5, 20, align=centre) & Box(7.2, 20, 30, align=centre)
    return Box(30, 30, 10, align=centre) - cutter


def _turned_shaft():
    return Rot(0, 90, 0) * (Cylinder(10, 30) + Pos(0, 0, 20) * Cylinder(6, 10))


def _linear_array_plate():
    part = Box(120, 40, 10)
    for i in range(5):
        part -= Pos(-40 + i * 20, 0, 0) * Cylinder(3, 10)
    return part


def _grid_plate(nx=3, ny=3, px=25, py=25):
    part = Box(px * (nx + 1), py * (ny + 1), 10)
    for i in range(nx):
        for j in range(ny):
            part -= Pos((i - (nx - 1) / 2) * px, (j - (ny - 1) / 2) * py, 0) * Cylinder(3, 10)
    return part


def _pocket_array_plate():
    part = Box(30, 150, 20)
    for cy in (-45, -15, 15, 45):  # four identical blind pockets on one Y centreline, pitch 30
        part -= Pos(0, cy, 7) * Box(10, 12, 6)  # floored (opening +Z) → a Pocket, not a Slot
    return part


def _pocket_grid_plate():
    part = Box(140, 110, 20)
    for i in range(2):  # 2×3 lattice of identical blind pockets (rect_grid needs n>=6)
        for j in range(3):
            part -= Pos((i - 0.5) * 40, (j - 1) * 30, 7) * Box(8, 10, 6)
    return part


def _slot_array_plate():
    part = Box(60, 200, 20)
    for cy in (-45, -15, 15, 45):  # four identical THROUGH slots on one Y centreline, pitch 30
        part -= Pos(0, cy, 0) * Box(30, 8, 20)  # cutter spans the full Z → a Slot, not a Pocket
    return part


def _slot_grid_plate():
    part = Box(180, 130, 20)
    for i in range(2):  # 2×3 lattice of identical through slots (rect_grid needs n>=6)
        for j in range(3):
            part -= Pos((i - 0.5) * 44, (j - 1) * 34, 0) * Box(24, 8, 20)
    return part


def _bolt_circle_plate(n=6, r=30):
    from math import cos, radians, sin

    part = Box(100, 100, 12)
    for i in range(n):
        a = radians(360 / n * i + 15.0)
        part -= Pos(r * cos(a), r * sin(a), 0) * Cylinder(4, 12)
    return part


def _passaged_block():
    """A hexagonal void running through the block, open at both ends."""
    with BuildPart() as bore:
        with BuildSketch(Plane.XY):
            RegularPolygon(9, 6)
        extrude(amount=40, both=True)
    return Box(60, 40, 20) - bore.part


def _angled_stepped_box():
    """A 45 degrees wedge stopped inside the block, so a triangular flat closes its blind end."""
    return Box(60, 40, 12) - Pos(-20, 20, 6) * Rot(45, 0, 0) * Box(30, 5.657, 5.657)


def _paired_ramp_side_cut():
    cutter = Pos(20, 20, 0) * extrude(Plane.XZ * Polygon((0, -8), (0, 8), (-10, 0)), 25)
    return Box(40, 40, 30) - cutter


def _rectangular_through_step():
    return Box(40, 30, 20) - Pos(15, 10, 0) * Box(20, 20, 30)


def _chamfered_box():
    box = Box(30, 30, 30)
    edge = box.edges().filter_by(Axis.Z).sort_by(Axis.X)[-1]
    return chamfer(edge, 3)


def _filleted_box():
    box = Box(30, 30, 30)
    edge = box.edges().filter_by(Axis.Z).sort_by(Axis.X)[-1]
    return fillet(edge, 3)


def _l_bracket():
    return Box(80, 40, 8) + Pos(-36, 0, 24) * Box(8, 40, 40)


def _polygonal_boss_plate():
    from build123d import RegularPolygon, extrude

    return Box(100, 80, 10) + Pos(0, 0, 5) * extrude(RegularPolygon(20, 6), 30)


def _polygonal_stock():
    from build123d import RegularPolygon

    return extrude(RegularPolygon(20, 6), 30)


def _raised_pad_plate():
    return Box(80, 60, 10) + Pos(0, 0, 7) * Box(30, 20, 4)


def _repeating_profile():
    points = []
    for index in range(16):
        angle = 2 * pi * index / 16
        radius = 20 if index % 2 == 0 else 16
        points.append((radius * cos(angle), radius * sin(angle)))
    return extrude(Polygon(*points), 10)


def _records_from_recognisers():
    """(name, record) pairs across every recogniser, on parts that actually trigger them."""
    csk = _csk_plate()
    stepped = _stepped()
    holes = recognise_holes(csk, csinks=recognise_countersinks(csk))
    slotted = Box(60, 40, 20) - Pos(0, 0, 0) * Box(30, 8, 20)
    pocketed = Box(60, 40, 20) - Pos(0, 0, 7) * Box(30, 18, 6)
    channel = (
        Box(50, 50, 12)
        + Pos(0, -18.75, 15) * Box(50, 12.5, 18)
        + Pos(0, 18.75, 15) * Box(50, 12.5, 18)
    )
    dshaft = Cylinder(10, 30) - Pos(10, 0, 0) * Box(10, 40, 40)  # round stock with one flat
    grooved = Cylinder(10, 40) - (Cylinder(10, 4) - Cylinder(8, 4))  # round stock with one groove
    levels = [f.z for f in recognise_face_levels(stepped)]

    out: list[tuple[str, object]] = []
    for name, recs in [
        ("recognise_holes", holes),
        ("recognise_countersinks", recognise_countersinks(csk)),
        ("recognise_double_d_bores", recognise_double_d_bores(_double_d_plate())),
        ("recognise_bosses", recognise_bosses(Cylinder(10, 20))),
        ("recognise_polygonal_bosses", recognise_polygonal_bosses(_polygonal_boss_plate())),
        ("recognise_polygonal_stock", recognise_polygonal_stock(_polygonal_stock())),
        ("recognise_rectangular_pads", recognise_rectangular_pads(_raised_pad_plate())),
        ("hole_patterns:bolt", recognise_hole_patterns(recognise_holes(_bolt_circle_plate()))),
        ("hole_patterns:linear", recognise_hole_patterns(recognise_holes(_linear_array_plate()))),
        ("hole_patterns:grid", recognise_hole_patterns(recognise_holes(_grid_plate()))),
        ("recognise_angled_steps", recognise_angled_steps(_angled_stepped_box())),
        ("recognise_paired_ramp_steps", recognise_paired_ramp_steps(_paired_ramp_side_cut())),
        ("recognise_through_steps", recognise_through_steps(_rectangular_through_step())),
        ("recognise_passages", recognise_passages(_passaged_block())),
        ("recognise_chamfers", recognise_chamfers(_chamfered_box())),
        ("recognise_channels", recognise_channels(channel)),
        ("recognise_fillets", recognise_fillets(_filleted_box())),
        ("recognise_slots", recognise_slots(slotted)),
        ("recognise_pockets", recognise_pockets(pocketed)),
        (
            "pocket_patterns:linear",
            recognise_pocket_patterns(recognise_pockets(_pocket_array_plate())),
        ),
        (
            "pocket_patterns:grid",
            recognise_pocket_patterns(recognise_pockets(_pocket_grid_plate())),
        ),
        (
            "slot_patterns:linear",
            recognise_slot_patterns(recognise_slots(_slot_array_plate())),
        ),
        (
            "slot_patterns:grid",
            recognise_slot_patterns(recognise_slots(_slot_grid_plate())),
        ),
        ("recognise_flats", recognise_flats(dshaft)),
        ("recognise_grooves", recognise_grooves(grooved)),
        ("recognise_plates", recognise_plates(_l_bracket())),
        ("recognise_face_levels", recognise_face_levels(stepped)),
        ("recognise_risers", recognise_risers(stepped)),
        (
            "recognise_repeating_radial_profiles",
            recognise_repeating_radial_profiles(_repeating_profile()),
        ),
        # `StepShoulder` stopped being a recogniser return in #1025 — it is now what
        # `project_step_shoulders` derives from riser evidence. It stays in this roster
        # because the record contract (frozen, JSON-serialisable, no leaked build123d object)
        # binds a projection's output exactly as it binds a recogniser's; dropping it would
        # retire the only coverage of that type on the grounds that it moved.
        (
            "project_step_shoulders",
            project_step_shoulders(recognise_risers(stepped), levels=levels),
        ),
        ("recognise_turned_steps", recognise_turned_steps(_turned_shaft())),
    ]:
        for r in recs:
            out.append((name, r))
    return out


def test_records_are_frozen_and_json_serializable():
    """Every record from every recogniser is a frozen, JSON-serializable ``Record``."""
    records = _records_from_recognisers()

    for name, rec in records:
        assert isinstance(rec, Record), f"{name}: {type(rec).__name__} is not a Record"
        assert dataclasses.is_dataclass(rec) and rec.__dataclass_params__.frozen, (
            f"{name}: {type(rec).__name__} must be a frozen dataclass"
        )
        d = rec.to_dict()
        assert isinstance(d, dict)
        # The teeth: a leaked build123d/OCP object makes this raise.
        json.dumps(d)


def test_every_record_type_is_actually_exercised():
    """The drive-parts must emit *each* recogniser record type — no silent under-coverage.

    Guards against the count-only trap: a record type whose drive-part stops producing it
    (or a new record added without coverage) fails here instead of passing on a bare tally.
    """
    seen = {type(rec) for _, rec in _records_from_recognisers()}
    missing = _EXPECTED_RECORD_TYPES - seen
    assert not missing, f"contract test never exercised these record types: {missing}"


def test_frozen_records_reject_mutation():
    """A record is immutable — assigning a field raises (frozen dataclass)."""
    hole = recognise_holes(_csk_plate())[0]
    with pytest.raises(dataclasses.FrozenInstanceError):
        hole.diameter = 99.0  # type: ignore[misc]


def test_part_based_recognisers_are_keyword_only_after_part():
    """Part-based recognisers take ``part`` then keyword-only args (ADR 0013)."""
    for fn in (
        recognise_holes,
        recognise_bosses,
        recognise_polygonal_bosses,
        recognise_polygonal_stock,
        recognise_rectangular_pads,
        recognise_countersinks,
        recognise_double_d_bores,
        recognise_angled_steps,
        recognise_paired_ramp_steps,
        recognise_through_steps,
        recognise_passages,
        recognise_chamfers,
        recognise_channels,
        recognise_fillets,
        recognise_slots,
        recognise_pockets,
        recognise_flats,
        recognise_grooves,
        recognise_plates,
        recognise_face_levels,
        recognise_risers,
        recognise_repeating_radial_profiles,
        recognise_turned_steps,
    ):
        params = list(inspect.signature(fn).parameters.values())
        assert params[0].name == "part"
        assert params[0].kind in (
            inspect.Parameter.POSITIONAL_ONLY,
            inspect.Parameter.POSITIONAL_OR_KEYWORD,
        )
        for p in params[1:]:
            assert p.kind == inspect.Parameter.KEYWORD_ONLY, (
                f"{fn.__name__}: '{p.name}' must be keyword-only (injected dep / tuning)"
            )


def test_cylinder_substrate_is_injectable():
    """#703: the three turned-stock recognisers accept a precomputed
    ``analyse_cylinders`` result (``cyls=``) and return identical records to a
    self-scan — the caller owns the one scan (ADR 0002 / ADR 0008),
    mirroring ``recognise_holes``/``recognise_bosses``."""
    dshaft = Cylinder(10, 30) - Pos(10, 0, 0) * Box(10, 40, 40)
    grooved = Cylinder(10, 40) - (Cylinder(10, 4) - Cylinder(8, 4))
    for fn, part in (
        (recognise_turned_steps, _turned_shaft()),
        (recognise_grooves, grooved),
        (recognise_flats, dshaft),
    ):
        records = fn(part)
        assert records, f"{fn.__name__}: fixture no longer triggers the recogniser"
        assert fn(part, cyls=analyse_cylinders(part)) == records


def test_every_recogniser_taking_face_edges_actually_consults_it():
    """ADR 0002: an injected dependency must be *used*, not merely accepted.

    ``face_edges=`` is a performance memo, so a recogniser that accepted it and dropped it on
    the floor would return byte-identical records and pass every equivalence test in this
    file — while quietly giving back the ~14% the shared memo buys a census. Nothing else
    here can catch that, so this spies on the memo and demands each recogniser ask it
    something.

    Each recogniser gets geometry it actually engages with. That matters more than it looks:
    ``recognise_bosses`` returns before touching a face when the part has no external
    cylinder, so pointing this test at a convenient prismatic block would have it read zero
    asks and "fail" against correct code — or, with the assertion inverted, pass against a
    dropped parameter.
    """

    prismatic = Box(60, 40, 12) - Pos(-18, 0, 0) * Cylinder(4, 12) - Pos(15, 0, 0) * Box(24, 8, 12)
    prismatic = chamfer(prismatic.edges().filter_by(Axis.Z).group_by(Axis.X)[0], 1.5)
    prismatic = fillet(prismatic.edges().filter_by(Axis.Z).group_by(Axis.X)[-1], 2.0)
    round_stock = Cylinder(10, 30) - Pos(10, 0, 0) * Box(10, 40, 40)
    bossed = Box(40, 40, 10) + Pos(0, 0, 10) * Cylinder(6, 10)

    class _Spy(FaceEdges):
        def __init__(self) -> None:
            super().__init__()
            self.asked = 0

        def of(self, face):
            self.asked += 1
            return super().of(face)

    # The flag marks the recognisers gated on a cylinder substrate: they return before
    # touching a face when the scan comes back empty, so for them "found nothing" and
    # "ignored the memo" look identical from here. Requiring a record separates the two, and
    # keeps a drifting fixture reporting as a drifting fixture rather than as broken source.
    # The rest legitimately consult the memo while finding nothing -- `recognise_pockets` and
    # `recognise_channels` walk every planar face of this part and report zero recesses --
    # so demanding a record of them would be wrong, not merely stricter.
    cases = (
        (recognise_angled_steps, _angled_stepped_box(), False),
        (recognise_passages, _passaged_block(), False),
        (recognise_chamfers, prismatic, False),
        (recognise_fillets, prismatic, False),
        (recognise_holes, prismatic, True),
        (recognise_slots, prismatic, False),
        (recognise_pockets, prismatic, False),
        (recognise_channels, prismatic, False),
        (recognise_bosses, bossed, True),
        (recognise_flats, round_stock, True),
    )
    for recognise, part, substrate_gated in cases:
        spy = _Spy()
        records = recognise(part, face_edges=spy)
        if substrate_gated:
            assert records, f"{recognise.__name__}: fixture no longer reaches the face scan"
        assert spy.asked, f"{recognise.__name__} accepts face_edges= but never consults it"


def test_derived_recogniser_takes_single_positional_inventory():
    """A derived recogniser (``recognise_hole_patterns``) takes one positional arg."""
    params = list(inspect.signature(recognise_hole_patterns).parameters.values())
    assert params[0].name == "holes"
    assert params[0].kind == inspect.Parameter.POSITIONAL_OR_KEYWORD


def test_riser_evidence_is_constructible_without_a_scan_tolerance():
    """``RiserEvidence`` can be hand-built, which is what keeps this a patch release.

    The field records the tolerance :func:`recognise_risers` resolved for a part, so ADR 0008
    briefly made it required. That broke every direct construction to buy nothing: a record
    built by hand was never scanned, so no value is more truthful than another, and the
    recogniser never reads the default because it always passes its own.

    The projection is included because it is the one consumer of the field — a default that
    constructs but does not project would be no use.
    """

    from quiddity import RiserEvidence, project_step_shoulders

    riser = RiserEvidence(
        vertical=True,
        axis="x",
        positions=(10.0,),
        other_axis="y",
        other_positions=(),
        z_lo=5.0,
        z_hi=15.0,
        lo_at_envelope=False,
        hi_at_envelope=False,
    )

    assert riser.tol == 0.5
    assert riser.body_levels is None
    assert project_step_shoulders([riser], levels=[5.0]) == [StepShoulder("x", 10.0)]
