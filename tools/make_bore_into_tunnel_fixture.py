"""Original analytic bores that run out into a curved tunnel; no external model.

Regenerate with: uv run --no-project --python 3.12 --with build123d \
    python tools/make_bore_into_tunnel_fixture.py
A 240 x 160 x 100 block (z 0..100, y -80..80) has an R22 tunnel along x at z=30. Each
bore is drilled from the top of a boss (R14) whose top face is square to the bore, with the
axis tilted 35 degrees off vertical. All bores pass the tunnel axis at y = `aim` and run
along (0, -sin 35, -cos 35); `s` is the distance from the boss-top centre to the point
where the axis reaches the tunnel axis height. Bores, left to right:
  x=-90 d16 ends at the tunnel axis: wholly in air, no floor face at all
  x=-50 d16 same, aimed 14 mm off the tunnel axis
  x=-10 d16 ends 0.8 mm before the tunnel wall on the axis: the floor disc is a thin
            crescent of material at its edges and air under the axis (a partly walled exit)
  x= 30 d9  blind, flat floor 40 mm deep, in material
  x= 70 d9  blind, flat floor, vertical (no boss), 2 mm above the tunnel crown
  x=110 d9  blind, flat floor aimed at the tunnel axis, 28 mm short of it
"""
import gzip, math
from pathlib import Path
from build123d import Align, Box, Cylinder, Plane, Pos, Rot, Vector, export_step

T = math.radians(35)
D = Vector(0, -math.sin(T), -math.cos(T))
TUNNEL_R = 22.0
block = Box(240, 160, 100, align=(Align.CENTER, Align.CENTER, Align.MIN))
block -= Pos(0, 0, 30) * Rot(0, 90, 0) * Cylinder(TUNNEL_R, 260)

def frame(x, aim):
    """Plane on the boss top, 20 mm above the top face along the axis, aimed at (aim, z=30)."""
    s = (70 + 20 * math.cos(T)) / math.cos(T)  # axis distance from the boss top to z=30
    top = Vector(x, aim + math.sin(T) * s, 30 + math.cos(T) * s)
    return Plane(origin=top, z_dir=D), s

def column(plane, r, start, length):
    return plane * Pos(0, 0, start) * Cylinder(r, length, align=(Align.CENTER, Align.CENTER, Align.MIN))

def tilted(x, aim, r, depth_from_tunnel_axis):
    """Boss plus a bore of radius r ending `depth_from_tunnel_axis` before the tunnel axis."""
    global block
    plane, s = frame(x, aim)
    block += column(plane, 14, 0, 30)
    block -= column(plane, r, -5, s - depth_from_tunnel_axis + 5)

tilted(-90, 0, 8, 0)
tilted(-50, -14, 8, 0)
tilted(-10, 0, 8, TUNNEL_R - 0.8)
# controls
plane, s = frame(30, 0)
block += column(plane, 14, 0, 30)
block -= column(plane, 4.5, -5, 60 + 5)
tilted(110, 0, 4.5, 28)
block -= Pos(70, 0, 54) * Cylinder(4.5, 50, align=(Align.CENTER, Align.CENTER, Align.MIN))
target = Path(__file__).resolve().parents[1] / 'tests/fixtures/bores_into_tunnel.step'
export_step(block, str(target))
target.with_suffix('.step.gz').write_bytes(gzip.compress(target.read_bytes(), mtime=0))
target.unlink()

# Two more parts: a d20 blind hole (floor at z=40) in an 80 x 80 x 60 block with passages along x.
def blind_block(passages):
    b = Box(80, 80, 60, align=(Align.CENTER, Align.CENTER, Align.MIN))
    b -= Pos(0, 0, 40) * Cylinder(10, 25, align=(Align.CENTER, Align.CENTER, Align.MIN))
    for y, z, r in passages:
        b -= Pos(0, y, z) * Rot(0, 90, 0) * Cylinder(r, 100)
    return b

for name, passages in (
    # Two R8 passages at y = +-11 leave a strip of floor across the axis.
    ('floor_strip_between_passages', [(11, 40, 8), (-11, 40, 8)]),
    # An R10 tunnel at y=6, z=36 removes the floor under the axis.
    ('floor_removed_under_axis', [(6, 36, 10)]),
    # A thin R3 cross hole under the floor crosses the axis; most of the floor remains.
    ('floor_under_small_cross_hole', [(0, 38, 3)]),
):
    target = Path(__file__).resolve().parents[1] / f'tests/fixtures/blind_hole_{name}.step'
    export_step(blind_block(passages), str(target))
    target.with_suffix('.step.gz').write_bytes(gzip.compress(target.read_bytes(), mtime=0))
    target.unlink()
