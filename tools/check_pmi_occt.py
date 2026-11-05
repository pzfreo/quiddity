"""Record what OpenCascade's XCAF reader makes of the files haecceity's PMI writer wrote.

    cargo test --release -p haecceity --test pmi_write export_written_files -- --ignored
    ~/cld/quiddity/.venv/bin/python tools/check_pmi_occt.py [CASE ...]

Every ``tests/fixtures/ap242/write-presentation/<case>.step.gz`` (written by the Rust test
``export_written_files``: every kind of item the writer makes on a specify-core part, and
specify-core's intents written onto their original corpus files) is first opened in two child
processes, and how each child ends is recorded (``true``, or the signal or exit status):

- ``loads``: specify-core's own ``load.load_all`` (the Python side of Draftwright Specify opens
  a file with it). It died with SIGSEGV on every file whose datum feature symbols'
  ``draughting_model`` a ``mechanical_design_and_draughting_relationship`` related to the part's
  shape representation, which the writer therefore no longer writes (docs/step-ap242.md,
  maintainer decisions of 2026-10-09).
- ``reads``: ``STEPCAFControl_Reader`` (GD&T, names, colours) and the name of every label under
  the shape tool, each attribute reached through ``TDF_AttributeIterator``. ``load.load_all``
  looks names up with ``label.FindAttribute(GUID, TDataStd_Name())``, an in/out handle the OCP
  binding mishandles: that call alone crashes on NIST CTC-01 when made on every shape label, and
  the files with the relationship read here without crashing, so the crash is in that call, not
  in OpenCascade's reader (the docs give the evidence).

A file that loads is then read exactly as ``tools/capture_pmi_occt.py`` reads the NIST and
specify-core files (specify-core's ``load.py`` numbering), and the capture is written beside it
as ``<case>.occt.json.gz``. ``crates/haecceity/tests/pmi_write.rs``
(``opencascade_reads_the_written_files``) requires every file to load and read, compares each
capture with haecceity's reading of the same file, which equals what was written, and requires
a verdict for every difference in ``tests/fixtures/references/pmi_write.json`` ("occt"). Each datum
also records the presentation XCAF links to it (its datum feature symbol: name, edge count,
whether it has an annotation plane). ``metadata`` records, per shape label that has any, the
strings and reals XCAF's metadata reader (``TDataStd_NamedData``) holds under each name the
file's property definitions, general properties and descriptive items carry (the OCP binding
cannot list the map's keys): XCAF puts user defined attributes there, so the writer's
'semantic text' notes show there. XCAF has no surface texture. OpenCascade is a cross-check,
not an authority: where it misreads (its ``known_misreads``), haecceity is rust-correct with
the clause and the file text.

Set QUIDDITY_PMI_WRITE_FIXTURES=write to capture the feature-off goldens instead.
Run with an OCP venv (OCP 7.9); nothing outside this checkout is changed. Optional CASE
names select fixtures. If specify_core is absent, its loader status is explicitly unavailable;
XCAF transfer, safe label walking and the semantic capture still run.
"""

from __future__ import annotations

import gzip
import os
import importlib.util
import json
import re
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import capture_pmi_occt  # noqa: E402
from capture_pmi_occt import Capture, _entry, _text, capture_file  # noqa: E402
from OCP.TCollection import TCollection_ExtendedString  # noqa: E402
from OCP.TDF import TDF_AttributeIterator, TDF_ChildIterator  # noqa: E402
from OCP.TopAbs import TopAbs_EDGE  # noqa: E402
from OCP.TopExp import TopExp  # noqa: E402
from OCP.TopTools import TopTools_IndexedMapOfShape  # noqa: E402
from OCP.XCAFDoc import XCAFDoc_Datum  # noqa: E402

_STRING = r"'((?:[^']|'')*)'"
_NAMES = [
    re.compile(r"PROPERTY_DEFINITION\(" + _STRING),
    re.compile(r"GENERAL_PROPERTY\(" + _STRING + "," + _STRING),
    re.compile(r"DESCRIPTIVE_REPRESENTATION_ITEM\(" + _STRING),
]


def _candidate_names(text: str) -> list[str]:
    """Every name a property definition, general property or descriptive item of the file
    states (doubled quotes undone), once each, sorted."""
    out = set()
    for pattern in _NAMES:
        for m in pattern.finditer(text):
            out.update(g.replace("''", "'") for g in m.groups())
    out.discard("")
    return sorted(out)


def _attribute(label, type_name: str):
    """The attribute of type ``type_name`` on ``label``, reached by ``TDF_AttributeIterator``
    (whose ``Value`` the binding returns as the attribute's own type), or ``None``."""
    it = TDF_AttributeIterator(label)
    while it.More():
        a = it.Value()
        if a.DynamicType().Name() == type_name:
            return a
        it.Next()
    return None


def _labels(shapes):
    it = TDF_ChildIterator(shapes.Label(), True)
    while it.More():
        yield it.Value()
        it.Next()


class WrittenCapture(Capture):
    """The capture, with what XCAF makes of each datum's presentation (the datum feature
    symbol the writer derives for every datum it adds, decision 6, linked by a
    ``draughting_model_item_association``) and the metadata XCAF holds on shape labels."""

    def datum(self, label) -> dict:
        out = super().datum(label)
        obj = XCAFDoc_Datum.Set_s(label).GetObject()
        if obj.IsDatumTarget() and obj.HasDatumTargetParams():
            x = obj.GetDatumTargetAxis().XDirection()
            out["target"]["ref_direction"] = [x.X(), x.Y(), x.Z()]
        shape = obj.GetPresentation()
        if shape.IsNull():
            out["presentation"] = None
        else:
            edges = TopTools_IndexedMapOfShape()
            TopExp.MapShapes_s(shape, TopAbs_EDGE, edges)
            out["presentation"] = {
                "name": _text(obj.GetPresentationName()),
                "edges": edges.Extent(),
                "plane": obj.HasPlane(),
            }
        return out

    def metadata(self, names: list[str]) -> list[dict]:
        out = []
        for label in _labels(self.shapes):
            data = _attribute(label, "TDataStd_NamedData")
            if data is None:
                continue
            data.LoadDeferredData()
            strings, reals = {}, {}
            for n in names:
                key = TCollection_ExtendedString(n)
                if data.HasString(key):
                    strings[n] = data.GetString(key).ToExtString()
                if data.HasReal(key):
                    reals[n] = data.GetReal(key)
            part = next((i for i, p in enumerate(self.parts) if p.IsEqual(label)), None)
            out.append({"label": _entry(label), "part": part, "strings": strings, "reals": reals})
        return out


capture_pmi_occt.Capture = WrittenCapture

WRITE = (
    Path(__file__).resolve().parent.parent / "tests" / "fixtures" / "ap242"
    / os.environ.get("QUIDDITY_PMI_WRITE_FIXTURES", "write-presentation")
)


def child(mode: str, path: str) -> None:
    """A child process: ``load`` is specify-core's ``load.load_all``; ``read`` reads the file
    and the name of every shape label without ``FindAttribute``. Exits 0 when done."""
    if mode == "load":
        from specify_core.load import load_all

        load_all(path)
        sys.exit(0)
    c = capture_pmi_occt
    c.STEPControl_Controller.Init_s()
    fmt = TCollection_ExtendedString("MDTV-XCAF")
    doc = c.TDocStd_Document(fmt)
    c.XCAFApp_Application.GetApplication_s().NewDocument(fmt, doc)
    reader = c.STEPCAFControl_Reader()
    reader.SetGDTMode(True)
    reader.SetNameMode(True)
    reader.SetColorMode(True)
    if reader.ReadFile(path) != c.IFSelect_RetDone or not reader.Transfer(doc):
        sys.exit(3)
    if mode == "notes-up":
        from OCP.XCAFDoc import XCAFDoc_View

        labels = c.TDF_LabelSequence()
        c.XCAFDoc_DocumentTool.ViewTool_s(doc.Main()).GetViewLabels(labels)
        found = False
        for i in range(1, labels.Length() + 1):
            view = XCAFDoc_View.Set_s(labels.Value(i)).GetObject()
            if view.Name() is not None and view.Name().ToCString() == "Notes":
                up = view.UpDirection()
                direction = (up.X(), up.Y(), up.Z())
                assert max(abs(a - b) for a, b in zip(direction, (0., 0., 1.))) < 1e-8, direction
                found = True
        assert found, "OCCT did not read the Notes camera"
        sys.exit(0)
    for label in _labels(c.XCAFDoc_DocumentTool.ShapeTool_s(doc.Main())):
        name = _attribute(label, "TDataStd_Name")
        if name is not None:
            name.Get().ToExtString()
    sys.exit(0)


def ends(mode: str, plain: Path) -> bool | dict:
    run = subprocess.run(
        [sys.executable, __file__, "--child", mode, str(plain)], capture_output=True, check=False
    )
    if run.returncode == 0:
        return True
    if run.returncode < 0:
        return {"signal": -run.returncode}
    return {"exit": run.returncode}


def check(path: Path) -> dict:
    data = gzip.decompress(path.read_bytes())
    with tempfile.TemporaryDirectory() as tmp:
        plain = Path(tmp) / path.name.removesuffix(".gz")
        plain.write_bytes(data)
        loads = (ends("load", plain) if importlib.util.find_spec("specify_core") is not None
                 else {"unavailable": "specify_core is not installed in this environment"})
        status = {"loads": loads, "reads": ends("read", plain)}
        if status["reads"] is not True:
            return {"file": path.name, "sha256": capture_pmi_occt.sha256(path), **status}
        record = capture_file(path)
        record["metadata"] = WrittenCapture(plain).metadata(_candidate_names(data.decode("latin-1")))
    record.update(status)
    return record


def main() -> None:
    if sys.argv[1:2] == ["--child"]:
        child(sys.argv[2], sys.argv[3])
    files = ([WRITE / f"{name}.step.gz" for name in sys.argv[1:]]
             if sys.argv[1:] else sorted(WRITE.glob("*.step.gz")))
    if not files:
        sys.exit(f"no written files in {WRITE}: run export_written_files first")
    for path in files:
        record = check(path)
        stem = path.name.removesuffix(".step.gz")
        text = json.dumps(record, allow_nan=False, sort_keys=True, separators=(",", ":")) + "\n"
        (WRITE / f"{stem}.occt.json.gz").write_bytes(gzip.compress(text.encode(), mtime=0))
        if record["loads"] is not True:
            print(path.name, "specify-core loader status:", record["loads"], file=sys.stderr)
        if record["reads"] is not True:
            print(path.name, "XCAF reader status:", record["reads"], file=sys.stderr)
            continue
        counts = [
            sum(len(p[k]) for p in record["parts"])
            for k in ("dimensions", "geometric_tolerances", "datums")
        ]
        print(path.name, "reads", record["reads"], "dims/tols/datums", *counts, file=sys.stderr)


if __name__ == "__main__":
    main()
