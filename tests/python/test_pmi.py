"""Outside-producer API: no CLI argument parser, subprocess, or temporary STEP/JSON files."""
from copy import deepcopy
import gzip
from importlib.resources import files
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile

import pytest
import quiddity
from quiddity import _native, pmi

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / "tests/fixtures"
XCAF = ROOT / "crates/haecceity/tests/fixtures/xcaf"


@pytest.fixture(scope="module")
def producer():
    geometry = gzip.decompress((XCAF / "pmi_plate.geom.step.gz").read_bytes())
    document = json.loads((XCAF / "pmi_plate.pmi.json").read_text())
    return geometry, document


@pytest.fixture(scope="module")
def authored(producer):
    return pmi.write(*producer)


def test_nist_roundtrip_onto_stripped_geometry():
    source = FIXTURES / "ap242/nist/nist_ctc_02_asme1_ap242-e2.stp.gz"
    document = pmi.read(source)
    assert document["pmi"]["dimensions"] and document["pmi"]["tolerances"]
    empty = pmi.read(FIXTURES / "ties/cube.step")
    geometry = pmi.write(source, empty)
    stripped = pmi.read(geometry)
    assert not stripped["pmi"]["dimensions"] and not stripped["pmi"]["tolerances"]
    output = pmi.write(geometry, document)
    assert isinstance(output, bytes)
    assert pmi.read(output) == document
    faces = re.findall(rb"#\d+\s*=\s*ADVANCED_FACE\b.*?;", geometry, re.DOTALL)
    assert faces and all(face in output for face in faces)
    assert pmi.validate(document) is None


@pytest.mark.parametrize("source, part", [
    ("ap242/nist/nist_ctc_02_asme1_ap242-e2.stp.gz", None),
    ("ap242/specify/assembly_plate_pin.step.gz", 0),
    ("ap242/specify/assembly_plate_pin.step.gz", 1),
])
def test_read_findings_match_cli(source, part):
    path = FIXTURES / source
    document, findings = pmi.read(path, part=part, findings=True)
    assert document == pmi.read(path, part=part)
    assert pmi.read(path, part=part, findings=False) == document
    run = subprocess.run([sys.executable, "-m", "quiddity", "pmi", "read", str(path),
                          *(["--part", str(part)] if part is not None else [])], capture_output=True)
    assert run.returncode == 0, run.stderr
    assert findings == [{"part": None, **finding} for finding in json.loads(run.stdout).get("findings", [])]
    for finding in findings:
        assert {"kind", "part", "ids", "entity", "detail"} == finding.keys()
        assert finding["part"] in (None, part or 0)
    if part is None:
        assert any(finding["entity"] == "datum_feature" and finding["ids"] == [79, 661]
                   for finding in findings)


def test_nist_write_report():
    source = FIXTURES / "ap242/nist/nist_ctc_02_asme1_ap242-e2.stp.gz"
    document = pmi.read(source)
    output, report = pmi.write(source, document, report=True)
    assert output == pmi.write(source, document)
    assert pmi.read(output) == document
    assert report["schema"] == "haecceity.pmi/1"
    assert report["part"] == 0 and report["differences"] == []
    assert report["added"] > 0
    assert report["added"] == report["writer"]["instances"]["added"]
    assert any(finding["entity"] == "datum_feature" and finding["ids"] == [79, 661]
               and "datum feature identifies no face" in finding["detail"]
               for finding in report["read_findings"])
    for key in ("read_findings", "output_findings"):
        assert isinstance(report[key], list)
        for finding in report[key]:
            assert {"kind", "part", "ids", "entity", "detail"} == finding.keys()
    assert any(finding["category"] == "kept_unconsumed" and finding["ids"] == [1530]
               and "identifies no face or edge" in finding["detail"]
               for finding in report["writer_findings"])
    assert all("category" in finding for finding in report["writer_findings"])


def test_preserved_normalizations_findings_shape_and_names():
    # Python's public writer currently uses Replace. Exercise the shared report
    # projection directly with canonical Add diagnostics, without a new Add API.
    writer = {"preserved_normalizations": [
        {"id": 1004, "normalization": "legacy-endpoint-retirement"},
        {"id": 2001, "normalization": "functional-sar-isolation"},
        {"id": 2002, "normalization": "functional-sar-unwrapping"},
        {"id": 2003, "normalization": "kept-usage-factoring"},
        {"id": 2004, "normalization": "legacy-text-rechunking"},
    ]}
    original = deepcopy(writer)
    assert pmi._writer_findings(writer) == [
        {"category": "preserved_normalizations", "id": 1004, "normalization": "legacy-endpoint-retirement"},
        {"category": "preserved_normalizations", "id": 2001, "normalization": "functional-sar-isolation"},
        {"category": "preserved_normalizations", "id": 2002, "normalization": "functional-sar-unwrapping"},
        {"category": "preserved_normalizations", "id": 2003, "normalization": "kept-usage-factoring"},
        {"category": "preserved_normalizations", "id": 2004, "normalization": "legacy-text-rechunking"},
    ]
    assert writer == original
    assert pmi._writer_findings({}) == []
    assert pmi._writer_findings({"preserved_normalizations": []}) == []
    nested = {"kept_unconsumed": [{"ids": [1004], "detail": "kept"}]}
    projected = pmi._writer_findings(nested)
    projected[0]["ids"].append(1109)
    assert nested["kept_unconsumed"][0]["ids"] == [1004]


def test_occt_density_repair_report():
    source = ROOT / "crates/haecceity/tests/fixtures/q142/occt-density-plate.step.gz"
    document = pmi.read(source)
    output, report = pmi.write(source, document, report=True)
    assert pmi.read(output) == document
    assert report["differences"] == []
    for detail in ("OCCT writes mass^3 · length^2", "density value untyped (OCCT)"):
        assert any(detail in finding["detail"] and 725 in finding["ids"]
                   for finding in report["read_findings"])
    assert not any("density" in finding["detail"] for finding in report["output_findings"])
    assert b"'density measure',POSITIVE_RATIO_MEASURE(2.7)" in output


def test_bytes_without_temp_files_or_cli(producer, tmp_path, monkeypatch):
    def forbidden(*args, **kwargs):
        pytest.fail("in-memory PMI operation attempted a temp file or subprocess")

    monkeypatch.chdir(tmp_path)
    for name in ("TemporaryDirectory", "NamedTemporaryFile", "mkstemp", "mkdtemp"):
        monkeypatch.setattr(tempfile, name, forbidden)
    monkeypatch.setattr(subprocess, "run", forbidden)
    geometry, document = producer
    output, report = pmi.write(geometry, json.dumps(document), report=True)
    assert isinstance(output, bytes) and output.startswith(b"ISO-10303-21;")
    assert report["differences"] == []
    assert pmi.read(output, anchors="name") == document
    pmi.validate(pmi.presentation(output))
    assert list(tmp_path.iterdir()) == []


def test_name_and_index_anchors(authored, producer):
    assert pmi.read(authored, anchors="name") == producer[1]
    indexed = pmi.read(authored)
    assert indexed != producer[1]
    for feature in indexed["pmi"]["features"]:
        for item in feature["value"].get("items", []):
            if "face" in item:
                assert isinstance(item["face"], int)
    # Index mode clears original transport bindings; re-import retains the same indices.
    assert pmi.read(pmi.write(producer[0], indexed)) == indexed


def test_generated_presentation(authored):
    document = pmi.presentation(authored)
    assert document["schema"] == "haecceity.presentation/1"
    assert document["items"]
    assert any(graphic["source"] == "generated" for graphics in document["items"].values() for graphic in graphics)
    assert pmi.validate(document) is None


def test_paths_and_part_selection(authored, tmp_path):
    path = tmp_path / "part.step"
    path.write_bytes(authored)
    class StepPath:
        def __fspath__(self):
            return str(path)
    assert pmi.read(StepPath(), part=0) == pmi.read(authored)
    assert pmi.presentation(path, part=0) == pmi.presentation(authored)
    assert pmi.read(pmi.write(path, pmi.read(path), part=0)) == pmi.read(path)
    with pytest.raises(quiddity.PmiError, match="unknown part"):
        pmi.read(authored, part=999)
    for invalid in (-1, 2**64, True):
        with pytest.raises(quiddity.PmiError, match="part must be"):
            pmi.read(authored, part=invalid)
    assembly = FIXTURES / "ap242/specify/assembly_plate_pin.step.gz"
    with pytest.raises(quiddity.PmiError, match="select part explicitly"):
        pmi.read(assembly)
    assert pmi.read(assembly, part=0)["schema"] == "haecceity.pmi/1"


@pytest.mark.parametrize("report", [False, True])
@pytest.mark.parametrize("numeric", [False, True])
def test_outside_producer_refusals(producer, numeric, report):
    geometry, original = producer
    document = deepcopy(original)
    if numeric:
        document["pmi"]["datums"][0]["value"]["feature"] = 0
        reason = "/pmi/datums/0/value/feature"
        with pytest.raises(quiddity.PmiError, match=reason):
            pmi.validate(document)
    else:
        document["pmi"]["features"][0]["value"]["items"][0]["face"] = {"name": "no-such-face"}
        reason = "unknown face name"
        assert pmi.validate(document) is None  # Existence requires geometry.
    with pytest.raises(quiddity.PmiError, match=reason) as caught:
        pmi.write(geometry, document, report=report)
    assert isinstance(caught.value, ValueError)
    assert isinstance(caught.value.findings, list)


def test_writer_preserves_dimensions_without_values():
    source = FIXTURES / "ap242/nist/nist_ctc_01_asme1_ap242-e1.stp.gz"
    document = pmi.read(source)
    dimensions = document["pmi"]["dimensions"]
    valueless = [d for d in dimensions if d["value"]["nominal"] is None]
    assert len(valueless) == 4
    assert pmi.validate(document) is None
    written = pmi.write(source, document)
    after = pmi.read(written)
    assert after == document
    assert [d for d in after["pmi"]["dimensions"] if d["value"]["nominal"] is None] == valueless


@pytest.mark.parametrize("document", ["{}", "not json", '{"schema":1,"schema":2}', {"schema": "unknown"}, {"bad": float("nan")}])
def test_malformed_documents(document, producer):
    with pytest.raises(quiddity.PmiError) as caught:
        pmi.validate(document)
    assert str(caught.value)
    assert isinstance(caught.value.findings, list)
    with pytest.raises(quiddity.PmiError):
        pmi.write(producer[0], document)


@pytest.mark.parametrize("operation", [pmi.read, pmi.presentation, lambda step: pmi.write(step, {})])
@pytest.mark.parametrize("step", [b"", b"not STEP", str(FIXTURES / "missing.step")])
def test_step_refusals(operation, step):
    with pytest.raises(quiddity.PmiError) as caught:
        operation(step)
    assert str(caught.value) and isinstance(caught.value.findings, list)


def test_name_anchor_refusals(producer):
    with pytest.raises(quiddity.PmiError, match="anchors must be"):
        pmi.read(producer[0], anchors="guess")
    # A producer may name an unambiguous face, but ambiguous names cannot be exported.
    geometry, document = producer
    names = [item["face"]["name"] for feature in document["pmi"]["features"]
             for item in feature["value"].get("items", []) if "face" in item]
    first, second = list(dict.fromkeys(names))[:2]
    ambiguous = geometry.replace(second.encode(), first.encode())
    indexed = pmi.read(pmi.write(geometry, document))
    output = pmi.write(ambiguous, indexed)
    with pytest.raises(quiddity.PmiError, match="ambiguous face name"):
        pmi.read(output, anchors="name")
    unnamed = geometry.replace(first.encode(), b"")
    output = pmi.write(unnamed, indexed)
    with pytest.raises(quiddity.PmiError, match="has no STEP name"):
        pmi.read(output, anchors="name")


def test_packaged_schemas():
    for name, value, native in [
        ("haecceity.pmi-1.json", pmi.SCHEMA, _native.PMI_SCHEMA),
        ("haecceity.presentation-1.json", pmi.PRESENTATION_SCHEMA, _native.PRESENTATION_SCHEMA),
    ]:
        packaged = files("quiddity").joinpath("schema", name).read_bytes()
        assert packaged == (ROOT / "crates/haecceity/schema" / name).read_bytes()
        assert packaged.decode() == native
        assert json.loads(packaged) == value


@pytest.mark.parametrize(
    "case,condition,count",
    [
        ("pmi_gaps", "shared-carrier-controlled-pair", 2),
        ("datum_targets_shared_support", "shared-carrier-datum-target-support", 5),
    ],
)
def test_geometry_carrier_exemptions_are_reported(case, condition, count):
    source = FIXTURES / f"ap242/write-presentation/{case}.step.gz"
    document = pmi.read(source)
    output, report = pmi.write(source, document, report=True)
    exemptions = report["writer"]["geometry_carrier_exemptions"]
    assert len(exemptions) == count
    assert {e["condition"] for e in exemptions} == {condition}
    assert all(e["part"] == 0 and e["aspect"] != e["feature"] for e in exemptions)
    assert pmi.read(output) == document


def test_write_source_presentation_and_handmade_flag(producer):
    geometry, _ = producer
    source_root = ROOT / "crates/haecceity/tests/fixtures/xcaf"
    semantic = json.loads((source_root / "pmi_plate.source.pmi.json").read_text())
    companion = json.loads((source_root / "pmi_plate.source.presentation.json").read_text())
    output, report = pmi.write(geometry, semantic, presentation=companion, report=True)
    assert pmi.read(output, anchors="name") == semantic
    drawn = pmi.presentation(output)
    for item, graphics in companion["items"].items():
        assert drawn["items"][item][0]["text_placements"][0]["origin"] == graphics[0]["text_placements"][0]["origin"]
    flag = deepcopy(companion["items"]["hole-diameter"][0])
    flag["text_placements"][0]["origin"] = [25, 35, 5]
    flag["leader"] = [5, 0, 5]
    note = semantic["pmi"]["notes"][1]["id"]
    companion["items"][note] = [flag]
    output = pmi.write(geometry, semantic, presentation=json.dumps(companion))
    drawn = pmi.presentation(output)
    assert drawn["items"][note][0]["name"] != "General notes"
    assert drawn["items"][note][0]["text_placements"][0]["origin"] == [25, 35, 5]
    assert drawn["items"][note][0]["leader"] == [5, 0, 5]
    companion["planes"][0]["normal"] = [0, 0, 0]
    with pytest.raises(pmi.PmiError, match="thickness"):
        pmi.write(geometry, semantic, presentation=companion)


def test_source_presentation_missing_text_origin_names_item(producer):
    geometry, _ = producer
    source_root = ROOT / "crates/haecceity/tests/fixtures/xcaf"
    semantic = json.loads((source_root / "pmi_plate.source.pmi.json").read_text())
    companion = json.loads((source_root / "pmi_plate.source.presentation.json").read_text())
    del companion["items"]["thickness"][0]["text_placements"][0]["origin"]
    with pytest.raises(pmi.PmiError, match="thickness.*origin"):
        pmi.write(geometry, semantic, presentation=companion)


@pytest.mark.parametrize("alignment,fraction", [(None, (0.5, 0.5)), ("bottom left", (0, 0)), ("top right", (1, 1))])
def test_source_alignment_and_leader_are_verified_on_ink(producer, alignment, fraction):
    geometry, _ = producer
    source_root = ROOT / "crates/haecceity/tests/fixtures/xcaf"
    semantic = json.loads((source_root / "pmi_plate.source.pmi.json").read_text())
    companion = json.loads((source_root / "pmi_plate.source.presentation.json").read_text())
    note = semantic["pmi"]["notes"][1]["id"]
    graphic = deepcopy(companion["items"]["hole-diameter"][0])
    origin, tip = [25, 35, 5], [5, 0, 5]
    graphic["text_placements"][0]["origin"] = origin
    if alignment is not None:
        graphic["text_placements"][0]["alignment"] = alignment
    graphic["leader"] = tip
    companion["items"] = {note: [graphic]}
    output = pmi.write(geometry, semantic, presentation=companion)
    lines = pmi.presentation(output)["items"][note][0]["polylines"]
    assert len(lines) > 2
    # This plain note has no frame: all strokes before the final leader are text.
    ink = [point for line in lines[:-1] for point in line]
    lower = [min(p[k] for p in ink) for k in range(3)]
    upper = [max(p[k] for p in ink) for k in range(3)]
    aligned = [lower[0] + fraction[0] * (upper[0] - lower[0]), lower[1] + fraction[1] * (upper[1] - lower[1]), (lower[2] + upper[2]) / 2]
    assert aligned == pytest.approx(origin, rel=0, abs=1e-9)
    leader = lines[-1]
    assert leader[0] == pytest.approx(tip, rel=0, abs=1e-9)
    assert leader[-1] == pytest.approx([max(lower[0], min(tip[0], upper[0])), lower[1], 5], rel=0, abs=1e-9)
    if alignment is not None:
        graphic["text_placements"][0]["alignment"] = "justify"
        with pytest.raises(pmi.PmiError, match=note + ".*alignment"):
            pmi.write(geometry, semantic, presentation=companion)
