# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Which faces a pocket was built from, and why the two paths claim different things.

The through-slot family already claims (`tests/test_slot_claims.py`); this is its blind
counterpart, and the interesting part is that the answer is not simply "the same, plus a
floor". A pocket is found two ways and the floor's role differs between them:

- **From opposed walls**, the floor reaches the candidate through `_end_capped`, which only
  asks whether *something* caps the footprint. The record's depth is the two walls' own
  overlap on the depth axis, not the floor's position. So the floor is consulted and not
  consumed, exactly as the through-slot's floor test is, and claiming it would have every
  pocket contest whatever else owns that face.
- **From a corner notch**, the loop is *over* floors and the notch's whole footprint is read
  off the one it finds. There the floor established the record as literally as the walls did.

Asserted against the geometry rather than against a captured count: every claimed face is
proved to be a wall of that pocket, or its floor on the path that should claim one.
"""

from __future__ import annotations

from build123d import Box, Cylinder, Pos

from quiddity._adjacency import FaceGraph
from quiddity._claims import ClaimLedger
from tools._legacy_recognition import namespace

r = namespace()

_AXES = {"x": 0, "y": 1, "z": 2}


def claimed(part):
    """Recognise with a ledger, and prove doing so changed nothing about the result."""

    ledger = ClaimLedger(FaceGraph(part))
    with_ledger = r.recognise_pockets(part, ledger=ledger)
    assert with_ledger == r.recognise_pockets(part), "claiming changed what was recognised"
    return ledger, with_ledger


def _bounds(ledger, node):
    return ledger.graph.bounds(node)


def test_a_pocket_from_opposed_walls_claims_the_walls_and_not_the_floor():
    """Two faces, both walls on the width axis, and the floor deliberately absent.

    The floor is what makes this a pocket rather than a slot -- and it is still not what the
    pocket measures. Depth comes from how far the walls run down the depth axis, which is why
    a pocket deeper than it is long is dimensioned correctly without the floor being read.
    """

    part = Box(60, 40, 12) - Pos(0, 0, 4) * Box(20, 12, 8)
    ledger, pockets = claimed(part)
    (pocket,) = pockets
    (claim,) = ledger.claims

    assert len(claim.defining) == 2, "the two walls, and nothing the candidate merely consulted"
    k = _AXES[pocket.width_axis]
    for node in claim.defining:
        lo, hi = _bounds(ledger, node)[k]
        assert abs(hi - lo) < 1e-6, "a claimed face is not flat on the width axis"
        normal = ledger.graph.normal(node)
        assert normal is not None and abs(normal[k]) > 0.99, "a claimed face is not a wall"
    # And the floor really is there -- otherwise this is a slot and the assertion above is
    # about a face that nothing had to decline to claim. The 8 mm cut is placed so it stops
    # 6 mm into the block: `d_lo` is the floor, `d_hi` the opening.
    assert (pocket.d_lo, pocket.d_hi, pocket.depth) == (0.0, 6.0, 6.0)


def test_a_corner_notch_claims_its_floor_as_well_as_its_two_walls():
    """The path that reads the footprint off the floor claims it, and the count says so.

    Three faces rather than two, and the extra one is horizontal where the other two are
    vertical. That asymmetry with the test above is the whole point: the rule is what the
    record was *established by*, not a fixed shape per record type.
    """

    part = Box(60, 40, 12) - Pos(25, 15, 4) * Box(20, 20, 8)
    ledger, pockets = claimed(part)
    (pocket,) = pockets
    (claim,) = ledger.claims

    assert pocket.edge_anchored, "this fixture must reach the corner-notch path"
    assert len(claim.defining) == 3

    normals = [ledger.graph.normal(node) for node in claim.defining]
    horizontal = [n for n in normals if n is not None and abs(n[2]) > 0.99]
    assert len(horizontal) == 1, "exactly one claimed face is the floor"


def test_a_pocket_recovered_from_round_ends_claims_both_caps():
    """The blind obround path owns both cylindrical faces that establish it."""

    end = Cylinder(5, 8)
    stub = Box(6, 10, 8) + Pos(-3, 0, 0) * end + Pos(3, 0, 0) * end
    part = Box(60, 40, 12) - Pos(0, 0, 4) * stub
    ledger, pockets = claimed(part)

    assert len(pockets) == 1 and not pockets[0].edge_anchored
    assert pockets[0].width == 10.0, "the round ends are what set the width"
    (claim,) = ledger.claims
    assert len(claim.defining) == 2
    assert all(not ledger.graph.is_planar(node) for node in claim.defining)


def test_the_aggregate_writes_pocket_and_slot_claims_through_one_writer(monkeypatch):
    """A split evidence issuer is the trap, so the aggregate must not create one.

    No rule reads pocket claims today. That is the reason to write them: a rule added later
    would read a ledger with no pocket claim in it and conclude there is no overlap, which is
    the exact silent failure `require_node` and `claims_of` refuse to allow everywhere else.

    Discovery now receives the write-only adapter rather than a readable ledger. Capturing it at
    both calls proves the two families write through one issuer without weakening that boundary.
    """

    import quiddity.slots as slots_module

    seen = {}
    real_pockets = slots_module._discover_pockets
    real_slots = slots_module._discover_slots

    def capture_pockets(part, **kwargs):
        seen["pockets"] = kwargs.get("writer")
        return real_pockets(part, **kwargs)

    def capture_slots(part, **kwargs):
        seen["slots"] = kwargs.get("writer")
        return real_slots(part, **kwargs)

    monkeypatch.setattr(slots_module, "_discover_pockets", capture_pockets)
    monkeypatch.setattr(slots_module, "_discover_slots", capture_slots)

    part = (Box(60, 40, 12) - Pos(0, 0, 4) * Box(20, 12, 8)) - Pos(0, -14, 0) * Box(8, 12, 40)
    result = r.build_recognition_result(part)

    assert len(result.pockets) == 1, "the fixture must carry a pocket"
    assert result.slots, "and a slot, or this proves nothing about sharing one ledger"

    assert seen["pockets"] is seen["slots"]
    writer = seen["pockets"]
    assert writer is not None and not hasattr(writer, "claims")


def test_a_ledger_built_from_another_block_is_refused_rather_than_left_empty():
    """The provenance check, on a twin that is equal by value.

    A graph built from a different part resolves nothing, so the ledger would come back empty
    -- and empty reads downstream as "this family claims nothing" rather than as "you paired
    the wrong graph".
    """

    def block():
        return Box(60, 40, 12) - Pos(0, 0, 4) * Box(20, 12, 8)

    part, twin = block(), block()
    assert r.recognise_pockets(twin) == r.recognise_pockets(part), "the twin is this block"

    foreign = ClaimLedger(FaceGraph(twin))
    try:
        r.recognise_pockets(part, ledger=foreign)
    except ValueError as refusal:
        assert "built from a different part" in str(refusal)
    else:
        raise AssertionError("recognise_pockets accepted another part's graph")
