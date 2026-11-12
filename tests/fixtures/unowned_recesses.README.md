# Unowned recess controls

I created these synthetic controls from analytic primitives in
[`tools/make_unowned_recess_fixtures.py`](../../tools/make_unowned_recess_fixtures.py).
The inputs and generator are original contributions under Apache-2.0; they contain
no customer geometry or geometry extracted from a customer model.

| Input | Construction | Licence |
| --- | --- | --- |
| `unowned_recess_solid.step.gz` | A rectangular solid with a triangular blind pocket | Apache-2.0 |
| `unowned_recess_shell.step.gz` | The pocket's three walls and floor, with no material solid | Apache-2.0 |
| `unowned_recess_mixed.step.gz` | The solid and a translated, disjoint copy of the open shell | Apache-2.0 |

`rust-snapshot/unowned_recesses.json.gz` contains the Apache-2.0 expected result
produced by the Rust `tests/internal/unowned_recesses.rs` control. The Python
generator supplies geometry only, not expected recognition results. Input and
expectation hashes are recorded in `rust-snapshot/provenance.json`.
