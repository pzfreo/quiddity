"""Generate original sealed cavities and open-hole controls; no external geometry.

Regenerate with the project's build123d environment:
    python tools/make_sealed_voids_fixture.py

A 64 x 36 x 24 block contains three sealed radius-2, length-8 cylinders,
spaced 16 mm apart. A second block provides controls: a through bore, a blind
bore, and an open rectangular pocket with a blind bore drilled into its floor.
"""
import gzip
from pathlib import Path
from build123d import Align, Box, Cylinder, Pos, export_step

minimum = (Align.MIN, Align.MIN, Align.MIN)
cylinder = (Align.CENTER, Align.CENTER, Align.MIN)
part = Box(64, 36, 24, align=minimum)
for x in (12, 28, 44):
    part -= Pos(x, 9, 8) * Cylinder(2, 8, align=cylinder)


def write_fixture(shape, name):
    path = Path(__file__).resolve().parents[1] / 'tests/fixtures' / (name + '.step')
    export_step(shape, str(path))
    path.with_suffix('.step.gz').write_bytes(gzip.compress(path.read_bytes(), mtime=0))
    text = path.read_text()
    path.unlink()
    return text


assert 'BREP_WITH_VOIDS' in write_fixture(part, 'sealed_cylindrical_voids')
part = Box(64, 36, 24, align=minimum)
part -= Pos(10, 27, 0) * Cylinder(2.5, 24, align=cylinder)
part -= Pos(26, 27, 18) * Cylinder(3, 6, align=cylinder)
part -= Pos(38, 20, 16) * Box(20, 12, 8, align=minimum)
part -= Pos(48, 26, 10) * Cylinder(3.5, 6, align=cylinder)
write_fixture(part, 'open_hole_controls')
