"""Original analytic hole exits; no external model.

Regenerate with: uv run --no-project --python 3.12 --with build123d \
    python tools/make_partial_exit_fixture.py
A 120 x 20 x 8 plate has six diameter-6 bores at x=10,30,50,70,90,110.
The first is clear. Blocks fused AFTER drilling cover a height-2 circular
segment, half a disk, and the entire fourth exit. The fifth is a blind
6-mm cylinder with a 1.8-mm conical drill point. This deliberately reproduces
an overlapping added rib, rather than drilling through that rib as well.
"""
import gzip
from pathlib import Path
from build123d import Align, Box, Cone, Cylinder, Pos, export_step

base = Box(120, 20, 8, align=(Align.MIN, Align.CENTER, Align.MIN))
for x in (10, 30, 50, 70, 110):
    base -= Pos(x, 0, 0) * Cylinder(3, 8, align=(Align.CENTER, Align.CENTER, Align.MIN))
base -= (Pos(90, 0, 0) * Cylinder(3, 6, align=(Align.CENTER, Align.CENTER, Align.MIN))
         + Pos(90, 0, 6) * Cone(3, 0, 1.8, align=(Align.CENTER, Align.CENTER, Align.MIN)))
for x, width in ((31, 5), (50, 6), (66, 8)):
    base += Pos(x, 0, 8) * Box(width, 10, 4, align=(Align.MIN, Align.CENTER, Align.MIN))
# Both mouths of the sixth bore are partly obstructed.
for z in (-4, 8):
    base += Pos(111, 0, z) * Box(5, 10, 4, align=(Align.MIN, Align.CENTER, Align.MIN))
target = Path(__file__).resolve().parents[1] / 'tests/fixtures/partial_hole_exits.step'
export_step(base, str(target))
target.with_suffix('.step.gz').write_bytes(gzip.compress(target.read_bytes(), mtime=0))
target.unlink()
