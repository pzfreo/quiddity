# Body-owned internal void evidence

## Decision

`InteriorVoid` describes finished geometry, not a casting core or subtraction
operation. Each record belongs to one valid source solid. `void_faces` are the
original faces of a sampled air region; `openings` groups adjacent original
faces at the boundary toward outside air. The face indices use the input part's
face order and are evidence, not persistent names.

Sampling uses original B-rep line intersections in the caller's XYZ frame.
Material and air votes must agree on three axes. A core cell has material on
both sides on every axis; only connected three-dimensional regions qualify.
Face-contact rays map sampled boundaries back to original faces. A face with a
proved straight escape ray marks an opening boundary. Uncertain ownership,
intersection parity, rays or face mapping refuse the record.

Six-axis enclosure alone also catches a blind pocket whose axis is oblique to
XYZ. A sampled widening behind an exterior throat distinguishes the cavity
from a constant-width pocket. A core bounded by at least two original spherical
faces can qualify despite a broad outlet, as on cgb243. Other broad-mouth
shapes remain outside this bounded detector until separate evidence distinguishes
them from recesses.

`estimated_volume = enclosed_samples × grid_pitch³` counts sampled core air.
The grid has 64 cells along the longest bounding-box axis. It omits an open
throat and features smaller than its pitch; it is not an exact removed volume.
Nothing in the record establishes the envelope or a unique reconstruction.

## Compatibility

This is a new public family and `RecognitionResult` field. ADR 0005 requires a
minor release and explicit consumer adoption; `0.4.0` is its introduction.
