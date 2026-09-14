# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Validation primitives shared by the package's two published JSON manifests.

:mod:`quiddity.capabilities` and :mod:`quiddity.inspection` each validate a closed
document format and each raises its own error type.  The checks that are common to
both live here so that one question has one answer, following the argument
:mod:`quiddity._geometry` records for the direction primitives: the risk in a
duplicated helper is not the repeated lines but that the copies are free to drift.

Callers bind their own error type once, normally with :func:`functools.partial`, so
every call site reads as it did when the helper was module-local.
"""

from __future__ import annotations

import re
from typing import Any

#: A package version as :pep:`440` and semantic versioning agree on it: three
#: dotted integers, optionally followed by a pre-release, post-release or local
#: segment.  Both manifests record the version of the package that produced them,
#: and both must read it the same way -- a disagreement here would be silent.
VERSION = re.compile(r"^(\d+)\.(\d+)\.(\d+)(?:[.+-][A-Za-z0-9.-]+)?$")


def check_keys(
    value: dict[str, Any],
    allowed: set[str],
    context: str,
    *,
    error: type[Exception],
) -> None:
    """Raise *error* if *value* carries any field outside *allowed*.

    Unknown fields are reported sorted so the message is stable across runs.
    """

    unknown = sorted(set(value) - allowed)
    if unknown:
        raise error(f"{context} has unknown fields: {', '.join(unknown)}")


def parse_version(value: object, context: str, *, error: type[Exception]) -> tuple[int, int, int]:
    """Return the ``(major, minor, patch)`` of *value*, or raise *error*.

    Any pre-release or local segment is matched but discarded: the manifests
    compare release precedence only.
    """

    if not isinstance(value, str) or not (match := VERSION.fullmatch(value)):
        raise error(f"{context} must be a semantic package version")
    major, minor, patch = match.groups()
    return int(major), int(minor), int(patch)
