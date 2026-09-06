#!/usr/bin/env python3
"""Paired enabled/disabled timing for Rectangular Blind Slot recognition."""

from __future__ import annotations

import argparse
import json
import platform
import statistics
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

ROOT = Path(__file__).parents[1]
sys.path.insert(0, str(ROOT))


def _commit() -> str:
    return subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, check=True, capture_output=True, text=True
    ).stdout.strip()


def _summary(values: list[float]) -> dict[str, float]:
    return {
        "total_seconds": sum(values),
        "median_seconds": statistics.median(values),
        "maximum_seconds": max(values),
    }


def _run_case(part: Any, enabled: bool) -> tuple[Any, float]:
    import quiddity._registry as registry
    from quiddity.result import _take_inventory

    original = registry.recognise_rectangular_blind_slots
    if not enabled:
        registry.recognise_rectangular_blind_slots = lambda *_args, **_kwargs: []
    try:
        started = time.perf_counter()
        product = _take_inventory(part)
        return product, time.perf_counter() - started
    finally:
        registry.recognise_rectangular_blind_slots = original


def _measure(parts: list[tuple[str, Any]]) -> dict[str, Any]:
    from quiddity._candidates import FamilyId
    from quiddity._dispositions import Outcome, ReasonCode
    from tools._legacy_recognition import detector_outputs_equal

    rows = []
    for index, (model_id, part) in enumerate(parts):
        order = (True, False) if index % 2 == 0 else (False, True)
        measurements = {enabled: _run_case(part, enabled) for enabled in order}
        disabled, disabled_seconds = measurements[False]
        enabled, enabled_seconds = measurements[True]
        accepted = enabled._legacy_result.rectangular_blind_slots
        raw = enabled.physical.candidate_set(FamilyId.RECTANGULAR_BLIND_SLOTS).candidates
        pocket_drops = tuple(
            disposition
            for disposition in enabled.reconciliation.for_family(FamilyId.POCKETS)
            if disposition.outcome is Outcome.REJECTED
            and disposition.reason is ReasonCode.POCKET_SUPERSEDED_BY_RECTANGULAR_BLIND_SLOT
        )
        expected_pocket_delta = len(disabled._legacy_result.pockets) - len(
            enabled._legacy_result.pockets
        )
        other_outputs_equal = detector_outputs_equal(
            enabled._legacy_result,
            disabled._legacy_result,
            excluding=("rectangular_blind_slots", "pockets"),
        )
        rows.append(
            {
                "id": model_id,
                "other_outputs_equal": other_outputs_equal,
                "pocket_delta_matches_reconciliation": expected_pocket_delta == len(pocket_drops),
                "raw_rectangular_blind_slots": len(raw),
                "accepted_rectangular_blind_slots": len(accepted),
                "pockets_superseded": len(pocket_drops),
                "disabled_seconds": disabled_seconds,
                "enabled_seconds": enabled_seconds,
                "enabled_first": order[0],
            }
        )
    disabled_times = [row["disabled_seconds"] for row in rows]
    enabled_times = [row["enabled_seconds"] for row in rows]
    return {
        "all_other_outputs_equal": all(row["other_outputs_equal"] for row in rows),
        "all_pocket_deltas_explained": all(
            row["pocket_delta_matches_reconciliation"] for row in rows
        ),
        "raw_rectangular_blind_slots": sum(row["raw_rectangular_blind_slots"] for row in rows),
        "accepted_rectangular_blind_slots": sum(
            row["accepted_rectangular_blind_slots"] for row in rows
        ),
        "pockets_superseded": sum(row["pockets_superseded"] for row in rows),
        "disabled": _summary(disabled_times),
        "enabled": _summary(enabled_times),
        "enabled_to_disabled_total_ratio": sum(enabled_times) / sum(disabled_times),
        "paired_median_delta_seconds": statistics.median(
            right - left for left, right in zip(disabled_times, enabled_times, strict=True)
        ),
        "models": rows,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("workload", choices=("mfcadpp", "census"))
    parser.add_argument("--root", type=Path)
    parser.add_argument("--limit", type=int, default=30)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    from quiddity import import_step_geometry as import_step

    if args.workload == "mfcadpp":
        if args.root is None:
            parser.error("mfcadpp requires --root")
        paths = sorted(args.root.glob("*.st*p"))[: args.limit]
    else:
        paths = sorted(
            path
            for corpus in ("nist", "gramel")
            for path in (ROOT / "tests" / "corpus" / corpus).glob("*.st*p")
        )
    if not paths:
        parser.error("the selected workload contains no STEP files")
    report = {
        "format": "b123d-recognisers-rectangular-blind-slot-paired-benchmark",
        "format_version": 1,
        "implementation_commit": _commit(),
        "workload": args.workload,
        "selection": [path.name for path in paths],
        "environment": {"python": platform.python_version(), "platform": platform.platform()},
        **_measure([(path.name, import_step(path)) for path in paths]),
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({key: value for key, value in report.items() if key != "models"}, indent=2))
    return 0 if report["all_other_outputs_equal"] and report["all_pocket_deltas_explained"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
