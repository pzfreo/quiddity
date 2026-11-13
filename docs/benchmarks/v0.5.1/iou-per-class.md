
### instances (one-to-one IoU>=0.5 for instances; defining faces for faces)
id | class | TP | FP | FN | precision | recall | F1
0 | chamfer | 1630 | 583 | 1384 | 0.737 | 0.541 | 0.624
1 | through_hole | 2871 | 121 | 184 | 0.960 | 0.940 | 0.950
2 | triangular_passage | 2466 | 345 | 659 | 0.877 | 0.789 | 0.831
3 | rectangular_passage | 2033 | 247 | 1040 | 0.892 | 0.662 | 0.760
4 | 6sides_passage | 2328 | 229 | 720 | 0.910 | 0.764 | 0.831
5 | triangular_through_slot | 292 | 54 | 350 | 0.844 | 0.455 | 0.591
6 | rectangular_through_slot | 406 | 371 | 219 | 0.523 | 0.650 | 0.579
7 | circular_through_slot | 387 | 40 | 216 | 0.906 | 0.642 | 0.751
8 | rectangular_through_step | 2150 | 10 | 1036 | 0.995 | 0.675 | 0.804
9 | 2sides_through_step | 2470 | 0 | 694 | 1.000 | 0.781 | 0.877
10 | slanted_through_step | 1489 | 1 | 1565 | 0.999 | 0.488 | 0.655
11 | Oring | 2637 | 0 | 464 | 1.000 | 0.850 | 0.919
12 | blind_hole | 2963 | 616 | 57 | 0.828 | 0.981 | 0.898
13 | triangular_pocket | 2676 | 192 | 331 | 0.933 | 0.890 | 0.911
14 | rectangular_pocket | 2651 | 1308 | 513 | 0.670 | 0.838 | 0.744
15 | 6sides_pocket | 2545 | 130 | 470 | 0.951 | 0.844 | 0.895
16 | circular_end_pocket | 2511 | 1424 | 581 | 0.638 | 0.812 | 0.715
17 | rectangular_blind_slot | 337 | 15 | 262 | 0.957 | 0.563 | 0.709
18 | v_circular_end_blind_slot | 468 | 15 | 136 | 0.969 | 0.775 | 0.861
19 | h_circular_end_blind_slot | 458 | 0 | 169 | 1.000 | 0.730 | 0.844
20 | triangular_blind_step | 2291 | 100 | 902 | 0.958 | 0.718 | 0.821
21 | circular_blind_step | 2500 | 0 | 646 | 1.000 | 0.795 | 0.886
22 | rectangular_blind_step | 2522 | 1452 | 648 | 0.635 | 0.796 | 0.706
23 | round | 2552 | 161 | 370 | 0.941 | 0.873 | 0.906
all mapped | | 45633 | 7414 | 13616 | 0.860 | 0.770 | 0.813

### faces (one-to-one IoU>=0.5 for instances; defining faces for faces)
id | class | TP | FP | FN | precision | recall | F1
0 | chamfer | 1850 | 363 | 1717 | 0.836 | 0.519 | 0.640
1 | through_hole | 3129 | 6 | 305 | 0.998 | 0.911 | 0.953
2 | triangular_passage | 8123 | 1500 | 2283 | 0.844 | 0.781 | 0.811
3 | rectangular_passage | 8931 | 1277 | 4684 | 0.875 | 0.656 | 0.750
4 | 6sides_passage | 15287 | 914 | 4885 | 0.944 | 0.758 | 0.841
5 | triangular_through_slot | 604 | 88 | 824 | 0.873 | 0.423 | 0.570
6 | rectangular_through_slot | 814 | 694 | 1324 | 0.540 | 0.381 | 0.447
7 | circular_through_slot | 403 | 30 | 297 | 0.931 | 0.576 | 0.711
8 | rectangular_through_step | 4305 | 18 | 2554 | 0.996 | 0.628 | 0.770
9 | 2sides_through_step | 7416 | 0 | 2635 | 1.000 | 0.738 | 0.849
10 | slanted_through_step | 2978 | 2 | 3635 | 0.999 | 0.450 | 0.621
11 | Oring | 7911 | 0 | 1812 | 1.000 | 0.814 | 0.897
12 | blind_hole | 5885 | 851 | 420 | 0.874 | 0.933 | 0.903
13 | triangular_pocket | 10694 | 1119 | 1414 | 0.905 | 0.883 | 0.894
14 | rectangular_pocket | 13730 | 2566 | 2325 | 0.843 | 0.855 | 0.849
15 | 6sides_pocket | 17820 | 828 | 3230 | 0.956 | 0.847 | 0.898
16 | circular_end_pocket | 12602 | 3019 | 2924 | 0.807 | 0.812 | 0.809
17 | rectangular_blind_slot | 1342 | 66 | 1112 | 0.953 | 0.547 | 0.695
18 | v_circular_end_blind_slot | 1872 | 60 | 592 | 0.969 | 0.760 | 0.852
19 | h_circular_end_blind_slot | 1832 | 0 | 729 | 1.000 | 0.715 | 0.834
20 | triangular_blind_step | 4593 | 100 | 2077 | 0.979 | 0.689 | 0.808
21 | circular_blind_step | 5002 | 0 | 1485 | 1.000 | 0.771 | 0.871
22 | rectangular_blind_step | 7571 | 3365 | 2304 | 0.692 | 0.767 | 0.728
23 | round | 2714 | 6 | 701 | 0.998 | 0.795 | 0.885
all mapped | | 147408 | 16872 | 46268 | 0.897 | 0.761 | 0.824

unmapped: [(24, 'stock')]
models 9375 face_scored 9371 instance_scored 9367
