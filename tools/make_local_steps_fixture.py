"""Original local-run step controls for #211; no external dataset geometry.

Regenerate with:
  uv run --no-project --python 3.12 --with build123d python tools/make_local_steps_fixture.py

A full-run corner step (height 2, depth 5) is interrupted by a larger corner cut.
That secondary cut leaves a common perpendicular end face convex to both step walls.
The oblique-end control clips the same step with a sloping end, so its run extent
cannot be represented by one pair of perpendicular stations. The blind-end control
has a concave closing wall and belongs to the existing blind recess families.
"""
import gzip
from pathlib import Path
from build123d import Align, Box, BuildPart, BuildSketch, Plane, Polygon, Pos, export_step, extrude

out = Path(__file__).resolve().parents[1] / 'tests/fixtures/local_steps'
out.mkdir(exist_ok=True)
minimum = (Align.MIN, Align.MIN, Align.MIN)
stock = Box(30, 60, 70, align=minimum)
full = stock - Pos(25, -1, 68) * Box(6, 62, 3, align=minimum)
local = full - Pos(20, 4, 60) * Box(11, 57, 11, align=minimum)
both = full - Pos(20, -1, 60) * Box(11, 11, 11, align=minimum)
both -= Pos(20, 20, 60) * Box(11, 41, 11, align=minimum)
with BuildPart() as cut:
    with BuildSketch(Plane.XY.offset(60)):
        Polygon((20, 4), (31, 15), (31, 61), (20, 61), align=None)
    extrude(amount=11)
oblique = full - cut.part
blind = stock - Pos(25, -1, 68) * Box(6, 5, 3, align=minimum)
for name, shape in [('local', local), ('both_ends', both), ('oblique_end', oblique), ('blind_end', blind), ('full', full)]:
    path = out / (name + '.step')
    export_step(shape, str(path))
    path.with_suffix('.step.gz').write_bytes(gzip.compress(path.read_bytes(), mtime=0))
    path.unlink()
