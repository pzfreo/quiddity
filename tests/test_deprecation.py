"""Retirement notices remain visible without duplicating CLI diagnostics."""

import json
import shutil
import subprocess
import sys

import pytest

NOTICE = "quiddity 0.3.12 is the final release of the Python implementation."
PIN = 'pip install "quiddity<0.4"'


def test_import_warns_once_with_pin_advice():
    completed = subprocess.run(
        [
            sys.executable,
            "-c",
            """
import json
import warnings
with warnings.catch_warnings(record=True) as caught:
    warnings.simplefilter("always")
    import quiddity
    import quiddity
    import quiddity.cli
print(json.dumps([(item.category.__name__, str(item.message)) for item in caught]))
""",
        ],
        capture_output=True,
        text=True,
        timeout=60,
        check=True,
    )
    # Only quiddity's notice: dependencies may raise their own DeprecationWarnings
    # (lib3mf does on Python 3.14).
    notices = [item for item in json.loads(completed.stdout) if NOTICE in item[1]]
    assert len(notices) == 1
    category, message = notices[0]
    assert category == "FutureWarning"
    assert message.startswith(NOTICE)
    assert PIN in message
    assert "0.4.0 is a Rust-built wheel with a new file-based API" in message
    assert "STEP path or bytes in, JSON (quiddity-rust/recognition/6) out" in message
    assert "build123d/OCP shapes" in message
    assert "dataclasses" in message and "recognise_* functions" in message
    assert "https://github.com/pzfreo/quiddity-rust/blob/main/docs/python.md" in message


@pytest.mark.parametrize("args,code", [(["--help"], 0), ([], 2), (["--capabilities"], 0)])
def test_cli_notice_goes_to_stderr_once(args, code):
    executable = shutil.which("quiddity")
    assert executable is not None
    completed = subprocess.run([executable, *args], capture_output=True, text=True, timeout=60)
    assert completed.returncode == code, completed.stderr
    assert completed.stderr.count(NOTICE) == 1
    assert PIN in completed.stderr
    assert "FutureWarning:" in completed.stderr
    assert NOTICE not in completed.stdout
    if args == ["--help"]:
        assert "usage: quiddity" in completed.stdout
    elif args == ["--capabilities"]:
        assert json.loads(completed.stdout)["package"]["version"] == "0.3.12"
