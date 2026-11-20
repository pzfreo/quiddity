"""Original analytic spoked discs (hub, rim, four spokes); no external model.

Regenerate with: uv run --no-project --python 3.12 --with build123d \
    python tools/make_spoked_rim_fixture.py
Writes tests/fixtures/spoked_rim.step.gz: four discs side by side along x (80 mm apart).
  x=0    flexure: hub tube r8..14 z26..28.5, rim tube r26..32 z26.5..29, four 13x6 spokes
         z26.5..28.5. d16 through hole in the hub; the rim wall is not a counterbore.
  x=80   the same with a real counterbore (d20, 1.5 deep from z28.5) in the hub top.
  x=160  hub with a real counterbore and a cross hole, in a spoked disc.
  x=240  plain stepped hub (control: genuine counterbore in a solid disc, no spokes).
"""
import gzip
from pathlib import Path
from build123d import Align, Box, Cylinder, Pos, Rot, export_step

MIN = (Align.CENTER, Align.CENTER, Align.MIN)

def tube(ro, ri, z0, z1):
    return Cylinder(ro, z1 - z0, align=MIN).moved(Pos(0, 0, z0)) - Cylinder(ri, z1 - z0 + 2, align=MIN).moved(Pos(0, 0, z0 - 1))

def spoked(cbore=False, cross=False):
    p = tube(14, 8, 26, 28.5) + tube(32, 26, 26.5, 29)
    for a in (0, 90, 180, 270):
        spoke = Pos(20, 0, 26.5) * Box(13, 6, 2, align=(Align.CENTER, Align.CENTER, Align.MIN))
        p += Rot(0, 0, a) * spoke
    if cbore:
        p -= Pos(0, 0, 27) * Cylinder(10, 3, align=MIN)
    if cross:
        p -= Pos(0, 0, 26.7) * Rot(0, 90, 0) * Cylinder(1.5, 40)
    return p

def solid_step():
    p = Cylinder(30, 3, align=MIN)
    p -= Cylinder(8, 5, align=MIN).moved(Pos(0, 0, -1))
    p -= Pos(0, 0, 1.5) * Cylinder(14, 2, align=MIN)
    return p

from build123d import Compound
parts = [spoked(), spoked(cbore=True), spoked(cbore=True, cross=True), solid_step()]
comp = Compound([Pos(80 * i, 0, 0) * p for i, p in enumerate(parts)])
target = Path(__file__).resolve().parents[1] / 'tests/fixtures/spoked_rim.step'
export_step(comp, str(target))
target.with_suffix('.step.gz').write_bytes(gzip.compress(target.read_bytes(), mtime=0))
target.unlink()
