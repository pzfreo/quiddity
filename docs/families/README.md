# Family records

Each record here decides the geometric contract of one recogniser family or one published value.
They were ADRs until 2026-09-15; the numbers stay citeable through stubs in `docs/adr/`, and the
text moved verbatim. Read the record for the family you are changing. None is needed to
understand the architecture, which is the eight live records in [`docs/adr/`](../adr/README.md).

A new family that needs a design record adds a file here and a row below, not an ADR.

| Record | Former ADR | Modules | Status |
| --- | --- | --- | --- |
| [Explicit step-ladder Z-span boundary](step-ladder-z-span.md) | 0006 | `levels` | Accepted |
| [Publish complete blend chains separately from dimension-worthy Fillets](blend-chains.md) | 0013 | `blends` | Accepted |
| [Represent planar Passage terminations in the section frame](planar-passage-ends.md) | 0016 | `passages`, `_section_passages` | Accepted |
| [Preserve an edge-open polygonal recess as an open profile](edge-open-polygonal-recess.md) | 0018 | `edge_open_prismatic_recesses` | Accepted |
| [Unify constant-section recesses in one JSON geometry](section-recess-json.md) | 0019 | `section_recesses`, `document` | Accepted |
| [Native cylindrical SectionRecess ends](cylindrical-section-ends.md) | 0020 | `_cylindrical_pockets`, `_cylindrical_end_surface` | Accepted |
| [Independently proved interior support apertures](interior-support-apertures.md) | 0021 | `_support_apertures` | Accepted |
| [Observed cylindrical channel terminations](cylindrical-channel-ends.md) | 0022 | `_cylindrical_channels` | Proposed |
| [Polygonal passages ending on an observed bore](cylindrical-passage-ends.md) | 0023 | `_cylindrical_passages` | Accepted |
| [Observed convex two-plane passage ends](plane-envelope-passage-ends.md) | 0024 | `_plane_envelope_passages` | Accepted |
| [Body-owned planar outer-profile inspection](planar-outer-profile.md) | 0025 | `_outer_profile_geometry`, `inspection` | Accepted |
