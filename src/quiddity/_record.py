# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""The recognition-record base mixin (package ADR 0002).

Every feature-recognition record is a frozen, geometry-only dataclass — no
build123d / OCP object leaks out. This mixin gives them the one uniform serialization
accessor the contract promises: :meth:`to_dict`, a plain nested dict of
primitives (``dataclasses.asdict`` recurses into nested records and hole tuples).

Leaf of the recognition DAG — imports only the stdlib.
"""

from __future__ import annotations

import dataclasses
from typing import cast


class Record:
    """Mixin: a uniform ``.to_dict()`` for every recognition record (package ADR 0002).

    The record must be a dataclass; ``to_dict`` returns a nested dict of pure
    primitives, so a leaked build123d type surfaces as a non-serializable value
    the contract test catches.
    """

    def to_dict(self) -> dict[str, object]:
        # `self` is always a dataclass instance in practice (the mixin is only used
        # on records); mypy can't see that through the plain-class base.
        # Record is intentionally a non-dataclass mixin, so mypy cannot prove that every
        # concrete subclass is a dataclass. The public contract test proves that invariant.
        return cast(
            dict[str, object],
            dataclasses.asdict(self),  # type: ignore[call-overload]
        )
