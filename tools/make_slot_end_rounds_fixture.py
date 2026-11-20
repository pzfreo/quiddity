"""Original analytic plate with a rounded slot; no external model (#307 item 7).

Regenerate with: uv run --no-project --python 3.12 --with build123d \
    python tools/make_slot_end_rounds_fixture.py
slot_end_rounds.step.gz: a 100 x 60 x 8 plate (R5 vertical corners) with a 30 x 8 through slot
at x = 32 along y, every straight edge rounded R0.8 (the slot's two end half-rings become torus
faces).
Three more solids must not read as slot-end rounds: round_hole_mouth.step.gz (R0.8 on a d8 hole:
a whole ring, a `blends` record), bore_groove.step.gz (a concave R1.5 torus groove in a d8 bore)
and bore_ridge.step.gz (a convex R1 torus ridge inside a d8 bore).
"""
import gzip
import sys
from pathlib import Path

from build123d import *

out = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parents[1] / "tests" / "fixtures"


def write(shape, name):
    tmp = out / (name + ".tmp.step")
    export_step(shape, str(tmp))
    (out / (name + ".step.gz")).write_bytes(gzip.compress(tmp.read_bytes(), mtime=0))
    tmp.unlink()


with BuildPart() as p:
    Box(100, 60, 8, align=(Align.CENTER, Align.CENTER, Align.MIN))
    fillet(p.edges().filter_by(Axis.Z), 5)
    with BuildSketch(Plane.XY):
        with Locations((32, 0)):
            SlotCenterToCenter(22, 8, rotation=90)
    extrude(amount=8, mode=Mode.SUBTRACT)
    fillet(p.edges().filter_by(GeomType.LINE), 0.8)
write(p.part, "slot_end_rounds")

with BuildPart() as p:
    Box(60, 40, 8, align=(Align.CENTER, Align.CENTER, Align.MIN))
    with Locations((0, 0, 8)):
        Hole(4)
    fillet([e for e in p.edges() if e.geom_type == GeomType.CIRCLE], 0.8)
write(p.part, "round_hole_mouth")

for name, mode, r in (("bore_groove", Mode.SUBTRACT, 1.5), ("bore_ridge", Mode.ADD, 1.0)):
    with BuildPart() as p:
        Cylinder(20, 20, align=(Align.CENTER, Align.CENTER, Align.MIN))
        Hole(8)
        with BuildSketch(Plane.XZ):
            with Locations((4, 10)):
                Circle(r)
        revolve(axis=Axis.Z, mode=mode)
    write(p.part, name)
