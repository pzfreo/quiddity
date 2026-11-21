"""Regenerate tests/fixtures/multi_axis_steps.step.gz with build123d.

A synthetic stand-in for quiddity#307 item 10 (no reporter geometry is used): a stepped block
whose steps rise along three axes. A 50 x 60 x 40 block (X 0..50) carries a narrower, lower
50 x 40 x 20 block (X 50..100, Y 0..40), which carries a still narrower 20 x 20 x 10 block
(X 100..120, Y 0..20). Each block's top is a level along Z, each block's +Y side a level along
Y (the part is 60, 40 then 20 wide), and the middle block's +X end a level along X (the part is
40 then 20 wide there, so that end is no riser of the Z steps).

Usage: uv run --no-project --python 3.12 --with build123d \
    python tools/make_multi_axis_step_fixture.py [output directory]
"""

import gzip
import sys
from pathlib import Path

from build123d import Align, Box, Location, export_step

FIXTURES = Path(__file__).resolve().parents[1] / "tests" / "fixtures"
out = Path(sys.argv[1]) if len(sys.argv) > 1 else FIXTURES
MIN = (Align.MIN, Align.MIN, Align.MIN)

part = (
    Box(50, 60, 40, align=MIN)
    + Box(50, 40, 20, align=MIN).moved(Location((50, 0, 0)))
    + Box(20, 20, 10, align=MIN).moved(Location((100, 0, 0)))
)
tmp = out / "multi_axis_steps.step.tmp"
export_step(part, str(tmp))
(out / "multi_axis_steps.step.gz").write_bytes(gzip.compress(tmp.read_bytes(), mtime=0))
tmp.unlink()
