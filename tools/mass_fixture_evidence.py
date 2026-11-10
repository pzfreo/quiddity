"""Reproduce OCCT mass-fixture reference defects without tessellating surfaces.

Run with $OCP_PY tools/mass_fixture_evidence.py [--freeform]. Reads committed inputs only;
prints OCCT's version, whole/per-face GProps and independently converged boundary
integrals. No downloads or file writes. OCCT heals/reorders the shaft's faces.
"""
import argparse
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


def spline_evidence(shape):
    """Independent Green quadrature, including projection of exact 3D edges.

    Deliberately different from mass.rs: polygonal boundary trapezoids, exact
    OCP surface partials, a fixed Gauss primitive, and explicit UV gap segments.
    The open primitive starts at the face's u_min, as OCCT's does.
    """
    from OCP.BRep import BRep_Tool
    from OCP.BRepTools import BRepTools
    from OCP.ShapeAnalysis import ShapeAnalysis_Surface
    from OCP.gp import gp_Pnt, gp_Vec
    nodes = []
    for i in range(16):
        x = math.cos(math.pi*(i+.75)/16.5)
        for _ in range(30):
            p0, p1 = 1., x
            for k in range(2, 17):
                p0, p1 = p1, ((2*k-1)*x*p1-(k-1)*p0)/k
            derivative = 16*(x*p1-p0)/(x*x-1)
            step = p1/derivative
            x -= step
            if abs(step) < 1e-16:
                break
        nodes.append((x, 2/((1-x*x)*derivative*derivative)))
    faces = g.indexed(shape, g.TopAbs_FACE)
    for index in [4, 17]:
        face = face_cast(faces[index])
        surface = BRepAdaptor_Surface(face)
        projector = ShapeAnalysis_Surface(BRep_Tool.Surface_s(face))
        spline = surface.BSpline()
        knots = [spline.UKnot(k) for k in range(1, spline.NbUKnots()+1)]
        reference = BRepTools.UVBounds_s(face)[0]
        point, su, sv = gp_Pnt(), gp_Vec(), gp_Vec()

        def primitive(u, v):
            cuts = sorted([reference, u] + [k for k in knots
                          if min(reference, u) < k < max(reference, u)])
            total = 0.
            for a, b in zip(cuts, cuts[1:]):
                half, mid = (b-a)/2, (a+b)/2
                for x, weight in nodes:
                    surface.D1(mid+half*x, v, point, su, sv)
                    total += half*weight*su.Crossed(sv).Magnitude()
            return total if u >= reference else -total

        for projected in [False, True]:
            for n in [1000, 10000]:
                explorer = TopExp_Explorer(face, TopAbs_EDGE)
                edges = []
                while explorer.More():
                    edge = edge_cast(explorer.Current())
                    curve = BRepAdaptor_Curve(edge)
                    pcurve = BRepAdaptor_Curve2d(edge, face)
                    a, b = curve.FirstParameter(), curve.LastParameter()
                    if edge.Orientation() == TopAbs_REVERSED:
                        a, b = b, a
                    samples = []
                    for k in range(n+1):
                        t = a+(b-a)*k/n
                        uv = pcurve.Value(t)
                        if projected:
                            uv = projector.NextValueOfUV(uv, curve.Value(t), 1e-12)
                        samples.append(uv.Coord())
                    edges.append(samples)
                    explorer.Next()
                # Explorer reverses edge senses but retains list order.
                if face.Orientation() == TopAbs_REVERSED:
                    edges.reverse()
                terms, gaps = [], []
                for j, edge in enumerate(edges):
                    values = [primitive(*uv) for uv in edge]
                    terms.extend((ga+gb)/2*(b[1]-a[1]) for a, b, ga, gb
                                 in zip(edge, edge[1:], values, values[1:]))
                    a, b = edge[-1], edges[(j+1) % len(edges)][0]
                    for x, weight in nodes:
                        t = (1+x)/2
                        gaps.append(weight/2*primitive(a[0]+t*(b[0]-a[0]),
                                    a[1]+t*(b[1]-a[1]))*(b[1]-a[1]))
                sign = -1 if face.Orientation() == TopAbs_REVERSED else 1
                print('spline face', index, n, 'projected3D', projected,
                      'open udv', sign*math.fsum(terms),
                      'closed udv', sign*(math.fsum(terms)+math.fsum(gaps)), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--freeform', action='store_true',
                        help='also run the slower independent spline quadrature')
    args = parser.parse_args()
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
            if args.freeform:
                spline_evidence(shape)
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
