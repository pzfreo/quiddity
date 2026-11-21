"""Original synthetic part for the per-face integration budget (#307 item 3); no external model.

Regenerate with: uv run --no-project --python 3.12 --with build123d \
    python tools/make_integration_budget_fixture.py
A 60 x 40 x 20 block (centred on x and y, z 0..20) with a d8 through hole along z, whose top
is cut by a dense B-spline sheet: a gentle wave interpolated through a 48 x 32 grid of points
(peak-to-peak 2 mm, about 12..14 mm above the base). That one face's area and moment
integration costs far more work than any plane's or the bore's, so a budget between the two
refuses it alone.
"""
import gzip, math
from pathlib import Path
from build123d import Align, Box, Cylinder, Face, Keep, Vector, export_step, split

NX, NY = 48, 32
points = [
    [
        Vector(-35 + 70 * i / (NX - 1), -25 + 50 * j / (NY - 1),
               13 + 0.5 * math.sin(7 * i / (NX - 1) * math.pi) + 0.5 * math.cos(5 * j / (NY - 1) * math.pi))
        for j in range(NY)
    ]
    for i in range(NX)
]
sheet = Face.make_surface_from_array_of_points(points)
block = Box(60, 40, 20, align=(Align.CENTER, Align.CENTER, Align.MIN))
block -= Cylinder(4, 40)
part = split(block, bisect_by=sheet, keep=Keep.BOTTOM)
target = Path(__file__).resolve().parents[1] / 'tests/fixtures/integration_budget_sheet.step'
export_step(part, str(target))
target.with_suffix('.step.gz').write_bytes(gzip.compress(target.read_bytes(), mtime=0))
target.unlink()
