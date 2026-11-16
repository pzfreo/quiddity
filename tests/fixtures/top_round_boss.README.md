# Short top-rounded bosses

Original synthetic Apache-2.0 fixtures for specify-core#181, generated with
build123d 0.13.0. No customer geometry is used.

`top_round_boss{13,8,6,4}.step.gz` use a 120 × 120 × 10 box at Z = −10..0
and a radius-32.5 cylinder at Z = 0..height. Union them and fillet the circular
top edge with radius 3. `top_round_boss8_chamfer.step.gz` uses a 1 mm chamfer
on that edge instead. The expected overall boss heights are the filename heights.

`top_round_non_tangent.step.gz` revolves this (radius, Z) profile about Z:
(0,0), (32.5,0), (32.5,5), a radius-3 arc centred at
(32.5 − 3√3/2, 3.5) from angle 30° to 90°, then (0,6.5), (0,0).
It is united with the same base. The convex torus meets the shaft at 30°,
so its 1.5 mm axial extension must not lengthen the 5 mm boss.

STEP face order is preserved as exported by build123d; tests identify surfaces
through the imported topology without renumbering them.
