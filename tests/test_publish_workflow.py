# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

import os
import re
import subprocess
import sys
from pathlib import Path

import pytest
import yaml

ROOT = Path(__file__).parents[1]
README = ROOT / "README.md"
WORKFLOW = ROOT / ".github" / "workflows" / "publish.yml"
CI_WORKFLOW = ROOT / ".github" / "workflows" / "ci.yml"
FULL_MATRIX_WORKFLOW = ROOT / ".github" / "workflows" / "full-matrix.yml"


def _parsed(path: Path) -> dict:
    """A workflow as structure, not as text.

    Every workflow assertion here used to be a substring search, and five review rounds each
    found one that could be satisfied by something other than the thing it meant to check --
    a `uv build` in a different job, a `RELEASE_TAG#v` two steps away, and at the low point an
    assertion that a command appears in `build-release` satisfied by a *comment about* that
    command, so deleting the step which ran it stayed green.
    Substring matching cannot express "this job needs that job" or "this step runs that
    command", which is what the assertions were always about.

    PyYAML resolves the `on:` key to the boolean True (YAML 1.1), so callers wanting triggers
    should use `_triggers` rather than indexing `"on"`.
    """

    return yaml.safe_load(path.read_text(encoding="utf-8"))


def _steps(job: dict) -> list[str]:
    """Every `run:` body in a job, so a command can be located in the step that runs it."""

    return [step["run"] for step in job["steps"] if "run" in step]


def _triggers(workflow: dict) -> set[str]:
    """The event names a workflow runs on, however `on:` is spelled."""

    on = workflow.get("on", workflow.get(True))
    if isinstance(on, str):
        return {on}
    return set(on)


def _job(workflow: str, name: str) -> str:
    """The body of one job, so an assertion cannot be satisfied by a different job.

    Several checks here were whole-file substring searches, and the snapshot job happens to
    contain `uv build` and the TestPyPI `repository-url` too. So deleting `uv build` from
    `build-release` passed, and redirecting the release's first leg at production PyPI --
    bypassing the `pypi` environment's approval -- passed as well.
    """

    start = workflow.index(f"\n  {name}:\n")
    rest = workflow[start + 1 :]
    following = re.search(r"^  [a-z][\w-]*:$", rest[len(f"  {name}:") :], re.M)
    return rest if following is None else rest[: len(f"  {name}:") + following.start()]


def test_publish_workflow_uses_oidc_environments_and_one_promoted_artifact() -> None:
    workflow = WORKFLOW.read_text(encoding="utf-8")

    assert "push:" in workflow and "release:" in workflow
    assert "\npermissions: {}\n" in workflow, "publishing must default the token to no permissions"
    assert "environment:\n      name: testpypi" in workflow
    assert "environment:\n      name: pypi" in workflow
    assert "repository-url: https://test.pypi.org/legacy/" in workflow
    assert "password:" not in workflow and "API_TOKEN" not in workflow
    assert "enable-cache: false" in workflow, "release workflows must not consume mutable caches"

    # The artifact is built here, from the tagged commit, and never uploaded from a
    # maintainer's machine. The previous shape asserted the opposite -- `"uv build" not in
    # workflow` -- because it promoted a hand-built asset attached to the GitHub release.
    # That could only check the asset's version against the tag, which a wheel built from a
    # dirty tree also satisfies.
    assert "gh release download" not in workflow, "nothing hand-attached may be promoted"

    # The release builds once and hands that artifact to the separately protected PyPI job:
    # one upload, one download, both in the release path. The snapshot leg builds and publishes
    # on a single runner and needs neither. Pinned by digest, not by tag -- this is the workflow
    # holding `id-token: write` against PyPI, and an earlier version of this test replaced the
    # digests with bare counts, which would have let `actions/upload-artifact@v6` through.
    assert workflow.count("actions/upload-artifact@") == 1
    assert workflow.count("actions/download-artifact@") == 1
    assert workflow.count("actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a") == 1
    assert workflow.count("actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c") == 1

    # The generated post-release branch gets its CI status explicitly: a GITHUB_TOKEN
    # push raises no event.
    assert 'gh workflow run ci.yml --ref "$branch"' in workflow
    assert "for workflow in" not in workflow
    # The bump derives its next version from `main`, as the ported workflow does, so there is
    # no RELEASE_TAG-in-the-wrong-scope bug to have. An earlier attempt derived it from the tag
    # and read that variable in a step where it was not defined, which would have failed every
    # release after PyPI had accepted the artifact.
    assert "actions: write" in _job(workflow, "bump-version")
    target = (ROOT / ".github" / "workflows" / "ci.yml").read_text(encoding="utf-8")
    assert "workflow_dispatch:" in target
    # Two publish steps: the main-push snapshot to TestPyPI, and the release to PyPI.
    assert workflow.count("pypa/gh-action-pypi-publish@") == 2
    assert (
        workflow.count("pypa/gh-action-pypi-publish@dc37677b2e1c63e2034f94d8a5b11f265b73ba33") == 2
    ), "the action holding id-token against PyPI must be pinned by digest"
    assert workflow.count("id-token: write") == 2


def test_ci_workflow_pins_node24_actions() -> None:
    workflow = CI_WORKFLOW.read_text(encoding="utf-8")

    assert workflow.count("actions/checkout@d23441a48e516b6c34aea4fa41551a30e30af803") == 4
    assert workflow.count("astral-sh/setup-uv@fac544c07dec837d0ccb6301d7b5580bf5edae39") == 4
    assert workflow.count("actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a") == 1
    assert workflow.count("codecov/codecov-action@fb8b3582c8e4def4969c97caa2f19720cb33a72f") == 1
    assert "actions/checkout@v" not in workflow
    assert "astral-sh/setup-uv@v" not in workflow
    assert "actions/upload-artifact@v" not in workflow
    assert "codecov/codecov-action@v" not in workflow
    assert "id-token: write" in workflow
    assert "use_oidc: true" in workflow
    assert "fail_ci_if_error: true" in workflow


def test_ci_has_one_coverage_authority_and_a_separate_exhaustive_matrix() -> None:
    ci = _parsed(CI_WORKFLOW)
    full = _parsed(FULL_MATRIX_WORKFLOW)
    triggers = ci.get("on", ci.get(True))

    assert list(ci["jobs"]) == ["lint", "fast-test", "test", "compatibility"]
    assert triggers["pull_request"] == {
        "types": ["opened", "synchronize", "reopened", "ready_for_review"]
    }
    assert ci["jobs"]["fast-test"]["if"] == "github.event_name == 'pull_request'"
    assert _steps(ci["jobs"]["fast-test"])[-1] == 'uv run pytest -n 2 -m "not slow"'
    assert ci["jobs"]["test"]["runs-on"] == "ubuntu-latest"
    assert ci["jobs"]["test"]["if"] == (
        "github.event_name != 'pull_request' || github.event.pull_request.draft == false"
    )
    assert "--cov-fail-under=91" in " ".join(_steps(ci["jobs"]["test"]))
    assert "--cov" not in " ".join(_steps(ci["jobs"]["compatibility"]))
    assert _steps(ci["jobs"]["compatibility"])[-1] == 'uv run pytest -n 2 -m "not slow"'
    assert ci["jobs"]["compatibility"]["strategy"]["matrix"]["include"] == [
        {"os": "ubuntu-latest", "python": "3.10"},
        {"os": "ubuntu-latest", "python": "3.14"},
        {"os": "macos-latest", "python": "3.12"},
        {"os": "windows-latest", "python": "3.12"},
    ]

    matrix = full["jobs"]["test"]["strategy"]["matrix"]
    assert matrix == {
        "os": ["ubuntu-latest", "macos-latest", "windows-latest"],
        "python": ["3.10", "3.12", "3.14"],
    }
    exhaustive = FULL_MATRIX_WORKFLOW.read_text(encoding="utf-8")
    assert _triggers(full) == {"schedule", "workflow_dispatch"}
    assert "pre-release" not in CI_WORKFLOW.read_text(encoding="utf-8")
    assert exhaustive.count("actions/checkout@d23441a48e516b6c34aea4fa41551a30e30af803") == 1
    assert exhaustive.count("astral-sh/setup-uv@fac544c07dec837d0ccb6301d7b5580bf5edae39") == 1
    assert "actions/checkout@v" not in exhaustive
    assert "astral-sh/setup-uv@v" not in exhaustive


def test_readme_links_the_codecov_badge_to_the_public_project() -> None:
    readme = README.read_text(encoding="utf-8")

    badge = "https://codecov.io/gh/pzfreo/quiddity/graph/badge.svg"
    project = "https://codecov.io/gh/pzfreo/quiddity"
    assert f"[![codecov]({badge})]({project})" in readme


def test_the_release_pipeline_is_the_ported_shape() -> None:
    """Four jobs and one chain, matching the workflow this was ported from.

    Asserted because the previous attempt drifted into six jobs with three extra guards, and
    every one of five review rounds found its defects in that added machinery rather than in
    the ported part. A `needs:` graph is also invisible to substring matching, which is what
    the assertions here used to be: pointing `publish-pypi` at the wrong job was green.
    """

    jobs = _parsed(WORKFLOW)["jobs"]

    assert list(jobs) == ["publish-testpypi", "build-release", "publish-pypi", "bump-version"]
    assert jobs["publish-pypi"]["needs"] == "build-release"
    assert jobs["bump-version"]["needs"] == "publish-pypi"
    # One build feeds the release; the snapshot builds and publishes on a single runner.
    uploads = [
        name
        for name, spec in jobs.items()
        if any("upload-artifact" in str(step) for step in spec["steps"])
    ]
    assert uploads == ["build-release"]


def test_the_release_artifact_is_built_here_and_published_unmodified() -> None:
    """The point of the port: the wheel is a function of the tagged commit.

    Located in the step that runs it. `assert "uv build" in job_text` was satisfied by the
    snapshot job, and at the low point `assert "verify_release_assets.py" in job_text` was
    satisfied by a *comment about* it, so deleting the step that ran it stayed green.
    """

    jobs = _parsed(WORKFLOW)["jobs"]
    build = [
        line.strip()
        for run in _steps(jobs["build-release"])
        for line in run.splitlines()
        if line.strip() and not line.strip().startswith("#")
    ]

    assert "uv build" in build
    assert any("scripts/update-recogniser-version" in line for line in build)
    assert "gh release download" not in WORKFLOW.read_text(encoding="utf-8")

    publish = jobs["publish-pypi"]["steps"]
    assert not any("run" in step for step in publish), "the release must not be rebuilt here"
    action = next(s for s in publish if "gh-action-pypi-publish" in s.get("uses", ""))
    assert "with" not in action, "PyPI is the default index; a repository-url would redirect it"


def test_the_bump_opens_a_pr_and_dispatches_its_ci_status() -> None:
    """`git push origin HEAD:main` was green; the job holds `contents: write`."""

    jobs = _parsed(WORKFLOW)["jobs"]
    joined = "\n".join(_steps(jobs["bump-version"]))

    assert 'git push origin HEAD:"$branch"' in joined
    assert "HEAD:main" not in joined and "origin main" not in joined

    assert 'gh workflow run ci.yml --ref "$branch"' in joined
    assert "for workflow in" not in joined
    assert "workflow_dispatch" in _triggers(_parsed(ROOT / ".github" / "workflows/ci.yml"))


@pytest.mark.skipif(
    sys.platform == "win32",
    reason="executes the workflow's bash; every publish job runs on ubuntu-latest",
)
def test_the_version_arithmetic_each_leg_performs(tmp_path) -> None:
    """The only logic deciding what version reaches an index, executed rather than named.

    Nothing asserted it. Four single-line mutations each survived the whole suite: dropping
    the `.dev` strip so `0.2.6.dev0` itself would go to production PyPI; bumping `minor`
    instead of `patch`; dropping `.dev${RUN_NUMBER}` from the snapshot; and pointing the bump
    at the tag rather than `main`. The one assertion covering that step only checked that
    `scripts/update-recogniser-version` is mentioned, which cannot tell those apart.

    """

    jobs = _parsed(WORKFLOW)["jobs"]

    def version_after(job: str, step_name: str, current: str, **env: str) -> str:
        step = next(s["run"] for s in jobs[job]["steps"] if s.get("name") == step_name)
        (tmp_path / "pyproject.toml").write_text(
            f'[project]\nversion = "{current}"\n', encoding="utf-8"
        )
        # The script itself is stubbed: what is under test is the shell that decides its
        # argument, not the file-moving it already has its own tests for.
        stub = tmp_path / "scripts"
        stub.mkdir(exist_ok=True)
        (stub / "update-recogniser-version").write_text(
            '#!/bin/sh\nprintf %s "$1" > "$(dirname "$0")/../called-with"\n', encoding="utf-8"
        )
        (stub / "update-recogniser-version").chmod(0o755)
        result = subprocess.run(
            ["bash", "-euo", "pipefail", "-c", step],
            capture_output=True,
            text=True,
            cwd=tmp_path,
            env={"PATH": os.environ["PATH"], **env},
        )
        assert result.returncode == 0, result.stderr
        return (tmp_path / "called-with").read_text(encoding="utf-8")

    # The release strips `.dev` and publishes the base version.
    strip = "Strip dev suffix for real release"
    assert version_after("build-release", strip, "0.2.6.dev0") == "0.2.6"
    assert version_after("build-release", strip, "0.2.6") == "0.2.6"
    # The snapshot never publishes a release version: it always carries a `.devN`.
    assert (
        version_after("publish-testpypi", "Set TestPyPI dev version", "0.2.6.dev0", RUN_NUMBER="57")
        == "0.2.6.dev57"
    )
    # The bump moves the patch, from `main`'s version rather than from any tag.
    bump = "Bump patch version with .dev0 suffix"
    assert version_after("bump-version", bump, "0.2.6.dev0") == "0.2.7.dev0"
    assert version_after("bump-version", bump, "0.2.9.dev0") == "0.2.10.dev0"


def test_each_job_runs_only_on_its_own_event() -> None:
    """Gating is what keeps a release from also firing a TestPyPI dev snapshot.

    Deleting any of the three `if:` conditions left the suite green; the shape test reads the
    job list and the `needs:` chain but never the conditions.
    """

    jobs = _parsed(WORKFLOW)["jobs"]

    assert jobs["publish-testpypi"]["if"] == "github.event_name == 'push'"

    # The bump reads `main`'s version, so it must check out `main`. Pointing it at the release
    # tag computes the next version from the tag instead -- the inversion `docs/releasing.md`
    # records as the source of an earlier chain of defects -- and was green.
    checkout = next(s for s in jobs["bump-version"]["steps"] if "checkout" in s.get("uses", ""))
    assert checkout["with"]["ref"] == "main"
    for name in ("build-release", "publish-pypi", "bump-version"):
        assert jobs[name]["if"] == "github.event_name == 'release'"
