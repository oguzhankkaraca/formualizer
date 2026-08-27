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
