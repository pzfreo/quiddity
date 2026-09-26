# Sheet-metal edge treatments and tab development (#758)

The vendored CADGenBench cgb245 enclosure has a measured 2 mm wall. Its paired skins are
predominantly planar or cylindrical; small spherical and B-spline corner patches make up less
than 0.5% of their area. The recogniser retains 26 flange groups, 31 bend groups and 62
radius-3 mm cylindrical cut-contour faces, along with other local edge treatments. The bend
graph is not a tree, so `flat_pattern` is absent and `flat_pattern_status` is `non_tree`.
This is a bounded sheet geometry reading, not a claim that the enclosure has a proved
single-blank unfolding or a unique manufacturing history.

Too Tall Toby's `sm-hanger.step` has a measured 4 mm wall. Both previously isolated tabs
now appear among nine flanges and eight bends, with no remaining `formed_features`. The
neutral development includes their source faces and bend strips. Its transformed triangles
overlap the main flange, so `flat_pattern.valid_blank` is false and the plan retains the
overlap witnesses. Consumers can inspect the tab geometry without treating this development
as a manufacturable blank.

The fixture and source-face identities are checked in `tests/test_sheet_metal.py`. The
overlap area on each witness is one intersecting triangle pair; summing witnesses does not
measure the total overlapped area.
