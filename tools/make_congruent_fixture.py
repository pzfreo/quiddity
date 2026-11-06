"""Original Apache-2.0 geometry for correspondence::congruent; no external model.

Regenerate tests/congruent/hex_bolt.step. Run with build123d 0.11.1 (cadquery-ocp 7.9.3.1) /
Python 3.14; the committed file was written by Open CASCADE 7.9.

An M3 x 8 hex-head bolt (5.5 mm across flats, 2 mm head, 3 mm shank), head up, built in place
at (36, -10.2411048785, 25.5) rather than moved there by a placement, as exporters write each
copy of a repeated bought part. Off the origin, the fingerprint alignment of two translated
copies returns a 60 or 120 degree turn about the bolt's axis: a symmetry of the hexagon but not
of the shank's seam, which `correspondence::congruent` must reject before trying the
translation (tests/congruent.rs). It lives outside tests/fixtures, which are the Rust snapshot's
inputs: it is not a recognition fixture. No external dataset is used.

Usage: uv run --no-project --python 3.14 --with build123d==0.11.1 \
    --with cadquery-ocp-novtk==7.9.3.1 python tools/make_congruent_fixture.py [output directory]
"""

import sys
from pathlib import Path

from build123d import BuildPart, BuildSketch, Circle, Plane, RegularPolygon, export_step, extrude

FIXTURES = Path(__file__).resolve().parents[1] / "tests" / "congruent"
out = Path(sys.argv[1]) if len(sys.argv) > 1 else FIXTURES
AT = (36.0, -10.2411048785, 25.5)

with BuildPart() as bolt:
    with BuildSketch(Plane(origin=(AT[0], AT[1], AT[2] + 8))):
        RegularPolygon(5.5 / 2, 6, major_radius=False)
    extrude(amount=2)
    with BuildSketch(Plane(origin=AT)):
        Circle(1.5)
    extrude(amount=8)

out.mkdir(parents=True, exist_ok=True)
export_step(bolt.part, str(out / "hex_bolt.step"))
