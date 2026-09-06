"""Deterministic external-corpus evaluation for the part-relative frame representative.

The tool uses development data only.  It compares accepted occurrences by defining-face evidence,
records every refusal/error, and separates import, normalization and recognition time.  Progress is
written to stderr so stdout remains one machine-readable JSON document.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import platform
import subprocess
import sys
import time
from collections import Counter
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from build123d import Axis, Pos  # noqa: E402

from quiddity import __version__  # noqa: E402
from quiddity import import_step_geometry as import_step  # noqa: E402
from quiddity.frames import (  # noqa: E402
    PartFrame,
    RefusedPartFrame,
    _normalize_part,
    infer_part_frame,
)
from tools.rigid_motion_sweep import Occurrence, _match, _occurrences  # noqa: E402

MFCADPP_TEST_SPLIT = (
    "MFCAD++ published test split; DOI 10.17034/d1fec5a0-8c10-4630-b02e-b92dc81df823"
)


def _selected_ids_sha256(paths: list[Path]) -> str:
    selected_ids = "".join(f"{path.stem}\n" for path in paths)
    return hashlib.sha256(selected_ids.encode()).hexdigest()


def _commit() -> str:
    return subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()


def _comparison(left: tuple[Occurrence, ...], right: tuple[Occurrence, ...]) -> dict[str, Any]:
    pairs, absent_indices, introduced_indices = _match(left, right)
    same = Counter()
    transitions = Counter()
    for left_index, right_index in pairs:
        before, after = left[left_index], right[right_index]
        if before.family == after.family:
            same[before.family] += 1
        else:
            transitions[f"{before.family}->{after.family}"] += 1
    absent = Counter(left[index].family for index in absent_indices)
    introduced = Counter(right[index].family for index in introduced_indices)
    return {
        "baseline": len(left),
        "same_family": sum(same.values()),
        "reclassified": sum(transitions.values()),
        "absent": len(absent_indices),
        "introduced": len(introduced_indices),
        "same_by_family": dict(sorted(same.items())),
        "transitions": dict(sorted(transitions.items())),
        "absent_by_family": dict(sorted(absent.items())),
        "introduced_by_family": dict(sorted(introduced.items())),
    }


def _merge(total: Counter[str], comparison: dict[str, Any]) -> None:
    for key in ("baseline", "same_family", "reclassified", "absent", "introduced"):
        total[key] += comparison[key]
    for field in ("transitions", "absent_by_family", "introduced_by_family"):
        for key, value in comparison[field].items():
            total[f"{field}:{key}"] += value


def evaluate(root: Path, *, limit: int = 500, progress_every: int = 25) -> dict[str, Any]:
    paths = sorted(root.glob("*.step"), key=lambda path: path.name)[:limit]
    if len(paths) < limit:
        raise ValueError(f"requested {limit} models but only {len(paths)} STEP files exist")

    raw_total: Counter[str] = Counter()
    framed_total: Counter[str] = Counter()
    compatibility_total: Counter[str] = Counter()
    gauges: Counter[str] = Counter()
    timings: Counter[str] = Counter()
    mismatches: list[dict[str, Any]] = []
    refused: list[dict[str, str]] = []
    completed = 0
    started = time.perf_counter()

    for index, path in enumerate(paths, 1):
        stage = "import"
        try:
            tick = time.perf_counter()
            part = import_step(str(path))
            timings["import_seconds"] += time.perf_counter() - tick
            presented = Pos(173, -91, 42) * part.rotate(Axis.X, 30)

            stage = "raw-baseline-recognition"
            tick = time.perf_counter()
            raw_baseline = _occurrences(part)
            timings["raw_recognition_seconds"] += time.perf_counter() - tick
            stage = "raw-presented-recognition"
            tick = time.perf_counter()
            raw_presented = _occurrences(presented)
            timings["raw_recognition_seconds"] += time.perf_counter() - tick

            stage = "baseline-frame"
            tick = time.perf_counter()
            baseline_frame = infer_part_frame(part)
            timings["frame_inference_seconds"] += time.perf_counter() - tick
            if isinstance(baseline_frame, RefusedPartFrame):
                refused.append(
                    {"file": path.name, "stage": stage, "reason": baseline_frame.reason.value}
                )
                continue
            stage = "presented-frame"
            tick = time.perf_counter()
            presented_frame = infer_part_frame(presented)
            timings["frame_inference_seconds"] += time.perf_counter() - tick
            if isinstance(presented_frame, RefusedPartFrame):
                refused.append(
                    {"file": path.name, "stage": stage, "reason": presented_frame.reason.value}
                )
                continue
            assert isinstance(baseline_frame, PartFrame)
            assert isinstance(presented_frame, PartFrame)
            gauges[baseline_frame.gauge.value] += 1

            stage = "normalization"
            tick = time.perf_counter()
            normalized_baseline = _normalize_part(part, baseline_frame)
            normalized_presented = _normalize_part(presented, presented_frame)
            timings["normalization_seconds"] += time.perf_counter() - tick
            stage = "framed-baseline-recognition"
            tick = time.perf_counter()
            framed_baseline = _occurrences(normalized_baseline)
            timings["framed_recognition_seconds"] += time.perf_counter() - tick
            stage = "framed-presented-recognition"
            tick = time.perf_counter()
            framed_presented = _occurrences(normalized_presented)
            timings["framed_recognition_seconds"] += time.perf_counter() - tick

            raw = _comparison(raw_baseline, raw_presented)
            framed = _comparison(framed_baseline, framed_presented)
            compatibility = _comparison(raw_baseline, framed_baseline)
            _merge(raw_total, raw)
            _merge(framed_total, framed)
            _merge(compatibility_total, compatibility)
            if any(
                comparison[key]
                for comparison in (framed, compatibility)
                for key in ("reclassified", "absent", "introduced")
            ):
                mismatches.append(
                    {
                        "file": path.name,
                        "framed_rigid_motion": framed,
                        "legacy_to_framed_baseline": compatibility,
                    }
                )
            completed += 1
        except Exception as exc:  # noqa: BLE001 - corpus errors are evidence, not a crashed run
            refused.append(
                {"file": path.name, "stage": stage, "reason": f"{type(exc).__name__}: {exc}"}
            )
        if progress_every and index % progress_every == 0:
            elapsed = time.perf_counter() - started
            print(
                f"progress {index}/{limit} completed={completed} refused={len(refused)} "
                f"elapsed={elapsed:.1f}s",
                file=sys.stderr,
                flush=True,
            )

    return {
        "schema": 1,
        "implementation_commit": _commit(),
        "package_version": __version__,
        "environment": {
            "python": platform.python_version(),
            "platform": platform.platform(),
        },
        "dataset": "MFCAD++ test split (development evidence)",
        "dataset_version": MFCADPP_TEST_SPLIT,
        "selection": f"first {limit} STEP filenames, lexical ascending",
        "selected_ids_sha256": _selected_ids_sha256(paths),
        "presentation": "X30 then translation (173, -91, 42)",
        "requested_models": limit,
        "completed_models": completed,
        "gauges": dict(sorted(gauges.items())),
        "raw_rigid_motion": dict(sorted(raw_total.items())),
        "framed_rigid_motion": dict(sorted(framed_total.items())),
        "legacy_to_framed_baseline": dict(sorted(compatibility_total.items())),
        "timings": {key: round(value, 6) for key, value in sorted(timings.items())},
        "wall_seconds": round(time.perf_counter() - started, 6),
        "mismatches": mismatches,
        "refused": refused,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path, help="directory containing MFCAD++ STEP files")
    parser.add_argument("--limit", type=int, default=500)
    parser.add_argument("--progress-every", type=int, default=25)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    rendered = json.dumps(
        evaluate(args.root, limit=args.limit, progress_every=args.progress_every),
        indent=2,
        sort_keys=True,
    )
    if args.output is not None:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(rendered + "\n", encoding="utf-8")
    else:
        print(rendered)


if __name__ == "__main__":
    main()
