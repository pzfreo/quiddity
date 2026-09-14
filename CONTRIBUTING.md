# Contributing

Contributions are welcome. By intentionally submitting a contribution for inclusion in this
project, you agree that it is provided under the Apache License 2.0, as described by section 5 of
that licence, unless you explicitly state otherwise in writing.

Changes to a recogniser or public record follow the tests-first
[`docs/delivery-protocol.md`](docs/delivery-protocol.md). Start with the recogniser issue template
and declare every downstream capability state. Downstream compatibility is proven in Draftwright's
own CI on the pull request that moves its exact dependency pin, not in this repository's checks.

## Architectural rules

- Recognition is geometry-only: no drawing, editing-session, CAM-operation or UI policy.
- Public results are immutable, typed and serialisable without leaking build123d/OCP objects.
- A base recogniser accepts `part` first and keyword-only tuning or injected dependencies.
- A derived recogniser is a pure function over already-recognised records.
- Recognisers do not rerun sibling recognisers; orchestration computes shared evidence once.
- Empty means confidently absent within supported topology. Ambiguous and unsupported cases are
  explicit diagnostics.
- Changes to a load-bearing contract require an ADR update.

## Local checks

Mypy 1.x on Python 3.12 is the supported type checker. Python 3.10 compatibility is enforced by
the runtime CI matrix and Ruff's `py310` syntax target. `uv run mypy` checks every typed implementation body
with incomplete private definitions admitted as the incremental baseline. That allowance does
not extend to the published boundary: `tests/test_public_typing.py` independently rejects any
missing, bare-container, or direct `Any` annotation on an exported function, class method,
property, or dataclass field,
and `tests/typing/public_consumer.py` is checked in strict mode against the built wheel. Keep all
three layers green when changing public types.

```bash
uv sync --dev
uv run ruff check . scripts/update-recogniser-version
uv run ruff format --check . scripts/update-recogniser-version
uv run mypy
uv run pytest -n 2 -m "not slow"
```

The command above is the fast edit/test tier. It excludes measured expensive whole-inventory,
corpus, transformation and packaging modules and uses two worker processes. Run
the complete coverage-free suite with `uv run pytest`, or only the expensive tier with
`uv run pytest -m slow`.

One canonical Linux/Python 3.12 CI job records line and branch coverage, prints missing branches,
writes `coverage.xml`, and fails below the 91% combined floor:

```bash
uv run pytest --cov=quiddity --cov-branch --cov-report=term-missing \
  --cov-report=xml:coverage.xml --cov-fail-under=91
```

Compatibility jobs do not repeat coverage instrumentation. The complete suite and coverage gate
remain required before merge; draft pull requests use the fast tier for shorter iteration. Raise
the floor when coverage improves durably; do not weaken it or replace behavior assertions with
execution-only tests.
