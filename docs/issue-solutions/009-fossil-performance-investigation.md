# Fossil Template Performance Investigation

- **Branch:** `ui/formualizer-canvas`
- **Status:** Investigation in progress
- **Primary workload:** Fossil estimating workbook, automatic Excel-style recalculation
- **Logical input mutation:** old template `Inputs!F6 = 300`; new template `Inputs!F7 = 300`
- **Cycle configuration:** `cycleDetection = runtime`, `cyclePolicy = iterate`, `maxIterations = 100`, `maxChange = 0.001`

## Goal and invariants

The goal is to make ordinary spreadsheet editing feel like Excel without changing Excel formula semantics. The investigation must not solve the problem by:

- hard-coding output cells;
- disabling iterative calculation;
- globally skipping volatile or dynamic formulas;
- replacing full workbook semantics with a targeted-output-only path.

Targeted evaluation and static/error cycle modes are used only as diagnostic controls, never as the proposed product behavior.

## Workbooks and logical input alignment

The two Fossil files are not coordinate-identical. The new workbook inserted a row in the Inputs sheet:

| Logical input | Old workbook | New workbook |
| --- | --- | --- |
| Required Plant Capacity | `Inputs!F6` | `Inputs!F7` |
| Starting value used in the test | blank/old input layout | blank |
| Test mutation | `300` | `300` |

Using old `Inputs!F7` would be incorrect because that cell is the capacity scenario field in the old workbook. All final A/B performance comparisons use the logical capacity input.

## Baseline measurements

Measurements below use one independent native Python process per workbook to avoid cross-workbook memory retention. `WorkbookSession.model()` was not used for the phase table because it includes model creation and initial evaluation in one call.

### Phase timings

| Phase | Old `2026-06-25_X` | New `2026-08-21-A` |
| --- | ---: | ---: |
| XLSX/Calamine load | 471 ms | 1,660 ms |
| Initial full evaluation | 3,760 ms | 33,369 ms |
| Input mutation | 9 ms | 17 ms |
| Full recalc after logical input mutation | 461 ms | 12,744 ms |

The new workbook is therefore approximately:

- **3.5x slower to load**;
- **8.9x slower on initial evaluation**;
- **27.6x slower on the F7 recalc**.

The raw Calamine load is not the main problem. The major cost begins after staged loading, when the graph is prepared and the workbook is evaluated.

### Cold planning measurements

A cold `get_eval_plan()` measurement was also taken before evaluation:

| Plan metric | Old | New |
| --- | ---: | ---: |
| Cold plan build | ~2.0 s | ~6.4 s |
| Planned vertices after input mutation | 3,964 | 6,402 |
| Dirty vertices after input mutation | 3,896 | 6,333 |
| Plan layers | 38 | 38 |
| Plan build after mutation | 39 ms | 44 ms |

The post-mutation plan build is not responsible for the 12.7 s recalc. The dirty evaluation and runtime SCC work are.

## Workbook shape difference

| Metric | Old | New |
| --- | ---: | ---: |
| XLSX size | 5.54 MB | 11.04 MB |
| Sheets | 15 | 23 |
| Formula cells | 74,312 | 94,966 |
| XML cell records | 517,802 | 2,558,340 |
| Defined names | 3,906 | 4,127 |

The new workbook adds several formula-heavy areas:

- `CashFlow Engine`: 8,246 formulas;
- `CashFlow Inputs`: 4,360 formulas;
- `Executive Comparison`: 2,996 formulas;
- `Monthly Spend Deliverable`: 1,155 formulas;
- `UID Tracker`: 827 formulas.

## Hypothesis results

### H1 — The input mutation invalidates too many SCCs

**Confirmed.**

New workbook after `Inputs!F7 = 300`:

```text
SCC units considered:       142
SCC units reused:             6
SCC units invalidated:       136
SCC member evaluations:   28,952
```

Old workbook after its equivalent `Inputs!F6 = 300`:

```text
SCC units considered:        78
SCC units reused:              6
SCC units invalidated:        72
SCC member evaluations:   17,508
```

### H2 — Iteration alone explains the slowdown

**Rejected as the complete explanation.**

Diagnostic comparison on the new workbook:

```text
iterate + runtime:  F7 recalc ~13.7 s, 137 SCCs
static + error:     F7 recalc  ~5.6 s, 0 SCCs
```

Iteration adds approximately 8 seconds, but the non-iterative path still costs approximately 5.6 seconds. Iteration cannot be disabled for the product because the goal is Excel-compatible circular calculation.

### H3 — Dynamic formulas alone explain the slowdown

**Partially confirmed, but rejected as the sole cause.**

The new workbook contains dynamic functions that are almost absent from the old workbook:

```text
CashFlow Inputs:
  FILTER 130
  INDIRECT 8
  OFFSET 2
  VSTACK 2
  UNIQUE 2
  MAP 1
  LAMBDA 1

CashFlow Engine:
  SEQUENCE 58
  OFFSET 26
  INDIRECT 12
```

The main mixed SCC contains:

```text
4,829 members
  270 volatile/dynamic members
4,559 non-dynamic members
```

Diagnostic copies that converted dynamic formulas to cached values still had approximately 11.4–13.0 s recalc times and approximately 137 SCCs. Dynamic functions contribute to the cost and affect reuse safety, but removing their calls does not remove the large SCC workload.

### H4 — Rayon/parallel scheduling is hurting the new workbook

**Rejected.**

New workbook, same logical input mutation:

```text
parallel=true:   ~12.8 s
parallel=false:  ~19.1 s
```

Native Rayon is active and useful. It should not be disabled.

### H5 — The new workbook repeats the same SCC work on a no-op recalc

**Confirmed.**

Old workbook:

```text
first input recalc:  ~0.5 s
second no-op recalc:  0.3 ms
SCC tasks:            0
```

New workbook:

```text
first input recalc:  ~12–16 s
second no-op recalc: ~12–15 s
SCC tasks:            84
SCC member evals:    14,802
```

This is the primary user-visible problem.

### H6 — Mixed SCC reuse is too coarse

**Confirmed as a strong optimization candidate, not yet proven safe as a production change.**

The large SCC is distributed across:

```text
CashFlow Inputs: 4,130 members
CashFlow Engine:   695 members
```

Its 270 dynamic/volatile members cause the whole 4,829-member SCC to be non-reusable under the current SCC-level reuse predicate.

After the F7 recalc, a no-op recalc reports:

```text
20,710 dirty vertices
4,829 iterative members redirtied
1 mixed SCC forced by iterative policy
```

However, a value fingerprint diagnostic over 9,420 formulas in `CashFlow Inputs` and `CashFlow Engine` found:

```text
changed formula values during no-op: 0
changed dynamic values during no-op:  0
changed static values during no-op:   0
```

This supports the idea that a dynamic frontier can be evaluated while stable static members are reused. A bounded final live-edge fingerprint was then added to the diagnostic and remained stable for the main mixed SCC:

```text
initial: 1142813687581787051
F7 recalc: 1142813687581787051
no-op recalc: 1142813687581787051
```

The current result is strong evidence for stable topology in this workbook, but the fingerprint is diagnostic-only and is not yet a production cache key. It includes the final sorted live-edge pairs and SCC member count; semantic configuration and boundary value revisions still need to be part of a production cache contract.

The first mixed-SCC partition diagnostic produced:

```text
frontier members:             270
static members:             4,559
static live edges:       2,037,865
frontier boundary edges:   38,265
static cycle count:             1
static cycle members:       3,868
```

This rejects the simpler design of treating the static side as an acyclic closure. The static side contains a large cycle of its own, so any reuse design must cache or partition topology-aware components rather than merely evaluating dynamic anchors and then walking a DAG.

### H7 — More memory or a constant buffer pool is the primary fix

**Rejected as the native CPU/recalc root cause.**

Native RSS measurements for the new workbook:

```text
start:       ~16 MB RSS
after load:  ~98 MB RSS
after eval:  ~493 MB RSS
after F7:    ~497 MB RSS
after no-op: ~497 MB RSS
peak:        ~566 MB RSS
```

The no-op recalc does not grow memory. It spends CPU inside the same SCC work. More native memory alone will not make this no-op recalc fast.

The browser/WASM Solar ingest failure is a separate memory/ingest-scale problem and should not be conflated with the Fossil no-op CPU problem.

## Current architecture versus HyperFormula

The HyperFormula pattern is:

```text
create sheets
create named ranges
set cells
suspend evaluation
calculate once
```

Formualizer already has the important load-side equivalent:

```text
WorkbookConfig::interactive
 defer_graph_building = true
Calamine formula staging
no evaluation for every source cell during load
```

The current cost is later:

```text
first evaluation builds/uses the deferred graph
runtime SCCs record live edges
iterative SCC members are redirtied for Excel persistence semantics
volatile/dynamic dirty propagation reaches a large mixed SCC
```

Therefore adding another generic “suspend evaluation” switch is not expected to solve the no-op problem. The optimization must preserve volatile/dynamic evaluation while narrowing the static work that depends on unchanged dynamic boundaries.

## Memory and Rayon architecture findings

### Native

- Rayon is a direct dependency of `formualizer-eval`.
- `EvalConfig.enable_parallel` defaults to `true`.
- `max_threads = None` lets the Rayon pool choose its default size.
- Native parallel evaluation is measurably faster for the new Fossil workload.

### Browser/WASM

The generated browser WASM module contains no thread/atomic imports or exports, and the project has no `wasm-bindgen-rayon` or SharedArrayBuffer worker-pool setup. Therefore native-style multi-thread Rayon is not currently active in the browser.

A future WASM parallel architecture would require:

- `wasm-bindgen-rayon` or equivalent thread bridge;
- SharedArrayBuffer;
- cross-origin isolation (`COOP`/`COEP`);
- worker thread pool startup and lifecycle management;
- fallback to single-thread execution.

This is a valid later optimization, but it is not the first fix for the native SCC reuse problem.

### Buffers and memory

The engine currently uses Arrow-backed stores, `Vec`-based staging, bumpalo scratch support, and formula replay spools. Native formula spool data can spill to disk; WASM uses memory-only spool behavior. There is no general constant buffer pool or alternate allocator switch currently wired as a product configuration.

## Production constraints

The following are explicitly not accepted as the solution:

```text
- turn off iterative calculation;
- skip dynamic/volatile formulas globally;
- hard-code Fossil sheet/cell output targets;
- make targeted Outputs evaluation the default product behavior;
- disable parallelism;
- return cached values without checking dynamic topology.
```

## Next steps

### 1. Completed: add live-edge topology fingerprinting

The runtime SCC evaluator already records per-member live edges using `LiveEdgeCollector`. A bounded fingerprint of the final live-edge set for the mixed SCC is now retained diagnostically and compared across:

```text
initial evaluation
F7 mutation recalc
no-op recalc
```

The current diagnostic fingerprint includes the SCC member count and sorted live-edge pairs. A production cache key must also include enough information to detect:

- member identity/order;
- live edge pairs;
- dynamic range/shape changes;
- named/table resolution changes;
- relevant semantic/topology revisions.

### 2. Completed: partition the mixed SCC diagnostically

The live-edge fingerprint is stable across initial, F7, and no-op recalculation. The 4,829 members were classified into:

```text
dynamic/volatile frontier
static members directly adjacent to the frontier
static components outside the frontier
```

The static side contains one large cyclic component with 3,868 members, so removing the frontier does not leave an acyclic graph.

### 3. Next: run a semantic comparison experiment

For a diagnostic-only mixed reuse mode:

- still evaluate dynamic/volatile frontier members;
- compare live-edge topology fingerprints;
- reuse only a candidate static component when its boundary fingerprint is unchanged;
- compare all formula outputs against the normal full iterate path;
- test F7 edit, no-op recalc, and topology-changing dynamic inputs.

No production flag should be enabled until the Excel oracle and full formula-output comparison pass.

### 4. Implement a general cache contract if the experiment passes

The eventual cache unit should be a topology-aware component, not a workbook-specific cell list or whole mixed SCC. Its key must include semantic configuration, topology/live-edge fingerprint, boundary values/revisions, and dynamic shape metadata.

### 5. Revisit WASM threads and buffers

Only after the native reuse experiment is complete:

- expose actual browser worker parallelism if required;
- profile WASM heap growth separately;
- evaluate constant/reusable scratch buffers for allocation churn;
- keep single-thread fallback and Excel output parity.

## Instrumentation commits

The branch contains diagnostic groundwork only:

```text
7d830e90  expose recalc workload telemetry
6e4a91f3  expose SCC dirty member diagnostics
be2f1c7b  report mixed SCC composition
```

The SCC reuse semantics have not been changed by this investigation.
