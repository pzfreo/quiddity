"""Original shaft controls for `flats` versus rib, fin and tab sides (#307).

Regenerate (build123d 0.13.0):
    uv run --no-project --python 3.12 --with build123d==0.13.0 python tools/make_flats_fixtures.py
Each part is an R20 shaft, 40 long (Z = 0..40), with:
  flats_finned_shaft  six 4-thick radial fins reaching R30
  flats_low_tab       one 4-thick tab whose tip stands 0.3 beyond R20
  flats_key_flat      one key flat 12 from the axis over Z = 20..40
"""
import gzip
from pathlib import Path

from build123d import Align, Axis, Box, Cylinder, Location, export_step

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
