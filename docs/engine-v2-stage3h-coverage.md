# Engine V2 Stage 3H Coverage

## Stage 3H-V decision

The virtual-demand root cause was measured, one generic optimization was retained, and the rejected rectangular-query experiment was removed.

Stage 3H remains open; Stage 4 has not started.

## Correctness oracle

Workbook:

```text
C:\Users\OXK0A0A\Downloads\Fossil_EstimatingTemplate_2026-06-25_X_Fossil.xlsx
```

Verified sequence:

```text
Inputs!F6 = 300
Outputs!D41 = 2816.3175654307174

Inputs!F6 = 500
Outputs!D41 = 3952.2073713873697

unchanged Inputs!F6 = 500
Outputs!D41 unchanged
```

## Virtual-demand attribution

### Before nonallocating visitor

```text
nodes visited                         13,425
explicit edges visited                32,844
virtual edges visited                 99,852
virtual expansion requests            13,425
unique virtual sources                13,425
sources with virtual edges             1,548
unique virtual targets                 3,890
range source lookups                  13,425
range sources with dependencies        1,571
range dependency records               3,238
range expansions                       3,238
dynamic source checks                 13,425
dynamic expansions                         0
coordinates examined                 762,286
vertex grid lookups                  762,286
formula-owner/kind lookups           717,700
raw edges emitted                     99,852
unique source/target pairs            99,852
duplicate pairs                            0
closure membership probes             99,852
closure new-target observations        8,325
stack pushes                           8,325
temporary Vec events                  48,299
temporary map/set events              16,663
```

Exclusive attribution:

```text
source lookup                         2.8 ms
range resolution                     12.3 ms
sheet-index materialization         446.2 ms
identity conversion                  22.4 ms
target lookup/filter                 25.6 ms
dynamic evaluation                    0.0 ms
builder deduplication                22.0 ms
builder map publication               0.9 ms
closure source lookup                 0.8 ms
closure edge publication              1.0 ms
closure membership                   21.3 ms
virtual traversal total             561.7 ms
```

### After nonallocating visitor

```text
nodes visited                         13,425
explicit edges visited                32,844
virtual edges visited                 99,852
virtual expansion requests            13,425
unique virtual sources                13,425
sources with virtual edges             1,548
unique virtual targets                 3,890
range dependency records               3,238
dynamic expansions                         0
coordinates examined                 762,286
raw edges emitted                     99,852
unique source/target pairs            99,852
duplicate pairs                            0
closure membership probes             99,852
stack pushes                           8,325
temporary Vec events                  41,823
temporary map/set events              13,425
```

Exclusive attribution:

```text
source lookup                         3.1 ms
range resolution                     13.0 ms
streamed enumeration and filtering   56.7 ms
builder deduplication                22.9 ms
builder map publication               0.8 ms
closure source lookup                 0.8 ms
closure edge publication              1.1 ms
closure membership                   21.5 ms
virtual traversal total             126.3 ms
```

The streamed timer includes interval-tree enumeration, row filtering, grid-address lookup, kind lookup, and dirty/volatile filtering. It intentionally avoids per-edge clocks.

## Rejected experiment

Replacing column enumeration with `SheetIndex::vertices_in_rect` was tested and removed.

Measured diagnostic result:

```text
coordinates examined                  717,700
sheet-index materialization             12.1 s
virtual traversal                       12.2 s
```

The rectangular API computes both axis cardinalities, materializes one axis set, and streams/intersects the other. It is appropriate for general rectangular queries but not for this repeated virtual-demand workload.

## Fresh-process benchmark

Baseline before Stage 3H-V:

| Sample | Wall | Kernel |
|---|---:|---:|
| 1 | 3.651 s | 3.012 s |
| 2 | 3.507 s | 2.861 s |
| 3 | 3.390 s | 2.764 s |
| Mean | 3.516 s | 2.879 s |
| Median | 3.507 s | 2.861 s |

After nonallocating virtual traversal:

| Sample | Wall | Kernel |
|---|---:|---:|
| 1 | 3.127 s | 2.947 s |
| 2 | 2.839 s | 2.679 s |
| 3 | 2.991 s | 2.828 s |
| Mean | 2.986 s | 2.818 s |
| Median | 2.991 s | 2.828 s |

Observed changes:

```text
wall mean      -15.1%
wall median    -14.7%
kernel mean     -2.1%
kernel median   -1.1%
```

Demand closure construction occurs primarily in the scoped admission phase outside `V2RunResult.elapsed`, which explains why the wall improvement is much larger than the reported kernel improvement.

## Work-counter parity

Before and after:

```text
formulas evaluated                 4,850
workspace formulas                 3,361
outside formulas                   1,489
dirty upstream                     1,768
exact-SCC evaluations              1,557
downstream                            36
solver passes                         56
exact read sets finalized          3,864
retained exact formula edges     525,671
logical range positions      383,529,255
physical cells fetched         1,620,416
retained plans                     19/19/0
runtime invalidations/reopens         0/0
workspace reopens                       0
runtime expansion reopens               0
```

## Regression coverage

Passed after the retained change:

- complete 64-test Engine V2 production suite;
- `stage3c_admission_and_schedule_share_one_demand_closure`;
- `stage3d_runtime_edge_change_reopens_workspace_fail_closed`;
- range-consumer and dynamic-reference V2 production tests;
- dedicated `SheetIndex` visitor/materialized-query parity test;
- real Light workbook targeted validation.

Final repository checks are recorded in the Stage 3H session report.

## Post-3H-V Light baseline

### Fresh uninstrumented requests

| Request | Wall samples | Wall mean | Wall median | Kernel samples | Kernel mean | Kernel median |
|---|---|---:|---:|---|---:|---:|
| F6=300 initial | 7.595, 7.085, 7.988 | 7.556 | 7.595 | 6.806, 6.368, 7.150 | 6.775 | 6.806 |
| F6 300→500 warm | 2.625, 2.565, 2.838 | 2.676 | 2.625 | 2.485, 2.425, 2.677 | 2.529 | 2.485 |
| F6=500 unchanged | 0.274, 0.271, 0.303 | 0.283 | 0.274 | 0.272, 0.268, 0.299 | 0.280 | 0.272 |

Warm working-set samples:

```text
380,825,600
380,981,248
379,527,168 bytes
mean    380,444,672
median  380,825,600
```

### Warm formula and finalization attribution

```text
category             wrapper      interpreter   observation   finalization
outside              863.880 ms   855.483 ms      7.782 ms     83.305 ms
dirty upstream       492.772 ms   393.137 ms     86.426 ms    302.279 ms
exact SCC            173.740 ms   142.847 ms     27.443 ms     32.786 ms
downstream             4.210 ms     4.210 ms      0.000 ms      0.321 ms
--------------------------------------------------------------------------
total              1,534.602 ms 1,395.678 ms    121.651 ms    418.691 ms
```

Finalization components:

```text
sorting                    87.056 ms
deduplication              59.204 ms
owner/edge extraction     252.426 ms
remaining metadata/copy   19.999 ms
total                     418.691 ms
```

Distributions:

```text
category          raw p50/p95/max       unique p50/p95/max    duplicate p50/p95/max
outside           12 / 93 / 276         9 / 88 / 276          1 / 12 / 56
dirty upstream    1016 / 1016 / 1016    507 / 507 / 507        509 / 509 / 509
exact SCC         2 / 1188 / 1188       2 / 594 / 594          0 / 594 / 594
downstream        2 / 4 / 4             2 / 2 / 2              0 / 2 / 2
```

Owner index and probes:

```text
index entries/builds/time   74,312 / 1 / 51.6 ms
owner lookups               817,107
owner hits                  269,347
owner misses                547,760
```

## Post-3H-V Heavy baseline

### Fresh uninstrumented requests

| Request | Wall samples | Wall mean | Wall median | Kernel samples | Kernel mean | Kernel median |
|---|---|---:|---:|---|---:|---:|
| F7=300 initial | 9.817, 8.666, 8.681 | 9.055 | 8.681 | 8.961, 7.897, 7.933 | 8.264 | 7.933 |
| F7 300→500 warm | 4.427, 3.947, 4.041 | 4.138 | 4.041 | 4.216, 3.743, 3.809 | 3.923 | 3.809 |
| F7=500 unchanged | 2.354, 2.087, 2.334 | 2.258 | 2.334 | 2.203, 1.962, 2.210 | 2.125 | 2.203 |

Warm output:

```text
Outputs!D53 = 5766.920229312803713
```

Warm working-set samples:

```text
531,042,304
529,428,480
532,684,800 bytes
mean    531,051,861
median  531,042,304
```

### Warm formula and finalization attribution

```text
category             wrapper       interpreter   observation   finalization
outside            1,367.060 ms   1,236.564 ms    129.843 ms    443.663 ms
dirty upstream       509.846 ms     382.614 ms    115.507 ms    396.005 ms
exact SCC             209.790 ms     154.212 ms     49.562 ms     75.683 ms
downstream              0.827 ms       0.827 ms      0.000 ms      0.150 ms
---------------------------------------------------------------------------
total               2,087.522 ms   1,774.217 ms    294.912 ms    915.501 ms
```

Finalization components:

```text
sorting                   131.310 ms
deduplication              67.443 ms
owner/edge extraction     685.220 ms
remaining metadata/copy    31.528 ms
total                     915.501 ms
```

Distributions:

```text
category          raw p50/p95/max       unique p50/p95/max       duplicate p50/p95/max
outside           14 / 2567 / 2567      10 / 2566 / 2566         1 / 3 / 56
dirty upstream    1024 / 1024 / 1024    511 / 511 / 511           513 / 513 / 513
exact SCC         2 / 1300 / 1300       2 / 650 / 650             0 / 650 / 650
downstream        2 / 2 / 4             2 / 2 / 2                 0 / 0 / 2
```

Owner index and probes:

```text
index entries/builds/time   94,966 / 1 / 59.0 ms
owner lookups               2,794,510
owner hits                    303,633
owner misses                2,490,877
```

The outside category accounts for 1,913,112 unique probes and 1,903,428 misses.

## Virtual demand comparison

```text
metric                              Light       Heavy
nodes visited                       13,425      15,825
explicit edges                      32,844      39,793
virtual edges                       99,852     138,495
sources with virtual edges           1,548       1,700
range sources with dependencies      1,571       2,665
range records/expansions             3,238       4,590
dynamic expansions                      0           0
coordinates examined               762,286     827,716
formula-owner/kind lookups          717,700     783,947
unique virtual targets               3,890       6,325
closure new targets/stack pushes     8,325      17,185
temporary Vec events                41,823      49,175
temporary map events                13,425      15,825
virtual traversal                  124.7 ms    154.9 ms
streamed enumeration/filter         57.0 ms     65.5 ms
builder deduplication                21.5 ms     31.3 ms
closure membership                  20.5 ms     27.5 ms
```

For both workbooks, raw emitted edges equal unique source/target pairs and duplicate pairs are zero. The nonallocating visitor scales with examined coordinates and range/dependency count and remains effective on Heavy.

## Validation comparison

```text
metric                                  Light       Heavy
warm read sets examined                  3,864       6,270
warm runtime edges                     274,574     312,564
warm reference observations              6,021      10,825
warm topology checks                     2,998       7,544
warm name entries                        1,763       5,401
warm spill entries                         935       2,117
warm selected references                   731         632
warm range/reference entries             9,236      16,561
runtime validation                     161.6 ms    178.1 ms
reference validation                     1.8 ms      3.3 ms
topology validation                      0.6 ms      1.4 ms
metadata validation                      1.1 ms      2.1 ms
retained-plan validation               192.3 ms    369.1 ms
```

Runtime certificate probes:

```text
                 cold candidates/hits/misses       warm candidates/hits/misses
Light            525,669 / 490,018 / 29,963        274,574 / 272,811 / 0
Heavy            433,898 / 395,463 / 30,236        312,564 / 307,163 / 0
```

Cold certificate counts are not used as warm counts.

## Cleanup and residual trace

```text
metric                             Light       Heavy
retained-state scan                53.2 ms      69.4 ms
demand scheduling                 157.8 ms     211.0 ms
dirty seed selection               33.2 ms      47.0 ms
schedule construction              37.5 ms      52.2 ms
scoped admission                   30.7 ms      67.1 ms
spill/effect commit                 8.2 ms      18.8 ms
cleanup                           173.9 ms     410.7 ms
explicit residual                205.1 ms     288.1 ms
top-level unattributed           278.9 ms     463.2 ms
```

Cleanup contains final revision validation, evaluated-set conversion, dirty clearing, volatile and iterative redirty, iterative-state refresh, graph-wide dirty telemetry sets, provenance construction, and diagnostic sample construction.

The explicit residual is not one phase. Code tracing identifies these untimed contributors:

- prior workspace-profile cloning and old-metrics destruction;
- request initialization;
- post-schedule scans and schedule-unit bookkeeping;
- complete `ExactReadSet` equality checks before edge replacement;
- retained workspace output/read-set clones and reverse maps;
- SCC orchestration outside member formula timers;
- metrics/profile publication and evaluated-set maintenance;
- temporary structure destruction between units.

These are inferred from timer boundaries; they are not independently measured subphases.

## Light versus Heavy scaling

```text
metric                         Light          Heavy       dominant scaling
warm wall median              2.625 s        4.041 s     formula and observed-cell work
warm kernel median            2.485 s        3.809 s     formula and workspace work
interpreter attribution       1.396 s        1.774 s     formula count/type
V2 end-to-end overhead        1.697 s        3.040 s     cells, validation, cleanup
exact finalization            0.419 s        0.916 s     observed unique cells
virtual traversal             0.125 s        0.155 s     ranges/dependencies/topology
retained validation           0.192 s        0.369 s     workspace/read-set metadata
contract validation           0.166 s        0.186 s     warm certificate edges
cleanup                       0.174 s        0.411 s     evaluated/dirty/iterative topology
residual                      0.205 s        0.288 s     formulas/workspaces/temporary state
formulas evaluated            4,850          6,881       formula count
physical cells                1,620,416      3,702,377   observed-cell count
exact edges                   525,671        433,899     retained dependency count
virtual edges                 99,852         138,495     range/dependency count
owner-index entries           74,312         94,966      workbook formula topology
working-set median            380,825,600    531,042,304 retained cells/topology
```

Near-constant or weakly scaling request costs include owner-index build per formula topology, revision getters, and small metadata handling. Virtual traversal scales moderately. Exact finalization, observation recording, cleanup, and retained validation scale materially on Heavy.

## Stage 3H continuation decision

Stage 3H continues. This pass made no architecture change.

Combined remaining priorities:

1. Exact-cell to formula-owner resolution and finalization: measured at 0.252 s Light and 0.685 s Heavy for owner/edge extraction, with Heavy performing 2.49 million misses.
2. Cleanup/redirty bookkeeping: measured at 0.174 s Light and 0.411 s Heavy; requires attribution separating semantic volatile/iterative redirty from observational telemetry/provenance construction.
3. Retained-plan validation: 0.192 s Light and 0.369 s Heavy, scaling with workspace metadata and retained exact-read contents.
4. Explicit residual: 0.205 s Light and 0.288 s Heavy, currently distributed across several inferred operations.

The first priority satisfies the continuation rule directly and exposes a clear Heavy scalability problem. The next Stage 3H substage should measure miss cost and compare generic per-sheet sparse ownership or sorted/batched intersection against the current request-scoped hash index. No representation should be changed before that focused attribution and safety review.

Stage 4 remains deferred.

## Stage 3H — v0.8.0 comparative architecture and benchmark

### Harness and equivalence boundary

An isolated `probe-real-workbook-lifecycle` binary was added to the old tree's existing benchmark crate. It uses one retained workbook instance per process, sets 300, evaluates the target, sets 500, evaluates the same target, then evaluates unchanged 500. Cycle configuration is runtime iteration with Excel defaults and deterministic time is fixed at the Unix epoch.

Exact equivalence could not be established:

```text
same XLSX files                         yes
same input mutations                    yes
same output targets                     yes
same retained-instance lifecycle        yes
same target-evaluation intent           yes
same loader backend                     no: old Umya versus V2 Calamine
same cold graph-build placement         no: old graph build deferred into cold calculation
same formula counts                     Light 74,312; Heavy old 94,932 versus V2 94,966
same output semantics                   no: old returned #VALUE!
same runtime cycle truth                no
```

Direct old Calamine load fails with `#NAME?: Undefined table: Main_GSU_Price_X`. This is a loader capability failure: that implementation does not discover/register XLSX tables. Umya is the closest supported old path and registers tables before formulas.

### Three fresh-process old-engine samples

Light:

| Sample | Cold calculation | Warm 300→500 | Unchanged 500 | Warm working set |
|---|---:|---:|---:|---:|
| 1 | 16.982 s | 4.695 s | 4.296 s | 522,067,968 B |
| 2 | 19.081 s | 4.750 s | 4.036 s | 528,834,560 B |
| 3 | 17.066 s | 4.954 s | 4.688 s | 424,108,032 B |
| Median | 17.066 s | 4.750 s | 4.296 s | 522,067,968 B |

Heavy:

| Sample | Cold calculation | Warm 300→500 | Unchanged 500 | Warm working set |
|---|---:|---:|---:|---:|
| 1 | 27.386 s | 11.155 s | 11.439 s | 574,803,968 B |
| 2 | 31.715 s | 11.022 s | 10.426 s | 605,835,264 B |
| 3 | 29.566 s | 10.228 s | 11.330 s | 587,927,552 B |
| Median | 29.566 s | 11.022 s | 11.330 s | 587,927,552 B |

Every old output was `#VALUE!`. Light reported 73 live iterative SCCs in the first sample; Heavy reported 169 cold and 168 warm/unchanged, one stamped circular error, 4,829 iterative vertices redirtied, and approximately 27,510 evaluation vertices remaining. These timings are not correctness-equivalent wins or losses.

### Proven old/new dependency difference

Old mechanism: static AST dependency extraction creates direct placeholder/cell vertices and compressed range dependencies at ingest. Ordinary evaluation consumes those graph edges without recording every execution read. Runtime `LiveEdgeCollector` is instantiated only inside a conservative SCC and maps scalar reads against a small SCC-member hash; rectangle reads intersect the rectangle with SCC members without enumerating all physical cells.

Why it is fast: work is proportional to static formula/range relationships and SCC membership, not every physical cell fetched by interpreter range kernels.

Why it is insufficient: static adjacency and broad resolved rectangles cannot express untaken branches, selected-reference identity, dynamic targets, complete exact global formula edges, or V2 fail-closed retained-plan reopening.

V2 reuse decision: retain the sparse/batched ownership principle, but not static truth. V2 exact cell evidence, reference/generation observations, and runtime formula-edge/SCC truth remain authoritative.

### Request-global owner reuse

Warm changed-input results:

```text
metric                              Light       Heavy
owner probe occurrences           817,107   2,794,510
globally unique coordinates        13,977      18,094
repeated coordinates               12,807      16,927
repeated positive probes          262,241     295,846
repeated negative probes          540,889   2,480,570
unique positive coordinates         7,106       7,787
unique negative coordinates         6,871      10,307
read sets                            3,824       6,209
read-set size p50                       36          51
read-set size p95                      507       2,566
read-set size max                      594       2,566
```

Heavy per-sheet extremes:

```text
sheet 4: 1,381,896 probes, 2,564 unique coordinates, 0 hits, 804 formula owners
sheet 5:   458,676 probes,   852 unique coordinates, 0 hits, 138 formula owners
sheet 3:   738,710 probes, 6,259 unique coordinates, 256,985 hits, 25,647 owners
```

The dominant Heavy outside read sets contain 2,566 cells with one formula hit and 2,565 misses.

### Candidate replay benchmark

All candidates replayed the exact captured warm read-set vectors three times and were required to match direct-hash hit count and owner checksum.

```text
candidate                         Light        Heavy
packed-coordinate owner hash    149.846 ms   546.582 ms
request coordinate memo         125.932 ms 4,559.164 ms
per-sheet sorted bounded merge   65.285 ms   116.851 ms
whole-read-set memo             178.638 ms   624.507 ms
adaptive, threshold 8            66.673 ms   105.588 ms
adaptive, threshold 64           67.768 ms   116.816 ms
adaptive, threshold 256          67.002 ms   120.449 ms
```

Read-set memo reuse was weak: 3,662 unique of 3,824 Light sets and 5,107 unique of 6,209 Heavy sets. Coordinate memo's second hash structure scaled pathologically on Heavy. Pure sorted merge was selected because it is the Light winner, removes the bad Heavy asymptotic/random-access behavior, needs only one owner representation, and remains close to the best adaptive Heavy measurement.

### Retained change and parity

The owner index is now request-scoped per-sheet sorted vectors. The existing run-length scan merges canonical exact cells against only the matching sheet vector starting at a lower bound. Exact cell evidence remains separate and unchanged, and raw formula-edge event counts are preserved without an additional pass or run-count allocation.

Instrumented warm comparison:

```text
owner/edge extraction             before       after
Light                             ~252 ms      ~194 ms
Heavy                             ~685 ms      ~404 ms
Heavy unchanged                       n/a      ~204 ms
```

Parity observed after the architecture change:

```text
Light 300 output     2816.3175654307174
Light 500 output     3952.2073713873697
Heavy 300 output     4212.843018909032
Heavy 500 output     5766.920229312803713
Light probes/hits/misses   817,107 / 269,347 / 547,760
Heavy probes/hits/misses 2,794,510 / 303,633 / 2,490,877
```

Fresh uninstrumented acceptance after fusing merge resolution into the existing run-length scan:

| Request | Wall samples | Wall median | Kernel samples | Kernel median | Working-set median |
|---|---|---:|---|---:|---:|
| Light 300 initial | 7.688, 8.013, 7.900 s | 7.900 s | 6.836, 7.229, 7.062 s | 7.062 s | 363,081,728 B |
| Light 300→500 | 2.543, 2.709, 2.613 s | 2.613 s | 2.375, 2.550, 2.460 s | 2.460 s | 380,272,640 B |
| Light unchanged | 0.280, 0.298, 0.288 s | 0.288 s | 0.277, 0.296, 0.286 s | 0.286 s | 380,293,120 B |
| Heavy 300 initial | 8.990, 8.736, 10.192 s | 8.990 s | 8.140, 7.929, 9.351 s | 8.140 s | 503,775,232 B |
| Heavy 300→500 | 3.736, 3.653, 4.312 s | 3.736 s | 3.521, 3.456, 4.098 s | 3.521 s | 531,361,792 B |
| Heavy unchanged | 1.894, 1.948, 2.238 s | 1.948 s | 1.762, 1.811, 2.092 s | 1.811 s | 531,992,576 B |

Compared with the post-3H-V medians, Light warm is neutral/slightly faster (2.625 to 2.613 s), Heavy warm improves 4.041 to 3.736 s, and Heavy unchanged improves 2.334 to 1.948 s. Working-set medians are flat.

Verification passed:

- complete 64-test Engine V2 production suite;
- runtime SCC, reopen, Stage 3C closure reuse, Stage 3D fail-closed edge-change, dynamic-reference, name, table, spill, selected-reference, range/provider/effect regressions within that suite;
- real Light and Heavy targeted lifecycle checks;
- GNU `formualizer-eval` check;
- GNU Calamine-enabled `formualizer-workbook` check;
- `cargo fmt --all -- --check`;
- `git diff --check`.

### Post-owner re-profile

The global-reuse and candidate-capture work is now separately gated from normal attribution so it cannot inflate residual timing. Normal warm attribution:

```text
phase                            Light       Heavy
interpreter semantics          1.130 s     1.429 s
formula wrappers               1.284 s     1.733 s
exact-read finalization        0.281 s     0.471 s
owner/edge extraction          0.129 s     0.248 s
owner index build              0.045 s     0.058 s
sorting                         0.077 s     0.122 s
deduplication                   0.053 s     0.064 s
retained-plan validation        0.172 s     0.324 s
cleanup                         0.164 s     0.360 s
explicit residual               0.172 s     0.245 s
demand scheduling               0.134 s     0.186 s
runtime contract validation     0.115 s     0.138 s
```

Owner/edge extraction is no longer dominant. Cleanup is the next campaign because it leads current Heavy bookkeeping and remains large enough on Light to satisfy the continuation threshold. Retained validation is second. Stage 3H remains open and Stage 4 has not started.

## Cleanup/redirty campaign

### Work classification

```text
final revision validation       correctness required
evaluated-set conversion        production state required
dirty clearing                  correctness required
volatile redirty                correctness required
iterative-SCC redirty           correctness required
iterative-state refresh/prune   correctness required
graph-wide dirty snapshots      observability only
root-vector clones/sorts        diagnostic only
provenance unions/maps          diagnostic only
address-string samples          diagnostic only
per-SCC sheet/sample records    diagnostic only
```

The old implementation kept SCC dirty telemetry enabled unconditionally in both Engine constructors. It scanned `get_evaluation_vertices()` before redirty, after volatile redirty, and after iterative redirty; built multiple `FxHashSet`/`FxHashMap` products; and formatted sampled cell addresses. Root and sample products were partly constructed even before the old telemetry return.

### Retained change

Telemetry is explicitly enabled by `FZ_TRACE_SCC_DIRTY_TELEMETRY` or `FZ_PROFILE_WORKSPACE_STRUCTURE`. Disabled production requests still:

- clear every successfully evaluated dirty vertex;
- redirty all required volatile vertices;
- redirty iterative SCC members;
- refresh and prune iterative-state values;
- preserve reusable iterative SCC state and scalar counters;
- restore user/request state on abort.

Disabled requests clear diagnostic seed/provenance scratch rather than carrying it forward. The explicitly enabled real Light path still produced `3952.2073713873697` and built telemetry, with 184.7 ms attributed cleanup.

### Measured attribution

```text
request                         before       after       removed diagnostic work
Light warm                       163.7 ms      2.8 ms      160.9 ms
Heavy warm                       360.0 ms     85.2 ms      274.8 ms
Heavy unchanged                      n/a      98.3 ms      semantic retained work
```

Heavy's remaining cleanup is volatile/iterative work and state maintenance, not provenance/sample construction.

### Fresh uninstrumented acceptance

| Request | Wall samples | Wall median | Kernel samples | Kernel median | Working-set median |
|---|---|---:|---|---:|---:|
| Light 300 initial | 7.346, 6.737, 7.070 s | 7.070 s | 6.546, 6.062, 6.308 s | 6.308 s | 358,166,528 B |
| Light 300→500 | 2.412, 2.300, 2.359 s | 2.359 s | 2.257, 2.155, 2.203 s | 2.203 s | 379,637,760 B |
| Light unchanged | 0.120, 0.120, 0.114 s | 0.120 s | 0.117, 0.118, 0.111 s | 0.117 s | 379,727,872 B |
| Heavy 300 initial | 8.555, 9.095, 9.569 s | 9.095 s | 7.733, 8.242, 8.703 s | 8.242 s | 496,775,168 B |
| Heavy 300→500 | 3.438, 3.499, 3.667 s | 3.499 s | 3.216, 3.286, 3.432 s | 3.286 s | 525,283,328 B |
| Heavy unchanged | 1.712, 1.762, 1.800 s | 1.762 s | 1.572, 1.616, 1.655 s | 1.616 s | 526,307,328 B |

Compared with the post-owner medians, Light warm improves 2.613 to 2.359 s and unchanged improves 0.288 to 0.120 s. Heavy warm improves 3.736 to 3.499 s and unchanged improves 1.948 to 1.762 s. The full 64-test V2 production suite passed. Stage 3H remains open pending the new post-cleanup ranking.
