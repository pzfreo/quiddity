"""Plan and check the Linux PR tier against the full nextest inventory."""

import argparse
import json
from pathlib import Path
import subprocess
import sys

from shards import assign, filterset, listed

MAX_SECONDS = 10
# At the current ~15 runner-minutes of fast tests, 12 jobs keep the test phase near 76 s.
PR_SHARDS = 12

# #[path] modules whose on-disk paths differ from their Rust test names.
SOURCE_ALIASES = {
    "src/bin/quiddity/pmi_interchange.rs": "engine::cli::pmi_interchange",
    "src/features/recess_floor_blends.rs": "engine::features::recess_core::floor_blends",
    "crates/haecceity/src/express_table.rs": "express::table",
    "crates/haecceity/src/json/presentation_layout.rs": "json::presentation::layout",
    "crates/haecceity/src/pmi/authoring.rs": "pmi::write::authoring",
    "crates/haecceity/src/pmi/notes_view.rs": "pmi::write::notes_view",
    "crates/haecceity/src/pmi/source_presentation.rs": "pmi::write::source_presentation",
    "crates/haecceity/src/pmi/upgrade.rs": "pmi::write::upgrade",
    "crates/haecceity/src/volume_witness_tests.rs": "volume::witness_tests",
}


def touched_tests(tests, paths):
    """Match integration targets and Rust module prefixes, scoped by binary ID."""
    prefixes = set()
    shared_crates = set()
    for filename in paths:
        path = Path(filename)
        if path.suffix != ".rs":
            continue
        crate = "quiddity"
        parts = path.with_suffix("").parts
        if parts[0] == "crates" and len(parts) > 1:
            crate, *parts = parts[1:]
        if len(parts) < 2:
            continue
        directory, *module = parts
        if directory == "tests":
            if crate == "quiddity" and module[0] == "internal":
                if module[-1] == "mod":
                    module.pop()
                prefixes.add((crate, "::".join(["regression_tests", *module[1:]]) + "::"))
            elif len(module) == 1 or module[1:] == ["main"]:
                prefixes.add((f"{crate}::{module[0]}", ""))
            else:
                shared_crates.add(crate)
        elif directory == "src":
            alias = SOURCE_ALIASES.get(filename)
            if alias is not None:
                prefixes.add((crate, alias + "::"))
            elif module[0] == "bin":
                prefixes.add((f"{crate}::bin/{module[1]}", ""))
            elif module == ["main"]:
                prefixes.add((f"{crate}::bin/{crate}", ""))
            elif module == ["lib"]:
                prefixes.add((crate, ""))
            else:
                if crate == "quiddity" and module[0] not in {"dimensioning", "engine"}:
                    module.insert(0, "engine")
                if module[-1] == "mod":
                    module.pop()
                prefixes.add((crate, "::".join(module) + "::"))
    # Root test helpers are also included by haecceity. Crate-local helpers stay in their crate.
    shared_root = "quiddity" in shared_crates
    if shared_root:
        prefixes.add(("quiddity", "regression_tests::"))
    for binary in {binary for binary, _ in tests}:
        crate, sep, target = binary.partition("::")
        if sep and "/" not in target and (shared_root or crate in shared_crates):
            prefixes.add((binary, ""))
    return {
        (binary, name) for binary, name in tests
        if any(binary == target and name.startswith(prefix) for target, prefix in prefixes)
    }


def select(tests, durations, touched):
    # A test without a timing record runs on PRs until a full run supplies one.
    return [t for t in tests if t in touched or durations.get(" ".join(t), 0) <= MAX_SECONDS]


def check_tiers(full, main, pr, durations, touched):
    if full != main:
        raise ValueError("the ci profile must run the entire unfiltered nextest inventory")
    slow = {tuple(key.split(" ", 1)) for key, seconds in durations.items() if seconds > MAX_SECONDS}
    main_only = (slow & full) - touched
    if pr & main_only or (pr | main_only) != full:
        raise ValueError("evaluated PR filters and recorded main-only tests must cover the full inventory")
    return main_only


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", help="reuse the CI nextest archive")
    parser.add_argument("--base", help="compare the tested tree to this Git revision")
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    reuse = ["--archive-file", args.archive, "--workspace-remap", "."] if args.archive else [
        "--workspace", "--release", "--locked",
    ]
    cmd = ["cargo", "nextest", "list", *reuse, "--profile", "ci", "--message-format", "json"]

    def inventory(*flags):
        return set(listed(json.loads(subprocess.check_output([*cmd, *flags]))))

    full = inventory("--ignore-default-filter")
    main_tests = inventory()
    durations = json.loads(Path(".config/test-times.json").read_text())["Linux"]
    paths = []
    if args.base:
        diff = subprocess.check_output([
            "git", "diff", "--no-renames", "--name-only", "-z", args.base, "HEAD", "--",
        ])
        paths = diff.decode(errors="surrogateescape").split("\0")
    touched = touched_tests(full, paths)
    planned, loads = assign(select(full, durations, touched), durations, PR_SHARDS)
    filters = [filterset(shard) for shard in planned]
    # Nextest unions repeated -E arguments. Passing one per shard limits each argument's size.
    pr = inventory(*(arg for filter_expr in filters for arg in ("-E", filter_expr)))
    main_only = check_tiers(full, main_tests, pr, durations, touched)

    args.output_dir.mkdir(parents=True, exist_ok=True)
    for filename, tests in [("full", full), ("pr", pr), ("main-only", main_only)]:
        lines = [f"# {len(tests)} tests on Linux ({filename})"]
        for test in sorted(tests):
            key = " ".join(test)
            seconds = f"{durations[key]:g} s" if key in durations else "unrecorded"
            lines.append(f"{key}\t{seconds}" + ("\truns on main" if filename == "main-only" else ""))
        (args.output_dir / f"{filename}.txt").write_text("\n".join(lines) + "\n")
    for i, (filter_expr, load) in enumerate(zip(filters, loads), start=1):
        (args.output_dir / f"pr-{i}.filter").write_text(filter_expr + "\n")
        print(f"PR shard {i}: {len(planned[i - 1])} tests, about {load:.0f} s", file=sys.stderr)
    print(f"Nextest inventory: {len(full)} full/main, {len(pr)} PR, {len(main_only)} main-only",
          file=sys.stderr)
    # The workflow uses the same shard numbers as the validated filter files.
    print(json.dumps(list(range(1, PR_SHARDS + 1))))


if __name__ == "__main__":
    main()
