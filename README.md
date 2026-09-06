# Quiddity

[![CI](https://github.com/pzfreo/quiddity/actions/workflows/ci.yml/badge.svg)](https://github.com/pzfreo/quiddity/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/pzfreo/quiddity/graph/badge.svg)](https://codecov.io/gh/pzfreo/quiddity)
[![PyPI](https://img.shields.io/pypi/v/quiddity.svg)](https://pypi.org/project/quiddity/)
[![Python versions](https://img.shields.io/pypi/pyversions/quiddity.svg)](https://pypi.org/project/quiddity/)
[![License](https://img.shields.io/pypi/l/quiddity.svg)](LICENSE)
[![Ruff](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/astral-sh/ruff/main/assets/badge/v2.json)](https://github.com/astral-sh/ruff)
[![mypy](https://img.shields.io/badge/mypy-checked-2A6DB2.svg)](https://mypy-lang.org/)

Recover useful engineering features from imported STEP and boundary-representation (B-Rep)
geometry.

A STEP file normally gives a CAD application faces, edges, and solids, but not the design intent
that produced them. Quiddity analyses that topology and returns deterministic semantic
records for features such as holes and counterbores, bosses, slots, pockets, pads, fillets,
chamfers, grooves, hole and pocket patterns, and turned steps. The records contain ordinary,
JSON-serialisable geometry values rather than build123d or OCP objects.

Most recognition families classify faces by native analytic surface type, so imported geometry
should preserve its planes, cylinders and cones. STEP carries them, and every pinned fixture is
proven to survive an export and re-import unchanged. Raised Pads additionally have measured support
for exact planes re-expressed as B-splines; other B-spline families remain outside the proven
domain. Raised Pad recognition also requires exact face membership in one valid closed solid;
open shells, invalid bodies, and ambiguous or missing solid ownership return no Pad records. See
[`docs/capabilities.md`](docs/capabilities.md).

That makes the library a useful foundation for systems which inspect, classify, annotate, compare,
or modify imported CAD. For example, a STEP editor can recognise a hole, present its diameter and
axis as editable intent, and use those values to drive its own topology-editing operation. The
recognisers recover evidence; the consuming CAD system decides what that evidence means and how an
edit should be performed.

The package is Apache-2.0 licensed and independent of any drawing or editing application. It uses
build123d/OCP internally as its B-Rep kernel, but its purpose is recovering meaning from geometry
whose construction history is not available.

## Package rename

Quiddity is the new distribution and Python namespace for `b123d-recognisers`.
Quiddity 0.2.0 is published; the old distribution is deprecated.
See the [migration guide](docs/quiddity-migration.md) for installation and import changes.

### Command line (next release)

```bash
quiddity part.step
quiddity part.step -o recognition.json
quiddity --capabilities
quiddity --version
```

Recognition is the default operation. JSON goes to stdout, diagnostics to stderr.
The CLI uses the same framed recognition and evidence as Python; it does not apply dataset
taxonomies or score accuracy. `quiddity-capabilities` remains available.
See [the CLI and document contract](docs/cli.md) for coordinates, face references and exit codes.

## Recognise an imported model

Load a STEP file through the package's geometry-only reader, then run the shared recognition
orchestration to obtain one consistent feature inventory:

```python
from quiddity import FramedRecognitionResult
from quiddity import build_framed_recognition_result
from quiddity import import_step_geometry

part = import_step_geometry("gearbox-housing.step")
framed = build_framed_recognition_result(part)

if isinstance(framed, FramedRecognitionResult):
    for hole in framed.result.holes:
        print(hole.location, hole.axis, hole.diameter, hole.depth, hole.bottom)
```

`import_step_geometry()` deliberately loads only B-Rep geometry. It flattens STEP assembly
structure and does not retain product names, colours, or layers, none of which recognition reads.
This avoids the XCAF metadata path that can terminate Python on an unnamed assembly component.
Applications that need assembly metadata may continue to use their own metadata-aware loader and
pass the resulting build123d shape to the recognisers.

`build_framed_recognition_result()` shares intermediate geometric analysis across recognisers and
is the ordinary entry point for a CAD application. Retain its frame and exact local working shape
while consuming its frozen result; the records can also be projected to JSON-compatible
dictionaries for storage, indexing, comparison, or an editing pipeline.

For bounded lifecycle explanations from the same single run, use
`build_framed_recognition_report()`. Its immutable report distinguishes evaluated-empty families,
classification-gated families, accepted/rejected candidates and supported residual diagnostics.
It is deliberately not an exhaustive explanation of unsupported geometry; a missing diagnostic
does not prove that no unsupported feature is present.

Individual recognisers are also public when an application needs a narrower answer. Reusable
evidence can be injected explicitly so it is not rediscovered:

```python
from quiddity import analyse_cylinders, recognise_hole_patterns, recognise_holes

cylinders = analyse_cylinders(part)
holes = recognise_holes(part, cyls=cylinders)
patterns = recognise_hole_patterns(holes)
```

### Consume recess geometry as JSON

```python
from quiddity import build_section_recess_document

document = build_section_recess_document(part).to_dict()
for occurrence in document["occurrences"]:
    print(occurrence["classification"], occurrence["geometry"])
```

This uses caller coordinates and the accepted aggregate inventory. JSON schema 2 replaces the
specialised pocket/recess/passage/channel records with a free frame, line/arc profile and explicit
end conditions. `patterns` reference occurrence indices; `refusals` retain source evidence where
an old detector cannot establish truthful section geometry. Face indices refer only
to this document's input-face roster, not persistent CAD identities. See the
[consumer migration guide](docs/section-recess-migration.md) for scope and counting rules.

### Inspect geometry for declared features

CAD front ends that create a declared feature from a selected face can use the supported,
single-face inspection namespace instead of importing recogniser internals:

```python
from quiddity.inspection import AnalyticSurface, SurfaceKind, inspect_face

inspected = inspect_face(selected_face)
if isinstance(inspected.surface, AnalyticSurface):
    if inspected.surface.kind is SurfaceKind.CYLINDER:
        print(inspected.surface.parameters, inspected.anchor)
```

The manifest and [capability documentation](docs/capabilities.md#declared-feature-inspection-api)
freeze the kind-specific parameter positions and units. When an anchor is present, it is proved
in or on the selected face's actual trim, including faces with holes or concave outer boundaries.

The namespace also groups the four consumer-proven family reads: `classify_bevel` /
`BevelReject`, `cone_rims`, `read_double_d_tool`, and `floor_face_anchor`. Existing root,
family-module, and `experimental_geometry.inspect_face` imports remain exact-object compatibility
aliases. `GeometryGraph`, adjacency, blend collapse, correspondence, Candidate identity, and
reconciliation are not part of this supported inspection API. The separate run-local evidence
view below exposes only opaque accepted-feature and caller-face references.

`inspection_api_manifest()` returns the separately versioned, installed-wheel contract for this
roster. It does not change the recognition capability-manifest schema. See
[`docs/capabilities.md`](docs/capabilities.md#declared-feature-inspection-api).

That contract includes the closed `BevelReject.reason` values and the ordered
`read_double_d_tool()` result: `(axis, major_diameter, across_flats, origin, depth,
profile_direction)`. Diameters, origin coordinates, and depth use model-length units;
`profile_direction` is unitless and `axis` is one of `x`, `y`, or `z`.

### Resolve accepted features to caller faces

When a consumer needs the exact faces behind accepted occurrences, use the separate within-run
evidence view:

```python
from quiddity.evidence import build_recognition_evidence

view = build_recognition_evidence(part)
coverage = view.association
print(coverage.face_count.associated, coverage.face_count.total)
print(coverage.surface_area.ratio)  # None only when the total area is zero
remaining_faces = [view.face(ref) for ref in coverage.unassociated_faces]
for feature in view.features:
    print(view.family(feature), view.record(feature).to_dict())
    proof_faces = [view.face(ref) for ref in view.defining_faces(feature)]
    feature_faces = [view.face(ref) for ref in view.constituent_faces(feature)]
```

`FeatureRef` keeps equal-valued occurrences distinct and `FaceRef` resolves to an original face
of the exact input part. Defining faces prove acceptance; constituent faces are the equal or wider
physical membership and do not participate in reconciliation. These opaque references are valid
only with their issuing view, cannot be
serialized, and are not persistent names across imports, transforms, edits, or separate runs.
The caller must not mutate the part while using the view. This entry point is explicitly
caller-coordinate/raw.

`view.association` accounts for the union of accepted constituent faces against every original
face. It reports face-count and surface-area totals, associated and unassociated values,
per-family union contributions, and the exact within-run references left unassociated. Family
contributions may overlap and are not additive. This is not an accuracy or recall score: accepted
classifications may be wrong, stock faces may intentionally remain unassociated, and incomplete
constituent publication produces incomplete association.

The evidence view uses unified `SectionRecess` records and may also return `SectionRecessRefusal`
for an accepted source association without reconstructible geometry. Check the record type before
consuming geometry. Refusal evidence contributes to association but not to the public geometric
`section_recess` census count.

For the ordinary framed lifecycle, use the paired view rather than running raw evidence after
framed recognition:

```python
from quiddity import FramedRecognitionEvidence
from quiddity import build_framed_recognition_evidence

framed = build_framed_recognition_evidence(part)
if isinstance(framed, FramedRecognitionEvidence):
    for ref in framed.association.unassociated_faces:
        local_face = framed.face(ref)  # exact face of framed.part
        original_face = framed.caller_face(ref)  # exact topology partner in part
```

The framed view owns its frame, exact local working part, caller part, result and evidence from
one aggregate run. Caller mapping applies the exact retained rigid placement to each caller face and
requires an `IsSame` topology bijection; it never matches by coordinate proximity or face order.

### Recognise independently of STEP placement

Use the opt-in framed route when the same physical part must produce local coordinates independent
of its placement in the imported file:

```python
from quiddity import FramedRecognitionResult, build_framed_recognition_result

framed = build_framed_recognition_result(part)
if isinstance(framed, FramedRecognitionResult):
    print(framed.frame.gauge)
    print(framed.part.bounding_box())  # the exact local shape used for recognition
    print(framed.result.holes)  # coordinates and axis letters are local to framed.frame
```

The paired `PartFrame` converts points in either direction with `to_local()` and `to_world()`.
`framed.part` is the exact topology-preserving local working shape passed to recognition, not a
consumer reconstruction. Its evaluated coordinates agree with `framed.result`, and
`framed.frame` converts between it and the caller's input coordinates. Keep the successful
`FramedRecognitionResult` alive while using topology-bearing recognition evidence: the result owns
the identity relationship between that evidence and `framed.part`; the original input shape is a
different caller-space object.
`FULL` means geometry establishes a directed, ordered basis. `ORTHOGONAL` exposes an unobservable
discrete sign or axis interchange, and `AXIAL` exposes unobservable roll. The axes returned for a
gauged frame are deterministic representatives and must not be treated as semantic material
directions. Geometry without an analytic direction returns a typed `RefusedPartFrame`.

This is the ordinary aggregate route for new integrations. If caller/world-coordinate records are
deliberately required, use the explicit `build_raw_recognition_result(part)` route. The historical
`build_recognition_result(part)` name remains a raw compatibility alias throughout 0.4.x and is
scheduled for removal in 0.5.0; it will not silently acquire a different return type.

Bounded explanations have the same paired lifecycle:

```python
from quiddity import FramedRecognitionReport, build_framed_recognition_report

framed_report = build_framed_recognition_report(part)
if isinstance(framed_report, FramedRecognitionReport):
    print(framed_report.report.families)
```

Use `build_raw_recognition_report(part)` only when the report and records intentionally remain in
caller coordinates. A typed frame refusal never falls back to either raw route automatically.

When classification itself depends on the normalized solid, prepare first and run the aggregate
once after making that local decision:

```python
from quiddity import PreparedFramedPart, prepare_framed_part

prepared = prepare_framed_part(part)
if isinstance(prepared, PreparedFramedPart):
    rotational = classify_local_part(prepared.part, prepared.cylinders)
    framed = prepared.recognise(rotational=rotational)
```

`PreparedFramedPart` owns the exact frame, local working shape, and one precomputed cylinder
inventory. Its `recognise()` method injects that inventory into the existing aggregate and pairs
the result with the same frame and shape. A `RefusedPartFrame` remains explicit, so callers may
choose one deliberate legacy fallback rather than silently guessing in caller coordinates.

Every `recognise_*` function returns a deterministic list of frozen dataclass records. Records
provide `to_dict()` projections containing only JSON-serialisable geometry values. The installed
package also exposes a versioned capability manifest so larger CAD systems can validate which
recognisers and record schemas they consume. See
[`docs/capabilities.md`](docs/capabilities.md) for the proven feature inventory and
[`docs/adr/0002-uniform-deterministic-recogniser-contract.md`](docs/adr/0002-uniform-deterministic-recogniser-contract.md)
for the complete contract.

### Project an aggregate step ladder

The aggregate owns the one geometry-only rule that chooses between Z-turned shoulders and already
filtered prismatic levels. Pass only the Z envelope it needs; no build123d object crosses this
projection boundary:

```python
z_min = part.bounding_box().min.Z
z_max = part.bounding_box().max.Z
step_zs = result.step_ladder_for_z_span(z_min, z_max)
```

The default `boundary_margin=0.6` is measured in model length units (normally millimetres) and
strictly excludes turned end faces at both ends. It can be overridden explicitly. The former
`result.step_ladder(bound_box)` call remains as a deprecated 0.2.x compatibility shim and will be
removed no earlier than 1.0.0. See
[`ADR 0006`](docs/adr/0006-explicit-step-ladder-z-span.md) for the caller inventory and boundary
decision.

## Scope

Feature recognition is deliberately separate from feature editing. This package reports geometric
facts; it does not mutate the source model, guess manufacturing intent, or prescribe a downstream
CAD representation. That boundary lets an editor, drawing engine, CAM tool, model checker, or
search/indexing service adopt the same recognition layer while retaining its own policy.

`b123d-recognisers` began as the recognition layer of
[Draftwright](https://github.com/pzfreo/draftwright), but the runtime package does not import
Draftwright and is designed for standalone use.

## Migrated behavior

The initial `0.1` release series preserves the recognition behavior of Draftwright commit
`3fe20b0f71a71deced06b310943dd44cc66e355e`. The migration includes every public recogniser,
shared cylinder/level substrates, the aggregate result, and `feature_census`. There are no feature
policy changes; one previously platform-dependent numerical axis tie is normalized to the pinned
baseline result. The checked-in semantic corpus records and continuously verifies the compatibility
boundary; see [`migration/PARITY.md`](migration/PARITY.md).

The dependency direction is:

```text
consumer → Quiddity → build123d/OCP
```

The runtime package does not import Draftwright and does not return build123d or OCP objects in
public feature records.

Contributors: see [Adding a recogniser](docs/adding-a-recogniser.md) for the AAG predicate,
candidate/evidence, registry, reconciliation, and verification path.

Maintainers: see [the release guide](docs/releasing.md) for the TestPyPI-first, OIDC-only
publication process.

## Licence

Apache License 2.0. See [`LICENSE`](LICENSE), [`NOTICE`](NOTICE), and
[`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).
