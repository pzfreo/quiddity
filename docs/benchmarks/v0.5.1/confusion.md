# Confusion analysis of the one-to-one scorer at v0.5.1 (3a20dceb), MFInstSeg test split

Derivation: a scratch example (`scripts/confusion.rs` plus `scripts/scratch_append.rs`, which is `examples/support/mfinstseg.rs` with one added function) that reuses the scorer's own `mapping()`, selectors, `attributed_faces` (superseded records dropped), face-set de-duplication and the same augmenting-path IoU >= 0.5 matching, returning which predictions and labelled instances matched; its per-class totals equal the scorer's (FP = predicted - TP, FN = labelled - TP) for all twelve rows below. Run on the droplet, 4 workers, 13 Nov 2026. Note: `step_levels` and `risers` count as "claims" in table (b) although the face-coverage "Any feature" metric excludes them.

Models analysed: 9367 (those the one-to-one scorer scores).

## (a) False-positive predictions: majority labelled class of their faces

A false positive is a prediction (one distinct defining-face set for the class) matched to no labelled instance of the class by the scorer's one-to-one IoU >= 0.5 matching.

| class | FP predictions | majority label of their faces (count) | source family (count) | also a prediction for another class | other classes sharing the record |
|---|---:|---|---|---:|---|
| rectangular_through_slot (6) | 371 | rectangular_passage 362, rectangular_through_step 6, rectangular_pocket 2, rectangular_through_slot 1 | channels 371 | 0 |  |
| circular_end_pocket (16) | 1424 | rectangular_pocket 555, v_circular_end_blind_slot 374, rectangular_through_slot 146, rectangular_passage 141, circular_end_pocket 100, rectangular_blind_slot 64, rectangular_blind_step 20, h_circular_end_blind_slot 8, other 16 | pockets 1424 | 1361 | rectangular_blind_step 1361, rectangular_pocket 1361 |
| rectangular_blind_step (22) | 1452 | rectangular_pocket 555, v_circular_end_blind_slot 374, rectangular_through_slot 146, rectangular_passage 141, rectangular_through_step 97, rectangular_blind_slot 70, circular_end_pocket 44, h_circular_end_blind_slot 8, other 17 | pockets 1452 | 1349 | circular_end_pocket 1349, rectangular_pocket 1349 |
| rectangular_pocket (14) | 1308 | v_circular_end_blind_slot 374, rectangular_pocket 353, rectangular_passage 162, rectangular_through_slot 146, rectangular_blind_slot 78, 6sides_pocket 62, circular_end_pocket 44, triangular_pocket 39, other 50 | pockets 1180, prismatic_pockets 128 | 1273 | circular_end_pocket 1165, rectangular_blind_step 1165, 6sides_pocket 108, triangular_pocket 108 |
| chamfer (0) | 583 | chamfer 220, triangular_blind_step 141, triangular_passage 90, 6sides_passage 40, triangular_through_slot 28, 2sides_through_step 24, slanted_through_step 18, triangular_pocket 11, other 11 | chamfers 557, oriented_chamfers 26 | 0 |  |
| blind_hole (12) | 616 | Oring 391, through_hole 216, blind_hole 5, circular_end_pocket 4 | holes 616 | 0 |  |

## (b) Missed labelled instances: what quiddity did with their faces

A missed instance is a labelled instance of the class matched by no prediction. Its faces are looked up in every family's defining faces (unmarked records, as the scorer). 'Top family' is the family defining most of its faces; '(nothing)' means no family defines any of its faces.

| class | missed instances | top family that claims its faces (count) | none of its faces claimed | fewer than half its faces claimed (incl. none) |
|---|---:|---|---:|---:|
| triangular_through_slot (5) | 350 | risers 144, (nothing) 95, prismatic_through_cuts 48, plates 40, chamfers 12, step_levels 9, angled_steps 2 | 95 | 102 |
| slanted_through_step (10) | 1565 | step_levels 518, risers 507, (nothing) 289, plates 133, pads 95, chamfers 12, angled_steps 5, edge_open_prismatic_recesses 2, other 4 | 289 | 538 |
| chamfer (0) | 1384 | (nothing) 801, prismatic_pockets 150, section_passages 133, chamfers 96, angled_steps 65, edge_open_prismatic_recesses 54, freeform_surfaces 27, countersinks 21, other 37 | 801 | 823 |
| rectangular_blind_slot (17) | 262 | step_levels 136, pockets 68, plates 28, (nothing) 17, edge_open_prismatic_recesses 7, slots 5, risers 1 | 17 | 113 |
| circular_through_slot (7) | 216 | (nothing) 185, cylindrical_profiles 15, step_levels 9, risers 4, holes 1, interior_voids 1, section_recesses 1 | 185 | 187 |
| rectangular_passage (3) | 1040 | prismatic_through_cuts 239, step_levels 159, channels 150, slots 129, (nothing) 94, pockets 63, section_recesses 62, risers 50, other 94 | 94 | 173 |
