"""A documented pending minor is the only way to accept detected API breaks."""
from pathlib import Path
import subprocess
import unittest
from tempfile import TemporaryDirectory
from unittest.mock import patch
from check_semver import check_package, release_override


def changelog(pending="0.5.0", body="- Open result types."):
    return f"## Unreleased\n\nPending version: {pending}\n\n### Breaking (Rust API)\n\n{body}\n\n### Fixed\n- Fix.\n\n## 0.4.9\n### Breaking (Rust API)\n- Historical.\n"


class BreakingOverride(unittest.TestCase):
    def setUp(self):
        temporary = TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.manifest = Path(temporary.name) / "Cargo.toml"
        self.manifest.write_text('[package]\nname = "fixture"\nversion = "0.4.9"\n')

    def test_compatible_patch_passes_without_override(self):
        success = subprocess.CompletedProcess([], 0, "", "")
        with patch("check_semver.subprocess.run", return_value=success) as run:
            check_package(self.manifest, "0.4.9", "## Unreleased\n")
            self.assertEqual(run.call_args.args[0][-2:], ["--release-type", "patch"])

    def test_empty_or_historical_sections_never_override(self):
        for text in ["", changelog(body=""), changelog(body="<!-- TODO -->"), "## Unreleased\n### Fixed\n- Fix\n## 0.4.9\n### Breaking (Rust API)\n- Old break"]:
            self.assertIsNone(release_override("0.4.9", "0.4.9", text))

    def test_pending_minor_before_and_after_050(self):
        for current, baseline, pending in [
            ("0.4.9", "0.4.9", "0.5.0"), ("0.5.0", "0.4.9", "0.5.0"),
            ("0.5.0", "0.5.0", "0.6.0"), ("0.5.1", "0.5.0", "0.6.0"),
        ]:
            self.assertEqual(release_override(current, baseline, changelog(pending)), pending)

    def test_release_section_of_an_unpublished_version_overrides(self):
        released = "## 0.5.0 (2026-11-12)\n\n### Breaking (Rust API)\n\n- Open result types.\n\n## 0.4.9\n"
        self.assertEqual(release_override("0.5.0", "0.4.9", released), "0.5.0")
        # Once published, or for a patch, or with an empty section, the release grants nothing.
        self.assertIsNone(release_override("0.5.0", "0.5.0", released))
        self.assertIsNone(release_override("0.4.9", "0.4.9", released))
        self.assertIsNone(release_override("0.5.0", "0.4.9", released.replace("- Open result types.", "")))
        with self.assertRaises(ValueError):
            release_override("0.4.10", "0.4.9", released.replace("0.5.0", "0.4.10"))

    def test_pending_patch_or_missing_version_is_rejected(self):
        for current, baseline, text in [
            ("0.4.9", "0.4.9", changelog("0.4.10")),
            ("0.5.0", "0.5.0", changelog("0.5.1")),
            ("0.7.0", "0.5.0", changelog("0.6.0")),
            ("1.0.0", "1.0.0", changelog("1.1.0")),
            ("0.4.9", "0.4.9", changelog().replace("Pending version: 0.5.0", "")),
        ]:
            with self.assertRaises(ValueError):
                release_override(current, baseline, text)

    def test_negative_control_and_documented_override_run_the_same_lints(self):
        failure = subprocess.CompletedProcess([], 100, "", "Summary semver requires new major version: 2 major and 0 minor checks failed\n")
        with patch("check_semver.subprocess.run", return_value=failure) as run:
            with self.assertRaises(subprocess.CalledProcessError):
                check_package(self.manifest, "0.4.9", "## Unreleased\n")
            negative = run.call_args.args[0]
            check_package(self.manifest, "0.4.9", changelog())
            self.assertEqual(negative, run.call_args.args[0])
            self.assertEqual(negative[-2:], ["--release-type", "patch"])

    def test_override_never_hides_tool_errors(self):
        for code in [1, 100, 101]:
            with patch("check_semver.subprocess.run", return_value=subprocess.CompletedProcess([], code, "", "build failed")):
                with self.assertRaises(subprocess.CalledProcessError):
                    check_package(self.manifest, "0.4.9", changelog())
