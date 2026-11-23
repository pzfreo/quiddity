"""Plan and check the Linux PR tier against the full nextest inventory."""

import argparse
from fnmatch import fnmatchcase
import json
from pathlib import Path
from posixpath import normpath
import re
import subprocess
import sys

from shards import assign, filterset, listed

MAX_SECONDS = 10
# At the current ~15 runner-minutes of fast tests, 12 jobs keep the test phase near 76 s.
PR_SHARDS = 12

# Match repository-relative paths, or basenames for patterns without a slash.
FULL_TIER_PATHS = {
    "tests/fixtures/*": "shared snapshots, references and STEP inputs",
    "crates/*/tests/fixtures/*": "crate fixture consumers span test binaries",
    "tools/as_compare/fixtures/*": "shared comparison reference inputs",
    "tests/support/section_recess_parts.txt": "shared corpus part list",
    "crates/haecceity/tests/oracle_ledger.json": "shared oracle expectations",
    "Cargo.toml": "workspace dependencies, features or test targets can change",
    "Cargo.lock": "resolved dependencies can affect every binary",
    "rust-toolchain*": "compiler changes can affect every binary",
    ".config/nextest.toml": "nextest configuration controls the full inventory",
    ".github/shards.py": "sharding changes affect every test job",
    ".github/ci_tiers.py": "selection changes affect which tests run",
    "tests/common/*": "cross-crate shared test code",
}

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

# Example code compiled into tests through #[path]. Keys identify the declaration so
# adding a consumer requires reviewing the mapping, even for an already-known helper.
EXAMPLE_TEST_ALIASES = {
    ("tests/assembly_layout.rs", "examples/support/assembly_layout.rs", "engine"):
        ("tests/assembly_layout.rs",),
    ("tests/common/manifest_layouts.rs", "examples/support/assembly_layout.rs", "engine"):
        ("tests/manifest.rs", "tests/manifest_cli.rs"),
    ("tests/internal/mfinstseg.rs", "examples/support/mfinstseg.rs", "scoring"):
        ("tests/internal/mfinstseg.rs",),
    ("crates/haecceity/tests/gvp.rs", "crates/haecceity/examples/gvp_support/mod.rs", "support"):
        ("crates/haecceity/tests/gvp.rs",),
}

PATH_ATTR = re.compile(r"#\s*\[\s*path\s*(?==)")
PATH_MODULE = re.compile(
    r'=\s*"([^"\\]+)"\s*\]\s*(?:#\[[^\]]*\]\s*)*'
    r'(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;'
)


def full_tier_reason(paths):
    for path in paths:
        for pattern, reason in FULL_TIER_PATHS.items():
            if fnmatchcase(path if "/" in pattern else Path(path).name, pattern):
                return f"{path} ({reason})"
    return None


def workspace_path(filename):
    parts = Path(filename).with_suffix("").parts
    if len(parts) >= 3 and parts[0] == "crates":
        return parts[1], parts[2:]
    return "quiddity", parts


def internal_prefix(module):
    if module[-1] == "mod":
        module = module[:-1]
    return "::".join(("regression_tests", *module[1:])) + "::"


def source_selector(filename, crate, module):
    alias = SOURCE_ALIASES.get(filename)
    if alias is not None:
        return crate, alias + "::"
    if module[0] == "bin" and len(module) > 1:
        return f"{crate}::bin/{module[1]}", ""
    if module == ("main",):
        return f"{crate}::bin/{crate}", ""
    if module == ("lib",):
        return crate, ""
    if crate == "quiddity" and module[0] not in {"dimensioning", "engine"}:
        module = ("engine", *module)
    if module[-1] == "mod":
        module = module[:-1]
    return crate, "::".join(module) + "::"


def covered_path_module(owner, target, name):
    if (owner, target, name) in EXAMPLE_TEST_ALIASES:
        return True
    # A new consumer of a shared helper must also run when its example input changes.
    for (consumer, _, _), owners in EXAMPLE_TEST_ALIASES.items():
        if consumer == target and consumer not in owners and owner not in owners:
            return False
    if full_tier_reason([target]):
        return True
    if (owner, target, name) == ("src/lib.rs", "tests/internal/mod.rs", "regression_tests"):
        return True
    if not target.endswith(".rs"):
        return False
    crate, parts = workspace_path(owner)
    target_crate, target_parts = workspace_path(target)
    if crate != target_crate or len(target_parts) < 2 or parts[0] != target_parts[0]:
        return False
    if parts[0] == "src":
        binary, prefix = source_selector(owner, crate, parts[1:])
        target_selector = source_selector(target, crate, target_parts[1:])
        # A source binary selection already covers every module in that binary.
        return target_selector == (binary, "" if "::bin/" in binary else prefix + name + "::")
    if len(target_parts) <= 2 or target_parts[2:] == ("main",):
        return False
    if crate == "quiddity" and target_parts[1] == "internal":
        return (parts[1] == "internal"
                and internal_prefix(parts[1:]) + name + "::" == internal_prefix(target_parts[1:]))
    # The selector treats nested test helpers as shared within their crate.
    return True


def check_source_aliases(root, files):
    example_helpers = {target for _, target, _ in EXAMPLE_TEST_ALIASES}
    for filename in files:
        _, parts = workspace_path(filename)
        if len(parts) < 2 or parts[0] not in {"src", "tests", "examples"}:
            continue
        # Standalone examples are built without a test harness. Nested example
        # modules can be compiled into Rust tests through #[path].
        example_only = parts[0] == "examples" and len(parts) == 2 and filename not in example_helpers
        contents = (root / filename).read_text(encoding="utf-8")
        for attr in PATH_ATTR.finditer(contents):
            line_start = contents.rfind("\n", 0, attr.start()) + 1
            if contents[line_start:attr.start()].lstrip().startswith("//"):
                continue
            declaration = PATH_MODULE.match(contents, attr.end())
            if declaration:
                relative, name = declaration.groups()
                target = normpath(str(Path(filename).parent / relative))
                if example_only or covered_path_module(filename, target, name):
                    continue
            line = contents.count("\n", 0, attr.start()) + 1
            raise ValueError(f"{filename}:{line}: uncovered #[path] module. Update .github/ci_tiers.py")


def touched_tests(tests, paths):
    """Match integration targets and Rust module prefixes, scoped by binary ID."""
    paths = set(paths)
    prefixes = set()
    shared_crates = set()
    aliases = {
        owner for (_, target, _), owners in EXAMPLE_TEST_ALIASES.items()
        if target in paths for owner in owners
    }
    for filename in paths | aliases:
        path = Path(filename)
        if path.suffix != ".rs":
            continue
        crate, parts = workspace_path(filename)
        if len(parts) < 2:
            continue
        directory, module = parts[0], parts[1:]
        if directory == "tests":
            if crate == "quiddity" and module[0] == "internal":
                prefixes.add((crate, internal_prefix(module)))
            elif len(module) == 1 or module[1:] == ("main",):
                prefixes.add((f"{crate}::{module[0]}", ""))
            else:
                shared_crates.add(crate)
        elif directory == "src":
            prefixes.add(source_selector(filename, crate, module))
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
    files = subprocess.check_output(["git", "ls-files", "-z", "--", "*.rs"])
    check_source_aliases(Path.cwd(), files.decode(errors="surrogateescape").split("\0")[:-1])
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
    force_full = full_tier_reason(paths)
    if force_full:
        print(f"Full Linux tier selected by {force_full}", file=sys.stderr)
        pr, touched = main_tests, full
        planned, loads, filters = [], [], []
    else:
        touched = touched_tests(full, paths)
        planned, loads = assign(select(full, durations, touched), durations, PR_SHARDS)
        filters = [filterset(shard) for shard in planned]
        # Nextest unions repeated -E arguments and each argument is limited to one shard.
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
    # The workflow reuses its full-suite jobs or the validated fast-tier filters.
    print(f"full={str(bool(force_full)).lower()}")
    print(f"shards={json.dumps(list(range(1, len(filters) + 1)))}")


if __name__ == "__main__":
    main()
