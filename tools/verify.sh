#!/usr/bin/env bash
# One command that runs what CI runs on a pull request (.github/workflows/*.yml), step by step,
# and ends with a final line `VERIFY: PASS`, `VERIFY: FAIL` or (with --quick) `VERIFY: PARTIAL`.
#
#   tools/verify.sh [--quick]
#
# Environment:
#   QUIDDITY_CORPUS   the pzfreo/quiddity-corpus checkout (required; CI pins the revision in
#                     .github/workflows/suite.yml). Checked against that pin when it is a git clone.
#   VERIFY_WORK       scratch directory for logs, the Python venv and wheels (default: a mktemp dir).
#
# Every step runs even after a failure; the summary lists the failing steps. A CI job this host
# cannot reproduce prints a SKIP line, and a SKIP is never a PASS: any SKIP of a job the host
# could run (a missing tool) makes the result FAIL. Jobs that are inherently elsewhere (other
# operating systems, release-only publishing) are listed as SKIP with the reason and do not.
#
# The host runs the suite's `ci` profile (the full "main push" run of the host's own OS), not
# the sharded PR run: sharding only splits the same tests.
set -u

cd "$(dirname "${BASH_SOURCE[0]}")/.." || exit 2
ROOT=$PWD

QUICK=0
for arg in "$@"; do
  case $arg in
    --quick) QUICK=1 ;;
    -h | --help)
      sed -n '2,19p' "${BASH_SOURCE[0]}"
      exit 0
      ;;
    *)
      echo "verify.sh: unknown option $arg" >&2
      exit 2
      ;;
  esac
done

WORK=${VERIFY_WORK:-$(mktemp -d "${TMPDIR:-/tmp}/quiddity-verify.XXXXXX")}
mkdir -p "$WORK"
VENV=$WORK/venv
DIST=$WORK/dist
SNAP=$WORK/snapshot-verified
# The corpus revision CI checks out (suite.yml); keep in step with that file.
CORPUS_PIN=$(sed -n 's/^ *ref: \([0-9a-f]\{40\}\) *$/\1/p' .github/workflows/suite.yml | head -1)

export CARGO_TERM_COLOR=never
STEPS=()
FAILED=()
SKIPPED=()
N=0
T0=$SECONDS

step() { # step NAME COMMAND [ARGS...]: run, log, record the exit code
  local name=$1
  shift
  N=$((N + 1))
  local log
  log=$(printf '%s/%02d.log' "$WORK" "$N")
  local t=$SECONDS
  "$@" >"$log" 2>&1
  local rc=$?
  local dt=$((SECONDS - t))
  printf 'STEP %02d  %-52s exit %d  (%ds)\n' "$N" "$name" "$rc" "$dt"
  STEPS+=("$name:$rc")
  if [ "$rc" -ne 0 ]; then
    FAILED+=("$name")
    echo "  --- last lines of $log"
    tail -n 15 "$log" | sed 's/^/  | /'
  fi
}

skip() { # skip NAME REASON: a job not reproduced here; never counts as a pass
  N=$((N + 1))
  printf 'SKIP %02d  %-52s NOT RUN: %s\n' "$N" "$1" "$2"
  SKIPPED+=("$1")
}

need() { # need NAME TOOL: a tool the host should have; absence fails the run, loudly
  command -v "$2" >/dev/null 2>&1
}

# ---- step bodies that are more than one command -------------------------------------------

dco() { # dco.yml: every commit since origin/main is signed off
  local missing=0 c
  for c in $(git rev-list --no-merges HEAD --not origin/main); do
    if ! git log -1 --format='%(trailers:key=Signed-off-by,valueonly)' "$c" |
      grep -Eq '^[^<>]*[^<> ][^<>]* <[^<>@ ]+@[^<> ]+>$'; then
      echo "no Signed-off-by: $(git log -1 --format='%h %s' "$c")"
      missing=1
    fi
  done
  return $missing
}

corpus_pin() {
  [ -n "${QUIDDITY_CORPUS:-}" ] && [ -d "$QUIDDITY_CORPUS" ] || {
    echo "QUIDDITY_CORPUS must name the pzfreo/quiddity-corpus checkout"
    return 1
  }
  if [ -d "$QUIDDITY_CORPUS/.git" ] || git -C "$QUIDDITY_CORPUS" rev-parse >/dev/null 2>&1; then
    local head
    head=$(git -C "$QUIDDITY_CORPUS" rev-parse HEAD)
    [ "$head" = "$CORPUS_PIN" ] || {
      echo "corpus is at $head, CI pins $CORPUS_PIN"
      return 1
    }
  else
    echo "corpus is not a git clone: cannot confirm it is at $CORPUS_PIN"
    return 1
  fi
}

nextest_full() { # suite.yml test job, whole suite in one run, with snapshot verification records
  rm -rf "$SNAP"
  mkdir -p "$SNAP"
  QUIDDITY_CORPUS_REQUIRED=1 QUIDDITY_SNAPSHOT_VERIFY_DIR=$SNAP \
    cargo nextest run --release --workspace --locked --profile ci
}

snapshot_coverage() { # suite.yml snapshot-coverage job, exactly
  cargo run -p snapshot-tool --release --locked -- --check --coverage-only --verify-dir "$SNAP"
}

python_venv() {
  rm -rf "$VENV" "$DIST"
  mkdir -p "$DIST"
  uv venv --python 3.11 "$VENV" || uv venv "$VENV"
  uv pip install --python "$VENV" pytest maturin==1.12.0
}

wheel_build() { # python.yml wheels job (host platform only)
  VIRTUAL_ENV=$VENV "$VENV/bin/maturin" build --release --locked \
    -m crates/quiddity-py/Cargo.toml --out "$DIST"
}

wheel_install_test() { # python.yml test job
  uv pip install --python "$VENV" --no-index --no-cache --find-links "$DIST" --reinstall quiddity || return 1
  "$VENV/bin/python" - "$DIST" <<'EOF' || return 1
import glob, sys, zipfile
(wheel,) = glob.glob(sys.argv[1] + "/*.whl")
names = zipfile.ZipFile(wheel).namelist()
for name in ("LICENSE", "NOTICE"):
    (found,) = [n for n in names if n.endswith(".dist-info/licenses/" + name)]
    text = zipfile.ZipFile(wheel).read(found).decode()
    assert len(text) > 100, (found, text)
EOF
  "$VENV/bin/python" -m pytest tests/python -q
}

sdist_test() { # python.yml sdist + test-sdist jobs
  rm -f "$DIST"/*.tar.gz
  "$VENV/bin/maturin" sdist -m crates/quiddity-py/Cargo.toml --out "$DIST" || return 1
  uv pip install --python "$VENV" --reinstall "$DIST"/*.tar.gz || return 1
  "$VENV/bin/python" -m pytest tests/python -q || return 1
  # CI runs `maturin develop` here; on a uv-managed Python that resolves the venv's interpreter to
  # the uv install and refuses to modify it, so build the same wheel and install it instead.
  local panic=$WORK/panic-wheel
  rm -rf "${panic:?}"
  "$VENV/bin/maturin" build --release --locked -m crates/quiddity-py/Cargo.toml \
    --features test-panic --out "$panic" || return 1
  uv pip install --python "$VENV" --reinstall --no-deps "$panic"/*.whl || return 1
  QUIDDITY_TEST_PANIC=1 "$VENV/bin/python" -m pytest tests/python/test_package.py -q -k internal_panic
}

audit() { # ci.yml advisories job
  cargo audit --file Cargo.lock
}

# ---- the steps, in CI order ---------------------------------------------------------------

echo "verify.sh: $(git rev-parse --short HEAD) on $(uname -sm); work dir $WORK; corpus ${QUIDDITY_CORPUS:-UNSET}"

# dco.yml
step "dco: commits since origin/main signed off" dco

# ci.yml lint job
step "lint: python unittest .github" python3 -m unittest discover -s .github -p 'test_*.py'
step "lint: as_compare test_score" python3 -I tools/as_compare/test_score.py
step "lint: as_compare test_score_mfcadpp" python3 -I tools/as_compare/test_score_mfcadpp.py
step "lint: cargo fmt --check" cargo fmt --all --check
step "lint: cargo clippy -D warnings" cargo clippy --workspace --all-targets --release --locked -- -D warnings
if [ $QUICK -eq 0 ]; then
  step "lint: cargo check --features f3d" cargo check --workspace --all-targets --release --locked --features f3d
fi
step "lint: haecceity pmi_write, no default features" cargo test -p haecceity --release --locked --no-default-features --test pmi_write
step "lint: quiddity check, presentation only" cargo check -p quiddity --release --locked --no-default-features --features presentation
step "lint: haecceity json-import nist_roundtrip" cargo test -p haecceity --release --locked --features json-import --test pmi_json_roundtrip nist_roundtrip -- --exact
step "lint: cargo test --doc" cargo test --doc --workspace --release --locked

# ci.yml wasm job
if rustup target list --installed 2>/dev/null | grep -qx wasm32-unknown-unknown; then
  step "wasm: haecceity" cargo build -p haecceity --release --locked --target wasm32-unknown-unknown --no-default-features --features presentation,json
  step "wasm: quiddity lib" cargo build -p quiddity --lib --release --locked --target wasm32-unknown-unknown --no-default-features --features presentation,json
else
  step "wasm: target installed (rustup target add wasm32-unknown-unknown)" false
fi

# crates.yml package and semver jobs
step "crates: cargo publish --dry-run" cargo publish --dry-run --locked -p haecceity -p quiddity
step "semver: unittest test_check_semver" python3 -m unittest discover -s .github -p test_check_semver.py
if command -v cargo-semver-checks >/dev/null 2>&1 && [ "$(cargo semver-checks --version | awk '{print $2}')" = 0.51.0 ]; then
  step "semver: check_semver.py (cargo-semver-checks 0.51.0)" python3 .github/check_semver.py
else
  step "semver: cargo-semver-checks 0.51.0 installed" false
fi

# ci.yml advisories job
if command -v cargo-audit >/dev/null 2>&1; then
  step "advisories: cargo audit" audit
else
  step "advisories: cargo-audit installed (cargo install cargo-audit --version 0.22.2 --locked)" false
fi

if [ $QUICK -eq 0 ]; then
  # suite.yml: corpus pin, full suite, snapshot coverage; then the full snapshot reproduction
  step "suite: corpus at the CI pin" corpus_pin
  step "suite: cargo nextest run --release --workspace (ci profile)" nextest_full
  step "suite: snapshot-tool --check --coverage-only" snapshot_coverage
  step "suite: snapshot-tool --check (reproduces HEAD)" cargo run -p snapshot-tool --release --locked -- --check

  # python.yml (host platform)
  if need python uv; then
    step "python: venv, pytest, maturin 1.12.0" python_venv
    step "python: wheel build" wheel_build
    step "python: wheel install, licence check, pytest" wheel_install_test
    step "python: sdist build, install, pytest, panic hook" sdist_test
  else
    step "python: uv installed" false
  fi
else
  skip "suite: full nextest, snapshot-tool --check" "--quick"
  skip "python: wheel, sdist, pytest" "--quick"
fi

# Inherently not reproducible here: listed so no one reads the run as covering them.
skip "ci.yml macos jobs / lint on macos" "CI runs full macOS only on main, dispatch and release*; PRs run it on the macos-pr pins. This host's run is the full ci profile on $(uname -s)"
skip "ci.yml linux job, 15 shards" "Linux-only job; this run executes the same tests unsharded on $(uname -s). Linux-specific behaviour needs CI"
skip "python.yml linux-aarch64, windows-x64 wheels+tests" "other operating systems and architectures"
skip "python.yml abi3 check on Python 3.15" "needs the CI Python 3.15 interpreter and the manylinux wheel"
skip "crates.yml / python.yml publish jobs" "release or dispatch only, never on a pull request"

# ---- summary -------------------------------------------------------------------------------
echo
echo "== summary: ${#STEPS[@]} steps run, ${#SKIPPED[@]} skipped, ${#FAILED[@]} failed, $((SECONDS - T0))s total =="
for s in "${STEPS[@]}"; do
  echo "  ${s%:*} -> exit ${s##*:}"
done
for s in "${SKIPPED[@]}"; do
  echo "  SKIPPED (not verified): $s"
done
if [ ${#FAILED[@]} -gt 0 ]; then
  echo "Failing steps:"
  for s in "${FAILED[@]}"; do echo "  - $s"; done
  echo "VERIFY: FAIL"
  exit 1
elif [ $QUICK -eq 1 ]; then
  echo "--quick ran only part of CI; this is not a pass."
  echo "VERIFY: PARTIAL"
  exit 1
else
  echo "VERIFY: PASS"
fi
