# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

import hashlib
import json
import os
import re
import subprocess
import sys
import tarfile
import zipfile
from pathlib import Path
from shutil import copytree, ignore_patterns

from quiddity import __version__

ROOT = Path(__file__).parents[1]


def test_quiddity_identity_and_second_alpha_version_history() -> None:
    from importlib.metadata import metadata

    from quiddity import capability_manifest
    from quiddity.evidence import evidence_api_manifest
    from quiddity.inspection import inspection_api_manifest

    assert metadata("quiddity")["Name"] == "quiddity"
    capabilities = capability_manifest()
    inspection = inspection_api_manifest()
    evidence = evidence_api_manifest()
    for manifest, format_name in (
        (capabilities, "quiddity-capabilities"),
        (inspection, "quiddity-inspection-api"),
        (evidence, "quiddity-evidence-api"),
    ):
        assert manifest["format"] == format_name
        assert manifest["package"] == {"name": "quiddity", "version": __version__}
    # The second alpha inherits every existing family/symbol at 0.2.0. New additions may
    # arrive later, but must not claim a pre-Quiddity introduction date.
    versions = [family["introduced_in"] for family in capabilities["families"]]
    versions += [symbol["introduced_in"] for symbol in inspection["api"]["symbols"]]
    assert all(tuple(map(int, version.split("."))) >= (0, 2, 0) for version in versions)


def test_every_copy_of_the_version_agrees() -> None:
    """The build version, fallback and all shipped manifests are one value.

    Derived rather than pinned to a literal, because a literal is what let the fallback go
    stale: it read 0.2.2 through both the 0.2.3 and 0.2.4 releases. Nothing exercises it
    outside a bare source tree, so only a comparison like this one can catch it, and
    ``scripts/update-recogniser-version`` is what keeps them together.
    """
    pyproject = (ROOT / "pyproject.toml").read_text(encoding="utf-8")
    declared = re.search(r'^version = "([^"]+)"$', pyproject, re.MULTILINE)
    assert declared, "pyproject.toml has no single project version"
    version = declared.group(1)

    assert __version__ == version
    init = (ROOT / "src" / "quiddity" / "__init__.py").read_text(encoding="utf-8")
    assert f'__version__ = "{version}"' in init, "the PackageNotFoundError fallback is stale"
    manifest = json.loads(
        (ROOT / "src" / "quiddity" / "capabilities.json").read_text(encoding="utf-8")
    )
    assert manifest["package"]["version"] == version
    inspection = json.loads(
        (ROOT / "src" / "quiddity" / "inspection_api.json").read_text(encoding="utf-8")
    )
    assert inspection["package"]["version"] == version
    evidence = json.loads(
        (ROOT / "src" / "quiddity" / "evidence_api.json").read_text(
            encoding="utf-8"
        )
    )
    assert evidence["package"]["version"] == version


def test_stable_release_notes_record_the_proven_downstream_cutover() -> None:
    notes = (ROOT / "RELEASE_NOTES.md").read_text(encoding="utf-8")

    # Past releases keep their notes. That the version *being released* has a section is a
    # different question, and asking it here was wrong: under the .devN scheme main always
    # carries an unreleased version, so this would have demanded speculative notes and failed
    # every post-release bump. The publish workflow asks it against the tag instead.
    assert notes.startswith("# Release notes\n")
    assert notes.count("\n## 0.2.5\n") == 1
    assert notes.count("\n## 0.2.4\n") == 1
    assert notes.count("\n## 0.2.3\n") == 1
    assert notes.count("\n## 0.2.2\n") == 1
    assert notes.count("\n## 0.2.1\n") == 1
    assert notes.count("\n## 0.2.0\n") == 1
    assert "capability manifest" in notes
    assert "Draftwright PR #1168" in notes
    assert "d659e7a6" in notes
    assert "no new recognition behaviour" in notes


def test_wheel_contains_runtime_modules_typing_marker_and_licence_files(tmp_path) -> None:
    subprocess.run(
        ["uv", "build", "--wheel", "--out-dir", str(tmp_path)],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
        timeout=120,
    )
    wheels = list(tmp_path.glob("*.whl"))
    assert len(wheels) == 1

    with zipfile.ZipFile(wheels[0]) as archive:
        names = set(archive.namelist())

    package_files = {
        path.relative_to(ROOT / "src").as_posix()
        for path in (ROOT / "src" / "quiddity").glob("*")
        if path.is_file() and (path.suffix in {".json", ".py"} or path.name == "py.typed")
    }
    assert package_files <= names
    assert not any(name.startswith("b123d_recognisers/") for name in names)
    assert wheels[0].name.startswith("quiddity-")
    entry_points = next(name for name in names if name.endswith(".dist-info/entry_points.txt"))
    with zipfile.ZipFile(wheels[0]) as archive:
        scripts = archive.read(entry_points).decode("utf-8")
        metadata_path = next(name for name in names if name.endswith(".dist-info/METADATA"))
        metadata = archive.read(metadata_path).decode("utf-8")
    assert "\nName: quiddity\n" in metadata
    assert "b123d-recognisers-capabilities =" not in scripts
    assert "quiddity-capabilities = quiddity.capabilities:main" in scripts
    assert any(name.endswith(".dist-info/licenses/LICENSE") for name in names)
    assert any(name.endswith(".dist-info/licenses/NOTICE") for name in names)
    assert any(name.endswith(".dist-info/licenses/THIRD_PARTY_NOTICES.md") for name in names)


def test_sdist_excludes_untracked_workspace_files(tmp_path) -> None:
    checkout = tmp_path / "checkout"
    copytree(
        ROOT,
        checkout,
        ignore=ignore_patterns(".git", ".venv", ".pytest_cache", "__pycache__", "dist"),
    )
    sentinel = checkout / "PRIVATE_BUILD_INPUT.txt"
    sentinel.write_text("must not ship\n", encoding="utf-8")
    output = tmp_path / "dist"

    subprocess.run(
        ["uv", "build", "--sdist", "--out-dir", str(output)],
        cwd=checkout,
        check=True,
        capture_output=True,
        text=True,
        timeout=120,
    )
    sdist = next(output.glob("*.tar.gz"))
    with tarfile.open(sdist, "r:gz") as archive:
        names = set(archive.getnames())

    assert not any(name.endswith("/PRIVATE_BUILD_INPUT.txt") for name in names)
    assert any(name.endswith("/src/quiddity/__init__.py") for name in names)
    assert any(name.endswith("/src/quiddity/capabilities.json") for name in names)
    assert any(name.endswith("/src/quiddity/inspection_api.json") for name in names)
    assert any(name.endswith("/src/quiddity/evidence_api.json") for name in names)
    assert any(name.endswith("/RELEASE_NOTES.md") for name in names)
    # The vendored STEP corpora are excluded: 9 MB of third-party geometry the tests read and
    # no consumer of the sdist needs. Deleting that exclusion would otherwise pass silently
    # while the sdist grew 5.4x and the corpus modules stopped skipping.
    assert not [name for name in names if "/tests/corpus/" in name]
    # THIRD_PARTY_NOTICES.md claims the vendored corpora are byte-identical to upstream, which
    # only holds if git is told not to rewrite their line endings -- Windows runners default
    # to core.autocrlf=true. Nothing else pins these two lines.
    # Asked of git, not of the file: `*.stp -text` being present does not mean it is in
    # effect. A later `* text=auto` line wins, and so does a nested tests/corpus/.gitattributes
    # -- either restores CRLF rewriting on Windows while the string assertion still passes.
    probes = ["tests/corpus/nist/nist_ctc_01_asme1_rd.stp", "tests/corpus/mfcadpp/1000.step"]
    attrs = subprocess.run(
        ["git", "check-attr", "text", "--", *probes],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    for line in attrs.splitlines():
        assert line.endswith(": text: unset"), f"line endings are not pinned: {line}"
    manifest = json.loads(
        (checkout / "src" / "quiddity" / "capabilities.json").read_text(encoding="utf-8")
    )
    references = {
        reference.split("#", 1)[0]
        for family in manifest["families"]
        for key in ("documentation", "golden_evidence", "test_evidence")
        for reference in family[key]
    }
    for reference in references:
        assert any(name.endswith(f"/{reference}") for name in names), reference


def test_installed_wheel_imports_without_the_repository_on_sys_path(tmp_path) -> None:
    wheel_dir = tmp_path / "wheel"
    target = tmp_path / "installed"
    subprocess.run(
        ["uv", "build", "--wheel", "--out-dir", str(wheel_dir)],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
        timeout=120,
    )
    wheel = next(wheel_dir.glob("*.whl"))
    subprocess.run(
        ["uv", "pip", "install", "--no-deps", "--target", str(target), str(wheel)],
        check=True,
        capture_output=True,
        text=True,
        timeout=120,
    )
    # Text mode normalises a checkout's platform newline convention. The public exporter is
    # deliberately canonical JSON with LF newlines on every platform.
    source_manifest = (ROOT / "src" / "quiddity" / "capabilities.json").read_text(
        encoding="utf-8"
    )
    manifest_digest = hashlib.sha256(source_manifest.encode()).hexdigest()
    inspection_manifest = (ROOT / "src" / "quiddity" / "inspection_api.json").read_text(
        encoding="utf-8"
    )
    inspection_digest = hashlib.sha256(inspection_manifest.encode()).hexdigest()
    evidence_manifest = (ROOT / "src" / "quiddity" / "evidence_api.json").read_text(
        encoding="utf-8"
    )
    evidence_digest = hashlib.sha256(evidence_manifest.encode()).hexdigest()
    completed = subprocess.run(
        [
            sys.executable,
            "-I",
            "-c",
            (
                "import sys; "
                f"sys.path.insert(0, {str(target)!r}); "
                "import quiddity as r; "
                "from quiddity.cli import main as cli_main; "
                "from quiddity.document import build_recognition_document; "
                "import quiddity.experimental_geometry as e; "
                "import quiddity.evidence as v; "
                "import quiddity.inspection as i; "
                "from build123d import Box, Compound, Pos, RegularPolygon, Rot, extrude; "
                "import hashlib; "
                "assert r.__version__; assert r.recognise_holes; "
                "assert r.PreparedFramedPart; assert r.prepare_framed_part; "
                "assert r.build_raw_recognition_result; "
                "assert r.build_raw_recognition_report; "
                "assert r.build_framed_recognition_report; "
                "raw = r.build_raw_recognition_result(Box(2, 3, 4)); "
                "assert isinstance(raw, r.RecognitionResult); "
                "framed = r.build_framed_recognition_report(Box(2, 3, 4)); "
                "assert isinstance(framed, r.FramedRecognitionReport); "
                "refused = r.build_framed_recognition_report(Compound(children=[])); "
                "assert refused == r.RefusedPartFrame(r.FrameRefusalReason.NO_MATERIAL); "
                "actual = hashlib.sha256(r.capability_manifest_json().encode()).hexdigest(); "
                f"assert actual == {manifest_digest!r}; "
                "inspection = hashlib.sha256("
                "i.inspection_api_manifest_json().encode()).hexdigest(); "
                f"assert inspection == {inspection_digest!r}; "
                "evidence = hashlib.sha256("
                "v.evidence_api_manifest_json().encode()).hexdigest(); "
                f"assert evidence == {evidence_digest!r}; "
                "evidence_view = v.build_recognition_evidence(Box(2, 3, 4)); "
                "assert evidence_view.result == raw; "
                "assert isinstance(evidence_view.report, r.RecognitionReport); "
                "assert evidence_view.report.result is evidence_view.result; "
                "assert len(evidence_view.faces) == 6; "
                "assert evidence_view.association.face_count.total == 6; "
                "assert evidence_view.association.face_count.associated == 0; "
                "assert evidence_view.association.face_count.ratio == 0.0; "
                "assert evidence_view.association.unassociated_faces == evidence_view.faces; "
                "framed_evidence = r.build_framed_recognition_evidence(Box(2, 3, 4)); "
                "assert isinstance(framed_evidence, r.FramedRecognitionEvidence); "
                "assert framed_evidence.report.result is framed_evidence.result; "
                "assert r.RefusedFramedEvidence is v.RefusedFramedEvidence; "
                "reason = r.FramedEvidenceRefusalReason.CALLER_FACE_MAPPING_UNAVAILABLE; "
                "assert r.RefusedFramedEvidence(reason).result is None; "
                "completed = r.FramedRecognitionResult(framed_evidence.frame, "
                "framed_evidence.part, framed_evidence.result); "
                "assert r.RefusedFramedEvidence(reason, completed).result is completed; "
                "assert framed_evidence.caller_part is not framed_evidence.part; "
                "assert all(framed_evidence.face(ref).wrapped.IsPartner("
                "framed_evidence.caller_face(ref).wrapped) for ref in framed_evidence.faces); "
                "symbols = {s['name']: s['contract'] for s in "
                "i.inspection_api_manifest()['api']['symbols']}; "
                "assert symbols['BevelReject']['attributes'] == "
                "[{'name': 'reason', 'type': 'str', 'values': "
                "['nonplanar', 'degenerate', 'aligned', 'compound']}]; "
                "assert [m['name'] for m in "
                "symbols['read_double_d_tool']['returns']['members']] == "
                "['axis', 'major_diameter', 'across_flats', 'origin', 'depth', "
                "'profile_direction']; "
                "assert [m['unit'] for m in "
                "symbols['read_double_d_tool']['returns']['members']] == "
                "[None, 'model-length', 'model-length', 'model-length', "
                "'model-length', 'unitless']; "
                "assert e.inspect_face is i.inspect_face; "
                "assert r.classify_bevel is i.classify_bevel; "
                "boss = Box(10, 100, 80) + Pos(5, 0, 0) * Rot(0, 90, 0) * "
                "extrude(RegularPolygon(20, 6), 30); "
                "found = r.recognise_polygonal_bosses(boss); "
                "assert len(found) == 1 and found[0].axis == 'x' and found[0].height == 30; "
                "pad = Rot(0, -90, 0) * (Box(80, 60, 10) + Pos(0, 0, 7) * "
                "Box(30, 20, 4)); "
                "raised = r.recognise_rectangular_pads(pad); "
                "assert len(raised) == 1 and raised[0].axis == 'x' and "
                "raised[0].direction == -1; "
                "bracket = Pos(0, 0, 5) * Box(80, 60, 10) + "
                "Pos(0, 0, 35) * Box(80, 10, 50); "
                "assembly = Compound(children=[Pos(-70, 0, 0) * bracket, "
                "Pos(70, 0, 0) * bracket]); "
                "plates = r.build_recognition_result(assembly, rotational=False).plates; "
                "assert len(plates) == 4 and sorted(p.u for p in plates) == "
                "[-70.0, -70.0, 70.0, 70.0]"
            ),
        ],
        cwd=tmp_path,
        check=True,
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert completed.stderr == ""

    consumer = ROOT / "tests" / "typing" / "public_consumer.py"
    environment = os.environ.copy()
    environment["MYPYPATH"] = str(target)
    checked = subprocess.run(
        [
            sys.executable,
            "-m",
            "mypy",
            "--config-file",
            str(ROOT / "tests" / "typing" / "mypy.ini"),
            "--no-incremental",
            str(consumer),
        ],
        cwd=tmp_path,
        env=environment,
        check=True,
        capture_output=True,
        text=True,
        timeout=120,
    )
    assert "Success: no issues found" in checked.stdout
