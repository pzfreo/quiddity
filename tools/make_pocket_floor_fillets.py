"""Original controls for #228; build123d, run with the project venv's python -I.

pocket_plain and pocket_floor_fillet reproduce ISSUE-228.md verbatim (renamed
with underscores to match fixture naming). pocket_partial_floor_fillet is the
same stock and sharp rectangular tool, with only its two long bottom edges
rounded. STEP files supplied with the issue are retained byte-for-byte for the
first two; regeneration may change STEP metadata but uses this exact geometry.
"""
import gzip
import sys
from pathlib import Path
from build123d import (Align, Axis, Box, BuildSketch, Plane, Rectangle,
                      RectangleRounded, export_step, extrude, fillet, Cylinder,
                      BuildPart, Polygon, GeomType, Location)

root = Path(__file__).resolve().parents[1] / 'tests' / 'fixtures'
if "--near-misses-only" not in sys.argv:
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


# Review controls for #228, from rv228/gen.py (independent review, 2026-11-09).
# The source directory is read-only input. These constructions reproduce its
# ball_corner, sharpcorner, deep_floor_eq_depth, open_slot, island, floor_hole,
# rim and sloped parts. New fixtures use deterministic gzip containers; STEP
# header timestamps can change when regenerated. The original three stay intact
# when this script is invoked with --near-misses-only.
def plate():
    return Box(60, 40, 10, align=(Align.CENTER, Align.CENTER, Align.MIN))


def tool(w=30, h=20, r=5, floor=1.5, x=0):
    with BuildSketch(Plane.XY.offset(6).move(Location((x, 0)))) as sk:
        if r:
            RectangleRounded(w, h, r)
        else:
            Rectangle(w, h)
    t = extrude(sk.sketch, amount=5)
    return fillet(t.faces().sort_by(Axis.Z)[0].edges(), floor) if floor else t


def island():
    p = plate() - tool(floor=0)
    p += Cylinder(3, 3, align=(Align.CENTER, Align.CENTER, Align.MIN)).moved(Location((0, 0, 6)))
    edges = p.edges().filter_by(GeomType.LINE).filter_by(
        lambda e: abs(e.center().Z - 6) < 1e-6 and abs(e.center().X) < 16 and abs(e.center().Y) < 11)
    return fillet(edges, 1.0) if edges else p


def rim():
    p = plate() - tool(floor=0)
    edges = p.edges().filter_by(
        lambda e: abs(e.center().Z - 10) < 1e-6 and abs(e.center().X) < 16 and abs(e.center().Y) < 11)
    return fillet(edges, 1.0)


def sloped():
    with BuildPart() as bp:
        with BuildSketch(Plane.XZ):
            Polygon((-15, 6), (15, 7), (15, 11), (-15, 11), align=None)
        extrude(amount=10, both=True)
    t = bp.part
    t = fillet(t.edges().filter_by(
        lambda e: e.center().Z < 7.5 and abs(abs(e.center().Y) - 10) < 1e-6), 1.0)
    return plate() - t


cases = {
    'pocket_ball_floor': lambda: plate() - tool(r=3, floor=3),
    'pocket_sharp_corner_floor': lambda: plate() - tool(r=0),
    'pocket_deep_floor': lambda: plate() - tool(floor=3.9),
    'pocket_floor_open_slot': lambda: plate() - tool(w=50, r=0, x=15),
    'pocket_floor_island': island,
    'pocket_floor_hole': lambda: plate() - tool() - Cylinder(2, 20),
    'pocket_mouth_rim': rim,
    'pocket_sloped_floor': sloped,
}
for name, build in cases.items():
    shape = build()
    assert shape.is_valid
    path = root / (name + '.step')
    export_step(shape, str(path))
    path.with_suffix('.step.gz').write_bytes(gzip.compress(path.read_bytes(), mtime=0))
    path.unlink()
