"""Check every published API as a patch; permit documented, versioned breaks."""
import json
from pathlib import Path
import re
import subprocess
import sys
import tomllib
import urllib.request


def section(changelog, heading):
    """Ignore other releases and commented-out template text."""
    changelog = re.sub(r"<!--.*?-->", "", changelog, flags=re.S)
    match = re.search(rf"^## {heading}\b[^\n]*$([\s\S]*?)(?=^## |\Z)", changelog, re.M)
    return match.group(1) if match else ""


def unreleased(changelog):
    return section(changelog, "Unreleased")


def breaks(text):
    breaking = re.search(r"^### Breaking \(Rust API\)\s*$([\s\S]*?)(?=^### |\Z)", text, re.M)
    return bool(breaking and re.sub(r"[\s*`#_\-]", "", breaking.group(1)))


def release_override(current, baseline, changelog):
    version = lambda text: tuple(map(int, text.split(".")))
    text = unreleased(changelog)
    if breaks(text):
        pending = re.search(r"^Pending version: (\d+\.\d+\.\d+)\s*$", text, re.M)
        if not pending:
            raise ValueError("Breaking (Rust API) requires 'Pending version: X.Y.Z' under Unreleased")
        pending = pending.group(1)
    elif version(current) > version(baseline) and breaks(section(changelog, re.escape(current))):
        # The release PR itself: the manifest's own, not yet published, release section.
        pending = current
    else:
        return None
    published, planned, manifest = map(version, (baseline, pending, current))
    minimum = (0, published[1] + 1, 0) if published[0] == 0 else (published[0] + 1, 0, 0)
    if planned < minimum or planned < manifest:
        raise ValueError(f"breaking API requires pending version >= {minimum}, and >= manifest {current}")
    return pending


def check_package(manifest, baseline, changelog):
    package = tomllib.loads(manifest.read_text())["package"]
    name, current = package["name"], package["version"]
    pending = release_override(current, baseline, changelog)
    # Always detect/report actual breaks, even while manifests await release.
    # Never let version inference or --release-type major disable all lints.
    command = [
        "cargo", "semver-checks", "--manifest-path", str(manifest),
        "--package", name, "--baseline-version", baseline, "--color", "never",
        "--default-features", "--features", "json-import", "--release-type", "patch",
    ]
    result = subprocess.run(command, capture_output=True, text=True)
    print(result.stdout, end="", flush=True)
    print(result.stderr, end="", file=sys.stderr, flush=True)
    if result.returncode == 0:
        return
    # Exit 100 alone is insufficient: build/network/tool failures must not be
    # excused. Require this pinned tool's completed API-lint failure summary.
    api_break = result.returncode == 100 and re.search(
        r"Summary\s+semver requires new (?:major|minor) version: \d+ major and \d+ minor checks failed",
        result.stdout + result.stderr,
    )
    if not pending or not api_break:
        raise subprocess.CalledProcessError(result.returncode, command)
    print(f"{name}: reported API breaks approved by the Breaking (Rust API) section for {pending}", flush=True)


def main():
    changelog = Path("CHANGELOG.md").read_text()
    for manifest in [Path("crates/haecceity/Cargo.toml"), Path("Cargo.toml")]:
        name = tomllib.loads(manifest.read_text())["package"]["name"]
        request = urllib.request.Request(
            f"https://crates.io/api/v1/crates/{name}",
            headers={"User-Agent": "quiddity-semver-ci (github.com/pzfreo/quiddity)"},
        )
        with urllib.request.urlopen(request, timeout=30) as response:
            baseline = json.load(response)["crate"]["max_stable_version"]
        if not baseline:
            raise RuntimeError(f"{name}: no published stable baseline")
        check_package(manifest, baseline, changelog)


if __name__ == "__main__":
    main()
