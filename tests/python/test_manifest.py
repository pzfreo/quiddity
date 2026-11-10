"""The manifest facade compared with the Rust CLI on the assembly acceptance pack."""
import gzip
import json
import os
from pathlib import Path
import subprocess
import sys

import pytest
import quiddity

ROOT = Path(__file__).resolve().parents[2]
ASSEMBLIES = sorted((ROOT / "tests/fixtures/ap242/assembly").glob("*.step"))
GRAMIL = Path(os.environ.get("QUIDDITY_GRAMIL", "/Users/paul/repos/gramil/dist/step"))


def inputs():
    paths = list(ASSEMBLIES)
    if GRAMIL.is_dir():
        paths.extend(sorted(GRAMIL.glob("*.step")))
    elif os.environ.get("QUIDDITY_GRAMIL_REQUIRED"):
        raise AssertionError("required gramil pack missing")
    return paths


def cli(*args):
    return subprocess.run([sys.executable, "-m", "quiddity", *map(str, args)], capture_output=True)


@pytest.mark.parametrize("path", inputs(), ids=lambda p: p.stem)
def test_manifest_product_and_iterator_match_rust(path, tmp_path):
    output = tmp_path / "manifest.json"
    m = quiddity.manifest(path)
    assert isinstance(m, dict)
    assert list(tmp_path.iterdir()) == []
    assert quiddity.manifest(path, out=output) == m
    native_manifest = tmp_path / "native.json"
    run = cli("manifest", path, "-o", native_manifest)
    assert run.returncode == 0, run.stderr
    assert json.loads(native_manifest.read_text()) == m
    assert output.read_bytes() == native_manifest.read_bytes()
    results = tmp_path / "results"
    run = cli("recognise", path, "--manifest", native_manifest, "--out-dir", results)
    assert run.returncode == 0, run.stderr
    expected = {p["id"]: json.loads((results / "products" / f'{p["id"]}.json').read_text()) for p in m["products"]}
    for product_id, result in expected.items():
        assert quiddity.recognise_product(path, m, product_id) == result
        assert quiddity.recognise_product(str(path), str(output), product_id) == result
    iterator = quiddity.recognise_products(path, output)
    assert iter(iterator) is iterator
    assert list(iterator) == list(expected.items())
    assert list(quiddity.recognise_products(path, m, parts=[])) == []
    selected = list(expected)[::2]
    assert list(quiddity.recognise_products(path, m, parts=reversed(selected))) == [(p, expected[p]) for p in selected]


def test_refusals_unknown_and_lazy_reads(tmp_path):
    path = tmp_path / "source.step"
    path.write_bytes(ASSEMBLIES[0].read_bytes())
    m = quiddity.manifest(path)
    with pytest.raises(ValueError, match="p999"):
        quiddity.recognise_product(path, m, "p999")
    with pytest.raises(ValueError, match="p999.*p998"):
        list(quiddity.recognise_products(path, m, parts=["p999", "p998"]))
    iterator = quiddity.recognise_products(path, m)
    first = next(iterator)
    assert first[0] == m["products"][0]["id"]
    # Removing the source after the first result proves this is not an eager result list.
    path.unlink()
    with pytest.raises(ValueError):
        next(iterator)


def test_compressed_stale_and_versions(tmp_path):
    path = tmp_path / "source.step"
    original = ASSEMBLIES[0].read_bytes()
    path.write_bytes(original)
    m = quiddity.manifest(path)
    p = m["products"][0]
    # Same-size valid name edit must refuse before geometry parsing.
    changed = bytearray(original)
    name = p["name"].encode()
    at = next(start + original[start:end].index(name) for start, end in p["ranges"] if name in original[start:end])
    changed[at] = ord("X") if changed[at] != ord("X") else ord("Y")
    path.write_bytes(changed)
    with pytest.raises(ValueError, match=f'{p["id"]}.*sha256 mismatch'):
        quiddity.recognise_product(path, m, p["id"])
    gz = tmp_path / "source.gz"
    gz.write_bytes(gzip.compress(original))
    compressed = quiddity.manifest(gz)
    assert compressed["source"]["compressed"] is True
    with pytest.raises(ValueError, match="compressed source"):
        quiddity.recognise_product(gz, compressed, p["id"])
    path.write_bytes(original)
    m["reader"]["haecceity"] = "another-version"
    with pytest.raises(ValueError, match="reader"):
        quiddity.recognise_product(path, m, p["id"])
