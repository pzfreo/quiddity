# Packed STEP provenance

The geometry reader retains the typed step-io model and its report while checking
original file identities. A second, generic attribute tree is expensive for files
with millions of records. Haecceity consumes `step_io::parser::stream()` in
file order and writes each DATA entity to a private in-memory store. The id
index points into byte blocks. A repeated id is reported at once with both
source lines. Reading through the end of the stream also validates the trailer.
The importer keeps the small header for per-product documents and uses it while resolving the declared units.

`RecordGraph` keeps traversal order and the observation boundary explicit. Packed
entity keywords borrow interned symbols. Selected attribute paths skip unrelated
payload without allocating it, including B-spline control-point lists when only
their first pole is needed. Full-record traversals own at most their current
record; there is no unbounded decoded-record cache. Ordinary graphs remain usable
by document/manifest paths and directed comparison tests. Original record shape
is queried independently of its keyword, so an unsupported simple entity named
`COMPLEX` cannot change complex-reference validation.

Declared length-unit metadata shares the same record access. It keeps the
existing graph traversal, schema slot lookup and exact unit resolver. A decoded
measure record remains alive while its numeric attribute is located by pointer
and its original REAL token is read; equal floating-point values are not used
to guess which textual token supplied a conversion factor. Semantic PMI continues
to borrow the editing document's original attribute tree. No full ordinary graph
is rebuilt for metadata, and existing unit caches retain resolved values only.

The same face and edge provenance predicates, reference types, curve-wrapper
bounds, dropped-shape reachability and assembly/product chains are checked. The
storage change does not replace these checks with scene order or geometric
heuristics. Complex-reference slot caches own their small set of type names so
they cannot borrow a decoded record after that record has been released.

The existing borrowed-byte and file API signatures remain available. Owned-byte
entry points finish parsing and provenance checks, then release that snapshot
before B-rep construction. File readers and recognition snapshots use this path;
recognition first hashes the unchanged original input. Parse diagnostics still
have the complete snapshot available. The file entry still initially buffers
its input; this is a lifetime change, not a streaming file implementation.
The lossless Part 21 editing document retains its separate ordinary raw graph
and original bytes.

Haecceity requires step-io 0.3.1 or a compatible patch release for
`parser::stream()`. Downstream builds resolve it directly from crates.io.

Directed tests compare full imported `Part` debug fields, assemblies and refusal
messages against the ordinary representation. The opt-in real-file test streams
those fields into a same-toolchain hash rather than allocating a whole-part
debug string. That hash is an experiment aid, not a recognition-evidence format
or a substitute for strict snapshots or downstream application acceptance.
