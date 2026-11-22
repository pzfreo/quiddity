"""Write the #341 corner, disjoint and mixed-radius STEP fixtures (build123d 0.13.0)."""

import gzip
import tempfile
from pathlib import Path

import build123d as bd


def make_parts():
    box = bd.Box(30, 24, 20)
    yield "fillet_chain_box", bd.fillet(box.edges(), 3)

    box = bd.Box(30, 24, 20)
    vertical = box.edges().filter_by(bd.Axis.Z).sort_by(
        lambda edge: edge.center().X + edge.center().Y
    )
    yield "fillet_chain_disjoint", bd.fillet([vertical[0], vertical[-1]], 3)

    box = bd.Box(30, 24, 20)
    box = bd.fillet(box.edges().filter_by(bd.Axis.Z), 3)
    yield "fillet_chain_mixed", bd.fillet(box.edges().group_by(bd.Axis.Z)[-1], 1)


if __name__ == "__main__":
    fixtures = Path(__file__).resolve().parents[1] / "tests" / "fixtures"
    with tempfile.TemporaryDirectory() as tmp:
        for name, part in make_parts():
            assert part.is_valid and len(part.solids()) == 1
            step = Path(tmp) / f"{name}.step"
            bd.export_step(part, step)
            (fixtures / f"{name}.step.gz").write_bytes(gzip.compress(step.read_bytes(), mtime=0))
