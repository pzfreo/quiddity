# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""STEP in, recognition JSON out."""

from __future__ import annotations

import argparse
import ctypes
import gc
import json
import os
import sys
from collections.abc import Iterator
from contextlib import contextmanager, redirect_stdout
from pathlib import Path
from tempfile import NamedTemporaryFile
from typing import cast

from quiddity import __version__, import_step_geometry
from quiddity._typing import Part
from quiddity.capabilities import capability_manifest_json
from quiddity.document import build_recognition_document


def _flush_native_stdio() -> None:
    """Flush C stdio buffers so native text written under the redirect cannot surface later."""
    try:
        libc = ctypes.CDLL(None)
    except Exception:  # noqa: BLE001 - no process-wide C library handle (Windows)
        return
    libc.fflush(None)


@contextmanager
def _diagnostics_to_stderr() -> Iterator[None]:
    """Keep Python and native CAD-kernel messages out of the JSON stdout stream."""
    sys.stdout.flush()
    saved = os.dup(1)
    try:
        os.dup2(2, 1)
        with redirect_stdout(sys.stderr):
            yield
    finally:
        # Native readers can report from their destructors (OCP 8), and C stdio may still
        # hold buffered text; both must land while fd 1 is still the diagnostics stream.
        gc.collect()
        _flush_native_stdio()
        os.dup2(saved, 1)
        os.close(saved)


def _write_output(encoded: str, destination: Path | None) -> None:
    if destination is None:
        sys.stdout.write(encoded)
        sys.stdout.flush()
        return
    # Finish serialization before touching the destination, and replace only a complete file.
    temporary: Path | None = None
    try:
        with NamedTemporaryFile(
            mode="w",
            encoding="utf-8",
            dir=destination.parent,
            prefix=f".{destination.name}.",
            delete=False,
        ) as stream:
            temporary = Path(stream.name)
            stream.write(encoded)
        os.replace(temporary, destination)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def main(argv: list[str] | None = None) -> int:
    """Return 0 on successful processing, 1 on processing/I/O failure, 2 on usage errors."""
    parser = argparse.ArgumentParser(prog="quiddity", description=__doc__)
    operation = parser.add_mutually_exclusive_group(required=True)
    operation.add_argument("step", nargs="?", type=Path, help="STEP file to recognise")
    operation.add_argument(
        "--capabilities", action="store_true", help="print the capability manifest"
    )
    parser.add_argument("-o", "--output", type=Path, help="write JSON here instead of stdout")
    parser.add_argument("--version", action="version", version=f"quiddity {__version__}")
    args = parser.parse_args(argv)
    try:
        if args.step is not None:
            if not args.step.is_file():
                raise ValueError(f"input is not a file: {args.step}")
            if args.output is not None and (
                args.output.resolve() == args.step.resolve()
                or (args.output.exists() and args.output.samefile(args.step))
            ):
                raise ValueError("output must not overwrite the input STEP file")
            failure: str | None = None
            with _diagnostics_to_stderr():
                try:
                    document = build_recognition_document(
                        cast(Part, import_step_geometry(args.step))
                    )
                    encoded = json.dumps(document, indent=2, sort_keys=True, allow_nan=False) + "\n"
                except Exception as error:  # noqa: BLE001 - reported below, after the redirect
                    # Handled inside the redirect so the traceback, and the native objects its
                    # frames hold, are released while kernel output still goes to stderr.
                    failure = str(error)
            if failure is not None:
                raise ValueError(failure)
        else:
            encoded = capability_manifest_json()
        _write_output(encoded, args.output)
    except Exception as error:
        print(f"quiddity: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
