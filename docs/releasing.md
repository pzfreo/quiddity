# Releasing

This is Draftwright's release process, which this package's had drifted away from. Between
releases `main` carries a `.devN` version — `0.2.6.dev0` after 0.2.5 shipped — and every push
to `main` publishes a snapshot of it to TestPyPI, so the publish path is exercised
continuously rather than only at a release.

A Quiddity release is one GitHub release. Use `quiddity-vX.Y.Z` tags: the old package
already owns tags such as `v0.2.0`, which must not be moved or replaced.

1. Manually make sure `main` is on the version you intend to ship (`0.2.6.dev0` to release
   `0.2.6`) and that `RELEASE_NOTES.md` has a `## 0.2.6` section. The workflow does not infer or
   enforce release-note completeness from the tag.
2. Create a GitHub release whose tag is `quiddity-v` followed by that version, with no `.dev`
   suffix (first Quiddity release: `quiddity-v0.2.0`).
   **Attach nothing.**
3. `build-release` checks out the tag, strips the `.dev` suffix, builds the wheel and sdist,
   and hands them to the protected `pypi` environment for approval.
4. When PyPI accepts, the workflow opens a PR moving `main` to the next `.dev0`. It explicitly
   dispatches `ci.yml` against that branch because a branch pushed with `GITHUB_TOKEN` raises no
   events and would otherwise arrive with no checks. The generated PR receives the same full package
   matrix as an ordinary change and has no merge bypass. Draftwright compatibility is not a status
   on this mechanical identity bump; it is proved in Draftwright's own CI when that repository moves
   its exact package pin.

The published wheel is a function of the tagged commit. It used to be built on a maintainer's
machine and attached to the release, which could only check that the attached artifact's
*version* matched the tag — something a wheel built from a dirty tree also satisfies.

## The version comes from `main`, not from the tag

The tag selects the commit; the version is whatever that commit's `pyproject.toml` says, minus
`.dev`. So tagging `quiddity-v0.3.0` on a commit whose version is `0.2.6.dev0` publishes **0.2.6**, and
the tag is simply a misleading label on it. Nothing rejects that, here or upstream.

Step 1 is therefore the whole safety story: move `main` first, with
`scripts/update-recogniser-version`, then tag what it says. An earlier version of this workflow
took the version from the tag instead and added a checker to compare the two; that inverted the
dependency and produced a chain of defects, and it is not what the proven process does.

## Prereleases are not supported by this path

The delivery protocol is deliberately stable-package-first, and this workflow supports stable
releases plus the development snapshots between them. It does **not** produce prereleases.

The obstacle is not the workflow. `capabilities.py`'s version pattern requires any suffix to
begin with `.`, `+` or `-`, so `0.2.6rc1` is not a version this package can describe itself as:
a wheel built at it raises `CapabilityManifestError` from `capability_manifest()`, from
`capability_manifest_json()`, and from the console script — the exact ADR 0005 contract a
prerelease exists to let a consumer validate against.

`scripts/update-recogniser-version` therefore refuses `rcN`, so the attempt fails before
anything is published rather than after. An earlier draft of this branch accepted it and
documented the flow, which would have shipped an artifact that fails its own contract.

Adding a prerelease path later would require an approved ADR 0005 transition, a version form the
capability manifest accepts, and end-to-end publication evidence. It is not part of the current
delivery contract.

## Minor and major versions

`bump-version` only ever does patch + 1. To release `0.3.0`, open a PR moving `main` to
`0.3.0.dev0` first, then tag `quiddity-v0.3.0`.

## One-time repository settings

**Settings → Actions → General → Workflow permissions → "Allow GitHub Actions to create and
approve pull requests"** must be enabled. It defaults to disabled, and without it the
post-release bump fails at `gh pr create` — after PyPI has accepted the release, so the release
itself is fine but `main` stays on the old `.devN` until someone opens that PR by hand.

Publishing uses PyPI Trusted Publishing (GitHub OIDC), never repository API-token secrets. The
one-time pending-publisher records on the two indexes must be:

| Index | Project | Owner | Repository | Workflow | Environment |
| --- | --- | --- | --- | --- | --- |
| TestPyPI | `quiddity` | `pzfreo` | `quiddity` | `publish.yml` | `testpypi` |
| PyPI | `quiddity` | `pzfreo` | `quiddity` | `publish.yml` | `pypi` |

Before merging the Quiddity rename, configure these new project publishers on both indexes.
The old project's publisher does not authorize a new distribution name. Main pushes publish
to TestPyPI automatically; stable PyPI publication still requires an explicit release.
The GitHub repository is now `pzfreo/quiddity`. Retained old-package publishers must also
use repository `quiddity` before publishing maintenance releases under the old package name.

TestPyPI and PyPI accounts and publisher registrations are independent. Protect the GitHub
`pypi` environment with required reviewer approval; `testpypi` does not need approval.

## Moving the version by hand

`scripts/update-recogniser-version X.Y.Z[.devN]` is the only supported way. Five files
hold the version — `pyproject.toml`, `uv.lock`, the `package.version` fields in
`capabilities.json` and `inspection_api.json`, and the `PackageNotFoundError` fallback in
`__init__.py` — and the script moves all five or restores all five. Editing any of them by hand
is how the fallback came to sit at 0.2.2 through both the 0.2.3 and 0.2.4 releases.
