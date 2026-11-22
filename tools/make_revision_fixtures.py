"""Build revision pairs for the correspondence tests, as a designer would revise a part.

    ../quiddity/.venv/bin/python tools/make_revision_fixtures.py

Writes ``tests/fixtures/revisions/<case>.old.step.gz`` and ``<case>.new.step.gz`` and
``tests/fixtures/revisions/expected.json``: per case, whether an alignment should be found and
the expected class of the features the revision touches, each named by family and by where it
sits (``old_at`` in the old part, ``new_at`` in the new one: the feature's faces' centroid,
which the test takes the nearest feature to). Every other feature is expected to be carried.

The parts are kept asymmetric (holes of different diameters, an off-centre boss) so that the
one right alignment is the only one; symmetric repeats are the patterns' business.
"""

from __future__ import annotations

import gzip
import json
import tempfile
from pathlib import Path

from build123d import (
    Axis,
    Box,
    Cylinder,
    Pos,
    Rot,
    export_step,
    fillet,
)

OUT = Path(__file__).resolve().parent.parent / "tests" / "fixtures" / "revisions"

# The plate every case starts from: 100 × 60 × 10, its top face at z = 10.
PLATE = (100.0, 60.0, 10.0)


def plate(t=PLATE[2]):
    return Pos(PLATE[0] / 2, PLATE[1] / 2, t / 2) * Box(PLATE[0], PLATE[1], t)


def through(part, x, y, d, t=PLATE[2]):
    """A through hole of diameter d at (x, y) in a plate t thick."""
    return part - Pos(x, y, t / 2) * Cylinder(d / 2, t + 2)


def boss(part, x, y, d, h):
    """A cylindrical boss of diameter d and height h on the top face."""
    return part + Pos(x, y, PLATE[2] + h / 2) * Cylinder(d / 2, h)


# Distinct diameters, so no two holes form a pattern or swap under a symmetry.
HOLES = [(15.0, 15.0, 5.0), (40.0, 45.0, 6.0), (85.0, 15.0, 8.0)]


def base(holes=HOLES, with_boss=True, boss_d=20.0):
    p = plate()
    for x, y, d in holes:
        p = through(p, x, y, d)
    if with_boss:
        p = boss(p, 70.0, 40.0, boss_d, 8.0)
    return p


def hole_at(x, y):
    return [x, y, PLATE[2] / 2]


def boss_at(d, h=8.0):
    # A boss's faces are its side and its top: their area-weighted centroid.
    side, top = 3.14159 * d * h, 3.14159 * d * d / 4
    z = (side * (PLATE[2] + h / 2) + top * (PLATE[2] + h)) / (side + top)
    return [70.0, 40.0, z]


def motion(shape, rot, pos):
    return Pos(*pos) * Rot(*rot) * shape


def moved_point(p, rot, pos):
    """Where build123d's Pos(pos) * Rot(rot) takes p (extrinsic X, then Y, then Z)."""
    loc = Pos(*pos) * Rot(*rot)
    v = loc * Pos(*p)
    t = v.position
    return [t.X, t.Y, t.Z]


def cases():
    out = []

    # A hole moved 15 mm along -x.
    moved = [HOLES[0], (25.0, 45.0, 6.0), HOLES[2]]
    out.append(
        dict(
            name="hole_moved",
            old=base(),
            new=base(holes=moved),
            aligned=True,
            expect=[
                dict(
                    label="the 6 mm hole moved",
                    family="holes",
                    class_="adapted",
                    old_at=hole_at(40, 45),
                    new_at=hole_at(25, 45),
                    changes=["position"],
                )
            ],
        )
    )

    # The boss grown from 20 to 26 mm.
    out.append(
        dict(
            name="boss_resized",
            old=base(),
            new=base(boss_d=26.0),
            aligned=True,
            expect=[
                dict(
                    label="the boss resized",
                    family="bosses",
                    class_="adapted",
                    old_at=boss_at(20.0),
                    new_at=boss_at(26.0),
                    changes=["sizes.diameter"],
                )
            ],
        )
    )

    # A 7 mm hole added, then the same revision read backwards: the hole removed.
    added = HOLES + [(30.0, 30.0, 7.0)]
    out.append(
        dict(
            name="hole_added",
            old=base(),
            new=base(holes=added),
            aligned=True,
            expect=[
                dict(
                    label="the 7 mm hole added",
                    family="holes",
                    class_="new",
                    old_at=None,
                    new_at=hole_at(30, 30),
                )
            ],
        )
    )
    out.append(
        dict(
            name="hole_removed",
            old=base(holes=added),
            new=base(),
            aligned=True,
            expect=[
                dict(
                    label="the 7 mm hole removed",
                    family="holes",
                    class_="orphaned",
                    old_at=hole_at(30, 30),
                    new_at=None,
                )
            ],
        )
    )

    # The boss removed: a feature of another family.
    out.append(
        dict(
            name="boss_removed",
            old=base(),
            new=base(with_boss=False),
            aligned=True,
            expect=[
                dict(
                    label="the boss removed",
                    family="bosses",
                    class_="orphaned",
                    old_at=boss_at(20.0),
                    new_at=None,
                )
            ],
        )
    )

    # The whole part re-modelled elsewhere: turned a quarter about z and x, and moved.
    rot, pos = (90.0, 0.0, 90.0), (250.0, -40.0, 33.0)
    # The plate made 2 mm thicker. Every face but the bottom changes, so nothing anchors an
    # alignment, and positions are down-weighted. The identity placement still separates holes
    # alike in everything else when they are far apart (a row at 20 mm pitch), but not when the
    # cost of swapping them is within the margin (a row at 5 mm pitch): those are ambiguous.
    wide = [(30.0, 30.0, 4.0), (50.0, 30.0, 4.0), (70.0, 30.0, 4.0)]
    close = [(55.0, 52.0, 2.5), (60.0, 52.0, 2.5), (65.0, 52.0, 2.5)]

    def thick(t):
        p = plate(t)
        for x, y, d in HOLES + wide + close:
            p = through(p, x, y, d, t)
        return p

    out.append(
        dict(
            name="plate_thickened",
            old=thick(10.0),
            new=thick(12.0),
            aligned=False,
            expect=[
                dict(
                    label=f"the {d:g} mm hole at ({x:g}, {y:g}) deepened",
                    family="holes",
                    class_="adapted",
                    old_at=hole_at(x, y),
                    new_at=[x, y, 6.0],
                    changes=["sizes.depth"],
                )
                for x, y, d in HOLES + wide
            ]
            + [
                dict(
                    label=f"the close row's hole at x = {x:g}",
                    family="holes",
                    class_="ambiguous",
                    old_at=hole_at(x, y),
                    new_at=[x, y, 6.0],
                )
                for x, y, _ in close
            ]
            + [
                dict(
                    label=f"the {name} row",
                    family="hole_patterns",
                    class_="carried",
                    old_at=[60.0 if name == "close" else 50.0, y, 5.0],
                    new_at=[60.0 if name == "close" else 50.0, y, 6.0],
                )
                for name, y in (("wide", 30.0), ("close", 52.0))
            ],
        )
    )

    out.append(
        dict(
            name="whole_part_moved",
            old=base(),
            new=motion(base(), rot, pos),
            aligned=True,
            expect=[],
        )
    )

    # The whole part moved, and a hole moved too.
    out.append(
        dict(
            name="part_and_hole_moved",
            old=base(),
            new=motion(base(holes=moved), rot, pos),
            aligned=True,
            expect=[
                dict(
                    label="the 6 mm hole moved",
                    family="holes",
                    class_="adapted",
                    old_at=hole_at(40, 45),
                    new_at=moved_point(hole_at(25, 45), rot, pos),
                    changes=["position"],
                )
            ],
        )
    )

    # A row of four 4 mm holes at 12 mm pitch grown to six.
    def row(n):
        return [(20.0 + 12.0 * k, 30.0, 4.0) for k in range(n)]

    four, six = HOLES + row(4), HOLES + row(6)
    out.append(
        dict(
            name="pattern_grown",
            old=base(holes=four, with_boss=False),
            new=base(holes=six, with_boss=False),
            aligned=True,
            expect=[
                dict(
                    label="the row of holes",
                    family="hole_patterns",
                    class_="adapted",
                    old_at=[38.0, 30.0, 5.0],
                    new_at=[50.0, 30.0, 5.0],
                    changes=["sizes.count"],
                ),
                dict(
                    label="the fifth hole",
                    family="holes",
                    class_="new",
                    old_at=None,
                    new_at=hole_at(68, 30),
                ),
                dict(
                    label="the sixth hole",
                    family="holes",
                    class_="new",
                    old_at=None,
                    new_at=hole_at(80, 30),
                ),
            ],
        )
    )

    # A fillet on one vertical edge, radius 5 grown to 8.
    def filleted(r):
        p = base(with_boss=False)
        edge = (
            p.edges()
            .filter_by(Axis.Z)
            .sort_by(lambda e: (e.center().X - 100) ** 2 + (e.center().Y - 60) ** 2)[0]
        )
        return fillet(edge, r)

    out.append(
        dict(
            name="fillet_resized",
            old=filleted(5.0),
            new=filleted(8.0),
            aligned=True,
            expect=[
                dict(
                    label=label,
                    family=family,
                    class_="adapted",
                    old_at=[97.0, 57.0, 5.0],
                    new_at=[95.0, 55.0, 5.0],
                    changes=["sizes.radius"],
                )
                for label, family in [
                    ("the corner fillet", "fillets"),
                    ("the corner chain", "fillet_chains"),
                    ("the corner radius group", "fillet_groups"),
                ]
            ],
        )
    )
    return out


def write_step(shape, path):
    with tempfile.TemporaryDirectory() as tmp:
        step = Path(tmp) / "part.step"
        export_step(shape, str(step))
        data = step.read_bytes()
    # A fixed timestamp, so a re-run rewrites identical files.
    with open(path, "wb") as raw:
        with gzip.GzipFile(fileobj=raw, mode="wb", mtime=0, filename="") as gz:
            gz.write(data)


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    expected = []
    for case in cases():
        write_step(case["old"], OUT / f"{case['name']}.old.step.gz")
        write_step(case["new"], OUT / f"{case['name']}.new.step.gz")
        expected.append(
            dict(
                name=case["name"],
                aligned=case["aligned"],
                expect=[
                    {("class" if k == "class_" else k): v for k, v in e.items()}
                    for e in case["expect"]
                ],
            )
        )
    (OUT / "expected.json").write_text(
        json.dumps({"tool": "tools/make_revision_fixtures.py", "cases": expected}, indent=2) + "\n"
    )
    print(f"{len(expected)} revision pairs in {OUT}")


if __name__ == "__main__":
    main()
