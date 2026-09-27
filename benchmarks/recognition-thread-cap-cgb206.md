# Recognition thread cap on CADGenBench 206

Issue [#770](https://github.com/pzfreo/quiddity/issues/770) reports six concurrent
recognitions taking about six times as long per part on an 18-core Mac. This check uses
[CADGenBench editing input 206](https://huggingface.co/datasets/HuggingAI4Engineering/cadgenbench-data/tree/main/206),
`input.step` SHA-256
`7f32ef2ce2202caafd7565499f5de82ebcf16128b13917ca0acc31ee4765a867`.

Each process loaded the STEP file, then timed `build_recognition_document(part)` with
`QUIDDITY_THREADS=1`, `OPENBLAS_NUM_THREADS=1`, `OMP_NUM_THREADS=1`,
`MKL_NUM_THREADS=1`, `TBB_NUM_THREADS=1`, and `VECLIB_MAXIMUM_THREADS=1` set before Python
started. The OCCT pool and its per-launch default both reported one thread. Each document
contained 270 features.

| Run | Recognition wall time |
| --- | ---: |
| One process alone | 172.58 s |
| Three processes together | 185.54 s, 185.27 s, 185.47 s |

The concurrent runs were about 7.4% slower per part than the single run, within a total
three-core local budget. This verifies the capped path under concurrent load. It does not
replicate the issue's six-process, 18-core environment; the local work was limited to at most
four cores. Timing is hardware- and workload-dependent, so this is a measured case rather
than a performance guarantee.
