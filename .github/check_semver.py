"""Check published Rust APIs, with one explicit 0.5.0 migration allowance."""
import json
from pathlib import Path
import subprocess
import tomllib
import urllib.request


def release_override(current, baseline):
    # Adding non_exhaustive is breaking. Versions are deliberately left for the
    # release process; this allowance expires as soon as 0.5.0 is published or
    # the manifest changes. Do not carry it into another release line.
    if current == baseline == "0.4.9":
        return ["--release-type", "major"]
    return []


def main():
    for manifest in [Path("crates/haecceity/Cargo.toml"), Path("Cargo.toml")]:
        package = tomllib.loads(manifest.read_text())["package"]
        name, current = package["name"], package["version"]
        request = urllib.request.Request(
            f"https://crates.io/api/v1/crates/{name}",
            headers={"User-Agent": "quiddity-semver-ci (github.com/pzfreo/quiddity)"},
        )
        with urllib.request.urlopen(request, timeout=30) as response:
            baseline = json.load(response)["crate"]["max_stable_version"]
        if not baseline:
            raise RuntimeError(f"{name}: no published stable baseline")
        override = release_override(current, baseline)
        if override:
            print(f"{name}: approved Breaking (Rust API) migration to 0.5.0; manifests await release", flush=True)
        # Explicitly cover normal public APIs plus optional JSON. Avoid the
        # heavyweight f3d converter and the test-only Python panic feature.
        subprocess.run([
            "cargo", "semver-checks", "--manifest-path", str(manifest),
            "--package", name, "--baseline-version", baseline,
            "--default-features", "--features", "json-import", *override,
        ], check=True)


if __name__ == "__main__":
    main()
