"""Original analytic parts with a bore that has an internal groove; no external model.

Regenerate with: uv run --no-project --python 3.12 --with build123d \
    python tools/make_grooved_bore_fixtures.py
Every part is a tube about z; the groove is cut into the tube wall, radially outward of the bore.
  grooved_bore_race    r 13.6..15.0, z -3.5..1.5, minus a torus R13.0 r1.3 at z -1.0 (a ball
                       groove): one d27.2 through bore, two 1.35 mm lands
  grooved_bore_ring    r 41.5..46.0, z 21..29, minus a 5.65 mm square turned 45 degrees at
                       r 39.5, z 25 (a V groove): one d83 through bore, lands z 21..23, 27..29
Controls, which must keep their existing readings:
  grooved_bore_web     the plain tube r 13.6..15.0, z -3.5..1.5, with a 1 mm web (r 0..13.6,
                       z -1.5..-0.5) across the bore: two blind d27.2 holes back to back
  grooved_bore_webgroove  the race with that web across its groove (r 0..14.6, z -1.5..-0.5):
                       the lands are separated by material inside the bore diameter
  grooved_bore_cbore   a d27.2 land and a d29.2 land (z 0..3.5) with a plain shoulder between them: one
                       stepped hole, never merged
  grooved_bore_steps   a d27.2 land and a d28.0 land separated by a ball groove (torus R13.3
                       r1.3): different diameters, never merged
  grooved_bore_cross   a 60 x 60 x 5 plate with a d27.2 bore and a d3 cross hole along x at z -1
"""
import gzip
from pathlib import Path
from build123d import Align, Box, Cylinder, Pos, Rot, Torus, export_step, Face, Polyline, revolve, Axis

C = (Align.CENTER, Align.CENTER, Align.MIN)

def tube(r_in, r_out, z0, z1):
    return Pos(0, 0, z0) * (Cylinder(r_out, z1 - z0, align=C) - Cylinder(r_in, z1 - z0, align=C))

def race():
    return tube(13.6, 15.0, -3.5, 1.5) - Pos(0, 0, -1.0) * Torus(13.0, 1.3)

def ring():
    # A square of side 5.65 turned 45 degrees: a diamond with half-diagonal 5.65/sqrt(2).
    h = 5.65 / 2 ** 0.5
    pts = [(39.5 - h, 25), (39.5, 25 + h), (39.5 + h, 25), (39.5, 25 - h), (39.5 - h, 25)]
    face = Face(Polyline(*[(x, 0, z) for x, z in pts]))
    cutter = revolve(face, Axis.Z)
    return tube(41.5, 46.0, 21, 29) - cutter

def ridge(r_out):
    return Pos(0, 0, -1.5) * Cylinder(r_out, 1.0, align=C)

def web():
    return tube(13.6, 15.0, -3.5, 1.5) + ridge(14.0)

def web_in_groove():
    return race() + ridge(14.6)

def cbore():
    return tube(13.6, 15.0, -3.5, 0) + tube(14.6, 16.0, 0, 3.5)

def steps():
    return tube(13.6, 16.0, -3.5, -1.0) + tube(14.0, 16.0, -1.0, 1.5) - Pos(0, 0, -1.0) * Torus(13.3, 1.3)

def cross():
    block = Pos(0, 0, -3.5) * Box(60, 60, 5, align=C) - Pos(0, 0, -3.5) * Cylinder(13.6, 5, align=C)
    return block - Pos(0, 0, -1.0) * Rot(0, 90, 0) * Cylinder(1.5, 80)

parts = {
    'grooved_bore_race': race(), 'grooved_bore_ring': ring(), 'grooved_bore_web': web(), 'grooved_bore_webgroove': web_in_groove(),
    'grooved_bore_cbore': cbore(), 'grooved_bore_steps': steps(), 'grooved_bore_cross': cross(),
}
for name, solid in parts.items():
    target = Path(__file__).resolve().parents[1] / f'tests/fixtures/{name}.step'
    export_step(solid, str(target))
    target.with_suffix('.step.gz').write_bytes(gzip.compress(target.read_bytes(), mtime=0))
    target.unlink()
