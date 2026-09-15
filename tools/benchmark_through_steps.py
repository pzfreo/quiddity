#!/usr/bin/env python3
"""Paired enabled/disabled timing for rectangular Through Step recognition."""

from __future__ import annotations

import argparse
import json
import platform
import statistics
import subprocess
import sys
import time
from dataclasses import replace
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
    import quiddity.through_steps as family
    from quiddity.result import _take_inventory

    # The declaration resolves its entry point in the family module at call time, so that is
    # where the benchmark disables it; the registry no longer holds the name.
    original = family.recognise_through_steps
    if not enabled:
        family.recognise_through_steps = lambda *_args, **_kwargs: []
    try:
        started = time.perf_counter()
        result = _take_inventory(part).result
        return result, time.perf_counter() - started
    finally:
        family.recognise_through_steps = original


def _measure(parts: list[tuple[str, Any]]) -> dict[str, Any]:
    rows = []
    for index, (model_id, part) in enumerate(parts):
        order = (True, False) if index % 2 == 0 else (False, True)
        measurements = {enabled: _run_case(part, enabled) for enabled in order}
        disabled_result, disabled_seconds = measurements[False]
        enabled_result, enabled_seconds = measurements[True]
        rows.append(
            {
                "id": model_id,
                "pre_existing_outputs_equal": (
                    replace(enabled_result, through_steps=()) == disabled_result
                ),
                "through_steps": len(enabled_result.through_steps),
                "disabled_seconds": disabled_seconds,
                "enabled_seconds": enabled_seconds,
                "enabled_first": order[0],
            }
        )
    disabled = [row["disabled_seconds"] for row in rows]
    enabled = [row["enabled_seconds"] for row in rows]
    return {
        "all_pre_existing_outputs_equal": all(row["pre_existing_outputs_equal"] for row in rows),
        "through_steps": sum(row["through_steps"] for row in rows),
        "disabled": _summary(disabled),
        "enabled": _summary(enabled),
        "enabled_to_disabled_total_ratio": sum(enabled) / sum(disabled),
        "paired_median_delta_seconds": statistics.median(
            right - left for left, right in zip(disabled, enabled, strict=True)
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
        "format": "b123d-recognisers-through-step-paired-benchmark",
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
    return 0 if report["all_pre_existing_outputs_equal"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
