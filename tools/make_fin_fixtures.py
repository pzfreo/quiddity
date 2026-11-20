"""Original synthetic fixtures for the `fins` family (#307 item 10); no customer geometry.

Regenerate (build123d 0.13.0):
    uv run --no-project --python 3.12 --with build123d==0.13.0 python tools/make_fin_fixtures.py

Positives:
  fins_flat_rib        100 x 60 x 6 plate (Z 0..6) with one 2-thick, 10-high, 80-long rib
                       along X at y = 0.
  fins_round_tip_rib   the same rib with a half-round (R1) tip.
  fins_gusset_plate    60 x 40 x 6 plate with a 6-thick upright wall along its back edge and
                       a 3-thick right-triangular gusset (30 x 25 legs) between them (a
                       `gusset_ribs` record, so no fin), plus a free 3-thick triangular rib
                       standing alone on the plate (a fin).
  fins_slender_ridges  a finned barrel built like a real motor housing: an R46/R50 tube
                       (Z -31..29), a rear wall (R13..46, Z -22..-18) and 64 radial 1.4-thick
                       fins, 2.5 proud of the barrel (box overlapping it by 0.5), Z -26.5..12.
                       Thickness is over a quarter of the height but 0.04 of the length.
                       The four axis-aligned fins are `pads` boxes, handed to the array: 64 fins.
Controls (no fin):
  fins_lone_bead       a lone 1.5 x 2.5 x 60 half-round bead on a plate: slender, but alone
  fins_ridge_wall      a lone 1.5 x 2.5 x 30 ridge running into a 6-thick upright wall
  fins_crossing_ribs   two crossing 70-long half-round-tip ribs on a plate: the junction splits
                       each into pieces whose length would be wrong, so no fin
  fins_ribbed_plate    the plate with three straight ribs at y = -20, 0, 20: `pads` claim
                       the ribs, so no fin is published over them
  fins_boss_pad        an R8 boss and a 20 x 10 x 3 pad on a 60 x 60 x 6 plate
  fins_pocket          a 60 x 40 x 20 block with a 30 x 20 pocket, 15 deep
  fins_thick_step      a 60 x 40 x 6 plate with a 40 x 12 x 8 bar standing on it (12 thick)
  fins_sheet_flange    a 2 mm sheet L bracket, sharp
  fins_sheet_bend      the same with an R2/R4 bend
"""
import gzip
from pathlib import Path

from build123d import (
    Align,
    Axis,
    Box,
    BuildPart,
    BuildSketch,
    Cylinder,
    Location,
    Plane,
    Polygon,
    export_step,
    extrude,
    fillet,
)

OUT = Path(__file__).resolve().parents[1] / "tests" / "fixtures"
MIN_Z = (Align.CENTER, Align.CENTER, Align.MIN)


def write(part, name):
    tmp = OUT / (name + ".tmp")
    export_step(part, str(tmp))
    (OUT / name).write_bytes(gzip.compress(tmp.read_bytes(), mtime=0))
    tmp.unlink()


plate = Box(100, 60, 6, align=MIN_Z)
for y in (-20, 0, 20):
    plate = plate + Box(80, 2, 10, align=MIN_Z).moved(Location((0, y, 6)))
write(plate, "fins_ribbed_plate.step.gz")


# A single 2-thick, 10-high, 80-long rib on a 100 x 60 x 6 plate: flat tip, then half-round tip.
base = Box(100, 60, 6, align=MIN_Z)
write(base + Box(80, 2, 10, align=MIN_Z).moved(Location((0, 0, 6))), "fins_flat_rib.step.gz")
def rib_70(angle):
    r = Box(70, 2, 9, align=MIN_Z) + Cylinder(1, 70).rotate(Axis.Y, 90).moved(Location((0, 0, 9)))
    return r.moved(Location((0, 0, 6))).rotate(Axis.Z, angle)


rib = Box(80, 2, 9, align=MIN_Z) + Cylinder(1, 80).rotate(Axis.Y, 90).moved(Location((0, 0, 9)))
write(base + rib.moved(Location((0, 0, 6))), "fins_round_tip_rib.step.gz")


def triangle(points, x0, thickness):
    """A (Y, Z) triangle extruded along +X from x0."""
    with BuildPart() as p:
        with BuildSketch(Plane.YZ.offset(x0)):
            Polygon(*points, align=None)
        extrude(amount=thickness)
    return p.part


part = Box(60, 40, 6, align=MIN_Z) + Box(60, 6, 30, align=MIN_Z).moved(Location((0, 17, 6)))
part = part + triangle([(14, 6), (-11, 6), (14, 31)], -16.5, 3)  # gusset against the wall
part = part + triangle([(-15, 6), (10, 6), (-15, 26)], 13.5, 3)  # free triangular rib
write(part, "fins_gusset_plate.step.gz")

part = Box(60, 60, 6, align=MIN_Z)
part = part + Cylinder(8, 10, align=MIN_Z).moved(Location((-15, 0, 6)))
part = part + Box(20, 10, 3, align=MIN_Z).moved(Location((15, 0, 6)))
write(part, "fins_boss_pad.step.gz")

write(Box(60, 40, 20, align=MIN_Z) - Box(30, 20, 15, align=MIN_Z).moved(Location((0, 0, 5))), "fins_pocket.step.gz")

part = Box(60, 40, 6, align=MIN_Z) + Box(40, 12, 8, align=MIN_Z).moved(Location((0, 0, 6)))
write(part, "fins_thick_step.step.gz")

with BuildPart() as sharp:
    with BuildSketch(Plane.XZ):
        Polygon((0, 0), (40, 0), (40, 2), (2, 2), (2, 30), (0, 30), align=None)
    extrude(amount=30)
write(sharp.part, "fins_sheet_flange.step.gz")

with BuildPart() as bent:
    with BuildSketch(Plane.XZ):
        Polygon((0, 0), (40, 0), (40, 2), (2, 2), (2, 30), (0, 30), align=None)
    extrude(amount=30)
edges = bent.part.edges().filter_by(Axis.Y)
outer = [e for e in edges if abs(e.center().X) < 1e-6 and abs(e.center().Z) < 1e-6]
inner = [e for e in edges if abs(e.center().X - 2) < 1e-6 and abs(e.center().Z - 2) < 1e-6]
part = fillet(inner, radius=2)
part = fillet([e for e in part.edges().filter_by(Axis.Y) if abs(e.center().X) < 1e-6 and abs(e.center().Z) < 1e-6], radius=4)
write(part, "fins_sheet_bend.step.gz")

tube = Cylinder(50, 60) - Cylinder(46, 60)
barrel = tube.moved(Location((0, 0, -1)))
wall = (Cylinder(46, 4) - Cylinder(13, 4)).moved(Location((0, 0, -20)))
part = barrel + wall
fin = Box(3.5, 1.4, 38.5).moved(Location(((49.5 - 0.5 + 52.5) / 2, 0, (-26.5 + 12.0) / 2)))
for i in range(64):
    part = part + fin.rotate(Axis.Z, i * 360 / 64)
write(part, "fins_slender_ridges.step.gz")

cross = base
for angle in (0, 90):
    cross = cross + rib_70(angle)
write(cross, "fins_crossing_ribs.step.gz")

bead = Box(60, 1.5, 1.75, align=MIN_Z) + Cylinder(0.75, 60).rotate(Axis.Y, 90).moved(Location((0, 0, 1.75)))
write(Box(100, 60, 6, align=MIN_Z) + bead.moved(Location((0, 0, 6))), "fins_lone_bead.step.gz")

part = Box(60, 40, 6, align=MIN_Z) + Box(60, 6, 30, align=MIN_Z).moved(Location((0, 17, 6)))
part = part + Box(1.5, 30, 2.5, align=MIN_Z).moved(Location((0, -1, 6)))
write(part, "fins_ridge_wall.step.gz")
