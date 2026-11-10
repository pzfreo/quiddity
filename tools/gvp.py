"""Print ours/A(GVP)/B(OCCT) accuracy tables; never downloads or modifies inputs.

Build: cargo build -p haecceity -j 6 --release --example gvp
Run: $OCP_PY tools/gvp.py --rust target/release/examples/gvp DIRECTORY ...
Optional --json writes detailed machine-readable evidence outside the repository.
"""
import argparse
import collections
import json
import math
from pathlib import Path
import subprocess
import sys
from gvp_occt import compare

BAND = 0.001
QUANTITIES = ('volume', 'area', 'centroid')
# Known OCCT (B) reference problems, applied only while ours meets the exact value.
# turned_body_disc.step: tools/make_turned_body_fixture.py's ring, inner radius 4,
# outer radius 11 over z in [-3.25, 3.25] with 45-degree chamfers to radius 10.75 at
# z = +-3.5. Its exact volume and area follow from that profile, and its centroid is
# the origin. OCCT measures about 0.8% less volume, likely from the
# negative-semi-angle cone.
EXACT = {
    'turned_body_disc.step': dict(
        volume=math.pi*(105*6.5 + 2*(0.25*(121 + 11*10.75 + 10.75**2)/3 - 4)),
        area=2*math.pi*(11*6.5 + 4*7 + (11 + 10.75)*0.25*math.sqrt(2) + 10.75**2 - 16),
        centroid=[0.0, 0.0, 0.0],
        reason='OCCT disagrees with the exact value of the generated profile'),
}


def error(a, b, quantity, diagonal):
    if a is None or b is None:
        return None
    if quantity == 'centroid':
        return math.dist(a, b)/diagonal if diagonal > 0 else math.inf
    return abs(a-b)/abs(b) if b else (0.0 if a == 0 else math.inf)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    source=parser.add_mutually_exclusive_group(required=True)
    source.add_argument('--rust', type=Path)
    source.add_argument('--measurements', type=Path, help='render previously captured measurement JSON; OCCT is recomputed')
    parser.add_argument('--json', type=Path)
    parser.add_argument('--jobs', type=int, choices=range(1,7), default=1)
    parser.add_argument('paths', nargs='*')
    args = parser.parse_args()
    if args.rust and not args.paths:
        parser.error('paths are required with --rust')
    process = subprocess.Popen([str(args.rust.resolve()), '--jobs', str(args.jobs), *args.paths], stdout=subprocess.PIPE, text=True) if args.rust else None
    lines=process.stdout if process else map(json.dumps,json.loads(args.measurements.read_text()))
    rows, counts, worst = [], collections.Counter(), []
    print('Units mm^3, mm^2, mm. Relative centroid error = distance / part box diagonal.')
    print('Acceptance <=0.001 for all three normalized errors. Missing values are UNAVAILABLE, never PASS.')
    print('file | part/property | reference | quantity | ours | reference value | error | status/classification')

    def known_b(file, quantity, a, reference, diagonal):
        exact = EXACT.get(Path(file).name)
        if exact is not None:
            e = error(a, exact[quantity], quantity, diagonal)
            return exact['reason'] if e is not None and e <= BAND else None
        # OCCT has no SOLID for this part, so it gives no volume or material centroid;
        # ours integrates the closed shell. A definition difference, not a measurement.
        if reference.get('no_solid') and quantity != 'area':
            return 'OCCT has no SOLID for this part, so no volume or material centroid'
        return None

    def emit(file, label, ref, ours, reference, diagonal, other=None):
        for quantity in QUANTITIES:
            b = reference.get(quantity)
            if b is None:
                continue
            a = ours.get(quantity)
            e = error(a, b, quantity, diagonal)
            status = 'UNAVAILABLE' if e is None else ('PASS' if e <= BAND else 'FAIL')
            known = ref == 'B' and status == 'FAIL' and known_b(file, quantity, a, reference, diagonal)
            if known:
                status = 'KNOWN-REFERENCE'
            counts[ref, quantity, status] += 1
            classification = known or ''
            if status == 'UNAVAILABLE':
                classification = 'a face haecceity cannot resolve'
            elif status == 'FAIL':
                c = (other or {}).get(quantity)
                ac, bc = error(a, c, quantity, diagonal), error(b, c, quantity, diagonal)
                if ref == 'A' and ac is not None and ac <= BAND and bc is not None and bc > BAND:
                    classification = 'wrong reference (A disagrees with B and ours)'
                else:
                    classification = 'real haecceity error; investigate exact faces' if ref=='A' else 'B-only discrepancy; inspect geometry and reference'
            if e is not None:
                worst.append((e, file, label, ref, quantity, a, b, classification))
            print(f'{file} | {label} | {ref} | {quantity} | {a} | {b} | {e} | {status} {classification}')

    for line in lines:
        row = json.loads(line)
        print(f'measuring {row["file"]}', file=sys.stderr, flush=True)
        try:
            compare(row)
        except Exception as exc:
            row['occt_error'] = str(exc)
        rows.append(row)
        file = row['file']
        if 'error' in row:
            print(f'{file} | READ ERROR | {row["error"]}')
            for i, reference in enumerate(row.get('occt_parts', [])):
                emit(file,f'OCCT part {i} (ours unreadable)','B',{},reference['reference'],1.0)
            continue
        if 'occt_error' in row:
            print(f'{file} | OCCT ERROR | {row["occt_error"]}')
            for i, reference in enumerate(row.get('occt_parts', [])):
                print(f'{file} | OCCT part {i} | independent measurements | {reference}')
        for part in row['parts']:
            label = f'{part["part"]}:{part["name"]}'
            ours = part['ours']
            diagonal = ours['diagonal']
            if ours.get('unintegrated_faces') or ours.get('unresolved_faces'):
                print(f'{file} | {label} | integration diagnostics | {ours}')
            emit(file, label, 'B', ours, part.get('occt', {}), diagonal)
            for prop in part['gvp']:
                label_a = label+f'/GVP#{prop["property"]}({prop["scope"]})'
                own = prop['ours'] or {}
                reference = prop['reference']
                occt = prop.get('occt', {})
                if own.get('area_scope') or occt.get('area_scope'):
                    scope_status = 'PASS' if own.get('area_scope') == occt.get('area_scope') else 'FAIL'
                    counts['scope', 'area', scope_status] += 1
                    print(f'{file} | {label_a} | area scope | ours {own.get("area_scope")} | OCCT {occt.get("area_scope")} | {scope_status}')
                emit(file, label_a, 'A', own, reference, diagonal, occt)
                for quantity in QUANTITIES:
                    a, b = reference.get(quantity), occt.get(quantity)
                    if a is not None and b is not None:
                        e = error(a, b, quantity, diagonal)
                        status = 'PASS' if e <= BAND else 'FAIL'
                        counts['A-B', quantity, status] += 1
                        print(f'{file} | {label_a} | A-B | {quantity} | {a} | {b} | {e} | {status}')
            if not any(any(p['reference'].get(q) is not None for q in QUANTITIES) for p in part['gvp']):
                print(f'{file} | {label} | A unavailable (no material GVP)')
        for prop in row['unattached_gvp']:
            print(f'{file} | UNATTACHED GVP | {prop}')
    if process is not None and process.wait():
        raise SystemExit('Rust measurement process failed')
    print('\nCOUNTS (each GVP representation is counted, including duplicate part/aspect checks):')
    for key, value in sorted(counts.items()):
        print(*key, value)
    print('\nWORST NUMERICAL ERRORS (three per reference/quantity):')
    for reference in ('A', 'B'):
        for quantity in QUANTITIES:
            selected = [r for r in worst if r[3:5] == (reference, quantity)]
            for item in sorted(selected, key=lambda r: r[0], reverse=True)[:3]:
                print(item)
    if args.json:
        args.json.write_text(json.dumps(rows, indent=2, allow_nan=False)+'\n')


if __name__ == '__main__':
    main()
