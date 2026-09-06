"""Deterministic authored boundary-blend evidence for Epic #290 / issue #277.

Each case changes only the boundary its named recogniser depends on. The sweep records aggregate
records, loss and reclassification separately at several valid radii. Timing is a separate mode so
the structural JSON remains byte-identical across runs.
"""

from __future__ import annotations

import argparse
import json
import statistics
import sys
from collections import Counter
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path
from time import perf_counter
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from build123d import (  # noqa: E402
    Axis,
    Box,
    Cylinder,
    GeomType,
    Plane,
    Pos,
    RegularPolygon,
    extrude,
    fillet,
)

from quiddity.result import _take_inventory  # noqa: E402

JSON_REPORT = ROOT / "docs" / "benchmarks" / "blend-boundary-sweep.json"
MARKDOWN_REPORT = ROOT / "docs" / "benchmarks" / "blend-boundary-sweep.md"
BASELINE_COMMIT = "5569f1405c87be8156e20726152d481623fee6c0"
IMPLEMENTATION_COMMIT = "50262610a82114276f736baec64278f5fc12b567"
PERFORMANCE_BUDGET_SECONDS = 30.0
PERFORMANCE_MEASUREMENT = {
    "environment": "Python 3.12.14, Linux, shared development host",
    "measured_at": "2026-08-29",
    "median_seconds": 16.098,
    "repeat": 3,
    "workload": "five plain controls plus fifteen blended aggregate inventories",
}

_FAMILIES = (
    "holes",
    "grooves",
    "pockets",
    "prismatic_pockets",
    "pads",
    "polygonal_bosses",
    "fillets",
)


@dataclass(frozen=True, slots=True)
class SweepCase:
    name: str
    expected_family: str
    boundary: str
    blend_kind: str
    radii: tuple[float, ...]
    build: Callable[[float], Any]
    reclassification_families: tuple[str, ...] = ()


def _rectangular_pad(radius: float):
    prism = Box(20, 15, 10)
    vertical = tuple(edge for edge in prism.edges() if abs(float(edge.tangent_at().Z)) > 0.99)
    shaped = prism if radius == 0.0 else fillet(vertical, radius)
    return Box(60, 50, 8) + Pos(20, 17.5, 8) * shaped


def _polygonal_boss(radius: float):
    prism = extrude(RegularPolygon(20, 6), 30)
    vertical = tuple(edge for edge in prism.edges() if abs(float(edge.tangent_at().Z)) > 0.99)
    shaped = prism if radius == 0.0 else fillet(vertical, radius)
    return Box(100, 80, 10) + Pos(0, 0, 5) * shaped


def _blind_pocket(radius: float):
    part = Box(80, 60, 20) - Pos(0, 0, 6) * Box(30, 20, 10)
    if radius == 0.0:
        return part
    floor = next(
        face
        for face in part.faces().filter_by(Plane.XY)
        if abs(float(face.center().Z) - 1.0) < 1e-6 and float(face.area) < 80 * 60 - 1
    )
    return fillet(floor.edges(), radius)


def _through_holes(radius: float):
    part = Box(80, 60, 20)
    part -= Pos(-18, 0, 0) * Cylinder(4, 20)
    part -= Pos(18, 0, 0) * Cylinder(4, 20)
    if radius == 0.0:
        return part
    return fillet(part.edges().filter_by(GeomType.CIRCLE), radius)


def _ring_groove(radius: float):
    part = Cylinder(15, 20)
    part += Pos(0, 0, 12.5) * Cylinder(12, 5)
    part += Pos(0, 0, 22.5) * Cylinder(15, 20)
    if radius == 0.0:
        return part
    boundary = part.edges().filter_by(GeomType.CIRCLE).group_by(Axis.Z)[1]
    return fillet(boundary, radius)


CASES = (
    SweepCase(
        "rectangular-pad-side-boundary",
        "pads",
        "all four vertical side intersections",
        "cylindrical fillet",
        (0.5, 1.0, 2.0),
        _rectangular_pad,
    ),
    SweepCase(
        "polygonal-boss-side-boundary",
        "polygonal_bosses",
        "all six vertical side intersections",
        "cylindrical fillet",
        (0.5, 1.0, 2.0),
        _polygonal_boss,
    ),
    SweepCase(
        "blind-pocket-floor-perimeter",
        "pockets",
        "complete floor perimeter",
        "cylindrical fillet",
        (0.5, 1.0, 1.5),
        _blind_pocket,
        ("prismatic_pockets",),
    ),
    SweepCase(
        "through-hole-rims",
        "holes",
        "both rims of both through bores",
        "toroidal fillet",
        (0.5, 1.0, 1.5),
        _through_holes,
    ),
    SweepCase(
        "ring-groove-lead-ins",
        "grooves",
        "both groove-floor rims",
        "toroidal fillet",
        (0.5, 0.75, 1.0),
        _ring_groove,
    ),
)


def _records(part) -> dict[str, list[dict[str, Any]]]:
    # This pinned report measures historical detector families, not the public API census.
    result = _take_inventory(part)._legacy_result
    records: dict[str, list[dict[str, Any]]] = {}
    for family in _FAMILIES:
        projected = []
        for record in getattr(result, family):
            value = json.loads(json.dumps(record.to_dict(), sort_keys=True))
            if family == "grooves":
                # This historical sweep compares the feature geometry changed by each authored
                # blend boundary. Public turned-profile ownership is an orthogonal schema field
                # with its own contract tests and must not rewrite the #277 geometry evidence.
                value.pop("profile", None)
            projected.append(value)
        records[family] = projected
    return records


def _outcome(
    expected_family: str,
    reclassification_families: tuple[str, ...],
    plain: dict[str, list[dict[str, Any]]],
    blended: dict[str, list[dict[str, Any]]],
) -> str:
    before, after = plain[expected_family], blended[expected_family]
    if after == before:
        return "same-family"
    if after:
        return "changed-record"
    introduced = any(not plain[family] and blended[family] for family in reclassification_families)
    return "reclassified" if introduced else "absent"


def sweep() -> dict[str, Any]:
    cases: dict[str, Any] = {}
    totals: Counter[str] = Counter()
    for case in CASES:
        plain = _records(case.build(0.0))
        if not plain[case.expected_family]:
            raise RuntimeError(f"{case.name} plain control has no {case.expected_family}")
        variants = []
        for radius in case.radii:
            blended_part = case.build(radius)
            if not blended_part.is_valid:
                raise RuntimeError(f"{case.name} radius {radius} produced an invalid solid")
            blended = _records(blended_part)
            outcome = _outcome(case.expected_family, case.reclassification_families, plain, blended)
            totals[outcome] += 1
            variants.append(
                {
                    "radius_model_units": radius,
                    "outcome": outcome,
                    "expected_records": blended[case.expected_family],
                    "introduced_families": [
                        family for family in _FAMILIES if not plain[family] and blended[family]
                    ],
                    "removed_families": [
                        family for family in _FAMILIES if plain[family] and not blended[family]
                    ],
                    "records": blended,
                }
            )
        cases[case.name] = {
            "expected_family": case.expected_family,
            "boundary": case.boundary,
            "blend_kind": case.blend_kind,
            "plain_records": plain,
            "variants": variants,
        }
    return {
        "schema": 1,
        "baseline_commit": BASELINE_COMMIT,
        "implementation_commit": IMPLEMENTATION_COMMIT,
        "selection": "authored cases; only each named recogniser boundary is blended",
        "radii_role": "authored input geometry, not recognition thresholds",
        "performance_budget_seconds": PERFORMANCE_BUDGET_SECONDS,
        "performance_measurement": PERFORMANCE_MEASUREMENT,
        "totals": {
            "cases": len(cases),
            "variants": sum(len(case["variants"]) for case in cases.values()),
            "same-family": totals["same-family"],
            "changed-record": totals["changed-record"],
            "reclassified": totals["reclassified"],
            "absent": totals["absent"],
        },
        "cases": cases,
    }


def markdown(report: dict[str, Any]) -> str:
    total = report["totals"]
    lines = [
        "# Authored boundary-blend sweep",
        "",
        f"Comparison baseline: `{report['baseline_commit']}`. Evaluated implementation:",
        f"`{report['implementation_commit']}`. Generated by",
        "`tools/blend_boundary_sweep.py`.",
        "",
        "Each case changes only the named boundary used by one recogniser. Radii are authored",
        "input geometry, not recognition tolerances. Results come from the aggregate inventory",
        "and separate same-family survival, changed records, absence and reclassification.",
        "",
        "| case | expected family | radii | outcomes |",
        "| --- | --- | ---: | --- |",
    ]
    for name, case in report["cases"].items():
        outcomes = Counter(variant["outcome"] for variant in case["variants"])
        summary = ", ".join(f"{key} {value}" for key, value in sorted(outcomes.items()))
        lines.append(
            f"| `{name}` | `{case['expected_family']}` | {len(case['variants'])} | {summary} |"
        )
    lines.extend(
        [
            "",
            f"Totals: {total['cases']} cases, {total['variants']} blended variants; "
            f"same-family {total.get('same-family', 0)}, changed-record "
            f"{total.get('changed-record', 0)}, reclassified {total.get('reclassified', 0)}, "
            f"absent {total.get('absent', 0)}.",
            "",
            "## Performance gate",
            "",
            f"Recorded on {report['performance_measurement']['measured_at']} under "
            f"{report['performance_measurement']['environment']}: median "
            f"`{report['performance_measurement']['median_seconds']}` seconds over "
            f"{report['performance_measurement']['repeat']} repeats for "
            f"{report['performance_measurement']['workload']}.",
            "",
            f"`--benchmark` fails above `{report['performance_budget_seconds']}` seconds. This",
            "wide harness ceiling detects gross regressions without pretending shared-host timing",
            "is deterministic; live timing is excluded from the byte-pinned structural report.",
            "",
            "## Decision signal",
            "",
            "Rectangular Pad now joins Polygonal Boss as a selected blend-view consumer; both",
            "preserve their exact sharp-control records across all three authored radii.",
            "Hole and Groove already survive their named rim blends through family-local geometry",
            "rules, with dimensions changing because the cylindrical flat span genuinely changes.",
            "Pocket consistently reclassifies to Prismatic Pocket and therefore needs a semantic",
            "and reconciliation decision before any migration. The measured Pad absence is closed.",
            "",
            "This report records the Pad predicate change but makes no MFCAD++ or MFInstSeg",
            "transfer claim. Those end-to-end measurements remain separate evidence gates.",
            "",
        ]
    )
    return "\n".join(lines)


def benchmark(*, repeat: int) -> dict[str, Any]:
    samples = []
    for _ in range(repeat):
        started = perf_counter()
        sweep()
        samples.append(perf_counter() - started)
    median = statistics.median(samples)
    return {
        "environment": f"Python {sys.version.split()[0]}, {sys.platform}",
        "repeat": repeat,
        "median_seconds": median,
        "budget_seconds": PERFORMANCE_BUDGET_SECONDS,
        "passed": median <= PERFORMANCE_BUDGET_SECONDS,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--write", action="store_true", help="update checked-in JSON/Markdown")
    parser.add_argument("--benchmark", action="store_true", help="run the reviewed time budget")
    parser.add_argument("--repeat", type=int, default=3)
    args = parser.parse_args()
    if args.repeat < 1:
        parser.error("--repeat must be positive")
    if args.benchmark:
        measured = benchmark(repeat=args.repeat)
        print(json.dumps(measured, indent=2, sort_keys=True))
        if not measured["passed"]:
            raise SystemExit(1)
        return
    report = sweep()
    rendered = json.dumps(report, indent=2, sort_keys=True) + "\n"
    if args.write:
        JSON_REPORT.write_text(rendered, encoding="utf-8")
        MARKDOWN_REPORT.write_text(markdown(report), encoding="utf-8")
    else:
        print(rendered, end="")


if __name__ == "__main__":
    main()
