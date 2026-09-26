# CADGenBench public editing inputs

These are unmodified starting STEP solids from the public
[CADGenBench input dataset](https://huggingface.co/datasets/HuggingAI4Engineering/cadgenbench-data),
released under ODC-BY. CADGenBench's solved ground truth is held out and is not present here.
The geometry was sourced by the dataset maintainers from Mecado. These files are regression
inputs for Quiddity issues #747, #751 and #755 and are excluded from the published package.

| File | Dataset path | SHA-256 |
|---|---|---|
| `cgb241.step` | `241/input.step` | `80fb0fbffea33913b71ac99af89b14bc2930b0f8d3cd9d454aea54d986213a7d` |
| `cgb203.step` | `203/input.step` | `2a029868cd4517774379695ec5c5928a98a7d10ff999520c0dbce6c3cf79935e` |
| `cgb207.step` | `207/input.step` | `0b2c689b1ec09c362c334a9b6c9714a5e4d658295f7f16078f5588472721dd26` |
| `cgb202.step.gz` | `202/input.step` | `661210dec702f8347603884bec0476da0a1af5376a55bf842c45ba25181e2860` (decompressed) |

The case 202 input is gzip compressed without changing the STEP bytes. Its one unorientable
1.7366 mm² face exercises bounded document degradation while distant holes remain readable.
