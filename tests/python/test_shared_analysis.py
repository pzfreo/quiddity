"""The shared binding preserves complete Rust recognition and evidence contracts."""

import ast
import gzip
import hashlib
import json
from pathlib import Path
import re

import pytest
import quiddity

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / "tests" / "fixtures"


@pytest.mark.parametrize("fixture", [
    "golden_simple_through_hole.step",
    "golden_oriented_slots.step",
    "golden_blind_pockets_and_pocket_patterns.step",
    "open_shell.step",
    "captured/frames/sphere.step",
])
@pytest.mark.parametrize("as_bytes", [False, True])
def test_shared_analysis_matches_both_public_calls(fixture, as_bytes):
    path = FIXTURES / fixture
    step = path.read_bytes() if as_bytes else str(path)
    recognition = json.loads(quiddity.recognise(step))
    framed = recognition["document"]["frame"]["status"] == "inferred"
    evidence = json.loads(quiddity.evidence(step, framed=framed))
    raw = quiddity.recognise_with_evidence(step)
    result = json.loads(raw)
    assert raw.endswith("\n")
    assert result == {
        "schema_version": "quiddity-rust/analysis/1",
        "recognition": recognition,
        "evidence": evidence,
        "evidence_error": None,
    }
    assert result["recognition"]["document"]["source_sha256"] == hashlib.sha256(path.read_bytes()).hexdigest()
    assert result["evidence"]["coordinate_space"] == ("part" if framed else "file")
    assert "face_sources" in result["evidence"] and "edge_sources" in result["evidence"]


def test_shared_analysis_gzip_preserves_file_digest(tmp_path):
    payload = (FIXTURES / "golden_simple_through_hole.step").read_bytes()
    path = tmp_path / "part.step.gz"
    path.write_bytes(gzip.compress(payload, mtime=0))
    result = json.loads(quiddity.recognise_with_evidence(str(path)))
    assert result["recognition"] == json.loads(quiddity.recognise(str(path)))
    assert result["recognition"]["document"]["source_sha256"] == hashlib.sha256(path.read_bytes()).hexdigest()
    framed = result["recognition"]["document"]["frame"]["status"] == "inferred"
    assert result["evidence"] == json.loads(quiddity.evidence(str(path), framed=framed))
    assert result["evidence_error"] is None


@pytest.mark.parametrize("as_bytes", [False, True])
def test_shared_analysis_keeps_local_refusal_and_independent_evidence(tmp_path, as_bytes):
    # A public pocket fixture presented as an open shell has no authoritative owner solid.
    # Only this in-memory negative input is changed; the committed fixture stays intact.
    payload = (FIXTURES / "golden_blind_pockets_and_pocket_patterns.step").read_bytes()
    payload, changed = re.subn(
        rb"MANIFOLD_SOLID_BREP\(([^,]*),\s*(#\d+)\)",
        rb"SHELL_BASED_SURFACE_MODEL(\1,(\2))",
        payload,
    )
    assert changed == 1
    payload = payload.replace(b"CLOSED_SHELL", b"OPEN_SHELL")
    path = tmp_path / "unowned-open-pocket.step"
    path.write_bytes(payload)
    step = payload if as_bytes else str(path)
    recognition = json.loads(quiddity.recognise(step))
    assert recognition["document"]["frame"]["status"] == "refused"
    assert recognition["document"]["records"]
    evidence = json.loads(quiddity.evidence(step, framed=False))
    result = json.loads(quiddity.recognise_with_evidence(step))
    assert result["recognition"] == recognition
    assert result["evidence"] == evidence
    assert result["evidence_error"] is None
    refusals = [
        feature for feature in evidence["features"]
        if feature["family"] == "section_recess_source_refusals"
    ]
    assert refusals
    for refusal in refusals:
        assert refusal["record"]["reason"] == "missing_common_valid_solid"
        assert "body" not in refusal["record"]
    raw_candidates = [
        record for record in recognition["document"]["records"]
        if record["family"] == "prismatic_pockets"
    ]
    assert raw_candidates
    assert all(record["evidence_status"] == "unproved" for record in raw_candidates)
    assert any(feature["family"] == "step_levels" for feature in evidence["features"])


@pytest.mark.parametrize("payload", [b"", b"not STEP", str(FIXTURES / "missing-shared-analysis.step")])
def test_shared_analysis_input_errors_raise(payload):
    with pytest.raises(ValueError, match=r".+"):
        quiddity.recognise_with_evidence(payload)


@pytest.mark.parametrize("payload", [None, 12, object()])
def test_shared_analysis_rejects_other_input_types(payload):
    with pytest.raises(TypeError, match="str path or STEP bytes"):
        quiddity.recognise_with_evidence(payload)


def test_shared_analysis_export_and_type_stub_agree():
    assert "recognise_with_evidence" in quiddity.__all__
    capabilities = json.loads(quiddity.capabilities())
    assert "recognise_with_evidence" in capabilities["functions"]
    assert capabilities["analysis"] == "quiddity-rust/analysis/1"
    stub = Path(quiddity.__file__).with_suffix(".pyi")
    functions = {node.name: node for node in ast.parse(stub.read_text()).body if isinstance(node, ast.FunctionDef)}
    signature = functions["recognise_with_evidence"]
    assert ast.unparse(signature.args.args[0].annotation) == "str | bytes"
    assert ast.unparse(signature.returns) == "str"
