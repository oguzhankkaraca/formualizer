# Excel vs Formualizer Fossil Calculation Investigation

- **Branch:** `investigation/fossil-excel-calculation`
- **Status:** Measurement phase complete; H3/H7 internal Excel mechanisms remain observable only through controlled proxies; no production cycle semantic changes
- **Primary workbook:** `Fossil_EstimatingTemplate_2026-08_21_A.xlsx`
- **Equivalent logical input:** old `Inputs!F6`, new `Inputs!F7`, both set to `300`
- **Objective:** determine why Excel recalculates this workbook faster before changing Formualizer’s production recalculation behavior.

## Constraints

This investigation must not use the following as a final solution:

- hard-coded Fossil cell/output targets;
- disabling iterative calculation;
- globally skipping dynamic or volatile formulas;
- making targeted Outputs evaluation the default product behavior;
- assuming a larger memory budget fixes a CPU/SCC workload;
- treating historical Excel patent material as proof of current Excel internals.

Diagnostic controls are allowed only to establish causality. Any eventual optimization must preserve full workbook semantics and pass Excel output parity.

## Source-claim table

| Claim | Source | Authority | Confidence | Implication for Formualizer |
| --- | --- | --- | --- | --- |
| Excel calculation consists conceptually of dependency-tree construction, calculation-chain construction, and cell recalculation. | [Microsoft — Excel Recalculation](https://learn.microsoft.com/en-us/office/client-developer/excel/excel-recalculation) | Microsoft documentation | High | Measure graph, chain/plan, and evaluation separately. |
| Changed cells/names mark direct and indirect dependents dirty; the next recalculation evaluates dirty cells and their dependents. | [Microsoft — Excel Recalculation](https://learn.microsoft.com/en-us/office/client-developer/excel/excel-recalculation) | Microsoft documentation | High | Compare dirty roots/closure with evaluated closure. |
| Volatile formulas and their dependents are recalculated on every recalculation. `NOW`, `TODAY`, `RANDBETWEEN`, `OFFSET`, `INDIRECT`, context-dependent `INFO`/`CELL`, and some `SUMIF` cases are listed as volatile. | [Microsoft — Excel Recalculation](https://learn.microsoft.com/en-us/office/client-developer/excel/excel-recalculation) | Microsoft documentation | High | Do not globally skip volatile work; narrow only proven safe boundaries. |
| Excel can revise its calculation chain when it finds a formula depending on an uncalculated formula; early calculations after opening can be slower. | [Microsoft — Excel Recalculation](https://learn.microsoft.com/en-us/office/client-developer/excel/excel-recalculation) | Microsoft documentation | High | Separate cold-open and warm-session timings. |
| Excel has Automatic, Automatic Except Tables, and Manual calculation modes. | [Microsoft — Excel Recalculation](https://learn.microsoft.com/en-us/office/client-developer/excel/excel-recalculation) | Microsoft documentation | High | COM harness must record and control calculation mode. |
| Smart recalculation usually evaluates changed/dirty cells, dependents, and volatile cells/dependents rather than every formula. | [Microsoft — Improving calculation performance](https://learn.microsoft.com/en-us/office/vba/excel/concepts/excel-performance/excel-improving-calculation-performance) | Microsoft documentation | High | Distinguish planned/evaluated work from total formula count. |
| Excel stores/reuses recent calculation sequence information; later calculations can be faster. Multi-core scheduling can also benefit from prior results. | [Microsoft — Improving calculation performance](https://learn.microsoft.com/en-us/office/vba/excel/concepts/excel-performance/excel-improving-calculation-performance) | Microsoft documentation | High | Measure retained state, not only formula execution. |
| Excel supports forward and backward references and dynamically determines calculation order rather than using fixed row/column order. | [Microsoft — Improving calculation performance](https://learn.microsoft.com/en-us/office/vba/excel/concepts/excel-performance/excel-improving-calculation-performance) | Microsoft documentation | High | XML order and sheet order are not sufficient calculation models. |
| `Application.Calculate` calculates all open workbooks; worksheet/range objects can scope calculation. | [Application.Calculate](https://learn.microsoft.com/en-us/office/vba/api/excel.application.calculate) | Microsoft API reference | High | Use as the smart/normal calculation control with explicit scope. |
| `Application.CalculateFull` forces full calculation of all data in open workbooks. | [Application.CalculateFull](https://learn.microsoft.com/en-us/office/vba/api/excel.application.calculatefull) | Microsoft API reference | High | Separate full calculation from smart recalculation. |
| `Application.CalculateFullRebuild` forces full calculation and rebuilds dependencies, similar to re-entering formulas. | [Application.CalculateFullRebuild](https://learn.microsoft.com/en-us/office/vba/api/excel.application.calculatefullrebuild) | Microsoft API reference | High | Measure retained calculation state versus dependency rebuild. |
| Open XML calculation chain records last calculated formula order, not the dependency tree; it need not dictate runtime order. | [Working with the calculation chain](https://learn.microsoft.com/en-us/office/open-xml/spreadsheet/working-with-the-calculation-chain) | Microsoft/Open XML documentation | High | Use calcChain as historical order evidence, not dependency truth. |
| Partial calculation can ignore formulas not required by changed cells and move newly required formulas earlier in the chain. | [Working with the calculation chain](https://learn.microsoft.com/en-us/office/open-xml/spreadsheet/working-with-the-calculation-chain) | Microsoft/Open XML documentation | High | Compare Formualizer dirty closure with Excel effective calculated closure. |
| Iterative circular calculation requires repeated calculations, is slow, and is single-threaded according to Microsoft’s performance guidance. Cross-sheet circular references are especially costly. | [Tips for optimizing performance obstructions](https://learn.microsoft.com/en-us/office/vba/excel/concepts/excel-performance/excel-tips-for-optimizing-performance-obstructions) | Microsoft documentation | High | Separate serial circular work from parallel acyclic work. |
| Before iterative calculation starts, Excel recalculates to identify circular references and dependents; Microsoft describes this as equivalent to two or three iterations. | [Tips for optimizing performance obstructions](https://learn.microsoft.com/en-us/office/vba/excel/concepts/excel-performance/excel-tips-for-optimizing-performance-obstructions) | Microsoft documentation | High | Fit fixed overhead plus per-iteration cost. |
| Each iteration can include circular cells, their dependents, and volatile cells/dependents. Reducing the circular calculation region is recommended. | [Tips for optimizing performance obstructions](https://learn.microsoft.com/en-us/office/vba/excel/concepts/excel-performance/excel-tips-for-optimizing-performance-obstructions) | Microsoft documentation | High | Measure the effective circular working set rather than assuming the static SCC is Excel’s set. |
| Structured table references are generally preferred over broad whole-column/dynamic-range alternatives; `OFFSET` is volatile and `INDEX` is generally preferred for dynamic ranges. | [Tips for optimizing performance obstructions](https://learn.microsoft.com/en-us/office/vba/excel/concepts/excel-performance/excel-tips-for-optimizing-performance-obstructions) | Microsoft documentation | High | Classify range origins and dynamic dependencies. |
| The patent describes dependency trees, child chains, dependency levels, multiple recalculation engines, and synchronization. | [US8032821B2](https://patents.google.com/patent/US8032821B2/en) | Historical patent | Medium | Use as historical architecture evidence only, not current Excel proof. |
| Excel remembers calculation sequence and can calculate independent branches in parallel. | [Decision Models — Excel Calculation Process](https://www.decisionmodels.com/calcsecretsc.htm) | Empirical/secondary | Medium | Supports cold/warm and retained-order experiments; COM remains primary evidence. |
| Circular iteration repeatedly calculates until max iterations or max change and uses a distinct circular calculation process. | [Decision Models — Circular References, Goal Seek and Solver](https://www.decisionmodels.com/calcsecretsk.htm) | Empirical/secondary | Medium | Compare convergence trajectories, not only final values. |
| Cross-sheet circular references and unnecessary circular regions can materially slow calculation. | [Decision Models — Circular References, Goal Seek and Solver](https://www.decisionmodels.com/calcsecretsk.htm) | Empirical/secondary | Medium | Map cross-sheet SCC boundaries and compare Excel behavior. |

## Existing Formualizer baseline

Measured before this investigation branch’s new experiments:

| Metric | Old `2026-06-25_X` | New `2026-08-21-A` |
| --- | ---: | ---: |
| XLSX size | 5.54 MB | 11.04 MB |
| Sheets | 15 | 23 |
| Formula cells | 74,312 | 94,966 |
| XML cell records | 517,802 | 2,558,340 |
| Defined names | 3,906 | 4,127 |

With `iterate`, `Inputs!F7 = 300` on the new workbook:

```text
load:                 ~1.66 s
initial evaluate:    ~33.37 s
F7 mutation:          ~17 ms
F7 full recalc:      ~12.74 s
no-op recalc:        ~12–15 s
```

The main mixed SCC is:

```text
members:                    4,829
static members:             4,559
dynamic/volatile members:     270
static internal live edges: 2,037,865
frontier boundary edges:      38,265
static cycle members:        3,868
```

It is distributed across:

```text
CashFlow Inputs: 4,130 members
CashFlow Engine:   695 members
```

After F7:

```text
SCC units considered:       142
SCC units reused:              6
SCC units invalidated:       136
SCC member evaluations:   28,952
iterated SCCs:                137
settle passes:                276
capped SCCs:                    0
```

No-op after F7:

```text
SCC tasks:                 84
SCC member evaluations: 14,802
dirty at request start: 20,710
```

## Benchmark methodology

### Excel

- Excel was automated through `Excel.Application` COM with `Visible=False` and `DisplayAlerts=False`.
- Excel version/build observed in the run: `16.0`, build `20228`.
- Iterative calculation was enabled with `MaxChange=0.001`.
- `MaxIterations` was swept over `1,2,3,5,10,20,50,100`.
- Warm runs reused the same Excel process for each iteration limit and reopened the unsaved workbook between samples; the workbook was not saved.
- Cold runs created a new Excel application and opened the workbook for every sample.
- Each measurement had seven samples; median, min, max, and p95 are retained in the raw data.
- `xlCalculating` was awaited. `xlPending` after a capped iterative `Calculate` call was treated as a valid returned state, not as an infinite wait condition.
- `Calculate`, `CalculateFull`, and `CalculateFullRebuild` were timed as separate operations.
- MTC controls were run with 24 threads enabled and with MTC disabled/one thread.
- Micro circular fixtures were persisted with numeric formula caches so Excel could start from a defined iterative state. The `_xlfn._xlws.FILTER` token was used for the dynamic-array fixture because the Excel build rejected the plain Open XML token.

### Formualizer

- The Fossil workbook was loaded with the Calamine backend in a fresh native Python process for baseline phase measurements.
- `cycleDetection=runtime`, `cyclePolicy=iterate`, `maxIterations=100`, and `maxChange=0.001` were retained for normal measurements.
- Native parallel measurements used `enable_parallel=true` and `false`.
- Live-edge origin and top-source diagnostics were opt-in via `FZ_TRACE_EDGE_ORIGINS=1`.
- Per-iteration trace was opt-in via `FZ_TRACE_SCC_ITERATIONS=1`.
- Formula/output comparisons used the logical input coordinate, not the same physical coordinate across the two template revisions.
- Diagnostic instrumentation did not bypass, cache, or alter the production SCC calculation path.

### Statistics

For a sample vector sorted as `x[0..n)`, p95 is reported as the element at `ceil(0.95*n)-1`. Raw samples remain available for re-analysis.

## Measurement results

### H1 — Static SCC versus final runtime-live SCC

The main mixed SCC was measured after a converged F7 run with opt-in live-edge diagnostics:

| Graph/view | Cycle count | Cycle members |
| --- | ---: | ---: |
| Static SCC task | 1 | 4,829 |
| Static graph after removing dynamic/volatile members | 1 | 3,868 |
| Final runtime-live graph | 1 | 4,139 |

The runtime-live cycle is therefore **690 members smaller** than the static SCC (14.3%), but it is still a 4,139-member cycle. H1 is confirmed as a bounded over-approximation, not as the entire explanation.

The final live-edge fingerprint was identical across initial, F7, and no-op:

```text
1142813687581787051
```

Raw data: `docs/issue-solutions/data/fossil-runtime-live-scc-topology.json`.

### H3/H5 — Formualizer per-iteration trajectory

For the main SCC (`stable_id=1321560910633541638`), with `maxIterations=100` and `maxChange=0.001`:

| Phase | Iteration | Evaluated members | Changed members | Max delta | Pass time |
| --- | ---: | ---: | ---: | ---: | ---: |
| Initial | 1 | 4,828 | 4,829 | 0 | 157 ms |
| Initial | 2 | 4,828 | 377 | 48,245 | 5,857 ms |
| Initial | 3 | 4,828 | 160 | 793 | 5,709 ms |
| Initial | 4 | 4,828 | 0 | 0 | 5,577 ms |
| F7 | 1 | 4,828 | 12 | 0 | 156 ms |
| F7 | 2 | 4,828 | 0 | 0 | 5,246 ms |
| No-op | 1 | 4,828 | 12 | 0 | 156 ms |
| No-op | 2 | 4,828 | 0 | 0 | 5,122 ms |

The no-op performs a full second pass over 4,828 members even though the first pass has no semantic value changes and the second pass also produces zero changes. This is direct evidence for H5.

Raw data: `docs/issue-solutions/data/fossil-scc-iteration-trace.json`.

### H2/H4 — Excel real-workbook sweep

Excel 16.0 build 20228 was measured with 24 calculation threads, `Iteration=True`, and `MaxChange=0.001`. The eight iteration limits were each measured with seven warm and seven cold samples.

The real-workbook `Application.Calculate` F7 timing was flat across `MaxIterations=1..100`, approximately 0.10–0.12 s. Observation targets remained unchanged across the iteration sweep. Corrected same-value-write and MTC control measurements were also approximately flat in `k`:

| Excel setting | `k=1` F7 median | `k=100` F7 median | `k=1` same-value median | `k=100` same-value median |
| --- | ---: | ---: | ---: | ---: |
| MTC enabled, 24 threads | 112.8 ms | 113.0 ms | 112.4 ms | 109.9 ms |
| MTC disabled, 1 thread | 353.9 ms | 338.4 ms | 365.6 ms | 334.9 ms |

With MTC enabled, the warm full/rebuild controls were:

```text
CalculateFull:        ~127–136 ms
CalculateFullRebuild: ~625–657 ms
```

Cold open measurements were approximately 8.3–9.2 s and include Excel process/application startup; they are not formula-evaluation time.

Raw data:

```text
docs/issue-solutions/data/excel-fossil-2026-08-21-A-recalc-sweep.json
docs/issue-solutions/data/excel-fossil-mtc-on-corrected.json
docs/issue-solutions/data/excel-fossil-mtc-off-corrected.json
```

Interpretation: retained calculation/dependency state matters (`CalculateFullRebuild` is slower than `Calculate`), but it cannot explain Formualizer’s 12–15 s no-op. Excel’s effective work for this edit path is much smaller or substantially more compact than Formualizer’s repeated mixed-SCC evaluation.

### H3/H8 — Excel micro-tests

The eight diagnostic workbooks were run with seven samples for each of `k=1,2,3,5,10,20,50,100`. Circular fixtures were seeded with persisted formula values so Excel started from a numeric converged state. Examples:

| Case | Excel observation | Formualizer observation |
| --- | --- | --- |
| Unused IF cycle | Each `Calculate` advances the active circular state; `k` controls how far the state moves. | Same numeric trajectory for the tested values. |
| Active IF cycle | Switching the predicate to the non-circular branch returns zero and the next no-op is sub-millisecond. | Same final zero values and no SCC after the branch removes the cycle. |
| Same-sheet cycle | Converges to approximately `B1=40`, `C1=20` after the seeded input mutation. | Same fixed point, with runtime SCC iteration telemetry. |
| Cross-sheet cycle | Converges to approximately `Sheet1!A1=2`, `Sheet2!A1=1`. | Same fixed point, but Formualizer exposes the cross-sheet SCC explicitly. |
| `INDIRECT` target change | `10 -> 20` after changing `D1` from `A1` to `B1`. | `10 -> 20`. |
| `OFFSET` target change | `10 -> 20` after changing the offset selector. | `10 -> 20`. |
| `FILTER` shape fixture | Dynamic-array serialization requires Excel’s `_xlfn._xlws.FILTER` token; target values are recorded, but this fixture is not used as a final shape-parity oracle. | Dynamic-array behavior remains a separate parity gate. |

The micro suite confirms that dynamic target changes and basic same-/cross-sheet cycle values can be compared, but it does not expose Excel’s internal cell/pass counters.

Raw data:

```text
docs/issue-solutions/data/excel-calculation-micro-manifest.json
docs/issue-solutions/data/excel-calculation-micro-results.json
docs/issue-solutions/data/formualizer-calculation-micro-results.json
```

### H6 — Dynamic formula inventory and invalidation

Expanded Formualizer member diagnostics identify 270 dynamic/volatile members in the mixed SCC. The workbook-level formula/XML inventory includes `INDIRECT`, `FILTER`, `SEQUENCE`, `OFFSET`, `VSTACK`, `UNIQUE`, `MAP`, `LAMBDA`, and `CELL`; shared-formula XML means raw XML token counts are lower than the expanded formula-member count.

The dynamic-call cache diagnostic reduced formula calls but left the large SCC workload in place (`~11.4–13.7 s`, 137 SCCs), so dynamic calls are a contributor and invalidation trigger, not the sole dominant cost.

### H7 — Parallelism

Corrected Excel MTC controls:

```text
MTC on, 24 threads:  F7 ~113 ms, FullRebuild ~625–657 ms
MTC off, 1 thread:   F7 ~338–354 ms, FullRebuild ~837–868 ms
```

Formualizer main mixed-SCC pass 2:

```text
parallel=true:  ~5.07 s
parallel=false: ~7.78 s
member count:   4,828 in both cases
```

The Formualizer SCC member loop is sequential, but inner range/aggregate work is thread-sensitive. This is not evidence that Excel’s circular solver itself is parallel; Microsoft documents iterative circular calculation as single-threaded. The safe verdict is that MTC/parallel scheduling materially affects total calculation, while the circular solver’s exact Excel execution mechanism remains unobservable.

### H8 — Cross-sheet circular traffic

Final runtime-live sheet map:

| Sheet | Members | Internal edges | Cross-sheet incoming | Cross-sheet outgoing |
| --- | ---: | ---: | ---: | ---: |
| CashFlow Engine | 695 | 8,940 | 2,022,729 | 33,032 |
| CashFlow Inputs | 4,130 | 3,393 | 33,032 | 2,022,729 |

The two sheets form a strongly coupled, asymmetric cross-sheet loop. This makes sheet order and compact cross-sheet dependency representation important candidates, but does not by itself prove Excel’s current sheet scheduling algorithm.

### H9 — Range/name representation

Final live graph degree distribution:

```text
fan-out median / p95 / max: 688 / 688 / 4,130
fan-in  median / p95 / max: 10 / 2,958 / 3,086
```

Static-to-static live-edge origin observations:

```text
direct_cell:  1,078
range:      812,168
named_range:2,029,240
whole_row:       0
whole_column:    0
table:           0
dynamic:         0
other:           0
```

Origin observations overlap: a named-range read can also materialize as a range read. The dominant structural signal is the approximately 2.03M named-range expansion observations for 2.04M static internal edges. Top source members read up to 4,131 internal targets each, and repeated source groups read 688 targets each.

Raw data: `docs/issue-solutions/data/fossil-static-edge-origin-breakdown.json` and `docs/issue-solutions/data/fossil-runtime-live-scc-topology.json`.

## Current investigation verdicts

| Hypothesis | Verdict | Evidence/status |
| --- | --- | --- |
| H1 — Formualizer static SCC is over-approximated | CONFIRMED, bounded | Static SCC has 4,829 members; final runtime-live cycle has 4,139 members. The reduction is material but not a small working set. |
| H2 — Excel iterates fewer cells | CONFIRMED by controlled timing/output evidence | Excel real-workbook `Calculate` is flat across `k=1..100` (~0.10–0.11 s) and observed targets are unchanged; direct Excel cell-count telemetry is unavailable. |
| H3 — Excel converges in fewer iterations/order | INCONCLUSIVE | Formualizer’s main SCC requires 4 passes on initial evaluation and 2 passes after F7/no-op. Excel’s COM API does not expose per-cell iteration counts; micro-tests show different initial-state/order behavior in some cycle cases. |
| H4 — Excel retained calculation order/state is a major factor | REJECTED as dominant for this gap | Excel `Calculate` is ~0.10–0.11 s, `CalculateFull` ~0.13–0.14 s, and `CalculateFullRebuild` ~0.62–0.66 s. Rebuild state matters, but cannot explain Formualizer’s 12–15 s no-op. |
| H5 — Formualizer repeats a stable fixed point unnecessarily | CONFIRMED | No-op values and live-edge fingerprint are unchanged, while 84 SCC tasks and 14,802 member evaluations still execute. |
| H6 — Dynamic formulas cause excessive invalidation | PARTIALLY CONFIRMED | 270 dynamic/volatile members participate and dirty propagation is large, but dynamic-call caching alone left the mixed SCC workload intact. |
| H7 — Circular execution is not benefiting from parallelism | CONFIRMED for Excel’s total path; solver-specific mechanism INCONCLUSIVE | Excel MTC 24-thread vs 1-thread changes F7 median from ~0.11 s to ~0.34–0.35 s, but `k=1` and `k=100` are flat. Formualizer’s main SCC pass 2 changes ~5.07 s to ~7.78 s with native parallel disabled. |
| H8 — Cross-sheet circular order matters | CONFIRMED structurally; causal timing INCONCLUSIVE | The main runtime-live SCC spans `CashFlow Inputs` and `CashFlow Engine`, with approximately 2,022,729 live edges in each cross-sheet direction. |
| H9 — Range representation is structurally dense | CONFIRMED | Static live graph has 2,037,865 edges; static origin observations are ~2.03M named-range and ~0.81M range, with fan-out median/p95/max 688/688/4,130 and fan-in median/p95/max 10/2,958/3,086. |

## HyperFormula comparison

HyperFormula’s setup pattern is:

```text
create sheets
create named ranges
set cells
suspend evaluation
calculate once
```

Formualizer already defers formula graph construction in its interactive load configuration and does not evaluate every source cell during Calamine ingest. The measured problem occurs after the graph is available:

```text
dynamic/volatile redirty
-> mixed SCC scheduling
-> repeated full SCC passes
```

Therefore another generic suspend switch is not expected to fix the no-op case. Any optimization must preserve Excel’s volatile and circular semantics.

## Final explanation ranking

Among the proposed explanations, the evidence ranks as follows:

1. **G — Multiple factors contribute materially.** This is the correct overall verdict.
2. **E — Formualizer’s dependency graph is excessively conservative/dense.** This is the primary measured structural cause: 2,037,865 static live edges, named-range expansion dominating edge observations, and a 4,829-member static task for a 4,139-member runtime-live cycle.
3. **A — Excel evaluates a much smaller circular working set.** Partially true for this edit path, but not a tiny set: Formualizer’s runtime-live cycle is still 4,139 members. Excel’s direct cell count is not exposed by COM, so this remains an inference from flat `k` timing and output behavior.
4. **B — Excel needs fewer iterations.** Likely true for the F7/no-op path, where Excel timing is independent of `k`, but direct Excel pass counters are unavailable. Formualizer needs two full passes after F7/no-op, including a zero-change second pass.
5. **C — Excel evaluates equivalent work much faster.** MTC and compact internal structures contribute, but the 12–15 s versus ~0.1 s gap is too large to attribute to arithmetic throughput alone.
6. **F — Dynamic references cause excessive invalidation.** Confirmed as a contributor, not the dominant cause. Dynamic-call caching did not eliminate the large SCC workload.
7. **D — Excel benefits from retained calculation order/state.** Present but not dominant for this gap. `CalculateFullRebuild` is ~5–6x normal `Calculate`, while normal Excel F7/no-op remains ~0.1 s.

The strongest causal statement supported by the data is:

```text
Excel avoids or compactly represents most of the work that Formualizer currently
re-executes through a dense, cross-sheet, mixed SCC. Formualizer also performs a
full zero-change confirmation pass on the main SCC during F7/no-op recalculation.
MTC/order state and dynamic invalidation are secondary contributors.
```

## Timing decomposition

For the new workbook:

```text
Formualizer F7 wall time:       ~12.7 s
  runtime SCC time:             ~11.6 s
  main SCC second pass:          ~5.2 s
  SCC member evaluations:       28,952

Formualizer no-op wall time:    ~12–15 s
  runtime SCC time:             ~12.2 s
  main SCC second pass:          ~5.1 s
  main SCC changed members:          0
  main SCC live-edge changes:        0

Excel warm F7 Calculate:       ~0.10–0.12 s
Excel warm same-value write:   ~0.10–0.12 s
Excel warm CalculateFull:      ~0.13–0.14 s
Excel warm FullRebuild:        ~0.62–0.66 s
```

A theoretical no-op optimization that safely eliminated all repeated runtime-SCC evaluation could remove approximately 12 s from the no-op path, but this is an upper bound, not a measured production benefit. It cannot be enabled without volatile, dynamic, boundary, and fixed-point correctness guards.

## Recommended greenfield recalculation architecture

A greenfield engine should separate five layers:

1. **Formula IR and reference descriptors** — preserve direct cells, bounded ranges, whole rows/columns, names, tables, dynamic references, conditional reads, and spill relationships as distinct dependency types.
2. **Compact dependency index** — represent large range/name relationships symbolically or in compressed interval structures; do not eagerly materialize millions of equivalent cell-to-cell edges when range semantics can answer invalidation queries directly.
3. **Retained calculation plan** — maintain a dependency-aware calculation chain/priority plan, dirty closure, layer metadata, and warm ordering state. Rebuild only after structural/semantic changes.
4. **Circular workspaces** — identify static SCCs, observe runtime live subgraphs, and keep convergence state per topology-aware workspace. Separate fixed-point members, boundary inputs, volatile/dynamic frontier, and dependent acyclic work.
5. **Semantic invalidation contract** — invalidate by dependency/topology/shape/value revisions, function-provider semantic generation, locale/date system, workbook seed, spill layout, names/tables, and cycle settings.

The circular workspace must not assume that a dynamic frontier leaves an acyclic remainder. Fossil proves that the static remainder can itself contain a 3,868-member cycle.

## Minimal safe retrofit for the current engine

Without changing current calculation semantics, the safest sequence is:

1. Keep the new live-edge, per-iteration, origin, fan-in/fan-out, and sheet-boundary diagnostics.
2. Add a compact range/name dependency representation behind the existing graph API; validate it against the expanded graph before switching scheduling.
3. Add an explicit retained-plan/order cache measurement and reuse only for unchanged topology/semantic configuration.
4. Build a diagnostic-only mixed-workspace simulator that compares the normal full path with a candidate path. Do not return candidate values to users yet.
5. Split deterministic dynamic-reference handling from true value-volatile functions; never treat `NOW`, `TODAY`, `RAND`, `RANDBETWEEN`, context-sensitive `CELL`/`INFO`, or UDF-declared volatility as ordinary cacheable dynamics.
6. Only after full output parity, add a production cache key containing topology/live-edge fingerprint, boundary revisions, dynamic shape metadata, semantic generation, cycle settings, and persisted iterative state.

No workbook-specific cell list should be present in the retrofit.

## Benefit/risk estimates

| Candidate | Evidence-based opportunity | Expected benefit estimate | Main correctness risk |
| --- | --- | --- | --- |
| Fixed-point/no-op workspace reuse | No-op values/topology unchanged; ~12 s runtime SCC time repeated | Up to ~90–96% of no-op wall time in this workbook if fully safe; unmeasured until prototype | Volatile functions, external state, dynamic shape/target changes, stale iterative state |
| Compact named-range/range dependencies | ~2.03M named-range and ~0.81M range origin observations | High potential for graph/SCC memory and scheduling cost; percentage not yet measured | Missing a dependency or changing range/whole-column semantics |
| Runtime-live SCC refinement | Static 4,829 vs live 4,139 members | At most ~14.3% member-count reduction for this SCC alone; not sufficient by itself | Live-edge guards can flip; conditional branches and dynamic reads need exact recording |
| Retained calculation order/plan | Excel FullRebuild ~5–6x normal Calculate | Can reduce rebuild/plan overhead; not the main 12 s gap | Reusing order after structural/name/provider changes |
| Native inner parallel work | Formualizer main SCC pass 2 ~5.07 s parallel vs ~7.78 s serial | ~35% for the measured pass; already available natively | Determinism, scheduling overhead, WASM portability |
| Excel-style MTC/WASM worker pool | Excel MTC on ~3x faster than off on this machine | Potentially material for browser/UI latency; not quantified for WASM | SharedArrayBuffer/COOP/COEP, worker lifecycle, fallback parity |

## Correctness and invalidation requirements

Any future optimization must invalidate on all of the following:

- formula or constant edits and direct/indirect dirty dependents;
- static topology or structural row/column/sheet edits;
- named-range or table definition/shape changes;
- dynamic target, range bounds, spill shape, or array projection changes;
- conditional branch activation changes that alter live reads;
- volatile value sources and UDF-declared volatility;
- function provider replacement or semantic-generation changes;
- locale, date system, workbook seed, deterministic mode, cycle policy, max iterations, and max change;
- external/source snapshot changes;
- iterative state seed and convergence policy changes;
- calculation chain/order revisions where the plan depends on them.

The current diagnostic fingerprint is not sufficient alone: it contains final live-edge pairs and member count, but not all semantic/boundary revisions.

## Source and experiment plan

### H1 — Static versus runtime-live SCC

For each runtime SCC after convergence:

- compute SCCs using only observed live edges;
- compare static and runtime-live membership;
- classify edge origins;
- report top formulas/ranges by internal-edge contribution.

### H2 — Excel maxIterations sweep

Sweep `maxIterations = 1,2,3,5,10,20,50,100` with `maxChange=0.001`. For each setting, measure at least seven runs for initial calculation, F7 edit, repeated no-op, changed-again, same-value write, and unrelated edit. Use fresh Excel processes for cold runs and one process for warm runs.

### H3 — Convergence trajectory

Formualizer must report per iteration:

```text
iteration
evaluated members
changed members
max delta
elapsed
live-edge fingerprint
```

Excel must export relevant formula values after controlled maxIterations runs.

### H4 — Retained order/state

Compare Excel `Application.Calculate`, `CalculateFull`, and `CalculateFullRebuild` across fresh open, warm recalc, F7 alternating values, same-value writes, and no-op calls.

### H5 — Fixed-point reuse opportunity

Do not implement reuse. Measure unchanged values, topology, shapes, boundaries, and actual repeated member work.

### H6 — Dynamic invalidation

For all `FILTER`, `INDIRECT`, `OFFSET`, `SEQUENCE`, `VSTACK`, `UNIQUE`, `MAP`, `LAMBDA`, and volatile formulas touching the mixed SCC, record declared/live dependencies, output values, shapes, topology, and dirty fanout.

### H7 — Parallelism

Separate acyclic and circular work. Compare Excel multithread enabled/disabled and Formualizer parallel enabled/disabled. Do not infer circular parallelism from total workbook time.

### H8 — Cross-sheet ordering

Map SCC members and cross-sheet edges, then compare with small same-sheet/cross-sheet Excel micro-workbooks.

### H9 — Range representation

Measure edge fan-in/fan-out, range/whole-column origins, duplicate dependencies, and compact range-node alternatives without changing formula results.

## Micro-workbook matrix

Create Excel oracle cases for:

- unused IF branch containing a cycle;
- active IF branch containing a cycle;
- changing `INDIRECT` target;
- changing `OFFSET` target;
- changing `FILTER` shape;
- simple two-cell iterative cycle;
- same-sheet cycle;
- cross-sheet cycle.

Each case must export formula values, errors, formulas, timing, calculation mode, iterative settings, and calculation scope.

## Verification status

```text
cargo check -p formualizer-eval:                 passed
iterate_corpus_numeric tests:                    10 passed
Python cycle telemetry tests:                    6 passed
Python diagnostic script compilation:             passed
Excel Fossil sweep:                              passed (7 samples per limit)
Excel micro suite:                               passed (8 cases × 8 limits × 7 runs)
```

The full `formualizer-eval` unit suite ran 2,760 tests with 2,745 passed, 14 ignored, and one formula-suite test failing because of two pre-existing complex-number string round-trip last-bit differences (`IMSIN` and `IMCOS`). No changed production formula implementation is part of this investigation, and those unrelated parity failures were not modified.

## Architecture gate

No production mixed-SCC cache or reuse change is allowed until:

1. Excel’s effective circular working set is measured;
2. Excel’s per-iteration trajectory is measured;
3. Excel cold/warm/rebuild timing is decomposed;
4. static and runtime-live SCCs are compared;
5. dynamic topology/boundary behavior is measured;
6. full formula-output parity passes for initial, edit, no-op, and topology-changing cases.

## Raw data

Tracked machine-readable data belongs under:

```text
docs/issue-solutions/data/
```

The tracked raw result set is:

```text
docs/issue-solutions/data/fossil-excel-calculation-investigation.json
docs/issue-solutions/data/excel-fossil-2026-08-21-A-recalc-sweep.json
docs/issue-solutions/data/excel-fossil-mtc-on-corrected.json
docs/issue-solutions/data/excel-fossil-mtc-off-corrected.json
docs/issue-solutions/data/excel-calculation-micro-manifest.json
docs/issue-solutions/data/excel-calculation-micro-results.json
docs/issue-solutions/data/formualizer-calculation-micro-results.json
docs/issue-solutions/data/fossil-scc-iteration-trace.json
docs/issue-solutions/data/fossil-runtime-live-scc-topology.json
docs/issue-solutions/data/fossil-static-edge-origin-breakdown.json
docs/issue-solutions/data/fossil-top-edge-formulas.json
docs/issue-solutions/data/fossil-dynamic-inventory.json
```
