"""Reproduce OCCT mass-fixture reference defects without tessellating surfaces.

Run with $OCP_PY tools/mass_fixture_evidence.py. Reads committed inputs only;
prints OCCT's version, whole/per-face GProps and independently converged boundary
integrals. No downloads or file writes. OCCT heals/reorders the shaft's faces.
"""
import math
from pathlib import Path

import gvp_occt as g
from OCP.BRepAdaptor import BRepAdaptor_Curve, BRepAdaptor_Curve2d, BRepAdaptor_Surface
from OCP.TopAbs import TopAbs_EDGE, TopAbs_REVERSED
from OCP.TopExp import TopExp_Explorer
from OCP.TopoDS import TopoDS

ROOT = Path(__file__).resolve().parents[1] / 'tests/fixtures'
face_cast = getattr(TopoDS, 'Face_s', None) or TopoDS.Face
edge_cast = getattr(TopoDS, 'Edge_s', None) or TopoDS.Edge


def boundaries(face, n, lifted=False):
    surface = BRepAdaptor_Surface(face)
    explorer = TopExp_Explorer(face, TopAbs_EDGE)
    edges = []
    while explorer.More():
        edge = edge_cast(explorer.Current())
        curve = BRepAdaptor_Curve2d(edge, face) if lifted else BRepAdaptor_Curve(edge)
        a, b = curve.FirstParameter(), curve.LastParameter()
        if edge.Orientation() == TopAbs_REVERSED:
            a, b = b, a
        points = [curve.Value(a + (b-a)*k/n).Coord() for k in range(n+1)]
        edges.append(points)
        explorer.Next()
    return surface, edges


def segments(edges, close):
    for i, edge in enumerate(edges):
        yield from zip(edge, edge[1:])
        if close:
            yield edge[-1], edges[(i+1) % len(edges)][0]


def main():
    print('OCP', g.OCP.__version__, 'BRepGProp eps', g.EPS)
    for name in ['truck_master_shaft_hole.step.gz',
                 'modelled_threads/varying_pitch_sweep.step.gz',
                 'captured/effective_surfaces/parts/2fe0e2383b80a0b8.step.gz']:
        shape = g.load(ROOT / name)[0]
        faces = [face_cast(f) for f in g.indexed(shape, g.TopAbs_FACE)]
        print(name, g.material_measure(shape))
        for i, face in enumerate(faces):
            print('OCCT face', i, g.measure(face))
        if name.startswith('modelled_threads'):
            for i in [7, 8, 20]:
                for n in [100, 1000, 10000, 100000]:
                    _, edges = boundaries(faces[i], n, lifted=True)
                    h = math.fsum(-5*(a[1]+b[1])*(b[0]-a[0])
                                  for a, b in segments(edges, True))
                    closed_g = math.fsum(5*(a[0]+b[0])*(b[1]-a[1])
                                         for a, b in segments(edges, True))
                    open_g = math.fsum(5*(a[0]+b[0])*(b[1]-a[1])
                                       for a, b in segments(edges, False))
                    print('thread face', i, n, 'closed -vdu', h,
                          'closed udv', closed_g, 'open udv (OCCT)', open_g)
        elif name.startswith('truck'):
            # Quarter disks in x=constant planes. OCCT's healed order differs
            # from source order: our4..7 -> OCCT17,18,20,23; our11,12,13,18
            # -> OCCT9,10,11,14 (equal areas within each group).
            for i, exact in [(17, 225*math.pi/4), (9, 25*math.pi)]:
                for n in [100, 1000, 10000]:
                    surface, edges = boundaries(faces[i], n)
                    _, pcurves = boundaries(faces[i], n, lifted=True)
                    lifted = [[surface.Value(*q).Coord() for q in edge] for edge in pcurves]
                    area = lambda curves: abs(math.fsum(
                        (a[1]*b[2]-b[1]*a[2])/2 for a, b in segments(curves, True)))
                    print('shaft cap', i, n, '3D boundary', area(edges),
                          'surface(pcurve)', area(lifted), 'analytic', exact)


if __name__ == '__main__':
    main()
