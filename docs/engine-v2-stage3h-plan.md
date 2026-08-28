# Engine V2 Stage 3H Plan

## Status

Stage 3H remains open. Stage 3H-V virtual-demand investigation has reached its decision gate and retains one generic optimization.

The Light warm changed-input path remains the acceptance workload:

```text
Inputs!F6: 300 -> 500
Outputs!D41: 3952.2073713873697
```

Stage 4 unchanged-request work is not part of this stage.

## Objective

Reduce Engine V2 dependency bookkeeping toward the cost of production formula evaluation without changing formula semantics, exact dependency truth, runtime SCC membership, or fail-closed behavior.

## Completed representation work

Stage 3H currently includes:

1. packed scalar observations using `PackedSheetCell`;
2. append-only scalar event recording under one recorder-state lock;
3. adaptive sort and run-length finalization;
4. canonical contiguous exact-cell and formula-edge vectors;
5. request-scoped packed formula-owner lookup;
6. hash-based runtime contract certificates;
7. nonallocating virtual range traversal through `SheetIndex`.

## Stage 3H-V: virtual demand

### Measured baseline

The warm Light demand closure reported:

```text
nodes visited             13,425
explicit edges visited    32,844
virtual edges visited     99,852
explicit traversal        ~5-7 ms
virtual traversal         ~562 ms
```

Detailed attribution showed:

```text
virtual expansion requests          13,425
unique virtual sources              13,425
sources emitting virtual edges       1,548
range sources with dependencies      1,571
range dependency records             3,238
dynamic expansions                       0
coordinates examined               762,286
raw virtual edges                   99,852
unique source/target pairs          99,852
duplicate source/target pairs            0
unique targets                        3,890
closure membership probes           99,852
stack pushes                          8,325
```

The dominant exclusive component was sheet-index column-range materialization:

```text
source lookup                 ~3 ms
range extent resolution      ~13 ms
sheet-index materialization ~446 ms
identity conversion          ~22 ms
target filtering             ~26 ms
builder deduplication        ~22 ms
closure publication           ~1 ms
closure membership           ~21 ms
```

### Root cause

`RangeVirtualDepProvider` called `SheetIndex::vertices_in_col_range` for every compressed range. That API built a temporary `HashSet<VertexId>` and converted it to a temporary `Vec<VertexId>`. The provider then allocated another vector while applying row filtering.

The virtual edge count was not excessive or duplicated. The cost came from materializing general-purpose collections before a streaming filter.

### Rejected hypotheses

- Repeated expansion of the same virtual source: rejected; expansion requests and unique sources were both 13,425.
- Duplicate virtual source/target pairs: rejected; raw and unique pairs were both 99,852.
- Dynamic-reference interpretation: rejected for this Light request; dynamic expansion count was zero.
- Closure hash insertion as the dominant cost: rejected; closure publication and membership together were about 22 ms.
- Existing `vertices_in_rect` as a replacement: rejected experimentally. It performed two interval-tree count traversals and an axis-set intersection, increasing attributed materialization to about 12.1 seconds in debug mode.

### Retained change

`SheetIndex` now exposes a nonallocating column-range visitor. `RangeVirtualDepProvider` streams each indexed vertex directly through:

1. row-bound filtering;
2. vertex-kind filtering;
3. dirty/volatile filtering;
4. dependency collection.

The builder's existing final sort/dedup remains authoritative, so no dependency truth is lost even if a future index implementation emits a duplicate.

Post-change attribution:

```text
virtual traversal          ~126 ms
streamed expansion/filter   ~57 ms
temporary Vec events     48,299 -> 41,823
temporary map/set events 16,663 -> 13,425
```

All virtual source, target, coordinate, edge, and stack-push counts remained unchanged.

## Safety contract

The virtual optimization must continue to preserve:

- exact rectangular range bounds;
- current sheet and resolved `SheetId` semantics;
- dirty and volatile formula filtering;
- dynamic-reference evaluation when present;
- name, table, spill, selected-reference, provider, effect, and generation behavior;
- topology revision validation;
- Stage 3C same-request demand closure reuse;
- exact runtime SCC and retained-plan reopen behavior.

No virtual expansion is retained across topology generations by this change. The visitor reads the current authoritative sheet index during the current closure build.

## Remaining Stage 3H boundary

After Stage 3H-V, the warm changed-input median is approximately:

```text
wall    2.991 s
kernel  2.828 s
```

Further work toward the sub-two-second goal requires a new measured target. The remaining cost is distributed across formula execution, exact-read finalization, retained/contract validation, cleanup, residual request work, and demand closure bookkeeping. Stage 4 remains deferred.

## Post-3H-V Light and Heavy measurement pass

The measurement pass uses three fresh uninstrumented processes per workbook, with each process performing cold, warm changed-input, and unchanged requests on one Engine V2 instance. Attribution runs are separate and are not performance baselines.

### Light post-3H-V cost model

Fresh warm changed-input results:

```text
wall mean / median      2.676 / 2.625 s
kernel mean / median    2.529 / 2.485 s
working-set median      380,825,600 bytes
```

Instrumented warm decomposition:

```text
interpreter semantics        1.396 s
formula wrappers             1.535 s
exact-read finalization      0.419 s
virtual demand               0.125 s
retained-plan validation     0.192 s
runtime contract validation  0.166 s
cleanup                      0.174 s
explicit residual            0.205 s
```

The current fresh samples are faster than the earlier retained 2.991 s wall / 2.828 s kernel median. Both sets are retained as historical measurements; the work counters and output are identical.

### Heavy post-3H-V cost model

Fresh warm changed-input results:

```text
wall mean / median      4.138 / 4.041 s
kernel mean / median    3.923 / 3.809 s
working-set median      531,042,304 bytes
```

Instrumented warm decomposition:

```text
interpreter semantics        1.774 s
formula wrappers             2.088 s
exact-read finalization      0.916 s
virtual demand               0.155 s
retained-plan validation     0.369 s
runtime contract validation  0.186 s
cleanup                      0.411 s
explicit residual            0.288 s
```

Heavy unchanged requests are not clean no-ops: they continue to evaluate 3,380 formulas because retained volatile/iterative work remains active, and have a 2.334 s wall median.

### Light versus Heavy

```text
                                  Light       Heavy
warm wall median                  2.625 s     4.041 s
warm kernel median                2.485 s     3.809 s
interpreter attribution           1.396 s     1.774 s
exact finalization                0.419 s     0.916 s
virtual traversal                 0.125 s     0.155 s
retained validation               0.192 s     0.369 s
contract validation               0.166 s     0.186 s
cleanup                           0.174 s     0.411 s
residual                          0.205 s     0.288 s
formulas evaluated                4,850       6,881
physical cells                    1,620,416   3,702,377
virtual edges                     99,852      138,495
owner-index entries               74,312      94,966
```

Virtual demand scales moderately with topology and dependency count and is no longer dominant. Heavy exposes a different representation pressure: outside-workspace finalization processes 1,913,112 unique scalar coordinates, of which only 9,684 resolve to formula owners.

## Stage 3H continuation decision

Stage 3H continues. No optimization was made during this measurement pass.

The next measured generic target is exact-cell to formula-owner resolution and exact-read finalization:

```text
owner/edge extraction    Light ~0.252 s   Heavy ~0.685 s
exact finalization       Light ~0.419 s   Heavy ~0.916 s
```

Heavy performs approximately 2.79 million owner probes with approximately 2.49 million misses. A future substage should compare the current request-scoped hash index against per-sheet sparse ownership, sorted/batched intersection, or another compact miss-efficient representation while preserving exact cell evidence independently from formula edges.

A second measured target is cleanup/redirty bookkeeping:

```text
cleanup                   Light ~0.174 s   Heavy ~0.411 s
```

Code tracing shows evaluated-set conversion, dirty clearing, volatile and iterative redirty, graph-wide dirty telemetry sets, provenance maps, diagnostic samples, and iterative-state refresh inside this aggregate. This path requires dedicated attribution before deciding which work is semantic and which is observational.

Stage 4 remains deferred.

## Stage 3H — v0.8.0 comparative architecture and benchmark

### Comparison boundary

The comparison tree is `C:\rust_engines\formualizer-v0.8.0` on `perf/value-cache-allocator`, three commits after the `v0.8.0` tag. Its only pre-existing working-tree change is a rustfmt-only import reorder in `engine/eval.rs`. An isolated benchmark binary was added under `formualizer-bench-core`; no old-engine behavior was changed.

The v0.8.0 Calamine loader cannot load either real workbook because it does not register XLSX tables before formula graph construction. The old Umya loader does register tables, so the measured comparison uses Umya plus the supported deferred-graph mode. Load time is therefore not backend-equivalent; cold calculation includes deferred graph construction. Warm and unchanged requests retain one workbook/engine instance.

### Measured old-engine baseline

All three old-engine samples were semantically invalid: both target outputs were `#VALUE!` instead of the Excel/V2 values. Light classified 73 static SCCs as live iterative cycles. Heavy classified 168-169, retained approximately 27,510 evaluation vertices after calculation, and stamped one circular error. Performance remains useful only as labeled architectural evidence.

```text
                         Light median      Heavy median
cold calculation          17.066 s          29.566 s
warm changed input          4.750 s          11.022 s
unchanged                   4.296 s          11.330 s
warm working set          522.1 MB          587.9 MB
```

### Architecture comparison

CURRENT V2 records packed exact execution-read coordinates for every evaluated formula, canonicalizes them, resolves formula ownership, retains exact formula edges, validates generations and reference shapes, reconstructs runtime SCC truth, and reopens fail-closed when retained topology changes. This work is correctness-required, but the previous mostly-negative random owner-probe implementation was not.

V0.8.0 resolves ordinary dependencies statically at ingest into graph vertices/range descriptors. Runtime live-edge recording is limited to an already admitted conservative SCC: scalar reads probe only SCC membership and rectangle reads scan SCC members rather than physical range cells. FormulaPlane also uses compressed producer/consumer regions and affine dirty projection. It therefore avoids a global exact-cell-to-owner phase entirely.

Classification:

- REQUIRED FOR V2 CORRECTNESS: exact cells, selected-target identity, generation observations, retained exact adjacency, exact runtime SCC truth, and fail-closed reopen.
- OLD IMPLEMENTATION UNSAFE FOR V2: using static graph adjacency or broad resolved ranges as complete runtime truth.
- REUSABLE IDEA: formula ownership is sparse and should be resolved in batches/regions rather than by random lookup for every physical cell.
- REQUIRED BUT IMPLEMENTED INEFFICIENTLY: the request-scoped packed owner hash and one lookup per read-set coordinate.
- BETTER NEW DESIGN POSSIBLE: request-scoped per-sheet canonical owner vectors merged with already sorted exact cells.

### Global owner-coordinate reuse

Warm request attribution proved that per-read-set totals greatly overstate global coordinate diversity:

```text
                                  Light       Heavy
probe occurrences               817,107   2,794,510
globally unique coordinates      13,977      18,094
coordinates repeated             12,807      16,927
repeated positive probes        262,241     295,846
repeated negative probes        540,889   2,480,570
unique positive coordinates       7,106       7,787
unique negative coordinates       6,871      10,307
read sets                          3,824       6,209
read-set p95                         507       2,566
```

Heavy sheets 4 and 5 account for approximately 1.84 million zero-hit probes over only 3,416 distinct coordinates. This justified testing memoization, but did not make coordinate memoization the winner.

### Owner resolver candidate benchmark

Real captured warm read sets were replayed three times per candidate in an attribution-only microbenchmark. Best elapsed values:

```text
candidate                    Light        Heavy
current packed owner hash   149.8 ms     546.6 ms
coordinate memo             125.9 ms   4,559.2 ms
sorted bounded merge         65.3 ms     116.9 ms
whole-read-set memo         178.6 ms     624.5 ms
adaptive threshold <=256  ~65-70 ms    ~105-122 ms
```

The read-set memo had little reuse: 3,662/3,824 Light and 5,107/6,209 Heavy read sets were distinct. The coordinate memo added an expensive second hash table and scaled pathologically on Heavy. Adaptive resolution did not justify retaining a second owner representation; pure bounded merge won Light and remained within approximately 11 ms of the best Heavy threshold result.

### Retained owner architecture

The request-scoped owner hash is replaced by:

```text
BTreeMap<SheetId, Vec<(PackedSheetCell, VertexId)>>
```

Each per-sheet vector is sorted once when first needed in a request. Exact scalar events remain canonicalized independently. The existing run-length scan performs a bounded lower-bound plus linear merge and preserves formula-edge event counters without an additional pass or run-count allocation. The index is discarded at every V2 request boundary and therefore cannot retain stale `VertexId` values across topology generations.

Instrumented warm owner/edge extraction after the change:

```text
                         before       after
Light                    ~252 ms     ~194 ms
Heavy                    ~685 ms     ~404 ms
Heavy unchanged              n/a     ~204 ms
```

Outputs and owner hit/miss/edge counters remained unchanged. Three fresh uninstrumented processes retained the change: Light warm changed-input median was 2.613 s with 380,272,640-byte median working set; Heavy warm changed-input median was 3.736 s with 531,361,792-byte median working set. Heavy unchanged fell to a 1.948 s median. The complete 64-test V2 production suite, package checks, real-workbook checks, formatting, and diff checks passed.

### Post-owner re-profile

Normal attribution excludes the separately gated owner-reuse capture experiment.

```text
phase                            Light       Heavy
interpreter semantics          1.130 s     1.429 s
formula wrappers               1.284 s     1.733 s
exact-read finalization        0.281 s     0.471 s
owner/edge extraction          0.129 s     0.248 s
sorting                         0.077 s     0.122 s
deduplication                   0.053 s     0.064 s
retained-plan validation        0.172 s     0.324 s
cleanup                         0.164 s     0.360 s
explicit residual               0.172 s     0.245 s
demand scheduling               0.134 s     0.186 s
runtime contract validation     0.115 s     0.138 s
```

Exact finalization fell from approximately 0.419/0.916 s to 0.281/0.471 s for Light/Heavy. Owner resolution is no longer the dominant Stage 3H structure. Cleanup is now the leading generic bookkeeping phase, narrowly ahead of retained validation on Heavy. Stage 3H therefore continues with cleanup attribution separating semantic dirty clearing and volatile/iterative redirty from eager telemetry, provenance, samples, and temporary destruction. Stage 4 remains deferred.

## Stage 3H cleanup campaign

### Classification and root cause

Correctness-required cleanup consists of final revision validation, evaluated-set conversion, clearing evaluated dirty flags, volatile redirty, iterative-SCC redirty, iterative-state persistence/pruning, and abort-safe request state. Production telemetry scalars for evaluated/volatile/iterative counts are retained.

The previous hard-coded `scc_dirty_telemetry_enabled = true` additionally performed three graph-wide dirty snapshots, root-vector cloning/sorting, provenance unions/maps, address-string samples, and per-SCC sheet/sample construction on every request. This work is diagnostics-only. Some root/provenance construction also occurred before the old telemetry early return.

V0.8.0 uses the same semantic multi-source volatile/iterative redirty primitives but ordinary evaluation does not eagerly build V2's root/provenance diagnostic products. Its generation-bound FormulaPlane dirty leases remain a useful future pattern, but are not required for this targeted cleanup fix.

### Retained design

SCC dirty telemetry is now opt-in through `FZ_TRACE_SCC_DIRTY_TELEMETRY` or the existing `FZ_PROFILE_WORKSPACE_STRUCTURE` diagnostic mode. All graph-wide snapshots, provenance maps, root samples, and SCC samples are behind that gate. When disabled, stale diagnostic scratch state is cleared. Volatile and iterative redirty plus iterative-state maintenance always execute.

Normal attribution measured:

```text
cleanup                         before      after
Light warm                       164 ms       2.8 ms
Heavy warm                       360 ms        85 ms
Heavy unchanged                     n/a        98 ms
```

The explicitly enabled diagnostic path was also exercised on the real Light workbook, produced the exact output, and reported 185 ms cleanup, proving diagnostics remain available.

Three fresh uninstrumented processes:

```text
request                       wall median    kernel median    working set median
Light initial                    7.070 s        6.308 s          358.2 MB
Light warm 300->500              2.359 s        2.203 s          379.6 MB
Light unchanged                  0.120 s        0.117 s          379.7 MB
Heavy initial                    9.095 s        8.242 s          497.0 MB
Heavy warm 300->500              3.499 s        3.286 s          525.3 MB
Heavy unchanged                  1.762 s        1.616 s          526.3 MB
```

The complete 64-test V2 production suite passed after the change. Exact Light/Heavy outputs and runtime dependency behavior remain unchanged. Stage 3H remains open; the next re-profile decides between retained-plan validation, residual, and remaining finalization.

## Stage 3H retained-plan validation campaign

### Root causes

Retained classification repeated two kinds of proven work:

1. Every exact runtime edge recomputed the same revision-heavy contract-certificate key and probed the persistent certificate map. Light examined 274,574 edges and Heavy 312,564, while dependencies repeat heavily.
2. Every request rebuilt workspace-local membership, reverse adjacency, exact-SCC upstream/downstream closures, and two topological orders even when the prior execution had already proved exact SCC membership and read topology unchanged.

V0.8.0's coarse retained plans avoid per-edge proof by binding the complete plan to engine/topology/symbol/semantic generations. V2 cannot use that coarse proof alone because selected references, dynamic targets, and exact runtime topology can reopen. The retained design therefore combines generation-scoped reuse with V2's post-execution exact verification.

### Retained design

- A request-scoped direct `VertexId` validity vector memoizes persistent contract-certificate checks during classification and post-evaluation contract validation. Revision checks bound the memo lifetime. Candidate/hit/skip counters remain unchanged.
- A structural classification is retained only after a successful final revision validation and authoritative final exact reads/SCCs are available.
- Reuse requires a prior valid classification whose exact components match the prior actual components.
- Any runtime invalidation or workspace reopen drops the pre-execution classification. The authoritative fallback result may establish a new certificate only after successful final validation.
- Dirty members, generation/reference validity, mutation intersection, and contract validity are rechecked every request; only immutable local topology/ordering is reused.

Measured attribution:

```text
phase                              Light before/after     Heavy before/after
retained-plan validation             188 -> ~90 ms          343 -> ~89 ms
post-evaluation contract validation  121 -> ~38 ms          145 -> ~50 ms
Heavy unchanged retained plan             n/a               262 -> ~94 ms
```

Cold now materializes the first structural certificate and attributes that work to retained validation; warm requests avoid it.

Three fresh uninstrumented processes:

```text
request                       wall median    kernel median    working set median
Light initial                    7.399 s        6.678 s          359.2 MB
Light warm 300->500              2.203 s        2.050 s          380.4 MB
Light unchanged                  0.123 s        0.121 s          380.5 MB
Heavy initial                    9.033 s        8.110 s          497.4 MB
Heavy warm 300->500              2.977 s        2.771 s          526.2 MB
Heavy unchanged                  1.344 s        1.207 s          527.2 MB
```

The Heavy warm objective is crossed. Light remains above 2.0 s, and obvious generic finalization/residual costs remain, so Stage 3H stays open and Stage 4 remains deferred.

### Retained validation follow-ons

Normal post-change attribution reduced retained-plan validation to approximately 48 ms Light and 89-94 ms Heavy, and contract validation to approximately 38 ms Light and 40-55 ms Heavy.

The sorted owner index is now retained as an immutable topology-generation-bound structure rather than rebuilt each request. Its key is the same composite topology revision used by V2 (`graph topology revision` plus `topology_epoch`). Any mismatch rebuilds the complete per-sheet vectors before use. Warm attribution reports zero owner-index builds, removing approximately 47 ms Light / 71 ms Heavy of direct build work without permitting stale `VertexId` reuse.

Full `ExactReadSet` equality before authoritative edge replacement existed only to publish changed/unchanged diagnostic counters. It is now gated by detailed attribution. Required exact formula-edge comparison/replacement remains unconditional.

A proposed sortedness check before formula-edge sorting was measured and rejected. The vectors are not reliably monotonic, so the check added a scan and still took the sort path; Heavy deduplication rose from approximately 70 to 75 ms. The experiment was removed. Moving already-owned non-cell evidence from `RawReadSet` into `ExactReadSet` was retained, eliminating redundant clones.

Fresh wall samples during these small follow-ons were dominated by host drift: cold and unchanged requests moved by 15-25% even where owner resolution/equality work was absent. The direct phase removals, generation tests, exact outputs, and 64-test suite are authoritative for retention; no end-to-end improvement is claimed for this noisy batch. Stage 3H continues into observation/finalization and residual attribution.
