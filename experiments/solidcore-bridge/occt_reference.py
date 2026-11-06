"""OpenCascade reference for the bridge's cut: volume and face count of a part minus the
half-space above `cut` along `axis`, using the same oversized box as `bridge`.

Usage: python occt_reference.py FILE.step[.gz] AXIS CUT
"""
import gzip, sys, tempfile
from OCP.STEPControl import STEPControl_Reader
from OCP.BRepAlgoAPI import BRepAlgoAPI_Cut
from OCP.BRepPrimAPI import BRepPrimAPI_MakeBox
from OCP.BRepBndLib import BRepBndLib
from OCP.Bnd import Bnd_Box
from OCP.GProp import GProp_GProps
from OCP.BRepGProp import BRepGProp
from OCP.gp import gp_Pnt
from OCP.TopExp import TopExp_Explorer
from OCP.TopAbs import TopAbs_FACE

path, axis, cut = sys.argv[1], "xyz".index(sys.argv[2]), float(sys.argv[3])
data = gzip.open(path).read() if path.endswith(".gz") else open(path, "rb").read()
with tempfile.NamedTemporaryFile(suffix=".step") as f:
    f.write(data); f.flush()
    r = STEPControl_Reader(); r.ReadFile(f.name); r.TransferRoots(); shape = r.OneShape()

def volume(s):
    p = GProp_GProps(); BRepGProp.VolumeProperties_s(s, p); return p.Mass()
def nfaces(s):
    e, n = TopExp_Explorer(s, TopAbs_FACE), 0
    while e.More(): n += 1; e.Next()
    return n

b = Bnd_Box(); BRepBndLib.Add_s(shape, b); lo = b.CornerMin(); hi = b.CornerMax()
lo, hi = [lo.X(), lo.Y(), lo.Z()], [hi.X(), hi.Y(), hi.Z()]
margin = 10 + max(h - l for h, l in zip(hi, lo))
o = [l - margin for l in lo]; top = [h + margin for h in hi]
o[axis] = cut
tool = BRepPrimAPI_MakeBox(gp_Pnt(*o), gp_Pnt(*top)).Shape()
res = BRepAlgoAPI_Cut(shape, tool).Shape()
print(f"occt: before {nfaces(shape)} faces, volume {volume(shape):.3f}; after {nfaces(res)} faces, volume {volume(res):.3f}")
