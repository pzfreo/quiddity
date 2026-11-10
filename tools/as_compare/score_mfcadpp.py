"""Score face_claims against MFCAD++ STEP names, using score.py's comparisons.

python3 -I tools/as_compare/score_mfcadpp.py STEPS QR.jsonl OUT.json --quiddity-revision REV

Requires face_sources emitted by face_claims: face indices are NEVER STEP file order.
MFCAD++ supplies semantic labels only; instance recognition/FN are unavailable (except
stock's body-level comparison). Gap counts use the shared 'Any feature' selector.
"""
import argparse
from collections import Counter, defaultdict
import hashlib
import importlib.util
import json
from pathlib import Path
import re

spec = importlib.util.spec_from_file_location('comparison', Path(__file__).with_name('score.py'))
comparison = importlib.util.module_from_spec(spec)
spec.loader.exec_module(comparison)


def face_names(raw):
    # Only names attached to source face entities are eligible; whitespace/newlines are legal.
    entries = re.findall(rb"#(\d+)\s*=\s*(?:ADVANCED_FACE|FACE_SURFACE)\s*\(\s*'((?:[^']|'')*)'", raw)
    names = {}
    for entity, name in entries:
        entity = int(entity)
        if entity in names:
            raise ValueError(f'duplicate face entity #{entity}')
        names[entity] = name
    return names


def labels(raw, row):
    names = face_names(raw)
    sources = row.get('face_sources')
    if (not isinstance(sources, list) or len(sources) != row['faces']
            or any(type(s) is not int for s in sources) or len(set(sources)) != len(sources)):
        raise ValueError(f"{row['id']}: missing, duplicate or misaligned source-face binding")
    seg = []
    for entity in sources:
        name = names.get(entity, b'')
        if not name.isdigit() or not 0 <= int(name) <= comparison.STOCK:
            raise ValueError(f"{row['id']}: source #{entity} has no valid face class: {name!r}")
        seg.append(int(name))
    return seg


def score(steps, sweep, out, revision):
    rows = comparison.load(str(sweep))
    gaps = defaultdict(Counter)
    unavailable_labels = []

    def truth(model_id, digest):
        row = rows[model_id]
        try:
            raw = Path(steps, f'{model_id}.step').read_bytes()
        except OSError:
            unavailable_labels.append(model_id)
            digest.update(f'{model_id}:unreadable\n'.encode())
            return None, None
        digest.update(f'{model_id}:{hashlib.sha256(raw).hexdigest()}\n'.encode())
        if row.get('status') == 'ok':
            seg = labels(raw, row)
        else:
            # Failed imports have no caller-face binding and claim nothing. Only the
            # label multiset matters here; never assign file order to caller faces.
            names = face_names(raw)
            if not names or any(not n.isdigit() or not 0 <= int(n) <= comparison.STOCK
                                for n in names.values()):
                unavailable_labels.append(model_id)
                return None, None
            seg = [int(n) for n in names.values()]
        claim = comparison.qr_claim(row, None) if row.get('status') == 'ok' else set()
        bodies = {family: comparison.qr_claim(row, {family})
                  for family in comparison.BODY} if row.get('status') == 'ok' else {}
        for face, cls in enumerate(seg):
            if cls == comparison.STOCK:
                continue
            counts = gaps[cls]
            counts['faces'] += 1
            if face in claim:
                counts['claimed'] += 1
            else:
                counts['missed'] += 1
                evidence = sorted(family for family, faces in bodies.items() if face in faces)
                counts['body_only' if evidence else 'unclaimed'] += 1
                for family in evidence:
                    counts[f'body:{family}'] += 1
        return seg, None

    report = comparison.score(str(steps), str(sweep), '-', str(out), revision,
                              truth_loader=truth)
    report['provenance']['metric_scorer_sha256'] = report['provenance']['scorer_sha256']
    report['provenance']['scorer_sha256'] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    report['provenance']['scorer'] = 'tools/as_compare/score_mfcadpp.py using tools/as_compare/score.py'
    report['provenance']['unavailable_label_models'] = sorted(unavailable_labels)
    report['provenance']['dataset'] = 'MFCAD++; labels through quiddity face_sources entity IDs'
    report['provenance']['instances'] = 'Unavailable: STEP names contain classes, not instance IDs; stock is one body.'
    report['gap_ranking'] = [
        {'class': cls, 'name': comparison.NAMES[cls], **dict(counts),
         'claimed_pct': round(100 * counts['claimed'] / counts['faces'], 2)}
        for cls, counts in sorted(gaps.items(), key=lambda item: (-item[1]['missed'], item[0]))]
    Path(out).write_text(json.dumps(report, indent=2) + '\n')
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('steps')
    parser.add_argument('sweep')
    parser.add_argument('out')
    parser.add_argument('--quiddity-revision', required=True)
    args = parser.parse_args()
    report = score(args.steps, args.sweep, args.out, args.quiddity_revision)
    comparison.print_report(report)
    print('\nClass gaps (descending missed faces):')
    for row in report['gap_ranking']:
        print(json.dumps(row))


if __name__ == '__main__':
    main()
