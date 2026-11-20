"""Original analytic collared recesses whose floor is not one plain full-circle edge.

Regenerate with: uv run --no-project --python 3.12 --with build123d \
    python tools/make_collared_floor_fixture.py
A d60 x 10 disc has a d40 recess from z=6 up, a d20 collar standing on the recess floor
(z=6..10) and a d10 through bore. Three discs, 100 mm apart along x:
  x=0    R1 fillet between the recess wall and its floor   (cbore d40, depth 3)
  x=100  1 mm chamfer between the recess wall and its floor (cbore d40, depth 3)
  x=200  every face written in two halves split on the XZ plane (cbore d40, depth 4)
"""
import gzip
from pathlib import Path
from build123d import (Align, Compound, GeomType, Keep, Plane, Pos, Cylinder, ShapeList,
                       Shape, chamfer, export_step, fillet, split)
from OCP.BRepAlgoAPI import BRepAlgoAPI_Fuse

MIN = (Align.CENTER, Align.CENTER, Align.MIN)

def cyl(r, z0, z1):
    return Pos(0, 0, z0) * Cylinder(r, z1 - z0, align=MIN)

def collared():
    return cyl(30, 0, 10) - cyl(20, 6, 11) + cyl(10, 6, 10) - cyl(5, -1, 11)

def wall_floor_edges(p):
    return ShapeList([e for e in p.edges().filter_by(GeomType.CIRCLE)
                      if abs(e.center().Z - 6) < 1e-6 and abs(e.radius - 20) < 1e-6])

def halves(p):
    a = split(p, Plane.XZ, keep=Keep.TOP)
    b = split(p, Plane.XZ, keep=Keep.BOTTOM)
    op = BRepAlgoAPI_Fuse(a.wrapped, b.wrapped)
    op.Build()
    return Shape.cast(op.Shape())

p = collared()
parts = [fillet(wall_floor_edges(p), 1.0), chamfer(wall_floor_edges(collared()), 1.0), halves(collared())]
comp = Compound([Pos(100 * i, 0, 0) * q for i, q in enumerate(parts)])
target = Path(__file__).resolve().parents[1] / 'tests/fixtures/collared_floor_variants.step'
export_step(comp, str(target))
target.with_suffix('.step.gz').write_bytes(gzip.compress(target.read_bytes(), mtime=0))
target.unlink()
