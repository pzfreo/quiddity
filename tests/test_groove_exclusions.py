# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

"""What ``recognise_grooves`` refuses, and why.

Epic 0001 finding 4 asked for negative and boundary coverage where the geometry is hardest, and
``docs/capabilities.md`` already lists the excluded classes: internal grooves, end reliefs without
two larger neighbours, and non-turned recesses. The positive case is covered by the turned golden;
every branch exercised below is a ``continue`` that the golden never reaches.

Each test is a shape a looser recogniser would call a groove. The point of the module's gates is
that a reduced band is not sufficient evidence — a groove is a narrow channel cut into *uniform*
stock, and each gate removes one way of failing that.
"""

from __future__ import annotations

from attribution_audit import assert_groove_role, attributed_run, unattributed_run
from build123d import Axis, Cone, Cylinder, GeomType, Pos, Rot, chamfer, fillet

from quiddity import recognise_grooves
from quiddity._candidates import FamilyId


def _shaft_with_reduced_band(*, wall_r=15.0, band_r=12.0, lo_h=20.0, band_h=5.0, hi_h=20.0):
    """A shaft of three coaxial bands: wall, reduced band, wall."""

    shaft = Cylinder(wall_r, lo_h)
    shaft += Pos(0, 0, (lo_h + band_h) / 2) * Cylinder(band_r, band_h)
    shaft += Pos(0, 0, (lo_h + band_h) / 2 + (band_h + hi_h) / 2) * Cylinder(wall_r, hi_h)
    return shaft


def test_a_narrow_band_between_two_equal_walls_is_a_groove():
    """The positive control. Without it the exclusions below prove nothing."""

    part = _shaft_with_reduced_band()
    ledger, (groove,) = attributed_run(part, FamilyId.GROOVES, recognise_grooves)

    assert groove.diameter == 24.0
    assert groove.width == 5.0
    (candidate,) = ledger.candidate_set(FamilyId.GROOVES).candidates
    assert_groove_role(ledger, candidate, groove)


def test_a_monotonic_step_down_is_a_shoulder_not_a_groove():
    """A groove is a strict local OD *minimum* — the OD must step down into it and up out.

    A descending staircase satisfies one side only. Calling it a groove would report a shoulder
    as an annular channel and dimension it as a recess that is not there.
    """

    shaft = Cylinder(15, 20)
    shaft += Pos(0, 0, 12.5) * Cylinder(12, 5)
    shaft += Pos(0, 0, 22.5) * Cylinder(9, 15)

    assert recognise_grooves(shaft) == []


def test_a_band_between_walls_of_different_diameters_is_a_stepped_profile():
    """Cut into uniform stock means the two walls return to the *same* OD.

    Unequal neighbours are a stepped shaft that happens to dip, not round bar with a channel in
    it, and its ``diameter`` would not describe a real ring groove.
    """

    shaft = Cylinder(15, 20)
    shaft += Pos(0, 0, 12.5) * Cylinder(12, 5)
    shaft += Pos(0, 0, 22.5) * Cylinder(14, 15)

    assert recognise_grooves(shaft) == []


def test_a_band_as_wide_as_its_walls_is_a_staircase_step():
    """A groove is *narrow* relative to the stock it interrupts.

    Without this gate an alternating fine-step profile reports every reduced segment as a
    groove, which is how a turned staircase becomes a row of phantom channels.
    """

    shaft = _shaft_with_reduced_band(lo_h=6.0, band_h=20.0, hi_h=6.0)

    assert recognise_grooves(shaft) == []


def test_a_reduced_band_at_the_end_of_the_shaft_is_a_relief_not_a_groove():
    """An end relief has one wall, not two. It is an excluded class in `docs/capabilities.md`."""

    shaft = Cylinder(15, 30)
    shaft += Pos(0, 0, 17.5) * Cylinder(12, 5)

    assert recognise_grooves(shaft) == []


def test_bands_separated_by_a_gap_are_not_one_another_s_walls():
    """Contiguity is evidence: a band floating clear of its neighbours is a separate feature.

    Two coaxial discs with air between them are not a grooved shaft, and pairing them would
    measure a channel across a void.
    """

    shaft = Cylinder(15, 20)
    shaft += Pos(0, 0, 20) * Cylinder(12, 5)
    shaft += Pos(0, 0, 40) * Cylinder(15, 20)

    assert recognise_grooves(shaft) == []


def test_a_band_wider_than_the_wall_before_it_is_not_a_local_minimum():
    """The OD must step *down* into the band. Here it steps up, then down.

    The mirror of the monotonic-step case: a groove needs both sides to fail an ascent, and this
    profile fails only the second. Covered separately because the two gates are separate reads
    of the neighbour, and a copy-paste slip between them would leave one direction unchecked.
    """

    shaft = Cylinder(6, 20)
    shaft += Pos(0, 0, 12.5) * Cylinder(7.5, 5)
    shaft += Pos(0, 0, 22.5) * Cylinder(4.5, 15)

    assert recognise_grooves(shaft) == []


def _chamfered_groove():
    shaft = Cylinder(15, 20)
    shaft += Pos(0, 0, 11.5) * Cone(15, 12, 3)
    shaft += Pos(0, 0, 15.5) * Cylinder(12, 5)
    shaft += Pos(0, 0, 19.5) * Cone(12, 15, 3)
    shaft += Pos(0, 0, 31.0) * Cylinder(15, 20)
    return shaft


def test_a_groove_with_lead_in_chamfers_is_recognised():
    """A ring groove whose corners are chamfered rather than sharp.

    This used to be excluded: the cone faces sit between the cylindrical bands, so the walls
    were no longer the floor band's immediate neighbours and every such groove was dropped
    silently. It is the common manufactured shape — the sharp-cornered groove is the textbook
    drawing — so the cones are now read as the join they are.

    ``width`` stays the **flat floor**, not the full opening including the lead-ins. That is
    what an O-ring or circlip seat is dimensioned by, and it keeps the field meaning identical
    for a groove that gained a chamfer.
    """

    part = _chamfered_groove()
    ledger, grooves = attributed_run(part, FamilyId.GROOVES, recognise_grooves)

    assert len(grooves) == 1
    assert (grooves[0].diameter, grooves[0].width) == (24.0, 5.0)
    (candidate,) = ledger.candidate_set(FamilyId.GROOVES).candidates
    assert_groove_role(ledger, candidate, grooves[0])


def test_a_lead_in_cone_must_land_on_both_bands():
    """The cone is the join only when it reaches both rims at their own diameters.

    Without that, "allow a cone between the bands" would let any taper in the neighbourhood
    fuse two unrelated bands into a groove. Here the taper runs from the shaft OD down to a
    diameter the floor band does not have, so it joins nothing and there is no groove.
    """

    shaft = Cylinder(15, 20)
    shaft += Pos(0, 0, 11.5) * Cone(15, 9, 3)  # tapers past the floor diameter, to 18 mm
    shaft += Pos(0, 0, 15.5) * Cylinder(12, 5)
    shaft += Pos(0, 0, 19.5) * Cone(12, 15, 3)
    shaft += Pos(0, 0, 31.0) * Cylinder(15, 20)

    assert recognise_grooves(shaft) == []


def test_the_lead_in_at_the_far_end_must_land_on_both_bands_too():
    """The mirror of the case above, with the bad taper on the *upper* join.

    The two joins are separate reads of the neighbour — as with the two local-minimum gates —
    so a copy-paste slip between them would leave one direction unchecked, and a groove would
    be reported off a taper that only half fits.
    """

    shaft = Cylinder(15, 20)
    shaft += Pos(0, 0, 11.5) * Cone(15, 12, 3)
    shaft += Pos(0, 0, 15.5) * Cylinder(12, 5)
    shaft += Pos(0, 0, 19.5) * Cone(9, 15, 3)  # starts undercut of the floor, at 18 mm
    shaft += Pos(0, 0, 31.0) * Cylinder(15, 20)

    assert recognise_grooves(shaft) == []


def test_cones_elsewhere_on_the_part_are_not_lead_ins():
    """A real part carries cones that have nothing to do with the groove.

    Two of them here: a V-bottomed hole down the shaft axis, whose conical face runs to an apex
    and so has a single rim to offer; and the end chamfer of a cross boss, which has two rims
    but about the wrong axis. Neither may fuse a pair of bands, and the groove between the
    genuine lead-ins must still read exactly once.
    """

    part = _chamfered_groove()
    part -= Pos(0, 0, 37) * Cone(0, 4, 8)
    part += Rot(0, 90, 0) * Cylinder(8, 60)
    part = chamfer(part.edges().filter_by(GeomType.CIRCLE).group_by(Axis.X)[-1], 1.0)

    grooves = recognise_grooves(part)

    assert len(grooves) == 1
    assert (grooves[0].diameter, grooves[0].width) == (24.0, 5.0)


def test_a_radiused_lead_in_joins_the_groove_bands():
    """A torus/annular-wall chain is the rounded counterpart of a conical lead-in."""

    plain = Cylinder(15, 20) + Pos(0, 0, 12.5) * Cylinder(12, 5)
    plain += Pos(0, 0, 22.5) * Cylinder(15, 20)
    radiused = fillet(plain.edges().filter_by(GeomType.CIRCLE).group_by(Axis.Z)[1], 1.0)

    ledger, grooves = attributed_run(radiused, FamilyId.GROOVES, recognise_grooves)
    assert [(groove.diameter, groove.width) for groove in grooves] == [(24.0, 1.5)]
    (candidate,) = ledger.candidate_set(FamilyId.GROOVES).candidates
    assert_groove_role(ledger, candidate, grooves[0])


def test_a_chamfered_groove_reads_the_same_at_any_scale():
    """The lead-in read is gated by a proportional tolerance, per ADR 0008.

    ``tests/test_scale_invariance.py`` sweeps the golden corpus, and no golden carries a
    chamfered lead-in, so this path would not be swept there. It is exactly the shape of fault
    that erased small features on large parts once already: an absolute millimetre gap here
    would find the groove on 30 mm bar and lose it on 3 m bar.
    """

    for factor in (0.05, 5.0, 100.0):
        ledger, (groove,) = attributed_run(
            _chamfered_groove().scale(factor), FamilyId.GROOVES, recognise_grooves
        )
        assert groove.diameter == round(24.0 * factor, 3), f"at {factor}x"
        (candidate,) = ledger.candidate_set(FamilyId.GROOVES).candidates
        assert_groove_role(ledger, candidate, groove)


def test_a_plain_cylinder_has_no_groove():
    unattributed_run(Cylinder(15, 40), FamilyId.GROOVES, recognise_grooves)
