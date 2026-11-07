"""Record ratchet improvements from Rust evaluation; never lower a recorded score.

QUIDDITY_MFINSTSEG=/path/to/MFInstSeg python3 tools/update_mfinstseg_baseline.py
The fixed sample, input hashes, mapping and class supports must stay unchanged.
For an explicitly reviewed additive family mapping, pass --previous-mapping OLD.json
and --mapping-reason TEXT. The old mapping must match the baseline hash; only family
additions and their rationale may differ. Every original score/accounting guard still
applies, including precision. Dataset changes are never accepted by this tool.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
from fractions import Fraction


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def ratio(counts, denominator):
    return Fraction(counts['true_positive'], counts[denominator]) if counts[denominator] else Fraction(0)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--previous-mapping', type=Path)
    parser.add_argument('--mapping-reason')
    args = parser.parse_args()
    assert bool(args.previous_mapping) == bool(args.mapping_reason), 'mapping review needs old mapping and reason'
    repo = Path(__file__).resolve().parents[1]
    path = repo / 'tests/fixtures/mfinstseg_baseline.json'
    baseline = json.loads(path.read_text())
    root = Path(os.environ['QUIDDITY_MFINSTSEG'])
    mapping_path = repo / 'tests/fixtures/mfinstseg_mapping.json'
    mapping_hash = digest(mapping_path)
    if args.previous_mapping:
        assert digest(args.previous_mapping) == baseline['mapping_sha256'], 'previous mapping does not match baseline'
        old_mapping = json.loads(args.previous_mapping.read_text())
        new_mapping = json.loads(mapping_path.read_text())
        old_classes, new_classes = old_mapping.pop('classes'), new_mapping.pop('classes')
        assert old_mapping == new_mapping, 'mapping metadata changed'
        assert len(old_classes) == len(new_classes), 'classes changed'
        for old, new in zip(old_classes, new_classes):
            old_families, new_families = old.pop('families'), new.pop('families')
            assert len(set(new_families)) == len(new_families), 'duplicate family'
            assert set(old_families) <= set(new_families), 'family removed'
            old.pop('rationale', None)
            new.pop('rationale', None)
            assert old == new, 'only additive families and rationale may change'
    else:
        assert mapping_hash == baseline['mapping_sha256'], 'mapping changed'
    for model in baseline['models']:
        assert digest(root / 'steps' / (model['id'] + '.step')) == model['step_sha256'], model['id']
        assert digest(root / 'labels' / (model['id'] + '.json')) == model['labels_sha256'], model['id']
    with tempfile.TemporaryDirectory() as tmp:
        ids, out = Path(tmp) / 'ids.txt', Path(tmp) / 'scores.json'
        ids.write_text('\n'.join(m['id'] for m in baseline['models']) + '\n')
        env = dict(os.environ, CARGO_BUILD_JOBS=os.environ.get('CARGO_BUILD_JOBS', '6'), RUST_TEST_THREADS=os.environ.get('RUST_TEST_THREADS', '6'))
        subprocess.run(['cargo', 'run', '--release', '--example', 'mfinstseg_score', '--', str(root), str(ids), str(out), os.environ.get('MFI_WORKERS', '6')], cwd=repo, env=env, check=True)
        report = json.loads(out.read_text())
    assert not report['label_errors'] and not report['label_warnings'] and not report['recognition_failures'] and not report['section_recess_refusals']
    scores = [c['counts'] for c in report['classes']]
    assert len(scores) == len(baseline['scores'])
    for i, (got, old) in enumerate(zip(scores, baseline['scores'])):
        for metric in ['faces', 'instances']:
            assert got[metric]['labelled'] == old[metric]['labelled'], (i, metric, 'support changed')
            for denominator in ['predicted', 'labelled']:
                assert ratio(got[metric], denominator) >= ratio(old[metric], denominator), (i, metric, 'regressed')
    assert report['models'] == 200 and len(report['model_accounting']) == 200
    measured = {m['id']: m['accounting'] for m in report['model_accounting']}
    assert len(measured) == 200 and set(measured) == {m['id'] for m in baseline['models']}
    for model in baseline['models']:
        got = measured[model['id']]
        assert got['feature_as_body'] == 0, (model['id'], 'feature hidden as body')
        if 'accounting' in model:
            assert got['stock_unexplained'] <= model['accounting']['stock_unexplained'], (model['id'], 'stock unexplained regressed')
        model['accounting'] = got
    assert report['accounting']['feature_as_body'] == 0
    # Print every class, including unchanged ones, before recording this reviewed run.
    for row, old, got in zip(report['classes'], baseline['scores'], scores):
        print(f"{row['id']:2} {row['name']}: {old} -> {got}")
    if args.previous_mapping:
        changed = [f"{row['name']}: {old} -> {got}"
                   for row, old, got in zip(report['classes'], baseline['scores'], scores) if old != got]
        baseline['reason'] += '\n\n' + args.mapping_reason + ' Measured class changes: ' + '; '.join(changed) + '. Every other class unchanged; all precision, recall, support, input and accounting guards passed.'
        baseline['mapping_sha256'] = mapping_hash
    baseline['schema'] = 2
    baseline['accounting'] = report['accounting']
    baseline['scores'] = scores
    path.write_text(json.dumps(baseline, indent=2) + '\n')
    print('Recorded non-regressing Rust scores; review and commit the baseline diff.')


if __name__ == '__main__':
    main()
