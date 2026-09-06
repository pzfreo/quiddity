#!/usr/bin/env python3
"""Verify a Draftwright checkout against the pinned migration baseline."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).parents[1]
DEFAULT_MANIFEST = ROOT / "migration" / "source-baseline.json"


class BaselineError(RuntimeError):
    """The supplied checkout is not the pinned source baseline."""


def _git(checkout: Path, *args: str) -> str:
    result = subprocess.run(
        ["git", "-C", str(checkout), *args],
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode:
        detail = result.stderr.strip() or result.stdout.strip()
        raise BaselineError(f"git {' '.join(args)} failed: {detail}")
    return result.stdout.strip()


def _normalise_remote(url: str) -> str:
    url = url.removesuffix(".git").rstrip("/")
    if url.startswith("git@github.com:"):
        return "https://github.com/" + url.removeprefix("git@github.com:")
    return url


def verify(checkout: Path, manifest_path: Path = DEFAULT_MANIFEST) -> None:
    """Raise :class:`BaselineError` unless *checkout* exactly matches the manifest."""

    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    source = manifest["source"]
    expected_commit = source["commit"]

    actual_remote = _normalise_remote(_git(checkout, "remote", "get-url", "origin"))
    expected_remote = _normalise_remote(source["repository"])
    if actual_remote != expected_remote:
        raise BaselineError(f"origin is {actual_remote}, expected {expected_remote}")

    actual_commit = _git(checkout, "rev-parse", "HEAD")
    if actual_commit != expected_commit:
        raise BaselineError(f"HEAD is {actual_commit}, expected {expected_commit}")

    dirty = _git(checkout, "status", "--porcelain", "--untracked-files=all")
    if dirty:
        raise BaselineError("Draftwright checkout is dirty")

    for source_file in manifest["sources"]:
        path = source_file["path"]
        actual_blob = _git(checkout, "rev-parse", f"HEAD:{path}")
        if actual_blob != source_file["blob"]:
            raise BaselineError(f"blob for {path} is {actual_blob}, expected {source_file['blob']}")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--draftwright", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    args = parser.parse_args(argv)

    try:
        verify(args.draftwright.resolve(), args.manifest.resolve())
    except (BaselineError, OSError, ValueError, KeyError, json.JSONDecodeError) as error:
        print(f"baseline verification failed: {error}", file=sys.stderr)
        return 1
    print("Draftwright checkout matches the pinned clean source baseline")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
