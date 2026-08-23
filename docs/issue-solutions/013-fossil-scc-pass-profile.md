# Fossil SCC Pass 1 vs Pass 2 Profile

- **Branch:** `investigation/fossil-excel-calculation`
- **Status:** Diagnostic profiling complete; no calculation semantic change
- **Scope:** Fossil main SCC, F7/no-op controls, small synthetic SCC
- **Profiler:** `FZ_TRACE_SCC_PASS_PROFILE=1`
- **Raw data:** `docs/issue-solutions/data/scc-pass-profile.json`

## Executive correction

The previously reported:

```text
pass 1: ~156 ms
pass 2: ~5.1–5.2 s
```

was not a real pass-1/pass-2 comparison. The old iteration trace initialized `pass_started` after pass 1 had already completed. Its pass-1 value measured post-evaluation drain/analysis rather than pass-1 formula evaluation.

With the timer moved before pass 1, an instrumentation-disabled iteration-trace control reports:

| Phase | Pass 1 | Pass 2 | Pass 3 | Pass 4 |
| --- | ---: | ---: | ---: | ---: |
| Initial | 5.611 s | 5.770 s | 5.976 s | 5.599 s |
| F7 edit | 5.573 s | 5.410 s | — | — |
| No-op | 5.089 s | 5.128 s | — | — |

The correct conclusion is:

```text
There is no general 5-second pass-2 penalty.
Both passes are expensive because both execute the same expensive member path.
```

## Instrumentation

The opt-in profiler records:

```text
per-member formula evaluation time
scalar reads
range reads
resolved range volume
range membership checks
named reads
internal SCC target events
live-edge collection time
lookup-index cache builds/hits/misses
dynamic-source member/read counts
dirty-propagation visits
live-edge analysis time
convergence comparison time
post-evaluation bookkeeping time
```

It does not replace the global allocator or alter allocator/linker configuration. Direct allocation-byte measurement is therefore intentionally not claimed.

`range_cells` is the resolved requested view area, not a claim that every cell was individually materialized by the Arrow kernel. It is a range-volume proxy.

## Main Fossil SCC profile

Stable ID:

```text
1321560910633541638
```

### Initial evaluation

| Metric | Pass 1 | Pass 2 | Pass 3 | Pass 4 |
| --- | ---: | ---: | ---: | ---: |
| Evaluated members | 4,829 | 4,828 | 4,828 | 4,828 |
| Total pass elapsed | 5,520 ms | 5,641 ms | 6,330 ms | 5,963 ms |
| Formula evaluation | 5,419 ms | 5,545 ms | 6,231 ms | 5,863 ms |
| Post-eval bookkeeping | 93.5 ms | 88.1 ms | 90.2 ms | 89.9 ms |
| Live-edge analysis | 9.0 ms | 8.8 ms | 9.3 ms | 5.8 ms |
| Convergence comparison | 0 | 0.080 ms | 0.124 ms | 0.127 ms |
| Scalar reads | 31,861 | 31,981 | 31,981 | 31,981 |
| Range reads | 238,751 | 238,839 | 238,839 | 238,839 |
| Requested range volume | 198,939,780 | 198,951,859 | 198,951,859 | 198,951,859 |
| Range membership checks | 1,151,973,575 | 1,152,398,175 | 1,152,398,175 | 1,152,398,175 |
| Named reads | 189,215 | 189,258 | 189,258 | 189,258 |
| Internal target events | 60,327,133 | 60,326,139 | 60,326,139 | 60,326,139 |
| Read events | 459,827 | 460,078 | 460,078 | 460,078 |
| Dynamic-source members | 270 | 269 | 269 | 269 |
| Dynamic-source read events | 11,637 | 11,832 | 11,832 | 11,832 |
| Live-edge collection time | 1,106 ms | 1,074 ms | 1,191 ms | 1,119 ms |
| Lookup builds/hits/misses | 1/8/12 | 0/12/8 | 0/12/8 | 0/12/8 |
| Dirty propagation visits | 0 | 0 | 0 | 0 |

The pass-2 formula work is not qualitatively different:

```text
range reads:              +88
range volume:             +12,079 cells
named reads:              +43
internal targets:         -994
live-edge collection:     -32 ms
lookup cache:             better in pass 2
```

The large pass cost is present in both passes.

### F7 edit

| Metric | Pass 1 | Pass 2 |
| --- | ---: | ---: |
| Total pass elapsed | 6,550 ms | 6,454 ms |
| Formula evaluation | 6,438 ms | 6,346 ms |
| Post-eval bookkeeping | 99.4 ms | 98.7 ms |
| Live-edge analysis | 9.7 ms | 9.9 ms |
| Convergence comparison | 0 | 0.130 ms |
| Range reads | 238,839 | 238,839 |
| Requested range volume | 198,951,859 | 198,951,859 |
| Range membership checks | 1,152,398,175 | 1,152,398,175 |
| Named reads | 189,258 | 189,258 |
| Internal target events | 60,326,139 | 60,326,139 |
| Live-edge collection | 1,190 ms | 1,193 ms |
| Dirty propagation visits | 0 | 0 |

Pass 2 is slightly faster than pass 1 in this run. This rules out an intrinsic `iterating=true` pass-2 slowdown.

### No-op

| Metric | Pass 1 | Pass 2 |
| --- | ---: | ---: |
| Total pass elapsed | 5,878 ms | 5,812 ms |
| Formula evaluation | 5,776 ms | 5,709 ms |
| Post-eval bookkeeping | 92.8 ms | 95.0 ms |
| Live-edge analysis | 9.4 ms | 9.7 ms |
| Convergence comparison | 0 | 0.143 ms |
| Range reads | 238,839 | 238,839 |
| Requested range volume | 198,951,859 | 198,951,859 |
| Range membership checks | 1,152,398,175 | 1,152,398,175 |
| Named reads | 189,258 | 189,258 |
| Internal target events | 60,326,139 | 60,326,139 |
| Live-edge collection | 1,126 ms | 1,116 ms |
| Dirty propagation visits | 0 | 0 |

No-op pass 2 is 66 ms faster in this profile run.

## Top slowest members

The slowest repeated member family is on `CashFlow Inputs`:

```text
CashFlow Inputs!$AK$221
CashFlow Inputs!$AL$221
CashFlow Inputs!$AL$163
CashFlow Inputs!$AM$134
CashFlow Inputs!$AA$120
```

Representative timings per pass:

| Member | Typical eval time | Range reads | Requested range volume | Named reads | Collector time |
| --- | ---: | ---: | ---: | ---: | ---: |
| `CashFlow Inputs!$AK$221` | 25–31 ms | 81 | 59,777 | 65 | 25–29 ms |
| `CashFlow Inputs!$AL$221` | 25–30 ms | 81 | 59,777 | 65 | 25–28 ms |
| `CashFlow Inputs!$AL$163` | 14–18 ms | 81 | 59,777 | 65 | 12–16 ms |
| `CashFlow Inputs!$AM$134` | 7–10 ms | 81 | 59,777 | 65 | 6–7 ms |
| `CashFlow Inputs!$AA$120` | 4–6 ms | 81 | 59,777 | 65 | 3–5 ms |

The formula family is structurally:

```text
IF(
  IFERROR(
    SUMPRODUCT(... INDEX(Key_Project_Milestones,0,MATCH(...)) ...)
    + SUMPRODUCT(...)
    + SUMPRODUCT(...),
    0
  ) = 0,
  "",
  IFERROR(
    SUMPRODUCT(...)
    + SUMPRODUCT(...)
    + SUMPRODUCT(...),
    0
  )
)
```

These formulas repeatedly read named ranges such as:

```text
Key_Project_Milestones
Key_Project_Milestones_Cost_Category
Key_Project_Milestones_C
```

The dominant cost is not one special pass-2 formula. It is the repeated combination of:

```text
large range views
SUMPRODUCT scans
repeated INDEX/MATCH resolution
live-edge membership checks for every resolved range
```

## What changes when `iterating=true`?

The code path changes only after the first live-cycle classification:

```text
iterating = true
compare previous full pass with current full pass
possibly run another full member sweep
```

The member evaluator itself is the same `run_member!` path in both passes:

```text
RecordingContext
-> evaluate_vertex_recorded
-> range/name/scalar resolver interception
-> LiveEdgeCollector recording
-> formula value commit
```

Measured effects:

| Candidate cause | Evidence | Verdict |
| --- | --- | --- |
| Repeated range resolution | Range counts/volume nearly identical | Present in both; not pass-2-specific |
| Cache invalidation/bypass | Pass 2 has 0 lookup builds, 12 hits, 8 misses | Not the cause |
| Repeated large-range materialization | Requested volume differs by only 0–12k cells | Not the cause of a 5s delta |
| Loss of parallelism | SCC member passes are serial in both modes | No pass-2 transition |
| Extra live-edge recording | Internal target events nearly identical | Same cost in both |
| Additional allocations/copies | Not directly measured; `prev_pass` clone is small relative to 5s | No evidence of dominant cost |
| Different formula evaluator path | Same member path and same read profile | No |
| Dirty propagation during pass | 0 visits in every main pass | No |
| SCC analysis/bookkeeping | ~85–100 ms post-eval, ~6–10 ms live analysis | Not the 5s cost |
| Convergence bookkeeping | ~0.08–0.15 ms | Negligible |

## Dominant implementation cost

The strongest measured implementation hotspot is:

```text
LiveEdgeCollector::record_rect_with_origin
```

For each resolved range read it scans every SCC member:

```text
238,839 range reads × 4,828 SCC members
≈ 1.152 billion membership checks per pass
```

The exact observed counter is:

```text
1,152,398,175 checks per pass
```

The collector records only matching SCC targets, but it currently finds them by scanning the entire membership vector. The measured collector time is approximately:

```text
1.07–1.19 seconds per main-SCC pass
```

This is a substantial implementation cost, but it is not a pass-2-only cost. It is paid in pass 1 and every subsequent pass.

## Instrumentation-disabled control

The control run omitted `FZ_TRACE_SCC_PASS_PROFILE`. It recorded no profile rows and retained the same aggregate formula-value fingerprints:

```text
initial: [20710, 7185579490432752015]
F7/no-op: [20710, 1026544198047018979]
```

Control wall times in the captured process were:

| Phase | Profile enabled | Profile disabled |
| --- | ---: | ---: |
| Initial | 32,955 ms | 37,090 ms |
| F7 | 14,632 ms | 14,795 ms |
| No-op | 12,686 ms | 14,902 ms |

The two processes have normal workbook/load/OS variance, so these single-run wall numbers are not used as an instrumentation-overhead estimate. The profile’s tiny synthetic control shows the expected diagnostic overhead: approximately `0.23 ms` versus `0.14 ms` for initial evaluation.

The disabled control confirms that no semantic output change was introduced by profiling.

## Small synthetic SCC

For:

```text
S!A1 = B1 + 1
S!B1 = A1 / 2
```

profile-enabled initial evaluation ran 11 passes under the configured `maxChange=0.001`:

```text
pass 1: ~0.018 ms
pass 2: ~0.004 ms
later passes: ~0.002–0.004 ms
```

Each pass had:

```text
2 scalar reads
2 internal target events
0 range reads
0 named reads
0 dirty-propagation visits
```

The synthetic result confirms that the profile decomposition itself is functioning and that the large Fossil cost is range/collector workload, not convergence-loop bookkeeping.

## Are the expensive operations semantically necessary?

### Semantically necessary

- Formula evaluation of the SCC members for each iterative pass under the current Excel-compatible convergence contract.
- Reading range values and named ranges used by the formulas.
- Runtime-live target observation when exact live-cycle classification is required.
- Pass 2’s convergence comparison before declaring normal convergence.

### Implementation-expensive but not intrinsically necessary

- Scanning all 4,828 SCC members for every resolved range read.
- Taking a collector mutex for each scalar/name/range recording call, even though SCC evaluation is serial and uncontended.
- Repeating equivalent range membership queries without an SCC-local coordinate index.
- Repeating `INDEX/MATCH` work inside structurally repeated formulas where an exact, semantics-preserving lookup/index optimization might be possible.

## Smallest safe optimization

Do not skip pass 2 and do not reuse formula values yet.

The smallest safe optimization is an SCC-local exact member-coordinate index for live-edge collection:

```text
Build once per SCC:
  sheet -> row/column/block or interval index of SCC member positions

For record_rect:
  query only member positions intersecting the rectangle
  preserve deterministic member ordering
  emit the same target edge set
```

Correctness requirements:

```text
- preserve self edges
- preserve cross-sheet separation
- preserve name-member targets
- preserve open/whole-row/whole-column bounds
- preserve duplicate edge deduplication
- preserve live edge origin masks
- preserve dynamic target changes between passes
- do not change formula values or evaluation order
```

This optimization reduces collector traversal work while retaining all target identities required for runtime-live SCC analysis.

A second possible optimization is reducing uncontended collector synchronization for serial SCC passes, but it is less isolated than the coordinate-index change and should follow the range-index measurement.

## Expected latency after fixing the collector

Measured collector time for the Fossil main SCC is approximately `1.07–1.19 s` per pass. A two-pass F7/no-op request therefore contains roughly `2.1–2.4 s` of potentially avoidable collector traversal in the main SCC.

An upper-bound expectation, not a guarantee:

```text
F7/no-op current end-to-end:  ~11–15 s depending on run variance
collector-index target:        ~9–12 s
```

For initial evaluation with four passes, the corresponding upper-bound opportunity is roughly `4.3–4.8 s` in the main SCC. The exact benefit must be measured after implementing the index; it cannot be inferred as a pass-2-only 5-second saving.

## Final answer

```text
The apparent pass-1/pass-2 5-second gap was a timing instrumentation bug.

Both passes spend ~5–6 seconds because both execute the same expensive formula
and live-edge collection path.

The strongest measured hotspot is the O(range_reads × SCC_members) scan in
LiveEdgeCollector::record_rect_with_origin, not convergence bookkeeping,
lookup-cache invalidation, dirty propagation, or a pass-2-only evaluator path.

Pass 2 remains semantically necessary for the current convergence contract,
but its implementation work can be made cheaper by indexing SCC member
coordinates for exact range intersection.
```
