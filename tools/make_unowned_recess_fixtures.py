"""Original Apache-2.0 solid/open-shell controls, independent of any customer model.

Run with build123d. The shell retains exactly a triangular pocket's three walls
and floor, with an open mouth and no material solid. No sewing or healing is used.
Recognition expectations are asserted in Rust, not supplied by this generator.
"""

import gzip
from pathlib import Path

from build123d import Align, Box, Compound, Face, Pos, Shell, Wire, export_step, extrude
from OCP.BRep import BRep_Builder
from OCP.TopoDS import TopoDS_Shell


def main():
    root = Path(__file__).resolve().parents[1] / "tests/fixtures"
    plate = Box(40, 30, 10, align=(Align.MIN, Align.MIN, Align.MIN))
    triangle = Face(Wire.make_polygon([(8, 8, 6), (25, 8, 6), (15, 22, 6)], close=True))
    solid = plate - extrude(triangle, amount=4)
    pocket_faces = [
        face for face in solid.faces()
        if face.bounding_box().min.X > 1 and face.bounding_box().max.X < 39
        and face.bounding_box().min.Y > 1 and face.bounding_box().max.Y < 29
    ]
    assert len(pocket_faces) == 4
    builder = BRep_Builder()
    topology = TopoDS_Shell()
    builder.MakeShell(topology)
    for face in pocket_faces:
        builder.Add(topology, face.wrapped)
    shell = Shell(topology)
    assert len(shell.solids()) == 0 and len(shell.faces()) == 4
    for name, shape in (
        ("solid", solid),
        ("shell", shell),
        ("mixed", Compound([solid, Pos(60, 0, 0) * shell])),
    ):
        path = root / f"unowned_recess_{name}.step"
        export_step(shape, str(path))
        path.with_suffix(".step.gz").write_bytes(gzip.compress(path.read_bytes(), mtime=0))
        path.unlink()
        print(name, len(shape.faces()), len(shape.solids()))


if __name__ == "__main__":
    main()
