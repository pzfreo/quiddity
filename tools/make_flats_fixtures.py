"""Original shaft controls for `flats` versus rib, fin and tab sides (#307).

Regenerate (build123d 0.13.0):
    uv run --no-project --python 3.12 --with build123d==0.13.0 python tools/make_flats_fixtures.py
Each part is an R20 shaft, 40 long (Z = 0..40), with:
  flats_finned_shaft  six 4-thick radial fins reaching R30
  flats_low_tab       one 4-thick tab whose tip stands 0.3 beyond R20
  flats_key_flat      one key flat 12 from the axis over Z = 20..40
  flats_chamfer_overrun  R10 shaft (Z 0..30), R15 collar (Z 30..40) with a 2 mm chamfer
                      on its outer lower edge; a flat 7 from the axis runs on into the chamfer
  flats_boss_through_block  an R5 boss, 10 high, poking 2 mm through the x = 3 side of a
                      60 x 6 x 10 block (the side is not a flat)
  flats_over_land     R10 shaft with a 4 mm R10.6 land (Z 20..24); a flat 7 from the axis
                      over Z = 10..40 crosses the land
"""
import gzip
from pathlib import Path

from build123d import Align, Axis, Box, Cylinder, GeomType, Location, SortBy, chamfer, export_step

OUT = Path(__file__).resolve().parents[1] / "tests" / "fixtures"
MIN_Z = (Align.CENTER, Align.CENTER, Align.MIN)


def write(part, name):
    tmp = OUT / (name + ".tmp")
    export_step(part, str(tmp))
    (OUT / name).write_bytes(gzip.compress(tmp.read_bytes(), mtime=0))
    tmp.unlink()


def shaft():
    return Cylinder(20, 40, align=MIN_Z)


part = shaft()
for i in range(6):
    fin = Box(15, 4, 30, align=MIN_Z).moved(Location((22.5, 0, 5)))
    part = part + fin.rotate(Axis.Z, i * 60)
write(part, "flats_finned_shaft.step.gz")

part = shaft() + Box(1.3, 4, 30, align=MIN_Z).moved(Location((19.65, 0, 5)))
write(part, "flats_low_tab.step.gz")

part = shaft() - Box(10, 60, 20, align=MIN_Z).moved(Location((17, 0, 20)))
write(part, "flats_key_flat.step.gz")


def flat_cut(offset, z0, z1):
    return Box(100, 200, z1 - z0, align=(Align.MIN, Align.CENTER, Align.MIN)).moved(
        Location((offset, 0, z0))
    )


def cyl(radius, height, z=0):
    return Cylinder(radius, height, align=MIN_Z).moved(Location((0, 0, z)))


part = cyl(10, 30) + cyl(15, 10, 30)
rim = part.edges().filter_by(GeomType.CIRCLE).group_by(Axis.Z)[1].sort_by(SortBy.RADIUS)[-1]
write(chamfer(rim, 2.0) - flat_cut(7, 15, 31), "flats_chamfer_overrun.step.gz")

write(cyl(10, 40) + cyl(10.6, 4, 20) - flat_cut(7, 10, 40), "flats_over_land.step.gz")

part = cyl(5, 10) + Box(60, 6, 10, align=(Align.MAX, Align.CENTER, Align.MIN)).moved(Location((3, 0, 0)))
write(part, "flats_boss_through_block.step.gz")
