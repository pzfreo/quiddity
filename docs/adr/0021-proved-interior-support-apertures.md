# ADR 0021 — Independently proved interior support apertures

- **Status:** Accepted; implementation subject to the validation gates below
- **Date:** 2026-09-06
- **Issues:** #538, #369, #290

## Decision

Extend ADR 0019's base-support interpretation to independently proved circular
apertures strictly inside a channel's original planar support patches. The base
U-profile, outer support contours and longitudinal ends remain unchanged. An
aperture is not an excuse to infer a missing wall, shorten the occurrence to a
common core or fill an unexplained gap.

This uses the existing SectionRecess geometry, as the earlier separately proved
entry-bevel exception does. The record describes base channel geometry with proved
support interruptions, not a complete unperforated physical boundary. Independently
recognised holes remain separate occurrences. Exact reconstruction of the fully
treated boundary is not promised by the base recess record alone; a future result
relationship contract is outside this slice.

## Bounded proof

For every proposed support patch, subtract the union of its actual source support
and individually proved aperture disks. Require no unexplained area remainder.
Each aperture must satisfy all of the following:

- Its complete original circular inner wire is strictly inside both its source
  face's outer contour and the proposed support patch. Touching or crossing an
  outer contour or run end is not admitted.
- That same original edge belongs to an adjacent native inward cylindrical face;
  the aperture plane is perpendicular to its axis. No nearby-cylinder substitution,
  recovery or ellipse approximation.
- Both finite axial limits come from complete original circular rim edges with
  observed planar boundary context. Global axis envelopes do not supply an end.
- The complete generated finite cylinder side and original cylindrical source
  support cover one another. The corresponding finite cell is empty.
- All original support, cylinder and end-context faces share one valid solid.
- Each disconnected bore segment is proved separately. Opposed channel-wall bores
  must not be joined across the channel's air gap.

Original channel walls remain defining evidence. Original floor/walls and proved
cylindrical faces are constituents. Generated disks and cylinder cells are private
proof geometry, never original face references. Existing complete base-channel
void, open-run-end and lateral-opening checks stay unchanged.

## Evidence and limits

The authored #538 fixture has a floor aperture of area `pi*6²` and two wall
apertures of area `pi*4²`. The private feasibility proof explains the floor's
finite counterbore segment `[2,6]` and each wall segment `[-25,-12.5]` and
`[12.5,25]` independently. It explains the complete support deficits in the plain,
floor-only, walls-only and combined cases.

Production acceptance requires public-path positives, asymmetric and transformed
cases, ownership, tolerance/STEP evidence, and negatives for boundary-touching,
partial cylindrical support, oblique openings, obstructions, unrelated cylinders,
cross-body/coaxial joins and mixed explained/unexplained gaps. Then independent
substantive review, open MFCAD++ comparison and required green CI.

The implementation passes 77 authored aperture tests, including public unchanged
base geometry and original constituent evidence across the four hole combinations,
asymmetric walls and right-angle placements, plus arbitrary rigid placements of the
private proof. Boundary-touch and near-boundary tests, obstructions, ownership and
STEP transport are retained. Private-proof and full-integration independent reviews
have no remaining substantive findings. Corpus comparison and required green CI
remain separate merge gates; these local results do not replace them.

This does not admit pierced capped run ends or the end-intersecting bore in
Draftwright's #1166 example. Those change the occurrence boundary and need their
own truthful representation/proof. No MFInstSeg data is used.
