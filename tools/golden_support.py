"""Versioned canonical semantic projection for migration goldens."""

from __future__ import annotations

import dataclasses
import json
import math
from enum import Enum
from pathlib import Path
from typing import Any

CANONICALIZER_VERSION = 1
FLOAT_DIGITS = 8
_OMIT_MAPPING_KEYS = frozenset({"face", "solid_idx", "oriented_slot"})
_POST_BASELINE_RESULT_FIELDS = frozenset(
    {
        "section_passages",
        "section_recesses",
        "section_recess_refusals",
        "section_recess_patterns",
        "oriented_slots",
        "oriented_slot_patterns",
    }
)


class CanonicalizationError(TypeError):
    """A value cannot cross the canonical golden boundary."""


def _float(value: float) -> float:
    if not math.isfinite(value):
        raise CanonicalizationError("non-finite floats are not canonical")
    value = round(value, FLOAT_DIGITS)
    return 0.0 if value == 0 else value


def canonicalize(value: Any) -> Any:
    """Project *value* to deterministic JSON primitives without CAD objects."""

    if dataclasses.is_dataclass(value) and not isinstance(value, type):
        fields = {
            field.name: canonicalize(getattr(value, field.name))
            for field in dataclasses.fields(value)
            if not (
                type(value).__name__ in {"RecognitionResult", "_LegacyRecognitionResult"}
                and field.name in _POST_BASELINE_RESULT_FIELDS
            )
        }
        name = type(value).__name__
        if name == "_LegacyRecognitionResult":
            name = "RecognitionResult"  # frozen historical detector-snapshot vocabulary
        return {"_type": name, **fields}
    if isinstance(value, Enum):
        return canonicalize(value.value)
    if value is None or isinstance(value, (bool, int, str)):
        return value
    if isinstance(value, float):
        return _float(value)
    if isinstance(value, dict):
        if not all(isinstance(key, str) for key in value):
            raise CanonicalizationError("canonical mapping keys must be strings")
        return {
            key: canonicalize(item)
            for key, item in sorted(value.items())
            if key not in _OMIT_MAPPING_KEYS
        }
    if isinstance(value, (list, tuple)):
        return [canonicalize(item) for item in value]
    if isinstance(value, (set, frozenset)):
        items = [canonicalize(item) for item in value]
        return sorted(items, key=canonical_json)
    raise CanonicalizationError(
        f"{type(value).__module__}.{type(value).__qualname__} is not canonical JSON data"
    )


def canonical_json(value: Any) -> str:
    """Serialize *value* using the versioned canonical projection."""

    return json.dumps(canonicalize(value), indent=2, sort_keys=True, ensure_ascii=False) + "\n"


def write_canonical(path: Path, value: Any, *, overwrite: bool = False) -> None:
    """Write canonical JSON, refusing to replace reviewed data by default."""

    if path.exists() and not overwrite:
        raise FileExistsError(f"refusing to overwrite {path}; pass overwrite=True")
    path.write_text(canonical_json(value), encoding="utf-8")
