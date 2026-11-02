#!/usr/bin/env python3
"""Original segmented spring and pin-pattern negatives; regenerate with OCP 7.9.3.1.

No CAD runtime is required by the tests. The swept positive is the existing
modelled_threads/round_wire_spring.step.gz (tools/make_thread_fixtures.py).
"""
import gzip
import math
import tempfile
from pathlib import Path

from OCP.BRep import BRep_Builder
from OCP.BRepAlgoAPI import BRepAlgoAPI_Fuse
from OCP.BRepPrimAPI import BRepPrimAPI_MakeCylinder
from OCP.TopoDS import TopoDS_Compound
from OCP.gp import gp_Ax2, gp_Dir, gp_Pnt

from make_thread_fixtures import write
import make_thread_fixtures

make_thread_fixtures.ROOT = Path(__file__).resolve().parents[1] / 'tests/fixtures/helical_springs'


def cylinder(a, b, radius, extension=0):
    d = [b[i] - a[i] for i in range(3)]
    length = math.sqrt(sum(v*v for v in d))
    d = [v/length for v in d]
    a = [a[i] - extension*d[i] for i in range(3)]
    return BRepPrimAPI_MakeCylinder(gp_Ax2(gp_Pnt(*a), gp_Dir(*d)), radius, length+2*extension).Shape()


def segmented():
    # Three turns, 24 straight wire segments per turn. Overlapping end disks mimic
    # text-to-cad's shock spring, without redistributing the original assembly.
    points = [(12*math.cos(i*math.tau/24), 12*math.sin(i*math.tau/24), i*4/24) for i in range(73)]
    result = None
    for a, b in zip(points, points[1:]):
        segment = cylinder(a, b, .75, .75)
        if result is None:
            result = segment
        else:
            fuse = BRepAlgoAPI_Fuse(result, segment)
            fuse.Build()
            assert fuse.IsDone()
            result = fuse.Shape()
    write('segmented', result)


def pins(name, centers):
    compound = TopoDS_Compound()
    builder = BRep_Builder()
    builder.MakeCompound(compound)
    for x, y in centers:
        builder.Add(compound, cylinder((x, y, 0), (x, y, 5), .75))
    write(name, compound)


def torus_chain():
    """Tangent biarcs interpolate a helix, producing 72 analytic torus patches."""
    from OCP.GC import GC_MakeArcOfCircle
    from OCP.gp import gp_Vec, gp_Circ
    from OCP.BRepBuilderAPI import BRepBuilderAPI_MakeEdge, BRepBuilderAPI_MakeWire, BRepBuilderAPI_MakeFace
    from OCP.BRepOffsetAPI import BRepOffsetAPI_MakePipe
    from OCP.GeomFill import GeomFill_IsFrenet
    R=12;pitch=4;wire=.75
    h=pitch/math.tau
    vec=lambda a:[-R*math.sin(a),R*math.cos(a),h]
    unit=lambda v:[x/math.sqrt(sum(x*x for x in v)) for x in v]
    dot=lambda a,b:sum(x*y for x,y in zip(a,b))
    pts=[(R*math.cos(i*math.tau/12),R*math.sin(i*math.tau/12),h*i*math.tau/12) for i in range(37)]
    spine=BRepBuilderAPI_MakeWire()
    for i,(a,b) in enumerate(zip(pts,pts[1:])):
     t0=unit(vec(i*math.tau/12));t1=unit(vec((i+1)*math.tau/12));v=[b[j]-a[j] for j in range(3)];s=[t0[j]+t1[j] for j in range(3)];vsum=dot(v,s);den=2*(1-dot(t0,t1));d=(-vsum+math.sqrt(vsum*vsum+den*dot(v,v)))/den
     pm=[(a[j]+b[j]+d*(t0[j]-t1[j]))/2 for j in range(3)]
     c0=GC_MakeArcOfCircle(gp_Pnt(*a),gp_Vec(*t0),gp_Pnt(*pm)).Value()
     c1=GC_MakeArcOfCircle(gp_Pnt(*b),gp_Vec(*[-v for v in t1]),gp_Pnt(*pm)).Value();c1.Reverse()
     spine.Add(BRepBuilderAPI_MakeEdge(c0).Edge());spine.Add(BRepBuilderAPI_MakeEdge(c1).Edge())
    profile=BRepBuilderAPI_MakeWire(BRepBuilderAPI_MakeEdge(gp_Circ(gp_Ax2(gp_Pnt(*pts[0]),gp_Dir(*vec(0))),wire)).Edge()).Wire()
    shape=BRepOffsetAPI_MakePipe(spine.Wire(),BRepBuilderAPI_MakeFace(profile).Face(),GeomFill_IsFrenet).Shape()
    write('torus_chain', shape)


if __name__ == '__main__':
    segmented()
    torus_chain()
    # Mirror the original true swept fixture to exercise intrinsic handedness.
    from OCP.BRepBuilderAPI import BRepBuilderAPI_Transform
    from OCP.STEPControl import STEPControl_Reader
    from OCP.gp import gp_Trsf
    original = Path(__file__).resolve().parents[1] / 'tests/fixtures/modelled_threads/round_wire_spring.step.gz'
    with tempfile.TemporaryDirectory() as temp:
        path = Path(temp) / 'sweep.step'
        path.write_bytes(gzip.decompress(original.read_bytes()))
        reader = STEPControl_Reader()
        reader.ReadFile(str(path))
        reader.TransferRoots()
        mirror = gp_Trsf()
        mirror.SetMirror(gp_Ax2(gp_Pnt(0, 0, 0), gp_Dir(0, 1, 0)))
        write('swept_left', BRepBuilderAPI_Transform(reader.OneShape(), mirror, True).Shape())
    pins('bolt_circle', [(12*math.cos(i*math.tau/24), 12*math.sin(i*math.tau/24)) for i in range(24)])
    pins('pin_row', [(i*3, 0) for i in range(24)])
