# Multi-solid and void-shell STEP files (#300)

- `sphere_and_box.step.gz` and `brep_with_voids.step.gz` are copied unchanged, gzipped
  (`mtime=0`), from springcraft-ai/brep-conformance 1.0.0 (MIT licence,
  https://github.com/springcraft-ai/brep-conformance). The first holds two
  `MANIFOLD_SOLID_BREP`s, a sphere and a box; the second one `BREP_WITH_VOIDS` with one void
  shell, written by OpenCascade.
- `two_boxes.step.gz` is derived from the box of `sphere_and_box.step.gz` (same licence): the
  sphere is dropped from the representation and the box's entities are copied once more
  with every 3D `CARTESIAN_POINT` moved by +60 along X, giving two separate boxes. Its frame
  is inferred, so the document is not frame-refused.
