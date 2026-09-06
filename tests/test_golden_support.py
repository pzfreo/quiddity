import json
import math
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

import pytest
from build123d import Box

ROOT = Path(__file__).parents[1]
sys.path.insert(0, str(ROOT / "tools"))

from golden_support import (  # noqa: E402
    CANONICALIZER_VERSION,
    CanonicalizationError,
    canonical_json,
    canonicalize,
    write_canonical,
)


@dataclass(frozen=True)
class Sample:
    z: float
    name: str


def test_canonicalizer_is_versioned_typed_quantized_and_ordered():
    value = {
        "records": [Sample(-0.0, "first"), Sample(1.2345678912, "second")],
        "unordered": {"z", "a"},
    }

    assert CANONICALIZER_VERSION == 1
    assert canonicalize(value) == {
        "records": [
            {"_type": "Sample", "z": 0.0, "name": "first"},
            {"_type": "Sample", "z": 1.23456789, "name": "second"},
        ],
        "unordered": ["a", "z"],
    }
    assert json.loads(canonical_json(value)) == canonicalize(value)


@pytest.mark.parametrize("value", [math.inf, math.nan, Box(1, 1, 1), {1: "non-string"}])
def test_canonicalizer_rejects_nonportable_values(value):
    with pytest.raises(CanonicalizationError):
        canonicalize(value)


def test_traversal_ids_and_cylinder_face_handles_are_omitted():
    assert canonicalize({"diameter": 4.0, "solid_idx": 7, "face": Box(1, 1, 1).faces()[0]}) == {
        "diameter": 4.0
    }


def test_other_unknown_objects_are_not_silently_omitted():
    with pytest.raises(CanonicalizationError):
        canonicalize({"unexpected": Box(1, 1, 1).faces()[0]})


def test_canonical_writer_requires_explicit_overwrite(tmp_path):
    output = tmp_path / "expected.json"
    write_canonical(output, {"value": 1})

    with pytest.raises(FileExistsError):
        write_canonical(output, {"value": 2})

    write_canonical(output, {"value": 2}, overwrite=True)
    assert json.loads(output.read_text(encoding="utf-8")) == {"value": 2}


def test_capture_cli_refuses_an_unpinned_commit_before_loading_draftwright():
    result = subprocess.run(
        [
            sys.executable,
            str(ROOT / "tools" / "capture_draftwright_goldens.py"),
            "--draftwright",
            str(ROOT),
            "--commit",
            "0" * 40,
        ],
        check=False,
        capture_output=True,
        text=True,
    )

    assert result.returncode == 1
    assert "--commit" in result.stderr
