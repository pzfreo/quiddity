# Draftwright review: unified section-recess JSON

**Status:** reviewed; superseded as a decision source by ADR 0019
**Audience:** Draftwright model, planner and rendering maintainers  
**Related recognisers work:** epic #290, issue #495, production PR #496

Draftwright's response accepted the direction and requested authoritative provider classification,
document-scoped face references, and a fail-closed classification/convention gate. Those decisions,
with the subsequent corrections on face identity, constant-section scope and bottom-condition
terminology, are recorded normatively in
[the section-recess family record](families/section-recess-json.md). This document remains the original review
request rather than being rewritten as the decision.

## Decision requested

Please review whether Draftwright can consume one domain-neutral, reconstructible section-recess
record in place of the growing set of shape-specific pocket and blind-slot records.

The proposal intentionally does not preserve the current pocket-record API. Draftwright is the
primary consumer and can move with the recognisers package. We would rather make one coordinated
breaking change now than accumulate permanent compatibility types while both projects are still
moving quickly.

This is a JSON contract. It must not expose build123d, OpenCascade, Draftwright IR, or any other
implementation-specific object.

## Problem

The recognisers package currently describes related subtractive geometry with several different
schemas:

- `Pocket`: principal-axis rectangular dimensions and spans;
- `PrismaticPocket`: principal run axis plus a polygonal section;
- `RectangularBlindSlot`: an edge-open rectangular U-section;
- `RoundBottomBlindSlot`: an edge-open rounded U-section;
- `EdgeOpenCircularPocket`: an interrupted line/arc section; and
- the proposed oriented circular-end pocket, which needs a free 3-D frame.

These distinctions partly reflect which recogniser found the geometry rather than what a consumer
needs to reconstruct or reason about it. Adding another bespoke oriented record would deepen that
split.

The recognisers may remain separate and conservative. The proposed change is to make their accepted
results converge on one geometric vocabulary.

## Proposed model

A constant-section subtractive occurrence is described by:

1. a free 3-D frame;
2. an increasing interval along the frame's run direction;
3. a truthful 2-D line/arc profile in the frame plane;
4. the physical condition at each run end; and
5. classification and source evidence outside the geometry value.

Illustrative closed-pocket JSON:

```json
{
  "type": "section_recess",
  "schema_version": 1,
  "geometry": {
    "frame": {
      "origin": [10.0, 20.0, 30.0],
      "run": [0.0, 0.0, -1.0],
      "u": [1.0, 0.0, 0.0],
      "v": [0.0, -1.0, 0.0]
    },
    "run_interval": [0.0, 8.0],
    "profile": {
      "closure": "closed",
      "segments": [
        {
          "kind": "line",
          "start": [-10.0, -5.0],
          "end": [10.0, -5.0]
        },
        {
          "kind": "arc",
          "start": [10.0, -5.0],
          "end": [10.0, 5.0],
          "center": [10.0, 0.0],
          "sweep": 3.14159265359
        },
        {
          "kind": "line",
          "start": [10.0, 5.0],
          "end": [-10.0, 5.0]
        },
        {
          "kind": "arc",
          "start": [-10.0, 5.0],
          "end": [-10.0, -5.0],
          "center": [-10.0, 0.0],
          "sweep": 3.14159265359
        }
      ],
      "openings": [],
      "islands": []
    },
    "ends": {
      "low": "floor",
      "high": "open"
    }
  },
  "classification": {
    "family": "pocket",
    "shape": "obround"
  },
  "evidence": {
    "defining_faces": ["face-12", "face-13", "face-14", "face-15"],
    "constituent_faces": ["face-12", "face-13", "face-14", "face-15", "face-16"]
  },
  "body": "body-1"
}
```

The boundary is continuous and closed. Arc radius is derivable from its centre and endpoints and
need not be duplicated. Canonical winding, start selection, numeric precision and frame validation
can reuse the existing `PassageFrame`/`PassageSection` rules. The final ADR may choose the existing
vertex-and-bulge encoding instead of explicit segment objects; this review asks about the geometric
contract, not that wire-format choice.

### Open profiles

An edge-open recess publishes only the walls that physically exist and the real opening between
the chain ends:

```json
{
  "closure": "open",
  "segments": [
    {"kind": "line", "start": [-5.0, 0.0], "end": [-5.0, -4.0]},
    {"kind": "line", "start": [-5.0, -4.0], "end": [5.0, -4.0]},
    {"kind": "line", "start": [5.0, -4.0], "end": [5.0, 0.0]}
  ],
  "opening": {
    "start": [5.0, 0.0],
    "end": [-5.0, 0.0]
  },
  "islands": []
}
```

The opening is not asserted to be a physical wall. A renderer or CAM adapter may close the removal
region temporarily against known stock or body geometry, but that construction must not be confused
with observed source geometry.

### End conditions

The same structural vocabulary can express:

| Geometry | Profile | Low end | High end |
| --- | --- | --- | --- |
| bounded blind pocket | closed | floor | open |
| reverse-facing blind pocket | closed | open | floor |
| edge-open blind recess | open | floor | open |
| through passage | closed | open | open |
| enclosed cavity | closed | floor | floor |

The first implementation need not admit every combination. Validation should reject combinations
that no recogniser can prove or no consumer can interpret.

## Classification is not geometry

`rectangular`, `circular`, `obround`, `triangular` and `hexagonal` should be derived classifications
of the profile, not separate foundational encodings. A specialised recogniser may still attach the
classification it proved, but consumers must be able to reconstruct the feature from `geometry`
without switching on `shape`.

Likewise, `pocket`, `edge_open_recess` and `passage` are useful engineering interpretations. They
should not change the meanings of frame, profile or end fields.

This separation lets different consumers use the same result:

- Draftwright can create declarations, plans and drawings;
- CAM can choose an approach and toolpath from boundary, depth and accessibility;
- viewers can highlight or reconstruct the removed region;
- quoting systems can calculate area, perimeter, volume and candidate tool sizes;
- defeaturing tools can use the source faces and base/floor relationship; and
- inspection tools can associate measurements with stable public face references.

## Treatments and interactions

Chamfers, blends, wall draft and bottom radii should not create new pocket schema classes. They
should be represented either by the exact profile geometry or as explicit related treatments when
the recogniser has proved them:

```json
{
  "treatments": {
    "mouth_chamfers": [],
    "bottom_blends": [],
    "wall_draft": null
  }
}
```

This section is deliberately provisional. Please advise which of these Draftwright needs as
first-class IR and which can remain evidence relationships. CAM requirements should be validated
before treatment fields are frozen.

Intersections should not be repaired by inventing an ideal feature boundary. Defining and
constituent face evidence remains attached to the occurrence, and incomplete geometry should fail
closed unless the complete removal volume can be proved from real boundaries and material tests.

## Proposed migration

Because compatibility is not a goal for this change, the proposed coordinated migration is:

1. agree the JSON structure and validation rules with Draftwright;
2. add the neutral record to the recognisers package;
3. make the existing specialised recognisers emit it;
4. update Draftwright's adapter, IR, planner and renderer in the same development window;
5. replace golden JSON and end-to-end examples;
6. remove superseded public pocket and blind-slot records rather than maintaining adapters; and
7. resume the oriented circular-pocket production work against the unified record.

Recognition behavior need not change in the schema migration. Existing results should map to the
new geometry first; recall improvements can then continue independently.

## Expected Draftwright impact

The likely work is concentrated in four places:

- decode and validate `section_recess` JSON;
- introduce one section/profile-oriented IR value;
- generate declarations or construction operations from the profile and run interval; and
- render line/arc profiles, their opening, floor and orientation in drawings.

Some current convenience paths based on `width`, `length` and named axes will need to derive those
values from rectangular profiles. That is intentional: those fields are conveniences, not universal
pocket geometry.

The change should reduce later Draftwright work because polygonal, circular-ended and oriented
pockets no longer require new adapters and IR classes for every recogniser family.

## Questions for Draftwright

Please answer these before the recognisers package accepts an ADR:

1. Can one `section_recess` geometry support the current declaration, planner and renderer paths?
2. Does Draftwright need `family` to be normative, or can it derive pocket/passage/open-recess from
   profile closure and end conditions?
3. Which convenience dimensions must be present in JSON rather than derived by Draftwright?
4. Can Draftwright consume arbitrary line/arc profiles and free 3-D frames without axis-specific IR?
5. For an open profile, what stock/body context does the planner need to construct a machining or
   rendering region without treating the opening as a wall?
6. Are islands needed in the first schema version? If so, is a list of closed profiles sufficient?
7. Which treatments must affect planning immediately: mouth chamfers, bottom blends, wall draft or
   bottom condition?
8. Does the renderer need both defining and constituent face references, or only the occurrence and
   reconstructible geometry?
9. Would including through passages in the same structural record simplify or complicate the
   Draftwright model? The shared primitives do not require one public semantic family.
10. Is there any Draftwright workflow that cannot tolerate removal of the old pocket/slot JSON in a
    coordinated version update?

## Acceptance direction

If Draftwright confirms the model is sufficient, the recognisers team should write a short ADR that
freezes only:

- the neutral frame;
- line/arc closed and open profiles;
- run interval and admitted end conditions;
- separation of geometry, classification and evidence; and
- the initial treatment/island scope agreed with Draftwright.

The ADR should not prescribe recogniser algorithms, Draftwright implementation classes, CAM
strategies, or build123d reconstruction code.
