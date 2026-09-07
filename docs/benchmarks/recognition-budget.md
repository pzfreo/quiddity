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

Measured at `740184c` on an Apple M5 Max (macOS 26.6, Python 3.14.7, build123d 0.11.1). That is
a **shared** developer machine, so these are minimums over repeated samples rather than medians,
taken with the one-minute load average below 4: the median moves with whatever else happens to
be running, and the minimum is the closest available reading of the machine's own answer. Peak
resident set is the whole process, so it includes the kernel's C++ allocations that
`tracemalloc` cannot see.

| Workload | Iterations | Minimum | Peak RSS |
| --- | ---: | ---: | ---: |
| `composite` | 5 | 0.851 s | 500 MB |
| `census` | 3 | 11.990 s | 592 MB |

`740184c` is the head of the three-PR round-2 series (`shared_occurrences` grouping, support-cut
and plate-loop skips, volume-probe short-circuits); the PR that records these numbers adds no
library code of its own, which is why the commit named here is the last one that changed any.

**These seconds are comparable with the ones they replace, and that is new.** The previous
recording was taken on this same box under this same protocol, and re-measuring the branch point
confirms it: `ac1543e` — the merge of the round-1 stack, recorded above as 16.022 s and 0.863 s —
read **15.896 s and 0.858 s** here, within 0.8% and 0.6% of what it recorded. So the drop below
is a code change and nothing else. Both arms were run alternating between the two checkouts in
one quiet window, and the rows above are that window's readings rather than the best seen
anywhere, so that the seconds and the ratio are one measurement:

| Workload | `ac1543e` | this stack | |
| --- | ---: | ---: | ---: |
| `composite` | 0.858 s | 0.851 s | x1.01 |
| `census` | 15.896 s | 11.990 s | **x1.33** |

Two earlier paired windows read 12.170 s against 15.893 s and 12.186 s against 15.991 s — the
same census ratio to three figures — and 0.849 s and 0.852 s against 0.855 s on the composite.
Over the five windows that stayed quiet throughout, this arm's own readings span 11.982 s to
12.186 s, 1.7%.

**The composite arm did not move, and that is the expected answer rather than a disappointment.**
Round 2's three changes are in wide-face adjacency grouping, support cuts that cannot reach, and
volume-probe booleans; four small golden fixtures barely reach any of them, and x1.01 is well
inside this arm's own 5.6% spread. Read it as *unchanged*, not as a 1% gain. The census arm is
the one round 2 is visible in.

Round 1 was measured the same way against `a5f1fcc`, its own branch point, at 75.032 s and
0.972 s. Chaining the two recordings — legitimate here only because the intermediate revision
re-measures to within 1% — puts the two rounds together at **x6.3** on the census arm and x1.14
on the composite. **The ratios are good to about this box's own spread and no further**, which is
why the interesting digit is the 6, not the 3.

**The census arm's headroom is now 1.2 seconds, and that is tighter than it has ever been.**
1.10 of 11.990 s is a **13.189 s** ceiling, against 1.6 s of slack at sixteen seconds and ten
seconds of slack on the hundred-second arm this file started with. The evidence for what that
costs is in this re-baselining itself. The first window taken for it **passed** the load check —
one-minute load 2.9 before the run — and measured the census arm at **13.116 s** on this branch:
99% of its own new ceiling, and 9.4% above what the same code gave in quieter windows minutes
later. The load was 4.9 by the time the run ended. Nothing had got slower; the box had got
busier while the run was in it. The previous recording watched the same effect push a reading
**OVER** (17.963 s against a 17.624 s ceiling, at load ~6).

So the load-under-4 protocol above was close to a nicety at a hundred seconds and is now a
precondition — and checking `uptime` before the run is no longer sufficient on its own. Check it
**after** as well, and throw the reading away if the window did not stay quiet through it. A
single OVER taken on a loaded machine is evidence about the machine. Re-run it quiet before
believing it, and if it is reproducibly over on a quiet box, that is the signal the ratio exists
for.

The budget is **1.10** by default, and a workload may record its own. Two arms of very
different length cannot share one ceiling on a shared box:

| Workload | Budget | Why |
| --- | ---: | --- |
| `census` | 1.10 | twelve seconds, stable to 1.7% across quiet windows here, and the arm every recognition change is felt in |
| `composite` | 1.40 | under a second, and inherited: on the host it was sized on its minimum-of-five ranged 1.93 s to 2.66 s over one evening with nothing else obviously running |

The composite figure is loose because of the host it was sized on, not because the code is
allowed to be forty percent slower. This box is steadier — six quiet windows during this
re-baselining spanned 0.839 s to 0.886 s, a 5.6% spread rather than the 1.38x the ceiling was
drawn under, and in the same range as the 4.1% the previous recording measured on it. Two
recordings agreeing on the order of the spread is better evidence than one, but it is still a
machine other work shares, a check that cries wolf stops being run, and the arm this ceiling
guards is not the one that moves; so the ratio is left where it is and the reason for revisiting
it is recorded here instead. **The census arm is the one to trust for a regression**: it is
fourteen times longer, it moved by 4.68x under the round-1 caching series and 1.33x under round
2 while the composite arm sat still through the second of those, and it is the arm the
one-inventory consolidation cost.

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
