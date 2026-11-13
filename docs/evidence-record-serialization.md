# Reusing the physical record JSON within one evidence document

`RecognitionEvidence::record` serializes the complete physical `Features` value to
select one family's record. That individual accessor remains unchanged. Calling it
for every physical record during document construction repeated the serialization
of all families each time.

Document construction now initializes that exact JSON tree on the first physical
record and clones the selected value from it for later physical records. The cache
exists only during construction of one document. Section recesses and their source
refusals retain their original accessor because their records live in a different
projection. A document with no physical records does not initialize the tree.

Record order, family-field mapping, numeric serialization, face evidence and member
references remain unchanged. Each physical record is selected from serialization
of the same immutable value with the same serializer and index. Work proportional
to the entire physical inventory is performed once, rather than once per feature;
the selected record values are still cloned as before. The retained intermediate
tree is released after document construction.

The targeted regression compares every record's serialized bytes with the unchanged
individual accessor, including hole patterns, native recesses, freeform faces and
open-shell inputs. Complete document byte comparisons and whole-part timing remain
separate acceptance checks.

The cache lives in `document_from_view`, used by both the separate evidence call
and shared recognition/evidence analysis. The regression includes body-owned,
open-shell and mixed controls, and requires a positive source-refusal count.
