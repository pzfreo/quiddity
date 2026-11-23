"""Regression checks for PR selection and the independently listed tier inventory."""

import unittest

from ci_tiers import check_tiers, select, touched_tests


class CiTiersTest(unittest.TestCase):
    def test_selects_fast_unrecorded_and_touched_tests(self):
        fast = ("quiddity", "engine::frames::tests::fast")
        unrecorded = ("haecceity", "mass::tests::new_test")
        integration = ("quiddity::manifest", "a_long_test")
        internal = ("quiddity", "regression_tests::invariance::motion::slice_2")
        source = ("quiddity", "engine::body::tests::slow")
        alias = ("haecceity", "volume::witness_tests::slow")
        other = ("haecceity::manifest", "a_long_test")
        untouched = ("quiddity", "engine::body_extra::tests::slow")
        tests = {fast, unrecorded, integration, internal, source, alias, other, untouched}
        durations = {" ".join(test): 20 for test in tests - {unrecorded}}
        durations[" ".join(fast)] = 10
        durations[" ".join(untouched)] = 10.001
        touched = touched_tests(tests, [
            "tests/manifest.rs", "tests/internal/invariance.rs", "src/body.rs",
            "crates/haecceity/src/volume_witness_tests.rs",
        ])
        self.assertEqual(set(select(tests, durations, touched)), tests - {other, untouched})
        self.assertEqual(touched_tests(tests, ["tests/common/parallel.rs"]),
                         {integration, internal, other})
        self.assertEqual(touched_tests(tests, ["crates/haecceity/tests/common/mod.rs"]), {other})

    def test_coverage_rejects_a_gap_in_either_inventory(self):
        fast = ("quiddity", "fast")
        slow = ("quiddity", "slow")
        full = {fast, slow}
        durations = {"quiddity slow": 11}
        self.assertEqual(check_tiers(full, full, {fast}, durations, set()), {slow})
        self.assertEqual(check_tiers(full, full, full, durations, {slow}), set())
        for main, pr in [({fast}, {fast}), (full, set()), (full, full)]:
            with self.subTest(main=main, pr=pr), self.assertRaises(ValueError):
                check_tiers(full, main, pr, durations, set())
