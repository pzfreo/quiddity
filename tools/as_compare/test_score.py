"""Semantic scorer regressions; no output hashes or dataset redistribution."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent
FIXTURES = ROOT / 'fixtures'


class Scoring(unittest.TestCase):
    def score(self, as_path, qr_path=None):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / 'score.json'
            subprocess.run([sys.executable, '-I', str(ROOT / 'score.py'),
                            str(FIXTURES / 'labels'), str(qr_path or FIXTURES / 'qr.jsonl'),
                            str(as_path), str(out), '--quiddity-revision', 'synthetic'],
                           check=True, capture_output=True)
            return json.loads(out.read_text())

    def test_selectors_duplicates_and_source(self):
        result = self.score(FIXTURES / 'as.jsonl')['comparisons']
        holes = result['Holes']['qr']
        self.assertEqual(holes['claimed_faces'], 3)
        self.assertEqual(holes['false_faces'], 0)
        # Inclusive half coverage, with no instance matching or IoU penalty.
        self.assertEqual(holes['recognised'], 3)
        self.assertEqual(result['Through holes (class 1)']['qr']['claimed_faces'], 1)
        blind = result['Blind holes (class 12)']['qr']
        self.assertEqual((blind['claimed_faces'], blind['false_faces']), (2, 1))
        # Superseded hole and boss are skipped; source-marked recess stays.
        self.assertEqual(result['Any feature']['qr']['claimed_faces'], 4)
        # The Shafts row must not use the union of feature families.
        self.assertEqual(result['Shafts']['qr']['claimed_faces'], 0)
        self.assertEqual(result['Shafts']['as']['claimed_faces'], 1)
        self.assertIsNone(result['Shafts']['qr']['false_positive_pct'])

    def test_unavailable_tool_is_not_scored_as_empty(self):
        result = self.score('-')
        self.assertEqual(result['health'], {'qr': {'ok': 1}})
        self.assertTrue(all(set(row) == {'qr'} for row in result['comparisons'].values()))

    def test_refused_hull_does_not_claim_every_face(self):
        with tempfile.TemporaryDirectory() as tmp:
            row = json.loads((FIXTURES / 'as.jsonl').read_text())
            row['hull'] = None
            path = Path(tmp) / 'as.jsonl'
            path.write_text(json.dumps(row) + '\n')
            result = self.score(path)
        self.assertEqual(result['health']['as']['recogniser_refused_or_threw'], 1)
        comparisons = result['comparisons']
        self.assertEqual(comparisons['Overall: + off-hull faces']['as']['claimed_faces'],
                         comparisons['Overall: holes+blends+cavities']['as']['claimed_faces'])

    def test_misaligned_model_claims_nothing(self):
        with tempfile.TemporaryDirectory() as tmp:
            row = json.loads((FIXTURES / 'qr.jsonl').read_text())
            row['faces'] = 8
            path = Path(tmp) / 'qr.jsonl'
            path.write_text(json.dumps(row) + '\n')
            result = self.score('-', path)
        self.assertEqual(result['health']['qr'], {'face_count_mismatch': 1})
        self.assertEqual(result['comparisons']['Any feature']['qr']['claimed_faces'], 0)


if __name__ == '__main__':
    unittest.main()
