# Rounds at a slot's ends

Original synthetic Apache-2.0 fixtures for quiddity#307 (item 7). No customer geometry is used.
`tools/make_slot_end_rounds_fixture.py` (build123d 0.13.0) writes:

- `slot_end_rounds.step.gz`: a 100 x 60 x 8 plate, R5 vertical corners, a 30 x 8 through slot at
  x = 32 along y, every straight edge rounded R0.8. The four tori at the slot's ends (faces 21,
  22, 29, 30) are fillets of radius 0.8.
- `round_hole_mouth.step.gz`, `bore_groove.step.gz`, `bore_ridge.step.gz`: a round hole with
  R0.8 mouth rounds (whole rings, a `blends` record), a concave torus groove and a convex torus
  ridge in a d8 bore. None is a slot-end fillet.

The test is `tests/slot_end_rounds.rs`.
