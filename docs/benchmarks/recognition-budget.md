# Recognition runtime budget

Two numbers this package is allowed to get slower than, and the workloads they are measured on.
Recorded because the one-inventory consolidation made `feature_census` about 35% slower and the
release workload about 4% slower, and a regression that size is only visible if there is
something to compare it with.

## The two workloads, and why there are two

| Workload | What it runs | Who pays it |
| --- | --- | --- |
| `composite` | two recognition results and one census over four golden fixtures | the release contract, and the shape the Draftwright comparison in `migration/PARITY.md` was made on |
| `census` | `feature_census` over the ten vendored NIST parts and three real gramel parts | a corpus sweep, and where the consolidation was paid for |

Quoting only the composite figure understates what a `feature_census` caller pays; quoting only
the census figure overstates what a consumer of the library pays. Both are recorded, and a claim
about performance that names one should say which.

## The recorded baseline

Measured at `8147f39` on an Apple M5 Max (macOS 26.6, Python 3.14.7, build123d 0.11.1). That is
a **shared** developer machine, so these are minimums over repeated samples rather than medians,
taken with the one-minute load average below 4: the median moves with whatever else happens to
be running, and the minimum is the closest available reading of the machine's own answer. Peak
resident set is the whole process, so it includes the kernel's C++ allocations that
`tracemalloc` cannot see.

| Workload | Iterations | Minimum | Peak RSS |
| --- | ---: | ---: | ---: |
| `composite` | 5 | 0.863 s | 498 MB |
| `census` | 3 | 16.022 s | 577 MB |

`8147f39` is the head of the five-PR run-scoped-caching series; the PR that records these
numbers adds no library code of its own, which is why the commit named here is the last one that
changed any.

**These seconds are not comparable with the ones they replace.** The previous recording was
taken on a different host as well as different code, so the drop from 99.683 s is a host change
and a code change added together and cannot be split by subtraction. The same two commands were
therefore also run on `a5f1fcc`, the branch point, on this box in the adjacent quiet window, and
that pair is the comparison that means something:

| Workload | `a5f1fcc` | this stack | |
| --- | ---: | ---: | ---: |
| `composite` | 0.972 s | 0.863 s | x1.13 |
| `census` | 75.032 s | 16.022 s | **x4.68** |

The `a5f1fcc` pair was taken against `7918c8a`, the revision immediately before the one recorded
above, which measured 0.866 s and 16.295 s — within this box's own run-to-run spread of the rows
in the table, and the reason the ratios are quoted to two figures and not three.

The budget is **1.10** by default, and a workload may record its own. Two arms of very
different length cannot share one ceiling on a shared box:

| Workload | Budget | Why |
| --- | ---: | --- |
| `census` | 1.10 | sixteen seconds, stable here, and the arm every recognition change is felt in |
| `composite` | 1.40 | under a second, and inherited: on the previous host its minimum-of-five ranged 1.93 s to 2.66 s over one evening with nothing else obviously running |

The composite figure is loose because of the host it was sized on, not because the code is
allowed to be forty percent slower. This box is steadier — three quiet windows over two hours,
on adjacent revisions of this stack, gave 0.863 s, 0.866 s and 0.898 s, a 4.1% spread rather
than the 1.38x the ceiling was drawn under. Three windows on a machine other work shares is not
enough evidence to tighten a ceiling on, and a check that cries wolf stops being run, so the
ratio is left where it is and the reason for revisiting it is recorded here instead. **The
census arm is the one to trust for a regression**: it is nineteen times longer, it moved by
4.68x under the caching series, and it is the arm the one-inventory consolidation cost.

**The `budget` fields in the JSON are the authority**: `--check` reads its ceiling from there --
a workload's own if it has one, the file's default otherwise -- so editing the policy changes
what is enforced. `--budget` on the command line overrides both for a one-off question and
defaults to not overriding.

## Running it

```
uv run python tools/benchmark_recognition.py --implementation package --workload census \
    --iterations 3 --check docs/benchmarks/recognition-budget.json
```

Exits non-zero when the measured minimum is over the ceiling, and prints both numbers either
way.

Peak RSS is reported but not checked. `getrusage` reports kibibytes on Linux and bytes on
macOS, so the macOS reading is converted to match the field name; Windows has no `resource`
module and the field comes back null there rather than wrong. The seconds are what the
budget is about.

**Not run in CI, deliberately.** A wall-clock assertion on a shared runner fails for reasons
that have nothing to do with the code, and a test that fails for unrelated reasons stops being
read. This is a tool to run when a change is expected to cost something, and a number to update
when it legitimately does — with the reason recorded in the commit that moves it.

The baseline is tied to the machine it was taken on. Re-measure both workloads on any other
box before comparing against it; the *ratio* between two arms measured back to back on the same
box is the portable part, not the seconds.

## Issue #173 post-consolidation A/B

Historical, and on the epic development container rather than the host the baseline above was
re-measured on. The seconds do not compare with anything current; the paired directions do.

Measured on that shared development host, alternating the pre-epic baseline `ccf3b8c` and
post-epic `d73f612` processes. Two five-sample composite runs crossed directions: current was
3.5% faster by minimum in one pair (2.628 s versus 2.725 s) and 1.8% slower in the other
(2.938 s versus 2.887 s). That is host noise, not a reproducible regression.

The trustworthy census arm was also alternated. Current was faster in the first pair
(133.253 s versus 152.556 s) and effectively tied in the second (146.100 s versus 146.137 s).
No budget or implementation change is justified by these measurements.
