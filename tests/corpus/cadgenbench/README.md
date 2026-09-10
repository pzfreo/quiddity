# CADGenBench generated-part regressions

These are generated build123d submission outputs, not benchmark reference drawings.
The canonical submission cited by Quiddity #587 and independently verified in PR #591 is
`cadgenbench-build123d/submit/build123d-mcp-0.3.85.dev0-opus-5-xhigh-complete81-r1`.
The review also verified identical bytes in
`build123d-mcp-0.3.85.dev0-opus-5-xhigh-recognition-guided-complete81-r1`.

The files were copied without modification from this workspace's local submission named
`opus5-xhigh-minimal-mcp-drawing-evidence-generation49-complete-r1`; that directory exists
in this workspace but is not present in the reviewer's checkout. Submission directory
names are checkout-specific; the hashes below identify the exact geometry in all copies.
These generated outputs come from the author's Apache-2.0 CADGenBench harness; see
`THIRD_PARTY_NOTICES.md` for their distinction from third-party reference geometry.

| File | Original case | SHA-256 |
|---|---|---|
| `threaded_connector_109.step` | `109/output.step` | `301cb3ce914fa28c68e3cc324d22a9b6e8ee119c7c76797e7d7052de7bccdcff` |
| `flanged_spool_132.step` | `132/output.step` | `58dfe8a594a5afe64652ea127807ad6141812e2e1d2556d429f6a42e4ec46747` |

The connector reproduces nonplanar B-spline face centres being published as shoulders.
The spool is the positive control for retaining a valid partial turned profile: its
three coaxial steps span 0..113 mm on a body whose total axial extent is 140 mm.
