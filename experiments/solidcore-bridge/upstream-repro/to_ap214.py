"""Re-export a STEP file's shape as a geometry-only AP214 file with OpenCascade.

Usage: python to_ap214.py IN.step[.gz] OUT.step
"""
import gzip, sys, tempfile
from OCP.STEPControl import STEPControl_Reader, STEPControl_Writer, STEPControl_AsIs
from OCP.Interface import Interface_Static
src, dst = sys.argv[1], sys.argv[2]
data = gzip.open(src).read() if src.endswith(".gz") else open(src, "rb").read()
with tempfile.NamedTemporaryFile(suffix=".step") as f:
    f.write(data); f.flush()
    r = STEPControl_Reader(); r.ReadFile(f.name); r.TransferRoots(); shape = r.OneShape()
Interface_Static.SetCVal_s("write.step.schema", "AP214IS")
Interface_Static.SetCVal_s("write.step.product.name", "part")
w = STEPControl_Writer(); w.Transfer(shape, STEPControl_AsIs); w.Write(dst)
