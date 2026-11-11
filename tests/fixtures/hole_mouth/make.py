import sys
from build123d import Box, Cylinder, Pos, Align, GeomType, chamfer, export_step
out = sys.argv[1]
below = (Align.CENTER, Align.CENTER, Align.MAX); above = (Align.CENTER, Align.CENTER, Align.MIN)
part = Box(30, 30, 5, align=below) + Cylinder(4, 2, align=above)
part -= Pos(0, 0, 2) * Cylinder(1.65, 7, align=below)
part -= Pos(0, 0, 2) * Cylinder(2.5, 2.25, align=below)
mouth = [e for e in part.edges() if e.geom_type == GeomType.CIRCLE and abs(e.radius - 2.5) < 1e-6 and abs(e.arc_center.Z - 2) < 1e-6]
assert len(mouth) == 1
part = chamfer(mouth, 0.25)
export_step(part, out)
