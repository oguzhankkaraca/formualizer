# Remaining Compatibility Issues

This document records the remaining known compatibility roots after the structured/table-backed defined-name branch. It is a backlog and oracle plan, not a license to patch against the Fossil workbook.

## Current baseline

After criteria implicit intersection and structured/table-backed names:

```text
Formula cells:              74,312
Same Excel/Formualizer error: 5,367
Formualizer-only error:          1
Different error kind:           21
Excel error -> value:          399
Value mismatch:                 0
Match:                      68,524
```

The remaining one Formualizer-only result is `CELL("Filename")`. The 21 and 399 groups are independent error semantics and must be solved separately.

---

## 1. IF condition error propagation — 21 cells

### Evidence

Representative formula:

```excel
=ROUND((IF(#REF!="Excluded",0,#REF!)+IF(#REF!="Excluded",0,#REF!))*1000000,-1)
```

Excel result:

```text
#REF!
```

Formualizer result:

```text
#VALUE!
```

The 21 cells are the `Cash_Flow!EK5:...` family. The parser already preserves sheet-qualified error literals as typed `ExcelErrorKind::Ref`; the mismatch occurs when the error is used as the `IF` condition.

### Likely root

The IF implementation handles Boolean, numeric, empty, and non-coercible conditions, but treats an error condition as a generic non-coercible condition and creates `#VALUE!` instead of returning the original error.

### Proposed solution branch

`fix/if-error-condition-propagation`

1. Add Excel COM oracle cases for every supported error kind as an IF condition.
2. Pin `IF(error, true, false)` to the same error kind.
3. Pin comparisons involving errors and nested `IF`/`IFERROR`/`IFNA` behavior.
4. Return exact `LiteralValue::Error(error)` before Boolean coercion.
5. Check short-circuit behavior so untaken branches remain unevaluated.
6. Re-run Fossil and ensure the 21 `#REF! -> #VALUE!` transitions become same-error without changing unrelated error policy.

### Acceptance criteria

- all isolated oracle cases pass;
- 21 different-error-kind cells become same Excel error;
- no change to valid Boolean/text condition behavior;
- no extra branch evaluation in runtime live-edge tests.

---

## 2. SUMIF error-range propagation — 399 cells

### Evidence

Representative formula:

```excel
=SUMIF($Y$6:$Y$178,$Y180,IV$6:IV$178)
```

Original cached workbook value:

```text
#REF!
```

Formualizer value:

```text
0
```

This affects the `Cash_Flow` columns beginning at `IV180` and continues through the repeated formula family.

### Cache freshness check

These results are not being treated as stale-cache evidence. A copy of the original workbook was opened in Microsoft Excel with iterative calculation enabled, recalculated with `CalculateFullRebuild`, and inspected without modifying the original file. Representative cells remained `#REF!` after recalculation.

### Likely root

The criteria aggregate implementation currently reduces the matched sum range while skipping or neutralizing error cells in a way that differs from Excel’s `SUMIF` behavior when the selected sum range contains broken references. The exact rule must be separated by:

- error in criteria range;
- error in sum range;
- matching versus non-matching row;
- direct `SUMIF` versus `SUMIFS`;
- range size mismatch and whole-column bounds;
- cached error versus evaluated error.

### Proposed solution branch

`fix/sumif-error-range-parity`

1. Build an Excel COM matrix with one error per criteria/sum position.
2. Compare `SUMIF`, `SUMIFS`, `AVERAGEIF`, `AVERAGEIFS`, `COUNTIF`, and `COUNTIFS`.
3. Record whether Excel propagates, skips, or returns zero for each case.
4. Keep criteria scalar implicit-intersection behavior from the merged branch.
5. Update scalar and Arrow/criteria-mask paths from one shared error policy.
6. Add matched/unmatched error and repeated-column regressions.
7. Re-run all 399 cells and classify any remaining transition with the report runner.

### Acceptance criteria

- isolated Excel matrix passes;
- the 399 cells are either same-error or a proven representation/staleness category;
- no regression to the 16 existing criteria aggregate tests;
- no full-column materialization regression.

---

## 3. `CELL("Filename")` workbook context — 1 cell

### Evidence

Representative formula:

```excel
="Filename:"&CELL("Filename")
```

Excel cached value contains the workbook path, sheet, and workbook name. Formualizer returns `#NAME?` because the engine has no host/workbook identity provider.

### Proposed solution branch

`feat/cell-workbook-context`

- add a privacy-safe workbook identity/path context provider;
- native loader may provide the source path;
- browser upload may provide an optional user-visible filename without exposing local paths;
- unsaved/anonymous workbooks return the documented empty/context value;
- add saved, unsaved, native, and WASM oracle cases;
- do not infer or fabricate a local path in browser code.

### Acceptance criteria

- native saved workbook behavior matches Excel oracle;
- browser behavior is deterministic and privacy-safe;
- no path/username leakage in telemetry or snapshots.

---

## Not started yet: performance and UI tracks

The compatibility roots above are now isolated. The approved plan’s next major tracks remain:

- `perf/recalc-observability-bindings`: expose phase/formula counters to Python/WASM;
- profile the approximately 607 ms heavy edit (`plan`, `acyclic`, and SCC phases separately);
- optimize only the measured dominant phase;
- move browser evaluation to a dedicated Worker;
- create `webapp/formualizer` with a Formualizer-specific model adapter, virtualized/canvas grid, formula bar, cell editing, and undo/redo.

The IronCalc engine/model is not used. Only selected UI interaction patterns may be adapted with attribution.
