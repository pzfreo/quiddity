"""Synthetic STEP-name/source binding and shared metric regression, no dataset files."""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('mfc', Path(__file__).with_name('score_mfcadpp.py'))
mfc = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mfc)

# Face file order differs from the engine's caller-face indices.
STEP = b"""ISO-10303-21;
DATA;
#90=ADVANCED_FACE('24',(),#1,.T.);
#7=ADVANCED_FACE('1',(),#1,.T.);
#51 = ADVANCED_FACE( '4',(),#1,.T.);
#8=ADVANCED_FACE('4',(),#1,.T.);
ENDSEC;
END-ISO-10303-21;
"""
ROW = {'id': 'synthetic', 'status': 'ok', 'faces': 4, 'face_sources': [51, 7, 90, 8],
       'defining': {'holes': [[1], [2]], 'section_passages': [[0]],
                    'plates': [[3]], 'bosses': [[3]]},
       'records': {'holes': [{'bottom': 'through'}, {'bottom': 'flat', 'superseded_by': ['holes', 0]}],
                   'section_passages': [{}], 'plates': [{}],
                   'bosses': [{'superseded_by': ['plates', 0]}]}}


class ScoringMfcadpp(unittest.TestCase):
    def test_source_binding_and_shared_comparisons(self):
        self.assertEqual(mfc.labels(STEP, ROW), [4, 1, 24, 4])
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / 'synthetic.step').write_bytes(STEP)
            sweep = root / 'qr.jsonl'
            sweep.write_text(json.dumps(ROW) + '\n')
            report = mfc.score(root, sweep, root / 'out.json', 'synthetic')
        c = report['comparisons']
        self.assertEqual(c['Any feature']['qr']['true_faces'], 2)
        self.assertEqual(c['Any feature']['qr']['false_faces'], 0)
        self.assertEqual(c['Through holes (class 1)']['qr']['true_faces'], 1)
        self.assertIsNone(c['Any feature']['qr']['recognition_pct'])
        self.assertEqual(c['Stock (convex hull)']['qr']['recognition_pct'], 100)
        gap = report['gap_ranking'][0]
        self.assertEqual((gap['class'], gap['missed'], gap['body_only'], gap['body:plates']), (4, 1, 1, 1))

    def test_unreadable_import_keeps_label_denominators_without_a_face_binding(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / 'synthetic.step').write_bytes(STEP)
            sweep = root / 'qr.jsonl'
            sweep.write_text(json.dumps({'id': 'synthetic', 'status': 'unreadable'}) + '\n')
            report = mfc.score(root, sweep, root / 'out.json', 'synthetic')
        self.assertEqual(report['health']['qr'], {'unreadable': 1})
        self.assertEqual(report['comparisons']['Any feature']['qr']['labelled_faces'], 3)
        self.assertEqual(report['comparisons']['Any feature']['qr']['true_faces'], 0)
        self.assertEqual(report['provenance']['scorer_sha256'],
                         hashlib.sha256(Path(mfc.__file__).read_bytes()).hexdigest())
        self.assertEqual(report['provenance']['metric_scorer_sha256'],
                         hashlib.sha256(Path(mfc.comparison.__file__).read_bytes()).hexdigest())

    def test_unreadable_truth_is_reported_and_has_no_invented_denominator(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            sweep = root / 'qr.jsonl'
            sweep.write_text(json.dumps(ROW) + '\n')
            report = mfc.score(root, sweep, root / 'out.json', 'synthetic')
        self.assertEqual(report['models'], 1)
        self.assertEqual(report['health']['qr'], {'unreadable': 1})
        self.assertEqual(report['comparisons']['Any feature']['qr']['labelled_faces'], 0)
        self.assertEqual(report['provenance']['unavailable_label_models'], ['synthetic'])

    def test_malformed_unreadable_input_is_reported(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / 'synthetic.step').write_bytes(STEP + STEP)
            sweep = root / 'qr.jsonl'
            sweep.write_text(json.dumps({'id': 'synthetic', 'status': 'unreadable'}) + '\n')
            report = mfc.score(root, sweep, root / 'out.json', 'synthetic')
        self.assertEqual(report['health']['qr'], {'unreadable': 1})
        self.assertEqual(report['provenance']['unavailable_label_models'], ['synthetic'])

    def test_invalid_binding_refused(self):
        for sources in ([51, 7, 90], [51, 7, 90, 90], [51, 7, None, 8], [51, 7, 666, 8]):
            with self.assertRaises(ValueError):
                mfc.labels(STEP, {**ROW, 'face_sources': sources})
        with self.assertRaises(ValueError):
            mfc.labels(STEP.replace(b"'4'", b"'25'"), ROW)


if __name__ == '__main__':
    unittest.main()
