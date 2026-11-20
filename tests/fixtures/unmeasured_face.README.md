# Faces whose area and centroid cannot be computed

Original synthetic Apache-2.0 fixtures for quiddity#303, generated with
build123d 0.13.0 and then edited as STEP text. No customer geometry is used.

Both rewrite a cylindrical bore wall to be bounded by a single loop of one closed
circle edge, the way some converters write a degenerate face. A cylinder bounded
by one circle has no extent along its axis, so `Part::face_moments` returns `None`
for it, correctly.

- `unmeasured_face_closed.step`: a 60 × 40 × 20 box (centred) with a radius-4
  through hole at x = −15 and a radius-5 blind hole at x = 12 (z 2..10). The blind
  hole's floor is removed from the shell and its wall's loop is the rim at z = 10,
  so the shell stays closed and sound. The through hole is recognised; the wall is
  published as an `unmeasured_face_refusals` record.
- `unmeasured_face_open.step`: the same box with one radius-5 through hole at
  x = 8, its bore wall's loop reduced to one rim circle. The shell is open (the other
  rim and the seam are used by no face), so no feature is recognised, and the wall
  is still reported by name.
