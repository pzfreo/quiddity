"""Original round-bottomed (toroidal) groove parts; no external model.

Regenerate with: uv run --no-project --python 3.12 --with build123d \
    python tools/make_round_groove_fixtures.py
Each part is cut with a single Torus, as an O-ring gland or a ball groove is:
  round_groove_gland_shaft   shaft R29, z 0..30, ring gland: torus R28.5 r1.3 at z=15
  round_groove_ball_race     tube r13.6..15.0, z -3.5..1.5, minus torus R13.0 r1.3 at z=-1.0
  round_groove_face_glands   plate 60 x 60 x 8 with two face glands: torus R8.2 r0.7
                             and R6.724 r0.7 centred on the top face (z=8)
  round_groove_controls      a filleted shaft step (torus fillet, tangent both sides) and a
                             rounded bead (convex torus) on a shaft: neither is a groove
"""
import gzip
from pathlib import Path
from build123d import Align, Box, Cylinder, Plane, Pos, Torus, export_step

root = Path(__file__).resolve().parents[1] / 'tests/fixtures'
MIN = (Align.CENTER, Align.CENTER, Align.MIN)


def write(name, solid):
    target = root / f'{name}.step'
    export_step(solid, str(target))
    target.with_suffix('.step.gz').write_bytes(gzip.compress(target.read_bytes(), mtime=0))
    target.unlink()


shaft = Cylinder(29, 30, align=MIN) - Pos(0, 0, 15) * Torus(28.5, 1.3)
write('round_groove_gland_shaft', shaft)

race = Pos(0, 0, -3.5) * (Cylinder(15.0, 5, align=MIN) - Cylinder(13.6, 5, align=MIN))
race -= Pos(0, 0, -1.0) * Torus(13.0, 1.3)
write('round_groove_ball_race', race)

plate = Box(60, 60, 8, align=MIN)
plate -= Pos(0, 0, 8) * Torus(8.2, 0.7)
plate -= Pos(0, 0, 8) * Torus(6.724, 0.7)
write('round_groove_face_glands', plate)

# Controls: an R30 shoulder with an R3 fillet into an R20 shaft, plus an R2 bead (convex torus)
# on the shaft at z=20.
step = Cylinder(30, 10, align=MIN) + Pos(0, 0, 10) * Cylinder(20, 30, align=MIN)
step = step.fillet(3, [e for e in step.edges() if abs(e.center().Z - 10) < 1e-6 and abs(e.length - 2 * 3.14159265 * 20) < 1e-3])
bead = Pos(0, 0, 20) * Torus(20, 2.0)
write('round_groove_controls', step + bead)
