# Derived shape aspect fixtures

`GRM-03_thumbwheel_drive_screw_AP242_PMI.step.gz` is the user-supplied GRM-03 input
from `scratchpad/derived-inputs`, compressed with gzip (`mtime=0`). No source geometry or
PMI was changed. SHA-256 of the uncompressed STEP text:
`4b6462b9cc9f0d419250933bd77fb305f9cfebb7ec2b3f377008732876010a21`.

`crates/haecceity/tests/pmi_derived.rs` tests the original file and synthetic edits of it:
agreeing supplemental items, derivation chains, median planes, all three distance-target
pairs, tolerance targets, mixed units, uncertainty, qualifier retention and STEP writing.
The original file's ten dimensions retain their exact nominal values and qualifiers, and
all ten now resolve: five explicit points on deriving cylinder surfaces cross-check their
axes, and ten single-plane endpoints supply their faces' own planes. The regression
checks every diameter against its axis radius and every length against its parallel-plane
separation. Synthetic refusals still cover points off both axis and surface, non-parallel
plane distances, ambiguous deriving faces and unsupported supplemental geometry.

`nist-baseline.json` records the before-change source hash, semantic-output hash, counts
of dimension and geometric-tolerance roots, read counts and refused counts. Its first ten
rows are the supplied `/Users/paul/repos/quiddity/tests/corpus/nist` directory (AP203
geometry-only files); its last seven rows are the committed NIST AP242 semantic fixtures.
No file in either set contains a derived shape aspect. The semantic hash is SHA-256 of
compact, sorted-key UTF-8 JSON `{"parts": <pmi read parts>, "findings": <pmi read findings>}`
from reader `/6` at the starting commit. Format/binding version changes are deliberately
outside that hash. `tests/pmi_cli.rs::derived_geometry_does_not_regress_any_nist_pmi`
requires exact equality after the change, protecting all previously read data and findings.
Set `QUIDDITY_CORPUS` to include the external ten files; the seven committed fixtures
are always checked. Refused dimension/tolerance counts mean stated roots minus read roots,
not the number of findings (which can concern other constructs or read-through defects).

Reader `/12` (2026-10-31) refreshes only STC10's semantic expectation: seven plain
constituents now alias their owning datum features instead of adding duplicate features
(173 → 166), so feature references are reindexed. Source hashes, dimension/tolerance
counts, refusal counts and every other row remain unchanged. The structural source chains
and face identities are checked by `pmi_import_repairs.rs`; the Rust PMI snapshot records
the complete changed structure. This retains the existing exact-output audit; it adds no
hash-based test and changes no known-difference record.

Reader `/13` (2026-11-01) refreshes only STC06 and STC09's semantic expectations:
each adds one non-fatal `Nonconformance` finding for the repeated source identifier
(`general property` and `mass property`, respectively), naming its carriers and
`id_attribute` instances. Resolved PMI, source hashes, dimension/tolerance counts and
refusal counts are unchanged. The other fifteen rows are unchanged. Structural tests
in `pmi_xcaf_read.rs` check source identifiers, unique JSON references and the recovered
non-runout zone independently; the existing audit remains strict.

Reader `/16` (#142) refreshes four existing semantic expectations. CTC-01 no
longer reports its four dimensions without values as nonconformances. FTC-07,
FTC-10 and STC-09 retain anonymous attribute units by source entity reference
instead of the ambiguous `derived_unit` label. Source hashes, all stated/read
PMI counts, and refusal counts are unchanged. The strict NIST JSON/STEP tests
check the complete model and source callout links independently of this legacy
audit; no output-hash test or baseline case is added.

Reader `/17` replaces only FTC-07, FTC-10 and STC-09's existing anonymous
attribute-unit expectations with portable content identities. Named-unit kinds,
prefixes, conversion factors and exact decimal exponents replace source ids.
All source hashes, stated/read counts and refusal counts remain unchanged.
The new B-rep test checks all 13 measured attributes (12 derived units) through
a fresh document and subsequent replacement; no audit case or hash test is added.
