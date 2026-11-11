"""The pre-release migration must never excuse a subsequent patch release."""
import unittest
from check_semver import release_override


class MigrationAllowance(unittest.TestCase):
    def test_only_the_approved_unversioned_transition_is_allowed(self):
        self.assertEqual(release_override("0.4.9", "0.4.9"), ["--release-type", "major"])
        for current, published in [
            ("0.4.10", "0.4.9"), ("0.5.0", "0.4.9"),
            ("0.5.1", "0.5.0"), ("0.5.0", "0.5.0"),
            ("0.4.9", "0.5.0"), ("0.6.1", "0.6.0"),
        ]:
            with self.subTest(current=current, published=published):
                self.assertEqual(release_override(current, published), [])
