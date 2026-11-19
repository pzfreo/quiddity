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

- `bicone12.step`: revolved profile r 10 - 15 - 12, a flare then a taper. No edge break.
- `leadtaper30.step`: revolved profile r 14.2 - 20 - 10 over 60 mm, a lead cone then a long
  taper. No edge break.
- `realchamfer042.step`: a frustum r 10 to 6 over 20 mm with a 4.2 x 45 degree chamfer on its
  base rim (legs 4.2 and 4.2).

The three revolved parts come from a second script, `rev(points, name)`: close the (r, z) profile
through the axis, revolve it 360 degrees about Z and export it:

```python
import math
from build123d import *
def rev(pts, name):
    w = Wire.make_polygon([Vector(r, 0, z) for r, z in pts], close=True)
    export_step(Solid.revolve(Face(w), 360, Axis.Z), f"{name}.step")
rev([(0,0),(10,0),(15,5),(12,15),(0,15)], "bicone12")
rev([(0,0),(14.2,0),(20,10),(10,60),(0,60)], "leadtaper30")
leg = 4.2; dz = leg / math.hypot(1, 0.2); dr = 0.2 * dz   # body r = 10 - 0.2 z
rev([(0,0),(10-leg,0),(10-dr,dz),(6,20),(0,20)], "realchamfer042")
```

The first four parts come from:

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
