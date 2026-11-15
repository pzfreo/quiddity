"""Original Apache-2.0 geometry for #134 and specify-core #181; no external model.

Run with build123d 0.10.0, ocp_gordon 0.1.17 / Python 3.12. The gramil files have their separate recipe
and OCL licence in tests/fixtures/revisions/.
"""
import gzip
from pathlib import Path
from build123d import Align, Box, Cylinder, GeomType, Pos, Sphere, export_step, fillet

root = Path(__file__).resolve().parents[1] / 'tests/fixtures'
def write(shape, path):
    temp = path.with_suffix('')
    export_step(shape, str(temp))
    path.write_bytes(gzip.compress(temp.read_bytes(), mtime=0))
    temp.unlink()

def bracket(thickness):
    wall = Box(100, 8, 30, align=(Align.MIN, Align.MIN, Align.MIN))
    return wall + Box(100, 60, thickness, align=(Align.MIN, Align.MIN, Align.MIN)) if thickness else wall

for name, thickness in [('old', 4), ('thinned', 2), ('removed', 0)]:
    write(bracket(thickness), root / f'revisions/q134_plate.{name}.step.gz')

boss = Box(20, 20, 4, align=(Align.CENTER, Align.CENTER, Align.MIN)) + Pos(0, 0, 4) * Cylinder(5, 20, align=(Align.CENTER, Align.CENTER, Align.MIN))
end = [e for e in boss.edges() if e.geom_type == GeomType.CIRCLE and abs(e.center().Z - 24) < 1e-6]
write(fillet(end, 1.5), root / 'boss_end_round.step.gz')
write(fillet(end, 0.15), root / 'boss_end_small_round.step.gz')
p = bracket(4)
edge = [e for e in p.edges() if e.geom_type == GeomType.LINE and abs(e.center().Y - 60) < 1e-6 and abs(e.center().Z - 4) < 1e-6]
write(fillet(edge, 1.5), root / 'plate_edge_round.step.gz')

write(boss + Pos(0, 0, 24) * Sphere(5), root / 'boss_end_sphere.step.gz')

# specify-core #181: an R3 top round on a Ø65 boss whose straight wall is shorter or
# longer than twice the round. Height (round and cap included) must stay 13 / 8 / 6.
def rounded_boss(h):
    part = Box(120, 120, 10, align=(Align.CENTER, Align.CENTER, Align.MAX)) + Cylinder(32.5, h, align=(Align.CENTER, Align.CENTER, Align.MIN))
    return fillet([e for e in part.edges() if abs(e.center().Z - h) < 1e-3], 3.0)

for h in (13, 8, 6):
    write(rounded_boss(h), root / f'boss_top_round_h{h}.step.gz')
