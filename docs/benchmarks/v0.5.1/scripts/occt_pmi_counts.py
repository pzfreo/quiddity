"""Count the semantic PMI OpenCascade's XCAF reader finds in STEP files (fresh capture).
usage: python occt_pmi_counts.py FILE.stp[.gz] ...   -> JSON lines"""
import sys, json, gzip, tempfile, os
import OCP
from OCP.collections import Sequence_TDF_Label
from OCP.STEPCAFControl import STEPCAFControl_Reader
from OCP.TDocStd import TDocStd_Document
from OCP.TCollection import TCollection_ExtendedString
from OCP.XCAFApp import XCAFApp_Application
from OCP.XCAFDoc import XCAFDoc_DocumentTool
from OCP.IFSelect import IFSelect_RetDone

app = XCAFApp_Application.GetApplication_s()
for path in sys.argv[1:]:
    p = path
    tmp = None
    if path.endswith(".gz"):
        tmp = tempfile.NamedTemporaryFile(suffix=".stp", delete=False)
        tmp.write(gzip.open(path).read()); tmp.close(); p = tmp.name
    doc = TDocStd_Document(TCollection_ExtendedString("MDTV-XCAF"))
    app.NewDocument(TCollection_ExtendedString("MDTV-XCAF"), doc)
    r = STEPCAFControl_Reader()
    r.SetGDTMode(True); r.SetNameMode(True)
    rec = {"file": os.path.basename(path)}
    st = r.ReadFile(p)
    if st != IFSelect_RetDone:
        rec["error"] = "read"
    else:
        ok = r.Transfer(doc)
        tool = XCAFDoc_DocumentTool.DimTolTool_s(doc.Main())
        for name, fn in (("dimensions", tool.GetDimensionLabels), ("tolerances", tool.GetGeomToleranceLabels), ("datums", tool.GetDatumLabels)):
            s = Sequence_TDF_Label(); fn(s); rec[name] = s.Length()
        rec["transfer_ok"] = bool(ok)
    if tmp: os.unlink(tmp.name)
    print(json.dumps(rec), flush=True)
