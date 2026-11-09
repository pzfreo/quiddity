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


def error(a, b, quantity, diagonal):
    if a is None or b is None:
        return None
    if quantity == 'centroid':
        return math.dist(a, b)/diagonal if diagonal > 0 else math.inf
    return abs(a-b)/abs(b) if b else (0.0 if a == 0 else math.inf)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--rust', type=Path, required=True)
    parser.add_argument('--json', type=Path)
    parser.add_argument('paths', nargs='+')
    args = parser.parse_args()
    process = subprocess.Popen([str(args.rust.resolve()), *args.paths], stdout=subprocess.PIPE, text=True)
    rows, counts, worst = [], collections.Counter(), []
    print('Units mm^3, mm^2, mm. Relative centroid error = distance / part box diagonal.')
    print('Acceptance <=0.001 for all three normalized errors. Missing values are UNAVAILABLE, never PASS.')
    print('file | part/property | reference | quantity | ours | reference value | error | status/classification')

    def emit(file, label, ref, ours, reference, diagonal, other=None):
        for quantity in QUANTITIES:
            b = reference.get(quantity)
            if b is None:
                continue
            a = ours.get(quantity)
            e = error(a, b, quantity, diagonal)
            status = 'UNAVAILABLE' if e is None else ('PASS' if e <= BAND else 'FAIL')
            counts[ref, quantity, status] += 1
            classification = ''
            if status == 'UNAVAILABLE':
                classification = 'a face haecceity cannot resolve'
            elif status == 'FAIL':
                c = (other or {}).get(quantity)
                ac, bc = error(a, c, quantity, diagonal), error(b, c, quantity, diagonal)
                if ref == 'A' and ac is not None and ac <= BAND and bc is not None and bc > BAND:
                    classification = 'wrong reference (A disagrees with B and ours)'
                else:
                    classification = 'real haecceity error; investigate exact faces'
            if e is not None:
                worst.append((e, file, label, ref, quantity, a, b, classification))
            print(f'{file} | {label} | {ref} | {quantity} | {a} | {b} | {e} | {status} {classification}')

    for line in process.stdout:
        row = json.loads(line)
        print(f'measuring {row["file"]}', file=sys.stderr, flush=True)
        if 'error' not in row:
            try:
                compare(row)
            except Exception as exc:
                row['occt_error'] = str(exc)
        rows.append(row)
        file = row['file']
        if 'error' in row:
            print(f'{file} | READ ERROR | {row["error"]}')
            continue
        if 'occt_error' in row:
            print(f'{file} | OCCT ERROR | {row["occt_error"]}')
        for part in row['parts']:
            label = f'{part["part"]}:{part["name"]}'
            ours = part['ours']
            diagonal = ours['diagonal']
            emit(file, label, 'B', ours, part.get('occt', {}), diagonal)
            for prop in part['gvp']:
                label_a = label+f'/GVP#{prop["property"]}({prop["scope"]})'
                own = prop['ours'] or {}
                reference = prop['reference']
                occt = prop.get('occt', {})
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
    if process.wait():
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
