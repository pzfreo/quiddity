# Edge-break fixtures

Our own synthetic parts (Apache-2.0, no third-party data), written with build123d 0.13.0 by the
script below (`python gen.py DIR`), for the chamfer regression tests in
`tests/internal/chamfer_values.rs` (#301, #298).

- `vslot.step`: 40 x 30 x 10 block, 3 x 45 degree chamfer on the top-front edge, and a V-groove
  along x whose lips (y = 6 and 12) leave a 3 mm strip beside the chamfer. The chamfer is real.
- `slantstep.step`: the same chamfered block with a slanted step on the bottom-front edge.
- `bicone.step`: a plane at r = 10, a cone out to r = 15 at z = 5, a cone back in to r = 6 at
  z = 15, and a plane on top. No edge break.
- `diamond.step`: two equal cones back to back (r 10 - 15 - 10). No edge break.

```python
import sys
from build123d import *
out = sys.argv[1]
def block(): return Box(40, 30, 10, align=(Align.MIN, Align.MIN, Align.MIN))
def front_top(p): return [e for e in p.edges() if e.geom_type == GeomType.LINE and abs(e.center().Y) < 1e-6 and abs(e.center().Z - 10) < 1e-6]
def prism(pts):
    w = Wire.make_polygon([Vector(-1, y, z) for y, z in pts], close=True)
    return Solid.extrude(Face(w), Vector(42, 0, 0))
b = chamfer(front_top(block()), 3)
export_step(b - prism([(6, 10.5), (12, 10.5), (12, 10), (9, 6), (6, 10)]), out + "/vslot.step")
export_step(b - prism([(-1, -1), (-1, 5), (0, 5), (8, 0), (8, -1)]), out + "/slantstep.step")
export_step(Solid.make_cone(10, 15, 5) + Solid.make_cone(15, 6, 10, Plane.XY.offset(5)), out + "/bicone.step")
export_step(Solid.make_cone(10, 15, 5) + Solid.make_cone(15, 10, 5, Plane.XY.offset(5)), out + "/diamond.step")
```
