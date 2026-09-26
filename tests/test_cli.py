"""Public CLI transport and shared recognition-document contracts."""

import gzip
import json
import os
import subprocess
import sys
from pathlib import Path
from typing import cast

import pytest
from build123d import Axis, Box, Compound, Cylinder, Pos, export_step

import quiddity.cli as cli
import quiddity.document as document
from quiddity import __version__, import_step_geometry
from quiddity._typing import Part
from quiddity.capabilities import capability_manifest
from quiddity.evidence import FramedRecognitionEvidence
from quiddity.frames import FrameRefusalReason, RefusedPartFrame, build_framed_recognition_evidence


def test_capabilities_and_version(capsys):
    assert cli.main(["--capabilities"]) == 0
    assert json.loads(capsys.readouterr().out) == capability_manifest()
    with pytest.raises(SystemExit) as version:
        cli.main(["--version"])
    assert version.value.code == 0
    assert capsys.readouterr().out == f"quiddity {__version__}\n"


@pytest.mark.parametrize("args", [[], ["part.step", "--capabilities"], ["--unknown"]])
def test_usage_errors(args):
    with pytest.raises(SystemExit) as error:
        cli.main(args)
    assert error.value.code == 2


def test_missing_input_and_output_failure(tmp_path, capsys):
    assert cli.main([str(tmp_path / "missing.step")]) == 1
    assert "input is not a file" in capsys.readouterr().err
    assert cli.main(["--capabilities", "-o", str(tmp_path / "absent" / "out.json")]) == 1
    captured = capsys.readouterr()
    assert captured.out == ""
    assert "quiddity:" in captured.err


def test_input_cannot_be_overwritten(tmp_path, capsys):
    source = tmp_path / "part.step"
    source.write_text("original")
    assert cli.main([str(source), "-o", str(source)]) == 1
    assert source.read_text() == "original"
    assert "overwrite" in capsys.readouterr().err


def test_transport_uses_shared_builder_and_keeps_diagnostics_off_stdout(
    tmp_path, monkeypatch, capfd
):
    source = tmp_path / "part.step"
    source.touch()
    part = object()
    monkeypatch.setattr(cli, "import_step_geometry", lambda path: part)

    def build(value):
        assert value is part
        print("Python diagnostic")
        os.write(1, b"native diagnostic\n")
        return {"features": []}

    monkeypatch.setattr(cli, "build_recognition_document", build)
    assert cli.main([str(source)]) == 0
    captured = capfd.readouterr()
    assert json.loads(captured.out) == {"features": []}
    assert "Python diagnostic" in captured.err and "native diagnostic" in captured.err
    output = tmp_path / "result.json"
    assert cli.main([str(source), "-o", str(output)]) == 0
    assert json.loads(output.read_text()) == {"features": []}
    assert capfd.readouterr().out == ""


def test_processing_failure_preserves_output(tmp_path, monkeypatch, capfd):
    source = tmp_path / "part.step"
    source.touch()
    output = tmp_path / "result.json"
    output.write_text("previous result")

    def fail(path):
        raise ValueError("bad STEP")

    monkeypatch.setattr(cli, "import_step_geometry", fail)
    assert cli.main([str(source), "-o", str(output)]) == 1
    assert output.read_text() == "previous result"
    captured = capfd.readouterr()
    assert captured.out == "" and "bad STEP" in captured.err


def test_atomic_write_failure_preserves_output(tmp_path, monkeypatch):
    output = tmp_path / "result.json"
    output.write_text("previous result")

    def fail(*args):
        raise OSError("cannot replace")

    monkeypatch.setattr(cli.os, "replace", fail)
    assert cli.main(["--capabilities", "-o", str(output)]) == 1
    assert output.read_text() == "previous result"
    assert list(tmp_path.iterdir()) == [output]


def test_document_refuses_unavailable_frame(monkeypatch):
    monkeypatch.setattr(
        document,
        "build_framed_recognition_evidence",
        lambda *args, **kwargs: RefusedPartFrame(FrameRefusalReason.NO_MATERIAL),
    )
    with pytest.raises(ValueError, match="no-material"):
        document.build_recognition_document(cast(Part, object()))


def test_document_projects_one_run_with_edit_fields(monkeypatch):
    part = cast(Part, Pos(12, 4, 9) * (Box(30, 20, 10) - Cylinder(3, 20)))
    view = build_framed_recognition_evidence(part)
    assert isinstance(view, FramedRecognitionEvidence)
    calls = []

    def recognise(source, *, rotational):
        calls.append(source)
        assert rotational is False
        return view

    monkeypatch.setattr(document, "build_framed_recognition_evidence", recognise)
    result = document.build_recognition_document(part)
    assert calls == [part]
    encoded = json.loads(json.dumps(result, allow_nan=False))
    assert encoded["format"] == "quiddity-recognition"
    assert encoded["format_version"] == 3
    assert len(encoded["faces"]) == len(part.faces())
    assert sorted(face["caller_index"] for face in encoded["faces"]) == list(
        range(len(part.faces()))
    )
    local = tuple(view.part.faces())
    caller = tuple(part.faces())
    for face in encoded["faces"]:
        assert local[face["index"]].wrapped.IsPartner(caller[face["caller_index"]].wrapped)
    assert encoded["features"]
    for feature, projected in zip(view.features, encoded["features"], strict=True):
        source_record = json.loads(json.dumps(view.record(feature).to_dict()))
        assert {key: projected["record"][key] for key in source_record} == source_record
        assert "named_dimensions" in projected["record"]
        assert "dependents" in projected["record"]
        assert set(projected["defining_faces"]) <= set(projected["constituent_faces"])
        assert set(projected["constituent_faces"]) <= set(range(len(local)))
    coverage = encoded["association"]["face_count"]
    assert coverage["total"] == coverage["associated"] + coverage["unassociated"]
    assert coverage["unassociated"] == len(encoded["association"]["unassociated_faces"])


def test_real_step_subprocess(tmp_path):
    source = tmp_path / "box.step"
    export_step(Box(10, 20, 30), source)
    completed = subprocess.run(
        [sys.executable, "-m", "quiddity.cli", str(source)],
        capture_output=True,
        text=True,
        timeout=120,
    )
    assert completed.returncode == 0, completed.stderr
    result = json.loads(completed.stdout)
    assert result["format"] == "quiddity-recognition"
    assert len(result["faces"]) == 6
    source.write_text("not a STEP file")
    failed = subprocess.run(
        [sys.executable, "-m", "quiddity.cli", str(source)],
        capture_output=True,
        text=True,
        timeout=120,
    )
    assert failed.returncode == 1
    assert failed.stdout == ""
    assert "quiddity:" in failed.stderr


def test_compound_recess_indices_keep_local_body_and_face_ownership():
    body = Box(40, 30, 20) - Pos(0, 0, 8) * Box(10, 8, 10)
    part = cast(Part, Compound([Pos(-50, 0, 0) * body, Pos(50, 0, 0) * body]).rotate(Axis.X, 27))
    result = json.loads(json.dumps(document.build_recognition_document(part), allow_nan=False))
    assert len(result["bodies"]) == 2
    assert all(len(face["body_indices"]) == 1 for face in result["faces"])
    recesses = [
        feature for feature in result["features"] if feature["record_type"] == "SectionRecess"
    ]
    assert len(recesses) == 2
    for feature in recesses:
        owners = {
            owner
            for index in feature["constituent_faces"]
            for owner in result["faces"][index]["body_indices"]
        }
        assert len(owners) == 1
    assert set(recesses[0]["constituent_faces"]).isdisjoint(recesses[1]["constituent_faces"])


def test_nonfinite_json_is_a_processing_error(tmp_path, monkeypatch, capfd):
    source = tmp_path / "part.step"
    source.touch()
    monkeypatch.setattr(cli, "import_step_geometry", lambda path: object())
    monkeypatch.setattr(cli, "build_recognition_document", lambda part: {"value": float("nan")})
    assert cli.main([str(source)]) == 1
    assert capfd.readouterr().out == ""


def test_edit_document_bore_through_boss_and_hole_pair() -> None:
    boss = Box(80, 80, 10) + Pos(0, 0, 5) * Cylinder(15, 20)
    bore = boss - Pos(0, 0, 10) * Cylinder(3, 40)
    features = document.build_recognition_document(cast(Part, bore))["features"]
    hole = next(feature for feature in features if feature["record_type"] == "HoleRecord")
    owner = next(feature for feature in features if feature["record_type"] == "BossRecord")
    assert owner["record"]["named_dimensions"]["axial_length"] == 10
    assert any(
        dependent["feature_index"] == hole["index"]
        and dependent["relation"] == "crossing_bore"
        and dependent["defining_faces"] == hole["defining_faces"]
        for dependent in owner["record"]["dependents"]
    )

    pair_part = Box(80, 60, 10)
    pair_part -= Pos(-15, 0, 0) * Cylinder(3, 20)
    pair_part -= Pos(15, 0, 0) * Cylinder(3, 20)
    pair = document.build_recognition_document(cast(Part, pair_part))["derived"]["hole_pairs"]
    assert len(pair) == 1
    assert pair[0]["center_spacing"] == 30
    assert len(pair[0]["holes"]) == 2

    array_part = Box(80, 60, 10)
    for x in (-20, 0, 20):
        array_part -= Pos(x, 0, 0) * Cylinder(3, 20)
    array_document = document.build_recognition_document(cast(Part, array_part))
    pattern = array_document["derived"]["hole_patterns"][0]
    assert pattern["named_dimensions"]["center_spacing"] == 20
    holes = [
        feature for feature in array_document["features"] if feature["record_type"] == "HoleRecord"
    ]
    assert all(
        sum(item["relation"] == "pattern_sibling" for item in feature["record"]["dependents"]) == 2
        for feature in holes
    )


@pytest.mark.slow
def test_invalid_cadgenbench_face_degrades_locally(tmp_path) -> None:
    fixture = Path(__file__).parent / "corpus/cadgenbench_inputs/cgb202.step.gz"
    if not fixture.is_file():
        pytest.skip("optional CADGenBench corpus is unavailable")
    source = tmp_path / "cgb202.step"
    source.write_bytes(gzip.decompress(fixture.read_bytes()))
    part = import_step_geometry(source)
    assert not part.is_valid
    result = document.build_recognition_document(part)
    assert result["proof"] == "local_degradation"
    unproven = {face["index"] for face in result["faces"] if face["proof"] == "not_proven"}
    assert len(unproven) == 6
    assert sum(feature["record_type"] == "HoleRecord" for feature in result["features"]) >= 20
    assert all(
        not (set(feature["defining_faces"]) & unproven)
        and not (set(feature["constituent_faces"]) & unproven)
        for feature in result["features"]
    )
