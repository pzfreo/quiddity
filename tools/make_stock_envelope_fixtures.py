"""Regenerate tests/fixtures/stock_envelope_{plate,housing}.step with build123d.

Synthetic stand-ins for quiddity#307 item 10 (no reporter geometry is used):

* stock_envelope_plate.step: a perforated 120 x 80 x 8 mounting plate whose outline has R5
  corners and whose outer top and bottom edges are rounded R0.8, with four holes and a slot.
  The rounding removes every box corner, so the box rules abstain on the top, bottom and
  four outer sides; the stock_envelope rule claims them.
* stock_envelope_housing.step: a finned cylindrical motor housing: an R30/R25 tube, 54 high,
  on a 100 x 100 R8 rounded-square flange (4 holes, outer bottom edges rounded R0.8), with
  sixteen 14 x 2 radial fins. The flange sides and bottom edge rounds exercise the rule; the
  housing top and the fins are not stock envelope.

Usage: uv run --no-project --python 3.12 --with build123d \
    python tools/make_stock_envelope_fixtures.py [output directory]
"""

import gzip
import sys
from pathlib import Path

from build123d import (
    Align,
    Axis,
    Box,
    BuildPart,
    BuildSketch,
    Circle,
    GeomType,
    Location,
    Locations,
    Mode,
    Plane,
    RectangleRounded,
    SlotOverall,
    export_step,
    extrude,
    fillet,
)

FIXTURES = Path(__file__).resolve().parents[1] / "tests" / "fixtures"
out = Path(sys.argv[1]) if len(sys.argv) > 1 else FIXTURES
HOLES = [(-35, -20), (35, -20), (-35, 20), (35, 20)]


def write(part, name):
    tmp = out / (name + ".tmp")
    export_step(part, str(tmp))
    (out / name).write_bytes(gzip.compress(tmp.read_bytes(), mtime=0))
    tmp.unlink()


with BuildPart() as plate:
    with BuildSketch():
        RectangleRounded(120, 80, 5)
        with Locations(*HOLES):
            Circle(3.2, mode=Mode.SUBTRACT)
        SlotOverall(30, 8, mode=Mode.SUBTRACT)
    extrude(amount=8)
edges = [e for e in plate.part.edges().filter_by(Plane.XY) if max(abs(e.center().X), abs(e.center().Y)) > 30]
write(fillet(edges, radius=0.8), "stock_envelope_plate.step.gz")

with BuildPart() as housing:
    with BuildSketch():
        RectangleRounded(100, 100, 8)
        Circle(25, mode=Mode.SUBTRACT)
        with Locations((40, 40), (-40, 40), (40, -40), (-40, -40)):
            Circle(3.5, mode=Mode.SUBTRACT)
    extrude(amount=6)
    with BuildSketch(Plane.XY.offset(6)):
        Circle(30)
        Circle(25, mode=Mode.SUBTRACT)
    extrude(amount=54)
    for i in range(16):
        fin = Box(14, 2, 54, align=(Align.CENTER, Align.CENTER, Align.MIN))
        housing.part = housing.part + fin.moved(Location((35, 0, 6))).rotate(Axis.Z, i * 22.5)
flange_edges = [
    e
    for e in housing.part.edges()
    if abs(e.center().Z) < 1e-6 and max(abs(e.center().X), abs(e.center().Y)) > 40
]
write(fillet(flange_edges, radius=0.8), "stock_envelope_housing.step.gz")
