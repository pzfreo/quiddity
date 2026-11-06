"""Original controls for shared hole/pocket floor claims (#205, #207).

Regenerate with the project's build123d environment:
    python tools/make_flat_floor_fixture.py
The same stock contains a blind bore, a bore intersecting a pocket at their
common floor level, a closed rectangular pocket and a pocket continuing into
an edge-open narrow slot. All cuts end at z=8 in 16-mm stock.
"""
import gzip
from pathlib import Path
from build123d import Align, Box, Cylinder, Pos, export_step

minimum = (Align.MIN, Align.MIN, Align.MIN)
cylinder = (Align.CENTER, Align.CENTER, Align.MIN)
part = Box(140, 40, 16, align=minimum)
part -= Pos(12, 20, 8) * Cylinder(4, 8, align=cylinder)
# The cylindrical wall is interrupted by a neighbouring pocket, with a shared floor.
part -= Pos(38, 20, 8) * Cylinder(5, 8, align=cylinder)
part -= Pos(41, 17, 8) * Box(14, 6, 8, align=minimum)
part -= Pos(65, 12, 8) * Box(18, 16, 8, align=minimum)
part -= Pos(100, 8, 8) * Box(18, 28, 8, align=minimum)
part -= Pos(106, 0, 8) * Box(6, 14, 8, align=minimum)
path = Path(__file__).resolve().parents[1] / 'tests/fixtures/flat_floor_controls.step'
export_step(part, str(path))
path.with_suffix('.step.gz').write_bytes(gzip.compress(path.read_bytes(), mtime=0))
path.unlink()
