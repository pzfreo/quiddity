# CADGenBench generated-part regressions

These are the generated build123d submission outputs cited by Quiddity #587 and
Draftwright #1132, not the benchmark reference drawings. Copied without modification from
the local `cadgenbench-build123d/submit/opus5-xhigh-minimal-mcp-drawing-evidence-generation49-complete-r1`
submission; the same files also occur in the drawing-evidence submission.

| File | Original case | SHA-256 |
|---|---|---|
| `threaded_connector_109.step` | `109/output.step` | `301cb3ce914fa28c68e3cc324d22a9b6e8ee119c7c76797e7d7052de7bccdcff` |
| `flanged_spool_132.step` | `132/output.step` | `58dfe8a594a5afe64652ea127807ad6141812e2e1d2556d429f6a42e4ec46747` |

The connector reproduces nonplanar B-spline face centres being published as shoulders.
The spool is the positive control for retaining a valid partial turned profile: its
three coaxial steps span 0..113 mm on a body whose total axial extent is 140 mm.
