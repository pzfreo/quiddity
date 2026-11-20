"""Compare current Rust face integrals to live OpenCascade, without writing captures.

Run with the read-only OpenCascade Python environment, QUIDDITY pointing at its source,
QUIDDITY_CORPUS pointing at the corpus, and --rust target/release/examples/face_integrals.
--report must point outside the repository. Existing frozen ledgers supply verdicts, never
expectations: unlisted differences fail and convergence failures are reported separately.
"""
from __future__ import annotations

import argparse
import gzip
import json
import math
import os
from pathlib import Path
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from capture_face_areas import _load, _edges
from OCP.BRep import BRep_Builder, BRep_Tool
from OCP.BRepBuilderAPI import BRepBuilderAPI_Copy
from OCP.ShapeAnalysis import ShapeAnalysis_Edge
from OCP.ShapeFix import ShapeFix_Face
from OCP.TopoDS import TopoDS
from OCP.BRepGProp import BRepGProp
from OCP.GProp import GProp_GProps

ROOT = Path(__file__).resolve().parent.parent


def reference(face, eps):
    props = GProp_GProps()
    error = BRepGProp.SurfaceProperties_s(face, props, eps)
    c = props.CentreOfMass()
    inertia = props.MatrixOfInertia()
    half_trace = sum(inertia.Value(i, i) for i in range(1, 4)) / 2
    return dict(area=props.Mass(), centroid=[c.X(), c.Y(), c.Z()],
                second_moments=[[((half_trace if i == j else 0) - inertia.Value(i+1, j+1))
                                 for j in range(3)] for i in range(3)], estimated_error=error)


def rebuilt_reference(face):
    """Same projection used by capture_face_moments, evaluated live for the tensor too."""
    copy = TopoDS.Face_s(BRepBuilderAPI_Copy(face, True, False).Shape())
    builder = BRep_Builder()
    for edge in _edges(copy):
        tolerance = BRep_Tool.Tolerance_s(edge)
        if BRep_Tool.IsClosed_s(edge, copy):
            builder.UpdateEdge(edge, None, None, copy, tolerance)
        else:
            builder.UpdateEdge(edge, None, copy, tolerance)
    fix = ShapeFix_Face(copy)
    fix.Perform()
    rebuilt = fix.Face()
    analysis = ShapeAnalysis_Edge()
    if not all(analysis.HasPCurve(edge, rebuilt) for edge in _edges(rebuilt)):
        return None
    return reference(rebuilt, 1e-9), reference(rebuilt, 1e-11)


def independent_reference(face, centre, panels):
    """Independent OCCT surface evaluator and Lobatto boundary interpolation, no pcurves.

    Reuses kernel_evidence's path, not Rust's Newton velocities or adaptive quadrature.
    The two panel counts must agree before this evidence supports a verdict.
    """
    import numpy as np
    from OCP.gp import gp_Pnt, gp_Vec
    from kernel_evidence import _Surface, _integrate

    class MomentSurface(_Surface):
        components = 10

        def densities(self, u, v):
            u, v = self._wrapped(u, v)
            p, du, dv = gp_Pnt(), gp_Vec(), gp_Vec()
            self.adaptor.D1(u, v, p, du, dv)
            x, y, z = np.array([p.X(), p.Y(), p.Z()]) - self.centre
            return du.Crossed(dv).Magnitude() * np.array([1, x, y, z, x*x, y*y, z*z, x*y, x*z, y*z])

    surface = MomentSurface(face)
    surface.centre = np.array(centre)
    values = _integrate(face, surface, panels)
    if values is None or values[0] == 0 or not np.isfinite(values).all():
        return None
    values *= math.copysign(1, values[0])
    area = values[0]
    mean = values[1:4] / area
    xx, yy, zz, xy, xz, yz = values[4:]
    q = np.array([[xx, xy, xz], [xy, yy, yz], [xz, yz, zz]]) - area*np.outer(mean, mean)
    return dict(area=float(area), centroid=(surface.centre+mean).tolist(), second_moments=q.tolist())


def errors(a, b, length, tolerance=1e-6):
    if a is None:
        return dict(missing=True)
    area = abs(a['area']-b['area']) / max(abs(b['area']), 1e-300)
    centroid = max(abs(x-y) / max(abs(y), 1) for x, y in zip(a['centroid'], b['centroid']))
    centroid_face_size = math.dist(a['centroid'], b['centroid']) / max(length, 1)
    q = b['second_moments']
    scale = max(abs(sum(q[i][i] for i in range(3))), 1e-300)
    second = max(abs(a['second_moments'][i][j]-q[i][j]) for i in range(3) for j in range(3)) / scale
    # Keep microscopic faces explicit: the frozen area harness has an absolute floor,
    # but this live comparison must also expose relative differences on those faces.
    failed = []
    if abs(a['area']-b['area']) > tolerance*abs(b['area']):
        failed.append('area')
    if centroid > tolerance:
        failed.append('centroid')
    if second*scale > tolerance*scale:
        failed.append('second_moments')
    return dict(area=area, centroid=centroid, centroid_face_size=centroid_face_size,
                second_moments=second, failed=failed)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--rust', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--file', action='append', help='restrict to these corpus-relative files')
    parser.add_argument('--face', type=int, action='append', help='restrict to these indices (requires exactly one --file)')
    parser.add_argument('--independent', action='store_true', help='also integrate exceptions over the 3D boundary at 16 and 32 Lobatto panels per curve span')
    args = parser.parse_args()
    if args.face and (not args.file or len(args.file) != 1):
        parser.error('--face requires exactly one --file')
    if args.report.resolve().is_relative_to(ROOT):
        parser.error('--report must be outside the repository')
    corpus = Path(os.environ['QUIDDITY_CORPUS']).resolve()
    capture = json.loads(gzip.decompress((ROOT/'tests/fixtures/face_areas.json.gz').read_bytes()))
    available = {entry['file'] for entry in capture['files']}
    if args.file and set(args.file) - available:
        parser.error(f'files absent from the frozen inventory: {sorted(set(args.file)-available)}')
    ledger = {}
    for name in ['references/face_areas.json', 'references/face_moments.json']:
        for row in json.loads((ROOT/'tests/fixtures'/name).read_text()):
            if row.get('check', 'occ') == 'occ':
                ledger[(row['file'], row['face'])] = row
    tensor_path = ROOT/'tests/fixtures/references/face_integrals.json'
    tensor_rows = json.loads(tensor_path.read_text()) if tensor_path.exists() else []
    tensor_known = {(r['file'], r['face']): r for r in tensor_rows
                    if (not args.file or r['file'] in args.file)
                    and (not args.face or r['face'] in args.face)}
    ledger.update(tensor_known)
    second_differences = set()
    checked_masters = set()
    rows, total, agreed, nonconverged = [], 0, 0, 0
    for entry in capture['files']:
        file = entry['file']
        if args.file and file not in args.file:
            continue
        rust = json.loads(subprocess.check_output([str(args.rust.resolve()), str(corpus/file)], text=True))['faces']
        faces = list(_load(corpus/file).faces())
        if len(rust) != len(faces):
            raise RuntimeError(f'{file}: inventory mismatch {len(rust)} vs {len(faces)}')
        if args.face and any(i < 0 or i >= len(faces) for i in args.face):
            parser.error(f'{file}: face index outside [0, {len(faces)})')
        for i, (got, face) in enumerate(zip(rust, faces)):
            if args.face and i not in args.face:
                continue
            total += 1
            fine, finer = reference(face.wrapped, 1e-9), reference(face.wrapped, 1e-11)
            length = math.dist(*got['bounds']) if got else 1
            convergence = errors(fine, finer, length)
            diff = errors(got, finer, length)
            pin = tensor_known.get((file, i))
            master = pin.get('step_master') if pin else None
            if master:
                checked_masters.add((file, i))
            if 'second_moments' in diff.get('failed', []):
                second_differences.add((file, i))
            # A malformed STEP import is known to reorder faces. Do not compare unrelated faces.
            order = file == 'inventory_refusal/13975.step.gz'
            if not master and not diff.get('missing') and not diff['failed'] and not convergence['failed'] and not order:
                agreed += 1
                continue
            known = ledger.get((file, i))
            rebuilt_pair = None if order else rebuilt_reference(face.wrapped)
            rebuilt = rebuilt_pair[-1] if rebuilt_pair else None
            rebuilt_convergence = errors(*rebuilt_pair, length) if rebuilt_pair else None
            rebuilt_diff = errors(got, rebuilt, length) if rebuilt else None
            row = dict(file=file, face=i, rust=got, occ=finer, errors=diff, convergence=convergence,
                       rebuilt=rebuilt, rebuilt_errors=rebuilt_diff, rebuilt_convergence=rebuilt_convergence)
            if args.independent and got is not None and not order:
                independent = [independent_reference(face.wrapped, got['centroid'], n) for n in (16, 32)]
                row['independent'] = independent
                if all(x is not None for x in independent):
                    row['independent_convergence'] = errors(*independent, length)
                    row['independent_errors'] = errors(got, independent[-1], length)
            independent_diff = row.get('independent_errors')
            if got is None:
                row.update(verdict='unlisted', reason='Rust refused this face integral; a missing integral is never a zero area.')
            elif master:
                row.update(verdict='rust-correct', reason='Independent integration of the STEP-selected pcurves gives the pinned area, centroid and tensor.')
            elif independent_diff and not independent_diff['failed'] and not row['independent_convergence']['failed']:
                row.update(verdict='rust-correct', reason='Area, centroid and central second moments agree with independent OCCT surface integration over the 3D edge boundary, converged at 16 and 32 Lobatto panels per curve span.')
            elif order:
                row.update(verdict='not-applicable', reason='Known OpenCascade healing reorders this inventory; references/face_areas.json order entry.')
            elif convergence['failed']:
                nonconverged += 1
                row.update(verdict='undetermined', reason='OpenCascade eps 1e-9 and 1e-11 disagree at the comparison tolerance.')
            elif rebuilt_diff and not rebuilt_diff.get('missing') and not rebuilt_diff['failed'] and not rebuilt_convergence['failed']:
                row.update(verdict='rust-correct', reason='All moments agree with live OpenCascade after projecting pcurves from the 3D edges; the supplied pcurves bound a different region.')
            elif known:
                row.update(verdict=known['verdict'], reason=known['reason'], evidence='Existing frozen face ledger; the same trimmed boundary is used for all moments.')
            else:
                row.update(verdict='unlisted', reason='Live OpenCascade comparison needs investigation.')
            expected = pin.get('step_master', pin['rust']) if pin else None
            if expected and (got is None or errors(got, expected, length, 1e-7)['failed']):
                row.update(verdict='unlisted', reason='The listed Rust tensor changed; the evidence and pin must be reviewed.')
            rows.append(row)
        print(f'{file}: {len(faces)} faces', file=sys.stderr, flush=True)
        result = dict(faces=total, agree=agreed, exceptions=len(rows), nonconverged=nonconverged, rows=rows,
                      tolerance='1e-6 relative area and tensor trace; each centroid coordinate 1e-6 of max(abs(coordinate), 1 mm); no absolute area or tensor floor')
        args.report.write_text(json.dumps(result, indent=2)+'\n')
    stale = sorted(set(tensor_known) - second_differences - checked_masters)
    result['stale'] = stale
    args.report.write_text(json.dumps(result, indent=2)+'\n')
    print(json.dumps({k: v for k, v in result.items() if k != 'rows'}))
    return int(bool(stale) or any(r['verdict'] == 'unlisted' for r in rows))


if __name__ == '__main__':
    sys.exit(main())
