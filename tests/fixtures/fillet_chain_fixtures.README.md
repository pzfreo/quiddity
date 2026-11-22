# Connected fillet chain fixtures

Original synthetic Apache-2.0 fixtures for quiddity #341. Regenerate them with
`tools/make_fillet_chain_fixtures.py` and build123d 0.13.0.

- `fillet_chain_box.step.gz` rounds all edges of a 30 × 24 × 20 mm box at R3.
  The 12 cylindrical rounds and 8 spherical corners form one connected blend.
- `fillet_chain_disjoint.step.gz` rounds two diagonally opposite vertical edges at R3.
  The rounds have no common boundary.
- `fillet_chain_mixed.step.gz` rounds all vertical edges at R3, then rounds the top
  perimeter at R1. The toroidal corners in the top rim meet the R3 patches
  tangentially, so the two radii can be distinguished independently of tangency.
