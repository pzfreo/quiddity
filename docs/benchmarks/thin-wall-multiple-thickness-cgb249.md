# Multiple wall thicknesses and CADGenBench cgb249

Issue #757 adds a measured offset to each `WallFacePair`. A shell with 2 mm side
walls and a 4 mm floor now returns one `ThinWallBody` with five pairs and both
thicknesses. The body-level `thickness` field remains the dominant class for
existing consumers; reconstruction should read each pair's offset.

The public CADGenBench `249/input.step` volute is still refused. On that input
the current recogniser sees 283 faces on one solid. Its material-normal ray
samples contain distinct distance groups, but reciprocal, locally constant
offsets yield only six accepted pairs across the eligible classes. Those faces
cover 0.330 of the body's total face area, below the existing 0.85 threshold
for a body-level wall reading. Its `2V/A` value is 9.449 mm, which is not a
reliable thickness measurement for the spiral channel. Assigning a thickness
to the remaining surfaces would claim wall correspondence the geometry has
not proved.

This result describes the present proof's limit, not an assertion that the
volute has no thin regions. The input is a public editing solid from the
[CADGenBench dataset](https://huggingface.co/datasets/HuggingAI4Engineering/cadgenbench-data);
the dataset provenance and licence are recorded in the
[corpus notice](../../tests/corpus/cadgenbench_inputs/README.md).
