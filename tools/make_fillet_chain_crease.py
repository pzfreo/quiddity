"""Write the original #341 line-crease STEP fixture (build123d 0.13.0)."""

import gzip
import tempfile
from pathlib import Path

import build123d as bd


def make_part():
    # Each box keeps both of its spring walls above or below the overlap.
    # The opposed R2 quarter-cylinders intersect transversely inside that overlap.
    a = bd.Box(12, 12, 10, align=(bd.Align.MIN, bd.Align.MIN, bd.Align.MIN))
    b = bd.Box(12, 12, 10, align=(bd.Align.MAX, bd.Align.MAX, bd.Align.MIN))
    a_edge = min(a.edges().filter_by(bd.Axis.Z), key=lambda e: e.center().X + e.center().Y)
    b_edge = max(b.edges().filter_by(bd.Axis.Z), key=lambda e: e.center().X + e.center().Y)
    a = bd.fillet(a_edge, 2)
    b = bd.fillet(b_edge, 2).moved(bd.Location((1.6, 1.6, 8)))
    part = a.fuse(b)
    assert part.is_valid and len(part.solids()) == 1
    return part


if __name__ == "__main__":
    out = Path(__file__).resolve().parents[1] / "tests" / "fixtures" / "fillet_chain_crease.step.gz"
    with tempfile.TemporaryDirectory() as tmp:
        step = Path(tmp) / "fillet_chain_crease.step"
        bd.export_step(make_part(), step)
        out.write_bytes(gzip.compress(step.read_bytes(), mtime=0))
