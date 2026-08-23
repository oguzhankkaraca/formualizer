# Exact Symbolic Dependency Investigation

- **Branch:** `investigation/fossil-excel-calculation`
- **Status:** Exact provenance/dirty validation complete; symbolic SCC replacement not production-eligible
- **Parent reports:** [`010`](010-excel-vs-formualizer-fossil-calculation-investigation.md), [`011`](011-scc-termination-and-compact-dependency-experiment.md)
- **Production evaluator changes:** none
- **Fixed-point caching:** not implemented
- **Tolerance-based early termination:** not extended

## Final answer to the key question

The engine can move partially from:

```text
expanded cell graph -> dirty propagation -> SCC discovery
```

to:

```text
symbolic range/name indexes -> dirty propagation -> SCC discovery
```

with an important boundary:

```text
Dirty propagation: yes, exact for the tested cell/range/name controls.
Static SCC discovery: possible as lazy traversal, but not an exact replacement
                    for the current scheduler without a specified range/conditional
                    semantic mapping.
Runtime-live SCC:    target-level live relationships remain necessary; they may be
                    transient/compactly encoded, but cannot be reduced to only
                    formula/name descriptors after dynamic evaluation.
```

The current symbolic SCC probe avoids persistent CSR edge storage but still visits millions of logical range neighbors and produces a different SCC partition. Therefore a compact storage shadow is not yet a compact execution model.

## 1. Why the 212 edges were unclassified

The previous validator recognized:

```text
direct cell
symbolic range
named range
```

The remaining 212 CSR dependencies had no cell coordinate and were not present in the formula-to-name reverse map. Their `VertexKind` histogram was:

```text
Table: 212
```

They are table vertices. The validator now has an explicit table provenance category:

```text
expanded formula edges: 573,502
direct cell edges:      523,841
named edges:             49,449
table edges:                212
unclassified edges:           0
```

This eliminates all 212 unclassified edges without pretending table vertices are ordinary cells.

## 2. Dependency provenance status

The current diagnostic model now records:

| Provenance | Exact status | Measurement |
| --- | --- | ---: |
| Direct cell dependencies | Exact for inspected CSR edges | 523,841 edges |
| Symbolic range dependencies | Exact as stored range descriptors and dirty-query inputs | 17,936 descriptors |
| Named-range dependencies | Exact for inspected reverse records | 49,449 records |
| Table dependencies | Exact for inspected CSR vertices | 212 edges / 2 table records |
| Dynamic references | Formula-level descriptor count plus runtime live-edge fingerprint | 270 mixed-SCC members |
| Conditional/live reads | Runtime live edges are recorded after branch execution; branch provenance is not yet a separate edge bit | 13,341 conditional formulas |
| Spill/array dependencies | Spill registry is available; no active Fossil spill anchors were present in the compact stats snapshot | 0 anchors in snapshot |
| Cross-sheet relationships | Explicit CSR count and runtime sheet boundary metadata | 32,928 CSR edges; 2,022,729 live edges per major direction |

Thus all previously unclassified CSR edges are classified, but a unified edge-level provenance record for conditional branches, dynamic targets, and spill projections is still required before a compact SCC driver can be considered exact.

## 3. Exact dirty-set parity

A compact dirty-closure probe was added behind the existing graph API. It compares:

```text
compact path:
  direct reverse edges
  cell -> name reverse records
  stripe/interval range lookup

independent oracle:
  direct reverse edges
  cell -> name reverse records
  exact scan of every registered symbolic range descriptor
```

Results for the existing controls:

| Control | Compact set | Oracle set | Missing | Extra | Exact |
| --- | ---: | ---: | ---: | ---: | --- |
| Fossil after initial | 28,565 | 28,565 | 0 | 0 | yes |
| Fossil F7=300 write | 28,565 | 28,565 | 0 | 0 | yes |
| Fossil same-value write | 28,565 | 28,565 | 0 | 0 | yes |
| Fossil F7=301 write | 28,565 | 28,565 | 0 | 0 | yes |
| Fossil unrelated cell | 0 | 0 | 0 | 0 | yes |
| INDIRECT control | 2 | 2 | 0 | 0 | yes |
| OFFSET control | 2 | 2 | 0 | 0 | yes |
| FILTER control | 2 | 2 | 0 | 0 | yes |
| Same-sheet cycle | 3 | 3 | 0 | 0 | yes |
| Cross-sheet cycle | 2 | 2 | 0 | 0 | yes |

Raw result:

```text
docs/issue-solutions/data/compact-dirty-parity.json
```

### Dirty propagation conclusion

For the tested cell-edit controls, interval/stripe indexes can answer range-dependent dirty queries exactly without materializing every range/cell edge. The current engine already takes this path. It is an exact traversal optimization, not merely a storage estimate.

The independent oracle is intentionally slower and scans descriptors. It proves query parity; it is not proposed as the runtime implementation.

## 4. Symbolic SCC experiment

The symbolic SCC probe builds transient neighbors from:

```text
direct CSR dependencies
virtual dynamic dependencies
symbolic range descriptor -> sheet interval index query
name/table dependencies
```

It does not insert the transient relationships into the persistent dependency graph.

### Fossil measurements

| Metric | Current static scheduler | Symbolic range probe |
| --- | ---: | ---: |
| Vertices | 167,161 | 167,161 |
| SCC count | 159,844 | 152,168 |
| Cyclic SCC count | 84 | 175 |
| Largest SCC | 4,829 | 4,829 |
| Direct edge count | 575,264 | 575,264 |
| Range descriptors | not traversed by current static Tarjan | 17,936 |
| Range neighbor visits | not applicable | 3,116,386 |
| Transient unique edges | persistent/direct control | 3,691,650 |
| Probe time | ~330 ms | ~22,141 ms |
| Transient memory estimate | not applicable | ~67.1 MB |

Partition fingerprints:

```text
current static + virtual: 13618816257583485369
symbolic ranges + virtual: 16460086251032592181
equal: false
```

Raw result:

```text
docs/issue-solutions/data/symbolic-scc-probe.json
```

### Why the SCC partition differs

A symbolic range descriptor is mapped to a set of cell vertices by an interval query. That mapping is cell-level exact for the declared range. However, current static scheduling does not feed every `formula_to_range_deps` descriptor into static Tarjan. It uses the direct CSR graph plus virtual dynamic dependencies and special compressed-range self-loop handling.

Adding all symbolic range relations therefore changes the static graph semantics:

```text
current control:    84 cyclic SCCs
symbolic relation: 175 cyclic SCCs
```

The largest SCC happens to remain 4,829, but the global partition is not equal. The difference is not caused by hashing or record ordering; it is caused by adding declared range relationships to a scheduler that currently treats those relationships through a separate invalidation/runtime-live path.

A symbolic range node is safe only when it is formula-specific and its target set has exactly the same membership, bounds, sheet resolution, branch state, and spill semantics as the cell-level relation it represents. A shared node for equal-looking ranges would incorrectly join formulas that merely read the same region.

## 5. Runtime-live SCC parity

The current runtime evaluator observes live reads after formula execution. For the Fossil main SCC:

```text
static SCC task:              4,829 members
final runtime-live cycle:     4,139 members
live cycle count:                 1
live-edge fingerprint: 1142813687581787051
```

The runtime live graph is built from member-to-member target identities observed during scalar/range reads. It is deliberately different from the declared/static dependency graph because conditional and dynamic references can be untaken or can resolve to a different region.

The symbolic static probe does not produce a runtime-live overlay; it only walks declared range descriptors plus virtual dependencies. Therefore an exact compact-runtime-live SCC parity result has not been claimed. The current runtime-live graph remains the correctness oracle.

To replace it, a future design must preserve an equivalent target identity representation for each executed dynamic/range read. It may be a compact bitmap, interval set, or sorted target vector, but it cannot be only a formula/name descriptor after values have selected a dynamic target.

## 6. Where cell-level expansion is required

| Operation | Cell-level expansion required? | Reason |
| --- | --- | --- |
| Formula evaluation | Not as graph edges; range values can be read through Arrow/chunk views | The evaluator needs values, not a persistent dependency edge per cell. |
| Dirty propagation for a cell edit | No, for declared ranges | Interval/stripe lookup plus exact bounds checks produced exact parity. |
| Dirty propagation for structural row/column edits | No persistent expansion, but exact bounds/occupancy queries are required | Open-ended/whole-axis bounds and used-region changes must be respected. |
| Static SCC detection for direct cell dependencies | Yes at least for direct graph adjacency, or an equivalent direct edge index | Tarjan needs dependency reachability. |
| Static SCC detection for symbolic ranges | No persistent expansion is theoretically required; logical target enumeration/query is required | A range can connect a formula to any formula cell in its region. The probe still visited 3.1M neighbors. |
| Runtime-live SCC tracking | Target identity is unavoidable; persistent cell edges are not strictly required | Conditional/dynamic reads must record exactly which SCC members were read. |
| Dynamic-reference resolution | Target/range bounds must be resolved at runtime | `INDIRECT`/`OFFSET` targets depend on values and can change topology. |
| Spill/array handling | Cell-level occupancy/ownership is required at spill boundaries | Collision, teardown, blockers, and projections are per-cell semantics. |
| Debugging/telemetry | No; compact samples/fingerprints are sufficient | Full edge materialization is optional for diagnostics. |

The narrowest expansion boundary is therefore:

```text
Persistent dirty dependency index:
  symbolic ranges/names + interval/stripe reverse lookup

Static SCC candidate traversal:
  direct CSR + virtual dependencies, with lazy range overlap traversal only
  when the selected semantic policy requires declared range relations

Runtime SCC execution:
  transient target-level live-edge overlay for the active SCC only

Spill:
  cell-level ownership/occupancy maps
```

A full eager expansion of all workbook ranges is not proven necessary for dirty propagation. It is also not justified for SCC discovery until the current static-vs-symbolic semantic mapping is resolved.

## 7. Graph-size, memory, and timing measurements

### Persistent layout estimate

After initial evaluation:

```text
expanded CSR edges:        575,264
compact shadow records:     88,678
ratio:                         6.4871x
estimated compact bytes:   1,578,336
```

The compact byte number covers symbolic descriptor/index entries only; it is not a complete RSS replacement. Native RSS remained approximately 493–497 MB across evaluation/F7/no-op in earlier measurements.

### Symbolic SCC execution

The symbolic probe avoided persistent CSR insertion but built transient relationships:

```text
range neighbor visits:      3,116,386
transient unique edges:     3,691,650
transient memory estimate: ~67.1 MB
probe time:                 ~22.1 s
```

It did not produce a safe wall-time benefit. It was slower than the current static SCC probe and changed the SCC partition.

### Dirty propagation

The current compact range path and independent descriptor oracle produced identical sets for all existing controls. No separate optimized driver was enabled, so:

```text
dirty-set semantic improvement: exact parity
measured runtime improvement:  not applicable
```

The compact path already avoids eagerly expanding range relationships for dirty queries.

### Plan/build/evaluation

The shadow stats collection itself took approximately `0.286 ms` after initial evaluation. The Fossil initial evaluation remained approximately `32.5 s`; F7/no-op control walls remained approximately `11.9 s/11.2 s` in the captured run.

Because the prototype does not drive the evaluator:

```text
graph build improvement:        not measured as an alternate driver
dependency-plan improvement:   not measured as an alternate driver
SCC discovery improvement:      none; symbolic probe is slower
F7 wall-time improvement:       0
no-op wall-time improvement:    0
```

This is intentional: no compact result is returned to users before exact parity is proven.

## 8. Can direct symbolic SCC traversal be exact?

### Possible algorithm

For a declared/static graph, a semantically careful algorithm could use:

```text
FormulaNode -> DirectCell targets
FormulaNode -> FormulaSpecificRangeDescriptor
FormulaNode -> NameNode/TableNode
RangeDescriptor -> interval-index target iterator
NameNode -> definition dependency descriptors
DynamicNode -> runtime virtual target overlay
```

The SCC walker would use a lazy neighbor cursor:

1. yield direct CSR dependencies;
2. yield formula-specific name/table dependencies;
3. for each range descriptor, resolve sheet/bounds and query the interval index;
4. deduplicate target vertices for that source;
5. feed those targets into an iterative Tarjan/Kosaraju walker;
6. retain only SCC membership/state, not the full expanded edge matrix.

This can avoid persistent edge materialization. It does **not** avoid logical neighbor traversal. In the Fossil probe, the cursor would still process 3,116,386 range targets.

### Why it cannot yet replace current SCC discovery

The algorithm needs a defined mapping for:

- declared ranges versus current scheduler’s compressed-range policy;
- conditional branches versus runtime live reads;
- dynamic target changes;
- named formula definitions;
- table vertices;
- cross-sheet `Current` resolution;
- spill/array projections;
- virtual dependency replan rounds.

The current probe proves that adding all declared range neighbors changes the current SCC partition. Until that semantic difference is resolved, direct symbolic SCC traversal is a diagnostic algorithm, not a production replacement.

## 9. Correctness risks

A compact implementation must not:

- merge formula-specific range descriptors into a shared node merely because bounds match;
- treat a declared but untaken conditional read as a runtime-live cycle;
- reuse a dynamic target after its selector changes;
- omit whole-row/whole-column used-region changes;
- forget name/table definition revisions;
- treat spill output as a scalar dependency only;
- miss cross-sheet range resolution or `Current` semantics;
- skip external/source revisions;
- change static cycle detection policy accidentally by adding compressed ranges;
- replace target identity with only range shape;
- assume interval candidate membership proves active/live execution;
- use compact data before the dirty/SCC parity oracle is exact.

## 10. Minimal safe production-adjacent step

Do not connect the symbolic SCC probe to evaluation yet. The next safe engineering step is:

```text
Create a typed DependencyRelation model and make the existing compact dirty
index expose exact reverse range/name queries, while retaining the current CSR
and SCC evaluator as the oracle.
```

The first diagnostic-only implementation should add:

```text
DirectCell(VertexId)
SymbolicRange(FormulaSpecificRangeId)
NamedRange(VertexId)
Table(VertexId)
DynamicDescriptor(DynamicDependencyId)
ConditionalLiveRead(LiveReadId)
SpillProjection(SpillAnchorId, CellRegion)
CrossSheetBoundary(SheetId, SheetId)
```

Then validate:

```text
compact dirty closure == current expanded dirty closure
compact declared SCC candidate == current logical policy
compact runtime-live SCC == current live-edge SCC
```

No scheduler cut-over is justified until all three are exact.

## 11. Recommended greenfield dependency architecture

```text
Formula IR
  -> formula-specific typed dependency relations
       direct cell
       bounded/open range
       named range
       table
       dynamic descriptor
       conditional branch descriptor
       spill projection
       cross-sheet boundary

Persistent indexes
  -> direct reverse adjacency
  -> interval tree / stripe index for ranges
  -> name/table reverse index
  -> dynamic selector/shape registry
  -> spill ownership index

Incremental calculation
  -> symbolic dirty closure
  -> retained calculation order
  -> lazy range neighbor cursor only at SCC/cycle boundaries
  -> runtime live-read overlay for active SCCs
  -> serial circular workspace
  -> parallel acyclic/range kernels
```

The core principle is:

```text
Do not store every equivalent range/cell edge permanently.
Do not pretend that a range descriptor eliminates the need to inspect target
identity when proving a cycle or handling a runtime dynamic/spill read.
```

## Final status

```text
212 unclassified edges:              eliminated; all are table edges
CSR provenance:                       exact for direct/name/table categories
Dirty-set parity:                     exact for Fossil/micro controls
Symbolic static SCC parity:           not achieved; partition differs
Runtime-live SCC parity:              not replaced; current overlay is oracle
Direct symbolic dirty traversal:      yes, proven on controls
Direct symbolic SCC traversal:        possible lazily, not semantics-equivalent yet
Persistent edge reduction:            ~6.49x shadow record estimate
Measured safe wall-time gain:         none from shadow prototype
Production compact driver:            not implemented
Production cycle semantics:           unchanged
Main merge/push:                      not performed
```
