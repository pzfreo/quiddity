#!/usr/bin/env python3
"""Produce a canonical Epic 0005 effectiveness report from a labelled STEP corpus."""

from __future__ import annotations

import argparse
import concurrent.futures
import contextlib
import hashlib
import json
import multiprocessing
import os
import platform
import subprocess
import sys
import tempfile
import time
from collections import Counter
from collections.abc import Callable, Iterable
from dataclasses import dataclass
from pathlib import Path
from typing import Any

ROOT = Path(__file__).parents[1]
sys.path.insert(0, str(ROOT))
DEFAULT_TAXONOMY = ROOT / "docs" / "benchmarks" / "effectiveness-taxonomy-v13.json"

from tools.effectiveness_report import (  # noqa: E402
    REPORT_FORMAT,
    REPORT_FORMAT_VERSION,
    DatasetTruth,
    EffectivenessDataError,
    canonical_json,
    load_mfcadpp_truth,
    load_mfinstseg_truth,
    load_taxonomy,
    score_inventory,
    summarize_rows,
    summarize_runtime,
    validate_report,
)


def _git_commit() -> str:
    return subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()


def _git_worktree_sha256(commit: str) -> str | None:
    """Fingerprint tracked changes relative to *commit*, or ``None`` when clean."""
    completed = subprocess.run(
        ["git", "diff", "--binary", commit, "--"],
        cwd=ROOT,
        capture_output=True,
    )
    if completed.returncode != 0:
        raise EffectivenessDataError("could not verify corpus-run source authority")
    return hashlib.sha256(completed.stdout).hexdigest() if completed.stdout else None


def _source_digest() -> str:
    """Fingerprint the Python bytes that can be imported by the corpus worker."""

    digest = hashlib.sha256()
    paths = sorted((*ROOT.glob("src/**/*.py"), *ROOT.glob("tools/**/*.py")))
    for path in paths:
        relative = path.relative_to(ROOT).as_posix().encode("utf-8")
        digest.update(len(relative).to_bytes(4, "big"))
        digest.update(relative)
        try:
            contents = path.read_bytes()
        except OSError as error:
            raise EffectivenessDataError("could not fingerprint corpus-run source") from error
        digest.update(len(contents).to_bytes(8, "big"))
        digest.update(contents)
    return digest.hexdigest()


@dataclass(frozen=True, slots=True)
class _RunAuthority:
    commit: str
    source_sha256: str
    taxonomy: bytes
    taxonomy_sha256: str
    worktree_sha256: str | None = None


_KNOWN_MFCADPP_2500_INVALID = frozenset(
    {"12939", "13975", "14052", "14307", "18628", "22386", "22439"}
)


@dataclass(frozen=True, slots=True)
class _ModelTask:
    truth: DatasetTruth
    taxonomy: dict[int, dict[str, Any]]
    recognition_frame: str
    input_error: str | None = None


def _score_model(task: _ModelTask) -> dict[str, Any]:
    """Evaluate one immutable model task in either this process or a worker."""

    if task.input_error is not None:
        return {
            "model_id": task.truth.model_id,
            "status": "invalid",
            "reason": task.input_error,
        }

    from quiddity import import_step_geometry as import_step
    from quiddity.frames import RefusedPartFrame, _normalize_part, infer_part_frame
    from quiddity.result import _take_inventory

    try:
        part = import_step(task.truth.step_path)
        started = time.perf_counter()
        working_part = part
        if task.recognition_frame == "framed":
            frame = infer_part_frame(part)
            if isinstance(frame, RefusedPartFrame):
                raise EffectivenessDataError(f"frame refused: {frame.reason.value}")
            working_part = _normalize_part(part, frame)
        product = _take_inventory(working_part)
        seconds = time.perf_counter() - started
        row = score_inventory(
            task.truth,
            working_part,
            product,
            task.taxonomy,
            seconds,
        )
        row["status"] = "evaluated"
        return row
    except (EffectivenessDataError, OSError, RuntimeError, ValueError) as error:
        return {"model_id": task.truth.model_id, "status": "invalid", "reason": str(error)}


def _unreadable_truth(dataset: str, root: Path, model_id: str) -> DatasetTruth:
    """Fingerprint malformed selected inputs while retaining their invalid report row."""

    candidates: tuple[Path, ...]
    if dataset == "mfcadpp":
        candidates = (root / f"{model_id}.step", root / f"{model_id}.stp")
    else:
        candidates = (
            root / "steps" / f"{model_id}.step",
            root / "steps" / f"{model_id}.stp",
            root / "labels" / f"{model_id}.json",
        )
    present = tuple(path for path in candidates if path.is_file())
    digest = hashlib.sha256()
    for path in candidates:
        name = str(path.resolve()).encode()
        digest.update(len(name).to_bytes(4, "big"))
        digest.update(name)
        if path in present:
            digest.update(b"\x01")
            contents = path.read_bytes()
            digest.update(len(contents).to_bytes(8, "big"))
            digest.update(contents)
        else:
            digest.update(b"\x00")
    step_path = next(
        (path for path in present if path.suffix.lower() in {".step", ".stp"}),
        candidates[0],
    )
    return DatasetTruth(
        model_id=model_id,
        step_path=step_path,
        semantic=(),
        instances=(),
        bottom=None,
        source_sha256=digest.hexdigest(),
    )


def _source_selection_hash(truths: Iterable[DatasetTruth]) -> str:
    digest = hashlib.sha256()
    for truth in truths:
        value = (f"{truth.model_id}\0{truth.step_path.resolve()}\0{truth.source_sha256}\n").encode()
        digest.update(value)
    return digest.hexdigest()


def _checkpoint_authority(
    authority: _RunAuthority,
    *,
    dataset: str,
    dataset_version: str,
    ids: list[str],
    truths: list[DatasetTruth],
    selection_limit: int | None,
    recognition_frame: str,
    allow_invalid: bool,
) -> dict[str, Any]:
    return {
        "format": "b123d-recognisers-effectiveness-checkpoint",
        "format_version": 2,
        "commit": authority.commit,
        "worktree_sha256": authority.worktree_sha256,
        "source_sha256": authority.source_sha256,
        "taxonomy_sha256": authority.taxonomy_sha256,
        "dataset": dataset,
        "dataset_version": dataset_version,
        "selected_ids_sha256": _selection_hash(ids),
        "selected_sources_sha256": _source_selection_hash(truths),
        "selection_limit": selection_limit,
        "recognition_frame": recognition_frame,
        "allow_invalid": allow_invalid,
    }


def _atomic_replace(path: Path, contents: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w", encoding="utf-8", newline="", dir=path.parent, delete=False
        ) as handle:
            temporary = Path(handle.name)
            handle.write(contents)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
        temporary = None
    except OSError as error:
        raise EffectivenessDataError(f"could not write checkpoint {path}: {error}") from error
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def _create_checkpoint_manifest(path: Path, contents: str) -> None:
    """Publish one immutable authority manifest without a concurrent overwrite race."""

    temporary: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w", encoding="utf-8", newline="", dir=path.parent, delete=False
        ) as handle:
            temporary = Path(handle.name)
            handle.write(contents)
            handle.flush()
            os.fsync(handle.fileno())
        with contextlib.suppress(FileExistsError):
            os.link(temporary, path)
    except OSError as error:
        raise EffectivenessDataError(
            f"could not create checkpoint authority {path}: {error}"
        ) from error
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def _checkpoint_key(model_id: str) -> str:
    return hashlib.sha256(model_id.encode("utf-8")).hexdigest()


def _require_known_invalid_policy(dataset: str, ids: list[str], allow_invalid: bool) -> None:
    if (
        dataset == "mfcadpp"
        and len(ids) == 2500
        and set(ids) >= _KNOWN_MFCADPP_2500_INVALID
        and not allow_invalid
    ):
        raise EffectivenessDataError(
            "the known MFCAD++-2,500 selection contains seven invalid models; "
            "supply the documented --allow-invalid policy before recognition"
        )


def _prepare_checkpoint(
    root: Path, checkpoint_authority: dict[str, Any]
) -> dict[str, dict[str, Any]]:
    """Create or validate a checkpoint and return its completed model rows."""

    manifest = root / "authority.json"
    expected = canonical_json(checkpoint_authority)
    if not manifest.exists() and root.exists() and any(root.iterdir()):
        raise EffectivenessDataError("checkpoint directory has no authority manifest")
    root.mkdir(parents=True, exist_ok=True)
    _create_checkpoint_manifest(manifest, expected)
    try:
        if manifest.read_text(encoding="utf-8") != expected:
            raise EffectivenessDataError("checkpoint authority does not match this run")
    except (OSError, UnicodeError) as error:
        raise EffectivenessDataError("checkpoint authority is unreadable") from error

    completed: dict[str, dict[str, Any]] = {}
    rows_root = root / "rows"
    if not rows_root.exists():
        return completed
    for path in sorted(rows_root.glob("*.json")):
        try:
            payload = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, UnicodeError, json.JSONDecodeError) as error:
            raise EffectivenessDataError(f"checkpoint row is corrupt: {path}") from error
        if (
            not isinstance(payload, dict)
            or set(payload) != {"model_id", "source_sha256", "row", "row_sha256"}
            or not isinstance(payload["model_id"], str)
            or not isinstance(payload["source_sha256"], str)
            or not isinstance(payload["row"], dict)
            or not isinstance(payload["row_sha256"], str)
            or payload["row_sha256"]
            != hashlib.sha256(canonical_json(payload["row"]).encode()).hexdigest()
            or payload["row"].get("model_id") != payload["model_id"]
            or path.stem != _checkpoint_key(payload["model_id"])
            or payload["model_id"] in completed
        ):
            raise EffectivenessDataError(f"checkpoint row is malformed: {path}")
        completed[payload["model_id"]] = payload
    return completed


def _write_checkpoint_row(root: Path, truth: DatasetTruth, row: dict[str, Any]) -> None:
    row_sha256 = hashlib.sha256(canonical_json(row).encode()).hexdigest()
    payload = {
        "model_id": truth.model_id,
        "source_sha256": truth.source_sha256,
        "row": row,
        "row_sha256": row_sha256,
    }
    _atomic_replace(
        root / "rows" / f"{_checkpoint_key(truth.model_id)}.json",
        canonical_json(payload),
    )


def _capture_run_authority(taxonomy_path: Path, *, canonical: bool = False) -> _RunAuthority:
    """Freeze the authority a long corpus run will claim in its metadata."""

    commit = _git_commit()
    worktree_sha256 = _git_worktree_sha256(commit)
    if canonical and worktree_sha256 is not None:
        raise EffectivenessDataError("canonical reports require tracked files to equal HEAD")
    source_sha256 = _source_digest()
    try:
        taxonomy = taxonomy_path.read_bytes()
    except OSError as error:
        raise EffectivenessDataError("taxonomy is unreadable") from error
    if _git_commit() != commit or _git_worktree_sha256(commit) != worktree_sha256:
        raise EffectivenessDataError("source authority changed while it was captured")
    if _source_digest() != source_sha256:
        raise EffectivenessDataError("source authority changed while it was captured")
    return _RunAuthority(
        commit=commit,
        worktree_sha256=worktree_sha256,
        source_sha256=source_sha256,
        taxonomy=taxonomy,
        taxonomy_sha256=hashlib.sha256(taxonomy).hexdigest(),
    )


def _verify_run_authority(authority: _RunAuthority, taxonomy_path: Path) -> None:
    """Refuse a report if its source or mapping changed while models were running."""

    commit_before = _git_commit()
    worktree_before = _git_worktree_sha256(authority.commit)
    source_before = _source_digest()
    try:
        taxonomy = taxonomy_path.read_bytes()
    except OSError as error:
        raise EffectivenessDataError("taxonomy changed during corpus run") from error
    if (
        commit_before != authority.commit
        or worktree_before != authority.worktree_sha256
        or source_before != authority.source_sha256
        or taxonomy != authority.taxonomy
        or _source_digest() != authority.source_sha256
        or _git_worktree_sha256(authority.commit) != authority.worktree_sha256
        or _git_commit() != authority.commit
    ):
        raise EffectivenessDataError("source authority changed during corpus run")


def _selection_hash(ids: list[str]) -> str:
    return hashlib.sha256(("\n".join(ids) + "\n").encode("utf-8")).hexdigest()


def _display_path(path: Path) -> str:
    """Keep repository paths portable and external provenance unambiguous."""

    resolved = path.resolve()
    try:
        return str(resolved.relative_to(ROOT))
    except ValueError:
        return str(resolved)


def _reported_commit(authority: _RunAuthority) -> str:
    """Keep clean report metadata stable and make exploratory source explicit."""

    if authority.worktree_sha256 is None:
        return authority.commit
    return f"{authority.commit}+dirty.{authority.worktree_sha256}"


def _mfcadpp_selection(
    root: Path,
) -> tuple[list[str], Callable[[str], DatasetTruth], dict[str, Any]]:
    paths = sorted(root.glob("*.st*p"), key=lambda path: path.name)
    if not paths:
        raise EffectivenessDataError(f"no MFCAD++ STEP files under {root}")
    by_id = {path.stem: path for path in paths}
    if len(by_id) != len(paths):
        raise EffectivenessDataError("MFCAD++ model IDs are not unique")
    ids = sorted(by_id)
    return ids, lambda model_id: load_mfcadpp_truth(by_id[model_id]), {"excluded": {}}


def _partition_ids(path: Path) -> list[str]:
    if not path.is_file():
        raise EffectivenessDataError(f"missing MFInstSeg partition: {path}")
    values = [
        line.strip() for line in path.read_text(encoding="utf-8").splitlines() if line.strip()
    ]
    if not values:
        raise EffectivenessDataError(f"empty MFInstSeg partition: {path}")
    return values


def _mfinstseg_selection(
    root: Path, partition_root: Path
) -> tuple[list[str], Callable[[str], DatasetTruth], dict[str, Any]]:
    partitions = {
        split: _partition_ids(partition_root / f"{split}.txt") for split in ("train", "val", "test")
    }
    duplicates = {
        split: sorted(model_id for model_id, count in Counter(values).items() if count > 1)
        for split, values in partitions.items()
    }
    memberships: dict[str, set[str]] = {}
    for split, values in partitions.items():
        for model_id in values:
            memberships.setdefault(model_id, set()).add(split)
    leaked = sorted(model_id for model_id, splits in memberships.items() if len(splits) > 1)
    excluded = set(duplicates["test"]) | set(leaked)
    ids = sorted(set(partitions["test"]) - excluded)
    if not ids:
        raise EffectivenessDataError("MFInstSeg test partition is empty after exclusions")
    return (
        ids,
        lambda model_id: load_mfinstseg_truth(root, model_id),
        {
            "excluded": {
                "duplicate_test_ids": duplicates["test"],
                "cross_split_ids": leaked,
            },
            "partition_counts": {
                split: {"rows": len(values), "unique": len(set(values))}
                for split, values in partitions.items()
            },
        },
    )


def _write_new_report(path: Path, contents: str) -> None:
    """Atomically create a report, refusing to replace historical evidence."""

    path.parent.mkdir(parents=True, exist_ok=True)
    temporary: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w", encoding="utf-8", newline="", dir=path.parent, delete=False
        ) as handle:
            temporary = Path(handle.name)
            handle.write(contents)
            handle.flush()
            os.fsync(handle.fileno())
        os.link(temporary, path)
    except FileExistsError as error:
        raise EffectivenessDataError(f"refusing to overwrite existing report: {path}") from error
    except OSError as error:
        raise EffectivenessDataError(f"could not create report {path}: {error}") from error
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def _environment() -> dict[str, str]:
    import build123d
    import OCP

    return {
        "python": platform.python_version(),
        "build123d": getattr(build123d, "__version__", "unknown"),
        "ocp": getattr(OCP, "__version__", "unknown"),
        "os": platform.platform(),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dataset", choices=("mfcadpp", "mfinstseg"))
    parser.add_argument("root", type=Path)
    parser.add_argument("--dataset-version", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--taxonomy",
        type=Path,
        default=DEFAULT_TAXONOMY,
    )
    parser.add_argument("--partition-root", type=Path)
    parser.add_argument("--limit", type=int)
    parser.add_argument(
        "--recognition-frame",
        choices=("raw", "framed"),
        default="raw",
        help="score caller-space recognition or the inferred local framed route",
    )
    parser.add_argument("--allow-invalid", action="store_true")
    parser.add_argument(
        "--canonical",
        action="store_true",
        help="require tracked files to equal HEAD before publishing",
    )
    parser.add_argument(
        "--workers",
        type=int,
        default=min(4, os.cpu_count() or 1),
        help="model workers (default: up to 4; use 0 for every available CPU)",
    )
    parser.add_argument(
        "--checkpoint-dir",
        type=Path,
        help="persist authority-bound model rows here and resume matching work",
    )
    args = parser.parse_args()
    if args.limit is not None and args.limit <= 0:
        parser.error("--limit must be positive")
    if args.workers < 0:
        parser.error("--workers must be non-negative")
    workers = (os.cpu_count() or 1) if args.workers == 0 else args.workers
    try:
        authority = _capture_run_authority(args.taxonomy, canonical=args.canonical)
        if args.dataset == "mfcadpp":
            ids, loader, selection_extra = _mfcadpp_selection(args.root)
        else:
            if args.partition_root is None:
                raise EffectivenessDataError("--partition-root is required for MFInstSeg")
            ids, loader, selection_extra = _mfinstseg_selection(args.root, args.partition_root)
        if args.limit is not None:
            ids = ids[: args.limit]
        _require_known_invalid_policy(args.dataset, ids, args.allow_invalid)
        taxonomy = load_taxonomy(args.taxonomy, args.dataset, contents=authority.taxonomy)
        # Loading truth also fingerprints every selected STEP/label source before costly
        # recognition. The immutable objects are safe inputs to independent workers.
        truths: list[DatasetTruth] = []
        input_errors: dict[str, str] = {}
        for model_id in ids:
            try:
                truths.append(loader(model_id))
            except (EffectivenessDataError, OSError, RuntimeError, ValueError) as error:
                truths.append(_unreadable_truth(args.dataset, args.root, model_id))
                input_errors[model_id] = str(error)
        checkpoint_authority = _checkpoint_authority(
            authority,
            dataset=args.dataset,
            dataset_version=args.dataset_version,
            ids=ids,
            truths=truths,
            selection_limit=args.limit,
            recognition_frame=args.recognition_frame,
            allow_invalid=args.allow_invalid,
        )
        completed = (
            _prepare_checkpoint(args.checkpoint_dir, checkpoint_authority)
            if args.checkpoint_dir is not None
            else {}
        )
        truth_by_id = {truth.model_id: truth for truth in truths}
        unknown = set(completed) - set(truth_by_id)
        if unknown:
            raise EffectivenessDataError("checkpoint contains a model outside the selection")
        for model_id, payload in completed.items():
            if payload["source_sha256"] != truth_by_id[model_id].source_sha256:
                raise EffectivenessDataError(
                    f"checkpoint source does not match selected model: {model_id}"
                )
    except EffectivenessDataError as error:
        parser.error(str(error))

    from quiddity import __version__

    try:
        _verify_run_authority(authority, args.taxonomy)
    except EffectivenessDataError as error:
        parser.error(str(error))

    rows_by_id = {model_id: payload["row"] for model_id, payload in completed.items()}
    pending = [truth for truth in truths if truth.model_id not in rows_by_id]
    started_run = time.monotonic()
    progress_every = max(1, len(ids) // 100)
    print(
        f"progress {len(rows_by_id)}/{len(ids)} "
        f"invalid={sum(item.get('status') == 'invalid' for item in rows_by_id.values())} "
        "elapsed=0.0s",
        file=sys.stderr,
        flush=True,
    )

    def record(truth: DatasetTruth, row: dict[str, Any]) -> None:
        rows_by_id[truth.model_id] = row
        if args.checkpoint_dir is not None:
            _write_checkpoint_row(args.checkpoint_dir, truth, row)
        done = len(rows_by_id)
        if done == len(ids) or done == 1 or done % progress_every == 0:
            invalid_so_far = sum(item.get("status") == "invalid" for item in rows_by_id.values())
            print(
                f"progress {done}/{len(ids)} invalid={invalid_so_far} "
                f"elapsed={time.monotonic() - started_run:.1f}s",
                file=sys.stderr,
                flush=True,
            )

    if pending and workers == 1:
        for truth in pending:
            record(
                truth,
                _score_model(
                    _ModelTask(
                        truth,
                        taxonomy,
                        args.recognition_frame,
                        input_errors.get(truth.model_id),
                    )
                ),
            )
    elif pending:
        # OCCT may already have native worker threads by this point. Forking that state can
        # deadlock, so every platform starts workers from a fresh interpreter.
        with concurrent.futures.ProcessPoolExecutor(
            max_workers=workers, mp_context=multiprocessing.get_context("spawn")
        ) as executor:
            futures = {
                executor.submit(
                    _score_model,
                    _ModelTask(
                        truth,
                        taxonomy,
                        args.recognition_frame,
                        input_errors.get(truth.model_id),
                    ),
                ): truth
                for truth in pending
            }
            for future in concurrent.futures.as_completed(futures):
                truth = futures[future]
                try:
                    row = future.result()
                except BaseException:
                    for other in futures:
                        other.cancel()
                    raise
                record(truth, row)

    rows = [rows_by_id[model_id] for model_id in ids]
    invalid = sum(row.get("status") == "invalid" for row in rows)
    try:
        _verify_run_authority(authority, args.taxonomy)
    except EffectivenessDataError as error:
        parser.error(str(error))
    summary = summarize_rows(rows, len(ids), invalid)
    report = {
        "format": REPORT_FORMAT,
        "format_version": REPORT_FORMAT_VERSION,
        "dataset": {"name": args.dataset, "version": args.dataset_version},
        "package": {
            "name": "quiddity",
            "version": __version__,
            "commit": _reported_commit(authority),
        },
        "environment": _environment(),
        "selection": {
            "rule": "unique model ID, lexical ascending",
            "limit": args.limit,
            "recognition_frame": args.recognition_frame,
            "selected_ids_sha256": _selection_hash(ids),
            **selection_extra,
        },
        "mapping": {
            "format_version": 1,
            "sha256": authority.taxonomy_sha256,
            "path": _display_path(args.taxonomy),
        },
        "models": rows,
        "summary": summary,
        "runtime": summarize_runtime(rows),
    }
    validate_report(report)
    if invalid and not args.allow_invalid:
        print(
            f"refusing partial report: {invalid}/{len(ids)} selected models invalid; "
            "use --allow-invalid only with an explicit recorded policy",
            file=sys.stderr,
        )
        return 2
    try:
        _write_new_report(args.output, canonical_json(report))
    except EffectivenessDataError as error:
        parser.error(str(error))
    print(canonical_json(summary), end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
