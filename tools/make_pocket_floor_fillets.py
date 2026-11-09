"""Original controls for #228; build123d, run with the project venv's python -I.

pocket_plain and pocket_floor_fillet reproduce ISSUE-228.md verbatim (renamed
with underscores to match fixture naming). pocket_partial_floor_fillet is the
same stock and sharp rectangular tool, with only its two long bottom edges
rounded. STEP files supplied with the issue are retained byte-for-byte for the
first two; regeneration may change STEP metadata but uses this exact geometry.
"""
from pathlib import Path
from build123d import (Align, Axis, Box, BuildSketch, Plane, Rectangle,
                      RectangleRounded, export_step, extrude, fillet)

root = Path(__file__).resolve().parents[1] / 'tests' / 'fixtures'
for name, radius, rounded in (("pocket_plain", 0, True),
                              ("pocket_floor_fillet", 1.5, True),
                              ("pocket_partial_floor_fillet", 1.5, False)):
    plate = Box(60, 40, 10, align=(Align.CENTER, Align.CENTER, Align.MIN))
    with BuildSketch(Plane.XY.offset(6)) as sk:
        if rounded:
            RectangleRounded(30, 20, 5)
        else:
            Rectangle(30, 20)
    tool = extrude(sk.sketch, amount=4)
    if radius:
        edges = tool.faces().sort_by(Axis.Z)[0].edges()
        if not rounded:
            edges = [e for e in edges if abs(e.length - 30) < 1e-7]
        tool = fillet(edges, radius)
    export_step(plate - tool, str(root / f'{name}.step'))
