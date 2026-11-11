"""The installed wheel on committed STEP fixtures; no CAD Python dependency."""
import gzip
import hashlib
import importlib.metadata
import json
import os
from pathlib import Path
import re
import subprocess
import sys
from collections import Counter

import pytest
import quiddity

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / "tests" / "fixtures"
PLATE = FIXTURES / "filleted_plate_with_holes.step"


def cli(*args):
    return subprocess.run([sys.executable, "-m", "quiddity", *map(str, args)], capture_output=True)


@pytest.mark.parametrize("fixture", ["filleted_plate_with_holes.step", "turned_body_disc.step", "golden_oriented_slots.step"])
def test_recognise_matches_cli(fixture):
    path = FIXTURES / fixture
    text = quiddity.recognise(str(path))
    run = cli(path)
    assert run.returncode == 0, run.stderr
    assert text.encode() == run.stdout
    assert quiddity.recognise(path.read_bytes()) == text
    document = json.loads(text)["document"]
    assert document["schema_version"] == "quiddity-rust/recognition/6"
    assert document["coordinate_space"] == "file"
    assert document["source_sha256"] == hashlib.sha256(path.read_bytes()).hexdigest()
    assert document["records"]
    native_cli = os.environ.get("QUIDDITY_CLI")
    if native_cli:
        native = subprocess.run([native_cli, str(path)], capture_output=True, check=True)
        assert text.encode() == native.stdout


@pytest.mark.parametrize("framed", [False, True])
def test_evidence(framed):
    data = json.loads(quiddity.evidence(PLATE.read_bytes(), framed=framed))
    assert data == json.loads(quiddity.evidence(str(PLATE), framed=framed))
    assert data["schema_version"] == "quiddity-rust/evidence/1"
    assert data["coordinate_space"] == ("part" if framed else "file")
    assert (data["frame"] is not None) == framed
    assert len(data["face_sources"]) == data["face_count"]
    assert all(source["entity"] > 0 for source in data["face_sources"])
    assert data["features"]
    for feature in data["features"]:
        assert isinstance(feature["index"], int)
        assert isinstance(feature["family"], str)
        assert isinstance(feature["record"], dict)
        for faces in [feature["defining_faces"], feature["constituent_faces"], feature["host_faces"], *feature["instance_faces"]]:
            assert faces == sorted(set(faces))
            assert all(0 <= face < data["face_count"] for face in faces)
        assert all(0 <= member < len(data["features"]) for member in feature["members"])
    holes = [f for f in data["features"] if f["family"] == "holes"]
    assert holes
    for hole in holes:
        assert set(("bottom", "cbore", "spotface", "csink", "diameter", "axis", "location", "depth")) <= hole["record"].keys()
        assert hole["defining_faces"]
    run = cli("evidence", PLATE, *(["--framed"] if framed else []))
    assert run.returncode == 0, run.stderr
    assert json.loads(run.stdout) == data


def test_evidence_sources_name_step_entities():
    data = json.loads(quiddity.evidence(str(PLATE)))
    types = {row["id"]: row["types"] for row in json.loads(quiddity.step_entities(str(PLATE)))}
    (part,) = json.loads(quiddity.parts(str(PLATE)))["parts"]
    assert len(data["edge_sources"]) == part["edges"] and part["edges"] > 0
    assert len(data["face_sources"]) == part["faces"]
    assert all(types[source["entity"]] == ["ADVANCED_FACE"] for source in data["face_sources"])
    assert all(types[source["entity"]] == ["EDGE_CURVE"] for source in data["edge_sources"])
    assert len({source["entity"] for source in data["edge_sources"]}) == part["edges"]
    assert all(source["instance"] == 0 for source in data["edge_sources"])


@pytest.mark.parametrize("fixture", [PLATE, FIXTURES / "ap242/specify/assembly_plate_pin.step.gz"])
def test_parts_matches_cli(fixture):
    text = quiddity.parts(str(fixture))
    run = cli("parts", fixture)
    assert run.returncode == 0, run.stderr
    assert text.encode() == run.stdout
    data = json.loads(text)
    assert data["format"] == "quiddity-parts"
    assert [p["part"] for p in data["parts"]] == list(range(len(data["parts"])))
    raw = fixture.read_bytes()
    assert quiddity.parts(gzip.decompress(raw) if fixture.suffix == ".gz" else raw) == text


def test_step_entities_in_file_order():
    text = PLATE.read_bytes()
    rows = json.loads(quiddity.step_entities(str(PLATE)))
    assert rows == json.loads(quiddity.step_entities(text))
    # An independent scan, with strings emptied (this fixture has no comments).
    data = re.sub(rb"'(?:[^']|'')*'", b"''", text[text.index(b"\nDATA;") + 6:text.rindex(b"ENDSEC;")])
    scanned = re.findall(rb"#(\d+)\s*=\s*([^;]*);", data)
    assert [row["id"] for row in rows] == [int(number) for number, _ in scanned]
    assert [row["refs"] for row in rows] == [[int(r) for r in re.findall(rb"#(\d+)", body)] for _, body in scanned]
    for row, (_, body) in zip(rows, scanned):
        if not body.lstrip().startswith(b"("):
            assert row["types"] == [re.match(rb"\s*(\w+)", body).group(1).decode()]
    assert ["GEOMETRIC_REPRESENTATION_CONTEXT", "GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT",
            "GLOBAL_UNIT_ASSIGNED_CONTEXT", "REPRESENTATION_CONTEXT"] in [row["types"] for row in rows]
    # File order, not id order; complex leaves as written; a '#' in a string is no reference.
    small = b"""ISO-10303-21;
HEADER;
FILE_DESCRIPTION((''),'2;1');
FILE_NAME('','',(''),(''),'','','');
FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));
ENDSEC;
DATA;
#5=CARTESIAN_POINT('#9;',(0.,0.,0.));
#2=( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) );
#3=vertex_point('',#5);
#1=SHAPE_ASPECT('',(#5,#3),#3);
ENDSEC;
END-ISO-10303-21;
"""
    assert json.loads(quiddity.step_entities(small)) == [
        {"id": 5, "types": ["CARTESIAN_POINT"], "refs": []},
        {"id": 2, "types": ["LENGTH_UNIT", "NAMED_UNIT", "SI_UNIT"], "refs": []},
        {"id": 3, "types": ["VERTEX_POINT"], "refs": [5]},
        {"id": 1, "types": ["SHAPE_ASPECT"], "refs": [5, 3, 3]},
    ]


def test_framed_evidence_refusal():
    sphere = FIXTURES / "captured/frames/sphere.step"
    with pytest.raises(ValueError, match="no part frame"):
        quiddity.evidence(str(sphere), framed=True)


def test_anchor_and_fingerprints():
    anchor = json.loads(quiddity.anchor(str(PLATE), 0))
    assert anchor == json.loads(quiddity.anchor(PLATE.read_bytes(), 0))
    assert anchor["face"] == 0
    assert len(anchor["point"]) == 3 and len(anchor["uv"]) == 2
    fp = json.loads(quiddity.fingerprints(str(PLATE)))
    assert fp["fingerprint_version"] == "quiddity-rust/fingerprint/3"
    assert fp == json.loads(quiddity.fingerprints(PLATE.read_bytes()))
    assert fp == json.loads(quiddity.recognise(str(PLATE)))["fingerprints"]
    assert fp == json.loads(cli("fingerprints", PLATE).stdout)
    with pytest.raises(ValueError, match="out of range"):
        quiddity.anchor(str(PLATE), 100000)
    with pytest.raises(OverflowError):
        quiddity.anchor(str(PLATE), -1)


def test_correspond():
    before = gzip.decompress((FIXTURES / "revisions/hole_moved.old.step.gz").read_bytes())
    after = gzip.decompress((FIXTURES / "revisions/hole_moved.new.step.gz").read_bytes())
    result = json.loads(quiddity.correspond(before, after))
    assert result == json.loads(cli("correspond", FIXTURES / "revisions/hole_moved.old.step.gz", FIXTURES / "revisions/hole_moved.new.step.gz").stdout)
    assert result["features"]


def test_capabilities_version_and_console_script():
    assert quiddity.__version__ == "0.4.9"
    assert not importlib.metadata.requires("quiddity")
    assert importlib.metadata.metadata("quiddity")["Requires-Python"] == ">=3.11"
    caps = json.loads(quiddity.capabilities())
    assert caps["evidence"] == "quiddity-rust/evidence/1"
    assert caps["fingerprints"] == "quiddity-rust/fingerprint/3"
    assert caps["pmi_json_import"] is True
    assert caps == json.loads(cli("capabilities").stdout)
    executable = Path(sys.executable).parent / ("quiddity.exe" if os.name == "nt" else "quiddity")
    run = subprocess.run([str(executable), "--help"], capture_output=True)
    assert run.returncode == 0 and b"Usage: quiddity" in run.stdout and b"\n  evidence " in run.stdout
    assert cli("evidence").returncode == 2


@pytest.mark.parametrize("input", [b"", b"this is not STEP", (FIXTURES / "broken/empty_data.step").read_bytes(), str(FIXTURES / "does-not-exist.step")])
@pytest.mark.parametrize("operation", [quiddity.recognise, quiddity.evidence, quiddity.fingerprints, lambda step: quiddity.anchor(step, 0), lambda step: quiddity.correspond(step, str(PLATE))])
def test_refusals(input, operation):
    with pytest.raises(ValueError, match=r".+"):
        operation(input)


def test_input_type_refused():
    with pytest.raises(TypeError, match="str path or STEP bytes"):
        quiddity.recognise(object())


def test_nist_ctc_counts():
    corpus = os.environ.get("QUIDDITY_CORPUS")
    if not corpus:
        if os.environ.get("QUIDDITY_CORPUS_REQUIRED"):
            pytest.fail("QUIDDITY_CORPUS_REQUIRED requires QUIDDITY_CORPUS")
        pytest.skip("set QUIDDITY_CORPUS for NIST CTC counts")
    # Counts frozen from the existing Rust caller-space evidence snapshot, independently
    # of this binding. This also checks every feature's face evidence on the five CTC parts.
    snapshot = json.loads(gzip.decompress((FIXTURES / "rust-snapshot/recognition_evidence.json.gz").read_bytes()))
    cases = [item for item in snapshot if item["file"].startswith("nist/nist_ctc_")]
    assert len(cases) == 5
    for item in cases:
        expected = Counter(feature["family"] for feature in item["view"]["Ok"]["features"])
        actual = json.loads(quiddity.evidence(str(Path(corpus) / item["file"])))
        assert Counter(feature["family"] for feature in actual["features"]) == expected, item["file"]
        recognised = json.loads(quiddity.recognise(str(Path(corpus) / item["file"])))
        assert recognised["document"]["face_count"] == actual["face_count"]


def test_helical_spring_record():
    data = json.loads(quiddity.recognise(str(FIXTURES / "helical_springs/segmented.step.gz")))
    spring, = data["helical_springs"]
    assert spring["wire_diameter"] == pytest.approx(1.5)
    assert spring["mean_diameter"] == pytest.approx(24)
    assert spring["pitch"] == pytest.approx(4)
    assert spring["active_coils"] is None
    assert "unknown" in spring["active_coils_basis"]
    records = [r for r in data["document"]["records"] if r["family"] == "helical_springs"]
    record, = records
    assert record["record_type"] == "HelicalSpring"
    assert record["faces"] == spring["faces"]
    assert record["evidence_status"] == "verified"
    assert not data["holes"] and not data["bosses"] and not data["fillets"]


@pytest.mark.parametrize("fixture", [
    "golden_simple_through_hole_no_bounds.step", "kissing_cylinders_knife_no_bounds.step",
    *[f"golden_complex_{case}.step" for case in [
        "loop_edges", "bound_loop", "oriented_edge", "inner_bound",
        "outer_bound", "edge_element", "face_surface",
    ]],
])
@pytest.mark.parametrize("operation", [
    quiddity.recognise, quiddity.evidence, quiddity.fingerprints,
    quiddity.recognise_with_evidence,
])
def test_boundless_face_is_refused(fixture, operation):
    owner, slot = {
        "loop_edges": ("EDGE_LOOP", "edge_list"),
        "bound_loop": ("FACE_BOUND", "bound"),
        "oriented_edge": ("EDGE_LOOP", "edge_list"),
        "inner_bound": ("ADVANCED_FACE", "bounds"),
        "outer_bound": ("ADVANCED_FACE", "bounds"),
        "edge_element": ("ORIENTED_EDGE", "edge_element"),
        "face_surface": ("ADVANCED_FACE", "face_geometry"),
    }.get(fixture.removeprefix("golden_complex_").removesuffix(".step"), ("ADVANCED_FACE", "bounds"))
    with pytest.raises(ValueError, match=f"NonstandardReference: {owner}: '{slot}' references"):
        operation((FIXTURES / "broken" / fixture).read_bytes())


def test_internal_panic_is_an_exception():
    # CI also builds a dedicated fault-injection extension. Release wheels do
    # not contain the hook; the required test build must not silently skip it.
    from quiddity import _native
    if not hasattr(_native, "_test_panic"):
        assert os.environ.get("QUIDDITY_TEST_PANIC") != "1", "build with --features test-panic"
        return
    for kind, message in enumerate(["forced internal panic", "forced owned panic", "non-string panic payload"]):
        try:
            _native._test_panic(kind)
        except Exception as error:
            assert type(error) is quiddity.QuiddityError
            assert str(error) == f"internal: {message}"
        else:
            pytest.fail("internal panic did not raise QuiddityError")
