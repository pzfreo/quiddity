#!/usr/bin/env python3
"""Paired bbox/body-oriented Plate area-authority aggregate benchmark."""

from __future__ import annotations

import argparse
import json
import math
import platform
import statistics
import subprocess
import sys
import time
from dataclasses import replace
from pathlib import Path
from typing import Any, cast

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
    import quiddity.plates as plates
    from quiddity.result import _take_inventory

    original = plates._oriented_cross_area

    def coordinate_envelope(
        value: Any,
        faces: Any,
        axis_index: int,
        extents: tuple[float, float, float],
        properties: Any,
    ) -> float:
        del value, faces, properties
        return math.prod(extents[index] for index in range(3) if index != axis_index)

    if not enabled:
        plates._oriented_cross_area = cast(Any, coordinate_envelope)
    try:
        started = time.perf_counter()
        product = _take_inventory(part)
        return product.result, time.perf_counter() - started
    finally:
        plates._oriented_cross_area = original


def _measure(parts: list[tuple[str, Any]]) -> dict[str, Any]:
    rows = []
    for index, (model_id, part) in enumerate(parts):
        order = (False, True) if index % 2 == 0 else (True, False)
        measured = {enabled: _run_case(part, enabled) for enabled in order}
        legacy, legacy_seconds = measured[False]
        oriented, oriented_seconds = measured[True]
        rows.append(
            {
                "id": model_id,
                "other_outputs_equal": replace(oriented, plates=legacy.plates) == legacy,
                "legacy_plates": len(legacy.plates),
                "oriented_plates": len(oriented.plates),
                "legacy_records_retained": all(
                    record in oriented.plates for record in legacy.plates
                ),
                "introduced": [
                    record.to_dict() for record in oriented.plates if record not in legacy.plates
                ],
                "legacy_seconds": legacy_seconds,
                "oriented_seconds": oriented_seconds,
                "oriented_first": order[0],
            }
        )
    legacy_times = [row["legacy_seconds"] for row in rows]
    oriented_times = [row["oriented_seconds"] for row in rows]
    return {
        "all_other_outputs_equal": all(row["other_outputs_equal"] for row in rows),
        "all_legacy_records_retained": all(row["legacy_records_retained"] for row in rows),
        "legacy_plates": sum(row["legacy_plates"] for row in rows),
        "oriented_plates": sum(row["oriented_plates"] for row in rows),
        "introduced_plates": sum(len(row["introduced"]) for row in rows),
        "legacy": _summary(legacy_times),
        "oriented": _summary(oriented_times),
        "oriented_to_legacy_total_ratio": sum(oriented_times) / sum(legacy_times),
        "paired_median_delta_seconds": statistics.median(
            right - left for left, right in zip(legacy_times, oriented_times, strict=True)
        ),
        "models": rows,
    }


def _acceptable(report: dict[str, Any]) -> bool:
    return bool(
        report["all_other_outputs_equal"]
        and report["all_legacy_records_retained"]
        and report["oriented_to_legacy_total_ratio"] <= 1.10
    )


def _framed(path: Path) -> Any:
    from quiddity import import_step_geometry as import_step
    from quiddity.frames import RefusedPartFrame, _normalize_part, infer_part_frame

    part = import_step(path)
    frame = infer_part_frame(part)
    if isinstance(frame, RefusedPartFrame):
        raise RuntimeError(f"{path.name}: frame refused: {frame.reason.value}")
    return _normalize_part(part, frame)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("workload", choices=("mfcadpp", "census"))
    parser.add_argument("--root", type=Path)
    parser.add_argument("--limit", type=int, default=500)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

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
        "format": "b123d-recognisers-plate-area-authority-paired-benchmark",
        "format_version": 1,
        "implementation_commit": _commit(),
        "workload": args.workload,
        "selection": [path.name for path in paths],
        "environment": {"python": platform.python_version(), "platform": platform.platform()},
        **_measure([(path.name, _framed(path)) for path in paths]),
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({key: value for key, value in report.items() if key != "models"}, indent=2))
    return 0 if _acceptable(report) else 1


if __name__ == "__main__":
    raise SystemExit(main())
