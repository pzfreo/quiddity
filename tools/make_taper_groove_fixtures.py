"""Original turned parts for tapers and V-grooves (#307 item 6); no external model.

Regenerate with: uv run --no-project --python 3.12 --with build123d \
    python tools/make_taper_groove_fixtures.py
Every part is turned about z. Records expected:
  taper_morse        a shaft d20 (z 0..30), a Morse-like taper d20 -> d12.5 (z 30..90), a d12.5
                     tail (z 90..100): one external taper, included angle ~ 4.3 degrees
  taper_shank        a bare frustum d24 -> d50, 16 mm long (the issue's shock eye shank)
  taper_bore_pin     a d40 x 40 block with a through taper-pin hole, d16 at z 40 narrowing to d10 at
                     z 0: one internal taper, included angle ~ 8.6 degrees
  vgroove_external   a d50 shaft (z 0..40) with a 90 degree V groove 4 mm wide at z 20: one
                     external V groove, mouth d50, bottom d46
  vgroove_internal   the ring of item 3: tube r 41.5..46.0, z 21..29, minus a revolved 5.65 mm
                     square turned 45 degrees at r 39.5, z 25: one internal V groove, 90 degrees
  vgroove_flat       the external V groove with a 0.2 mm flat at the bottom (already read as
                     `grooves` and two `chamfers`, which keep it: no V-groove record)
  taper_bore_socket, taper_bore_chamber  a blind taper socket and a closed d16 chamber between d10
                     bores: the existing `holes` and `countersinks` define their cones, which keep
                     them: no taper record
Controls, which must publish neither record:
  taper_control_chamfer       a d30 x 40 shaft with a 2 mm x 45 degree chamfer on each end
  taper_control_chamfer_narrow  the same shaft with a 2.5 x 1 mm edge break on one end (not 45 degrees)
  taper_control_countersink   a 30 x 30 x 10 plate with a d6 hole and a 90 degree countersink d12
  taper_control_drill_point   a d20 x 30 block with a d8 blind hole, 118 degree drill point
  taper_control_trapezoid     the external groove with a 2 mm flat bottom (two flanks, no V)
  taper_control_ridge         a d30 shaft with a V-shaped ridge (a peak, not a groove)
"""
import gzip
import math
from pathlib import Path
from build123d import (Align, Axis, Cone, Cylinder, Face, Plane, Polyline, Pos, Rot,
                       export_step, revolve)

C = (Align.CENTER, Align.CENTER, Align.MIN)


def lathe(points):
    """Revolve a closed (r, z) polygon about z."""
    return revolve(Face(Polyline(*[(r, 0, z) for r, z in points], close=True)), Axis.Z)


def write(shape, name):
    path = Path(__file__).resolve().parents[1] / "tests/fixtures" / (name + ".step")
    export_step(shape, str(path))
    path.with_suffix(".step.gz").write_bytes(gzip.compress(path.read_bytes(), mtime=0))
    path.unlink()


def v_shaft(flat=0.0, half_width=2.0):
    # Groove at z 20: half_width at the rim, 45 degree flanks, bottom half_width - flat/2 deep
    # of the d50 shaft.
    depth = half_width - flat / 2
    pts = [(0, 0), (25, 0), (25, 20 - half_width)]
    if flat:
        pts += [(25 - depth, 20 - flat / 2), (25 - depth, 20 + flat / 2)]
    else:
        pts += [(25 - depth, 20)]
    pts += [(25, 20 + half_width), (25, 40), (0, 40)]
    return lathe(pts)


write(lathe([(0, 0), (10, 0), (10, 30), (6.25, 90), (6.25, 100), (0, 100)]), "taper_morse")
write(lathe([(0, 0), (12, 0), (25, 16), (0, 16)]), "taper_shank")
# A blind socket: a taper from the mouth (z 40) narrowing to d10, then a d10 tail to a flat floor.
write(Cylinder(20, 40, align=C) - lathe([(0, 5), (5, 5), (5, 15), (8, 40), (0, 40)]), "taper_bore_socket")
# A through taper-pin hole: d16 at z 40 narrowing to d10 at z 0, with no cylinder at all.
write(Cylinder(20, 40, align=C) - lathe([(0, 0), (5, 0), (8, 40), (0, 40)]), "taper_bore_pin")
# A closed undercut: d10 bore from the bottom, widening by a taper to a d16 chamber below a d10 mouth.
write(Cylinder(20, 50, align=C) - lathe([(0, 0), (5, 0), (5, 10), (8, 20), (8, 30), (5, 40), (5, 50), (0, 50)]), "taper_bore_chamber")
write(v_shaft(), "vgroove_external")
h = 5.65 / 2 ** 0.5
ring = (Pos(0, 0, 21) * (Cylinder(46.0, 8, align=C) - Cylinder(41.5, 8, align=C))
        - lathe([(39.5 - h, 25), (39.5, 25 + h), (39.5 + h, 25), (39.5, 25 - h)]))
write(ring, "vgroove_internal")
write(v_shaft(flat=0.2), "vgroove_flat")
write(lathe([(0, 0), (13, 0), (15, 2), (15, 38), (13, 40), (0, 40)]), "taper_control_chamfer")
write(lathe([(0, 0), (14, 0), (15, 2.5), (15, 40), (0, 40)]), "taper_control_chamfer_narrow")
plate = Pos(0, 0, 0) * (Cylinder(15, 10, align=C) - Cylinder(3, 10, align=C)
                         - lathe([(0, 10), (6, 10), (3, 7), (0, 7)]))
write(plate, "taper_control_countersink")
tip = 4 / math.tan(math.radians(59))
write(Cylinder(10, 30, align=C) - lathe([(0, 12 - tip), (4, 12), (4, 30), (0, 30)]), "taper_control_drill_point")
write(v_shaft(flat=2.0, half_width=3.0), "taper_control_trapezoid")
write(lathe([(0, 0), (15, 0), (15, 18), (17, 20), (15, 22), (15, 40), (0, 40)]), "taper_control_ridge")
