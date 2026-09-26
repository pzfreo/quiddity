# Native freeform support on cgb241 (#759)

The vendored cgb241 STEP part has ten native B-spline faces. Four large outer
patches have source-face indices 12–15; their paired inner patches are 32–35.
Each output support includes the full native U/V degrees, pole grid, weights,
knots, multiplicities and periodic flags. These are the underlying untrimmed
B-spline surfaces; the face trimming wires are separate topology in the STEP
source. Tests reconstruct the four outer surfaces from only the published
parameters and compare interior values with the source supports.

The source patches do not form one smoothly continuous sheet: most of their
shared edges are sharp. Continuity groups therefore remain separate at those
joins, and only a proved smooth or same-support edge connects two patches.
The inner/outer links reuse reciprocal material-ray wall pairs measured at
about 3 mm. Their `offset_basis` names that finite geometric evidence; it
does not assert a globally exact offset or a unique shell operation.

A two-section degree-one B-spline sweep proves a ruled surface; when both
sections differ by one constant vector with matching weights, it proves a
linear extrusion. The published support retains the profile rows, sweep
parameter and vector. The four large cgb241 patches meet neither gate, so
their construction is null. A generic B-spline has no unique loft history.
This family currently publishes native
B-spline supports; other native freeform surface classes are outside its
bounded recognition domain.
