"""Synthetic finned barrels for the repeating-radial-profile family (#307 item 10).

Regenerate (build123d 0.13.0):
    uv run --no-project --python 3.12 --with build123d==0.13.0 python tools/make_radial_fin_fixtures.py
The geometry follows the construction of a finned motor housing (a barrel, a rear wall, a
stator sleeve, standoffs, retainer screw holes and 64 radial box fins that stop short of
both barrel ends), with invented dimensions. No reporter file is used.

  radial_fin_housing   R50/R46 barrel, Z -31..29; 64 fins 1.4 thick reaching R52.5 over
                       Z -26.5..12; rear wall, stator sleeve, six standoffs, wire holes
                       and screw holes
  radial_fin_tube      R50/R46 tube Z 0..40 with 24 fins 1.6 thick reaching R54 over the
                       whole length (the profile is on both end faces)
  radial_fin_short     the same tube with the fins on Z 6..34 only
  radial_fin_single    the tube with one radial rib
  radial_fin_walls     the tube with four radial walls of different sizes
  radial_fin_irregular the short-fin tube with every fin shifted off the pitch by a different amount
  radial_fin_notched   the tube with 24 radial notches cut into its wall over Z 6..34 (the profile
                       is cut into the host, not standing on it)
  radial_fin_hex      round R22 (Z 0..20), hex 40 across flats (Z 20..40), round R22 (Z 40..60)
  radial_fin_octagon  round R22.5, octagon of circumradius 23 (apothem 21.25), round R22.5
  radial_fin_star     round R20, six-point star R26/R14 (Z 20..40), round R20
                      The three are controls: each polygon's section sits between two round
                      sections, so the arcs between its caps are not one host's surface.
  bar_tube            R30/R20 tube Z 0..40 (both annular ends are the bar's end planes)
  bar_chamfered       R25 bar Z 0..40 with 2 mm chamfers on both ends (no end plane meets the cylinder)
"""
import gzip
import sys
from pathlib import Path

import math

from build123d import Align, Axis, Box, BuildPart, BuildSketch, Cylinder, GeomType, Location, Plane, Polygon, Pos, RegularPolygon, export_step, extrude

OUT = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parents[1] / "tests" / "fixtures"
MIN_Z = (Align.CENTER, Align.CENTER, Align.MIN)


def write(part, name):
    tmp = OUT / (name + ".tmp")
    export_step(part, str(tmp))
    (OUT / name).write_bytes(gzip.compress(tmp.read_bytes(), mtime=0))
    tmp.unlink()


def tube(outer, inner, z0, z1):
    return Pos(0, 0, z0) * (Cylinder(outer, z1 - z0, align=MIN_Z) - Cylinder(inner, z1 - z0, align=MIN_Z))


def cyl(r, z0, z1):
    return Pos(0, 0, z0) * Cylinder(r, z1 - z0, align=MIN_Z)


def fin(r0, r1, w, z0, z1):
    """A box from 0.5 inside r0 to r1, w thick, overlapping into the barrel to fuse."""
    return Pos((r0 - 0.5 + r1) / 2, 0, z0) * Box(r1 - r0 + 0.5, w, z1 - z0, align=MIN_Z)


def fins(count, r0, r1, w, z0, z1, phase=0.0):
    base = fin(r0, r1, w, z0, z1)
    return [base.rotate(Axis.Z, phase + i * 360.0 / count) for i in range(count)]


def housing(count=64, fin_list=None):
    part = tube(50, 46, -31, 29) + [tube(46, 13, -22, -18), tube(28, 15, -18, 1.5)]
    part = part + (fin_list if fin_list is not None else fins(count, 49.5, 52.5, 1.4, -26.5, 12))
    for az, rad, r, z0, z1 in [(90, 17.5, 2.5, -24.4, -22), (210, 17.5, 2.5, -24.4, -22), (330, 17.5, 2.5, -24.4, -22),
                               (30, 40, 3, -28.8, -22), (150, 40, 3, -28.8, -22), (270, 40, 3, -28.8, -22)]:
        part = part + (Pos(rad, 0, z0) * Cylinder(r, z1 - z0, align=MIN_Z)).rotate(Axis.Z, az)
    for az in (15, 135, 255):
        part = part - (Pos(34, 0, -22.1) * Cylinder(2.5, 4.2, align=MIN_Z)).rotate(Axis.Z, az)
    for i in range(8):
        a = 22.5 + 45 * i
        part = part - (Pos(48, 0, 21) * Cylinder(1.7, 8.1, align=MIN_Z)).rotate(Axis.Z, a)
        part = part - (Pos(48, 0, -31.1) * Cylinder(1.4, 5.1, align=MIN_Z)).rotate(Axis.Z, a)
    return part


write(housing(), "radial_fin_housing.step.gz")
write(tube(50, 46, 0, 40) + fins(24, 49.5, 54, 1.6, 0, 40), "radial_fin_tube.step.gz")
write(tube(50, 46, 0, 40) + fins(24, 49.5, 54, 1.6, 6, 34), "radial_fin_short.step.gz")
write(tube(50, 46, 0, 40) + fins(1, 49.5, 54, 1.6, 6, 34), "radial_fin_single.step.gz")
walls = [fin(49.5, 49.5 + h, w, 6, 34).rotate(Axis.Z, a)
         for a, h, w in [(0, 4, 1.6), (70, 6, 2.2), (150, 3, 1.0), (260, 5, 3.0)]]
write(tube(50, 46, 0, 40) + walls, "radial_fin_walls.step.gz")
irregular = [f.rotate(Axis.Z, 0.7 * ((i * 37) % 11) / 11) for i, f in enumerate(fins(24, 49.5, 54, 1.6, 6, 34))]
notches = [Pos((50 + 49.5 - 3) / 2 + 1.5, 0, 6) * Box(4.5, 1.6, 28, align=MIN_Z) for _ in range(1)]
notched = tube(50, 46, 0, 40)
for i in range(24):
    notched = notched - notches[0].rotate(Axis.Z, i * 15)
write(notched, "radial_fin_notched.step.gz")
write(tube(50, 46, 0, 40) + irregular, "radial_fin_irregular.step.gz")


def section_stack(sketch_fn, r):
    """Round R r, a prismatic section Z 20..40, round R r."""
    with BuildPart() as bp:
        with BuildSketch(Plane.XY.offset(20)):
            sketch_fn()
        extrude(amount=20)
    return cyl(r, 0, 20) + bp.part + cyl(r, 40, 60)


write(section_stack(lambda: RegularPolygon(40 / math.sqrt(3), 6), 22), "radial_fin_hex.step.gz")
write(section_stack(lambda: RegularPolygon(23, 8), 22.5), "radial_fin_octagon.step.gz")
star = [(26 if i % 2 == 0 else 14) for i in range(12)]
write(section_stack(lambda: Polygon(*[(r * math.cos(i * math.pi / 6), r * math.sin(i * math.pi / 6))
                                       for i, r in enumerate(star)], align=None), 20),
      "radial_fin_star.step.gz")

# Bar-end controls (#307 item 10): a plain tube and a bar with chamfered ends.
write(tube(30, 20, 0, 40), "bar_tube.step.gz")
bar = cyl(25, 0, 40)
write(bar.chamfer(2, None, bar.edges().filter_by(GeomType.CIRCLE)), "bar_chamfered.step.gz")
