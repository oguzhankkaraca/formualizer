# Excel Circular-Set Oracle for Heavy Fossil

- **Branch:** `investigation/fossil-upstream-integration`
- **Workbook:** `Fossil_EstimatingTemplate_2026-08_21_A.xlsx`
- **Excel raw evidence:** `docs/issue-solutions/data/excel-circular-set-oracle.json`
- **COM API control:** `tools/excel-oracle/probe_excel_circular_api.ps1`
- **Oracle harness:** `tools/excel-oracle/probe_excel_circular_set.ps1`
- **Formualizer baseline input:** `docs/issue-solutions/data/latest-upstream-heavy-baseline.json`
- **Production behavior:** unchanged; no Formualizer optimization or Engine V2 implementation.

## Executive conclusion

Excel itself successfully detects a known two-cell circular workbook through `Worksheet.CircularReference`, so the COM probe is operational. For the Heavy Fossil workbook, however, Excel exposed no circular-reference seed on any worksheet after a full dependency rebuild, both with iteration enabled and disabled.

```text
Heavy / iteration enabled:  0 Worksheet.CircularReference seeds
Heavy / iteration disabled: 0 Worksheet.CircularReference seeds
```

Excel also showed no iteration-dependent large working-set behavior:

```text
CalculateFull:        ~124–368 ms
CalculateFullRebuild: ~587–925 ms
warm Calculate:       ~84–307 ms median
```

The current evidence supports **H3** most strongly: Formualizer's 4,829-member region is not demonstrated to be an Excel circular calculation region in the current workbook state. H2 remains possible because Excel's public circular-reference and auditing APIs are incomplete. H1 is not supported by this Excel evidence.

Confidence: **medium-high for “Excel does not expose a comparable Heavy circular region”; not conclusive proof that Excel has no hidden internal feedback behavior.**

## Phase 1 — Excel circular seeds

### Original Excel settings

Captured before changing the disposable session:

```text
Excel version:       16.0
Excel build:         20228
calculation:         0
iteration:           false
max iterations:      0
max change:          0
MTC enabled:         true
MTC thread count:    24
```

Each oracle copy was then configured explicitly with:

```text
calculation:     manual (-4135)
max iterations:  100
max change:      0.001
MTC:             enabled or disabled per experiment
```

### Heavy seed result

| Excel setting | `Worksheet.CircularReference` seeds | Calculation state after rebuild |
| --- | ---: | ---: |
| Iteration enabled | 0 | 0 (`xlDone`) |
| Iteration disabled | 0 | 0 (`xlDone`) |

No sheet/formula/cell was returned by Excel as a circular seed. Therefore:

```text
Excel-reported seed membership in Formualizer static SCC: none
Excel-reported seed membership in runtime-live set: none
```

This is not a claim that the property enumerates a complete circular set. The property returns at most the first circular reference on a worksheet.

### COM control

A disposable workbook containing:

```text
A1 = B1 + 1
B1 = A1 / 2
```

produced:

```text
iteration enabled:
  CircularReference = null
  CalculationState  = 2 (pending/capped iterative calculation)

iteration disabled:
  CircularReference = A1
  CalculationState  = 0
```

The Heavy no-seed result is therefore meaningful evidence, not a blanket failure of the COM property.

## Phase 2 — Excel dependency tracing

Because Excel returned zero Heavy seeds:

```text
ShowPrecedents/ShowDependents seed set: empty
NavigateArrow seed set: empty
DirectPrecedents/DirectDependents seed set: empty
```

No Excel-exposed cross-sheet feedback path was available to trace. The harness records this as an empty evidence set, not as proof that Excel has no internal dependency relationship.

No Formualizer graph was substituted for the missing Excel path evidence.

## Phase 3 — Circular interventions

Ten fresh disposable copies were tested using bulk interventions derived from the Formualizer static member address set:

```text
CashFlow Inputs used range
CashFlow Inputs four row bands
CashFlow Engine used range
CashFlow Engine four row bands
```

Each selected region was frozen to its current Excel-calculated values through a bulk values-only paste, followed by `CalculateFullRebuild` and another `Worksheet.CircularReference` check.

All results were:

```text
before Excel seeds: 0
freeze operation:   succeeded
after Excel seeds:  0
```

Interpretation:

```text
No intervention destroyed an Excel-reported circular condition because no
Excel-reported circular condition existed before the intervention.
```

These are not feedback-cut results and do not identify full Excel SCC membership. They are consistent with H3 but cannot distinguish H2 from H3 by themselves.

## Phase 4 — Formualizer comparison evidence

Formualizer's prior verified evidence records:

```text
largest static SCC:          4,829 members
runtime-live members:        4,139
CashFlow Inputs members:     4,130
CashFlow Engine members:       695
Formualizer dynamic members:   270
Formualizer volatile members:  270
```

Existing live-edge evidence records:

```text
runtime-live cycle members: 4,139
static minus runtime-live:    690
named-range edge observations: approximately 2.03M
range edge observations:       approximately 812K
```

The existing Formualizer runtime-live artifact does not contain the complete runtime-live address set. Consequently, an Excel seed's runtime-live membership cannot be asserted when Excel returns no seed; it is recorded as absent Excel evidence, not as a negative membership proof.

### Dependency families requiring scrutiny

These are **Formualizer candidate causes**, not Excel-proven unsupported edges:

```text
named-range expansion
large range/open-range expansion
INDIRECT/OFFSET dynamic references
conditional/live-branch differences
array/spill handling
unsupported functions and #NIMPL propagation
#REF/#VALUE/error coercion differences
```

Formualizer's static-edge artifact specifically reports approximately 2.03M named-range edge observations and 812K range observations in the main region. The Excel oracle did not expose a corresponding circular seed or trace path supporting those edges as one broad circular calculation region.

This does not prove every named/range edge is wrong. It identifies the main gap between Formualizer's graph evidence and Excel-observable feedback evidence.

## Phase 5 — Excel calculation behavior

Fresh disposable sessions measured three warm `Application.Calculate` calls, then `CalculateFull` and `CalculateFullRebuild`.

| Iteration | MTC | Warm `Calculate` median | `CalculateFull` | `CalculateFullRebuild` |
| --- | --- | ---: | ---: | ---: |
| enabled | enabled | 83.709 ms | 124.455 ms | 607.055 ms |
| enabled | disabled | 299.939 ms | 363.384 ms | 827.586 ms |
| disabled | enabled | 106.671 ms | 126.269 ms | 587.411 ms |
| disabled | disabled | 307.098 ms | 368.599 ms | 925.238 ms |

The measurements show:

```text
MTC has a large observable timing effect.
Iteration enabled versus disabled does not create a large timing separation.
CalculateFullRebuild is slower than warm Calculate as expected from rebuilding.
```

Timings alone do not identify circular membership. In combination with zero Heavy `CircularReference` seeds under both iteration modes, they provide no evidence that Excel repeatedly solves a 4,829-member iterative working set on a true no-op.

## Phase 6 — calcChain

The workbook contains:

```text
xl/calcChain.xml
cell records: 99,003
```

This is retained only as secondary calculation-order evidence. It is not treated as a dependency graph, circular-set listing, or edge proof.

## Proven, Formualizer, and inferred evidence

### Proven by Excel

```text
Excel COM CircularReference reporting works on a disposable micro-cycle.
Heavy exposes no CircularReference seed with iteration enabled.
Heavy exposes no CircularReference seed with iteration disabled.
No Excel seed paths were available for ShowPrecedents, ShowDependents,
NavigateArrow, or direct precedent/dependent tracing.
All ten bulk Formualizer-region interventions had zero Excel seeds before and after.
Calculate/Full/FullRebuild timings are sub-second for the tested settings.
MTC changes timing materially; iteration mode does not produce a comparable timing shift.
calcChain.xml exists with 99,003 cell records.
```

### Formualizer evidence

```text
static SCC: 4,829 members
runtime-live set: 4,139 members
270 dynamic/volatile members in the main SCC
main spans CashFlow Inputs and CashFlow Engine
large named-range and range edge observations
formula-semantic mismatches versus Excel remain
```

### Inference

```text
H3 is best supported: the broad Formualizer region is not Excel-demonstrated
as a circular calculation region in the current workbook state.

H2 remains plausible: Excel may have a smaller/different feedback structure
that is not exposed through Worksheet.CircularReference or available auditing.

H1 is weakly supported: no Excel seed, path, intervention cut, or timing
signature demonstrates a comparable 4,829-member Excel working set.
```

## Final questions

### 1. Does Excel definitely detect circular calculation in the same workbook state?

No for Heavy. Excel definitely detects circular calculation in the disposable control workbook, but it reports no Heavy circular seed under either iteration mode.

### 2. On which sheets and formulas does Excel expose circularity?

For Heavy: none. No worksheet returned a seed.

For the control workbook: `Sheet1!A1` was returned with iteration disabled for the `A1/B1` two-cell cycle.

### 3. Are Excel-reported seeds inside Formualizer's 4,829-member SCC?

There are no Excel-reported Heavy seeds to compare. The answer is therefore not applicable, not an inferred negative membership result.

### 4. Can Excel demonstrate feedback paths through the same regions?

No with the available public APIs and this workbook state. There were no seeds from which to start Excel tracing, and the interventions did not expose a before/after circular condition.

### 5. Does evidence support a comparable Excel working set or substantial Formualizer over-expansion?

It supports substantial Formualizer over-expansion relative to what Excel publicly exposes. The evidence is not a proof of Excel's private calculation internals, so a smaller/different Excel circular structure remains possible.

### 6. Which Formualizer dependency families lack Excel support?

No individual family is conclusively disproven by this run because Excel exposed no seed/path. The families with the largest unsupported-by-Excel evidence gap are:

```text
named-range expansion
large range/open-range expansion
INDIRECT/OFFSET dynamic references
conditional branch/live-reference handling
array/spill and unsupported-function propagation
error/coercion-driven dependency behavior
```

They are investigation targets, not established defects solely from this oracle.

### 7. Are formula-semantic mismatches likely changing the effective circular graph?

Yes, likely. Existing mismatches in conditional branches, references, unsupported functions, errors, and coercion can change which references are evaluated and which feedback paths are live. This run did not isolate a particular Excel/Formualizer edge, so the causal statement remains an inference.

### 8. Which hypothesis is best supported?

**H3**, with medium-high confidence for the present workbook state:

```text
Excel does not expose the corresponding broad Formualizer region as circular.
```

H2 is the principal alternative because Excel's public dependency/circular APIs are incomplete. What remains unproven:

```text
Excel's complete private dependency graph
Excel's complete circular working-set membership
whether Excel has a smaller hidden feedback set
whether some Formualizer edges correspond to Excel relationships that COM cannot expose
```

### 9. Recommended Engine V2 basis

Recommend a combination, ordered as follows:

```text
B. precise dependency/live-reference semantics
+
C. demand-driven calculation-chain execution with runtime cycle discovery
+
A. retained cyclic workspaces only as a later, proven-safe optimization
```

Evidence for this recommendation:

```text
Excel does not publicly demonstrate the broad 4,829-member circular region.
Formualizer's main edge observations are dominated by named/range expansion.
Excel completes CalculateFullRebuild in roughly 0.6–0.9 s in the tested sessions.
Iteration on/off does not show a large working-set timing signature.
Current retained validation is O(|SCC|), not truly incremental.
Heavy's dynamic frontier is also marked volatile and lacks a generation contract.
```

Therefore Engine V2 should first make dependency semantics and demand discovery match observed Excel behavior. A retained workspace can be layered on later for an explicitly eligible region with exact state, target, shape, live-edge, upstream, external, and volatile-generation certificates. It should not be the primary architecture for the currently observed Heavy region.

## Scope and safety

```text
No Engine V2 implementation.
No Formualizer production optimization.
No default behavior change.
Original workbook was not modified.
All Excel interventions used disposable copies.
```
