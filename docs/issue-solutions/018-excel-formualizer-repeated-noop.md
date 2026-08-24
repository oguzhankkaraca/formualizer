# Excel vs Formualizer Repeated No-op Calculation

- **Branch:** `investigation/fossil-excel-calculation`
- **Workbook:** `Fossil_EstimatingTemplate_2026-08_21_A.xlsx`
- **Input:** `Inputs!F7 = 300`
- **Sequence:** initial calculation, F7 edit + seed calculation, then Calculate #1–#5 with no edits
- **Raw Excel:** `docs/issue-solutions/data/excel-heavy-repeated-noop.json`
- **Raw Formualizer:** `docs/issue-solutions/data/formualizer-heavy-repeated-noop.json`
- **Combined comparison:** `docs/issue-solutions/data/heavy-excel-formualizer-repeated-noop-comparison.json`

No caching or fixed-point certificate was implemented in this experiment.

## Settings

| Setting | Excel | Formualizer |
| --- | --- | --- |
| Calculation mode | Manual | — |
| Iteration | Enabled | `cycle_policy = iterate` |
| Max iterations | 100 | 100 |
| Max change | 0.001 | 0.001 |
| Parallel calculation | Enabled, 24 threads | Enabled |
| Cycle detection | Excel native | Runtime |

## Repeated no-op results

Values are for the completed Calculate result, not intermediate SCC passes.

| Calculate | Excel wall | Excel changed formulas | Excel max delta | Formualizer wall | Formualizer changed formulas | Formualizer max delta |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 85.989 ms | 0 | 0 | 11,047.688 ms | 0 | 0 |
| 2 | 92.735 ms | 0 | 0 | 11,322.282 ms | 0 | 0 |
| 3 | 91.768 ms | 0 | 0 | 10,790.236 ms | 0 | 0 |
| 4 | 89.111 ms | 0 | 0 | 11,351.109 ms | 0 | 0 |
| 5 | 88.333 ms | 0 | 0 | 12,002.791 ms | 0 | 0 |

Excel captured all `94,932` formula-cell outputs per step. Formualizer captured all `94,966` formula vertices per step. Every consecutive completed snapshot was unchanged in both engines.

Excel’s full formula-output fingerprint was identical for all five steps:

```text
249e277d3a5b7a0a8803e5a74f5332983be94ebb41ee5b0030ce9961635af1ff
```

Formualizer’s full snapshot SHA was identical for all five steps:

```text
37852a90ca30a949b1959ffd1577c057a7ee92a58bc8ef010e526998a134c633
```

The hash algorithms and value encodings are engine-specific, so these hashes are compared for within-engine stability, not as cross-engine equality claims.

## The 11 static-remainder members

Formualizer’s main SCC pass 1 reports the same 11 static members changing on every no-op calculation:

```text
Cash Flow Engine!Z33
Cash Flow Engine!Z84
Cash Flow Engine!Z85
Cash Flow Engine!Z86
Cash Flow Engine!Z93
Cash Flow Engine!Z94
Cash Flow Engine!Z95
Cash Flow Engine!Z96
Cash Flow Engine!Z97
Cash Flow Engine!Z109
Cash Flow Engine!Z110
```

The main SCC has two passes per Formualizer no-op:

```text
pass 1: 12 members change, including these 11 static members
pass 2: 0 members change
```

This is internal transient progression. It does not change the completed Formualizer formula snapshot between Calculate #1–#5.

Excel’s exact values for the same 11 formulas are stable on every step:

```text
Z33   0.78
Z84   0.12999999999999998
Z85   0.55100000000000016
Z86   0.09
Z93   0.63000000000000023
Z94   0.63000000000000023
Z95   0.63000000000000023
Z96   0.63000000000000023
Z97   0.063
Z109  2.0500000000000003
Z110  1.6800000000000004
```

Formualizer’s completed values for those same addresses are stable `#VALUE!` errors on every step. Therefore the engines do not currently reach the same absolute fixed point for this workbook, independent of repeated no-op scheduling.

## Answers

### 1. Does Excel advance iterative values across repeated no-op Calculate calls?

No observable formula output advances. All `94,932` Excel formula-cell outputs remain exactly unchanged from Calculate #1 through Calculate #5, including the 11 Z-column members.

The Excel no-op cost is approximately `86–94 ms` per Calculate.

### 2. Do the same 11 Formualizer members advance in Excel?

No. Excel keeps all 11 values unchanged at the numeric values listed above.

Formualizer re-evaluates them transiently during pass 1, but they settle during pass 2 and do not change the completed workbook output across repeated no-op calls.

### 3. Does Excel reach the same sequence/fixed point as Formualizer?

There are two separate answers:

```text
Repeated progression:
  yes, both engines’ completed outputs are stable across #1–#5.

Absolute output state:
  no, not currently.
```

The common-address comparison contains:

```text
common formula addresses:                 94,932
strict output differences at F7=300:       17,219
material/type differences:                 12,138
```

Many strict differences are floating-point representation differences, but there are also material engine-semantic differences such as Excel numeric values versus Formualizer `#VALUE!`, `#REF!`, and `#NIMPL` results. Those mismatches require a separate formula-semantics investigation and must not be attributed solely to iterative redirty.

### 4. If Excel outputs stay unchanged, why does Formualizer intentionally redirty and advance the SCC?

Formualizer currently implements the conservative Excel-compatible iterative contract as follows:

```text
live iterative SCC
  -> all SCC members are redirtied for the next recalc
  -> SCC is scheduled even without a user edit
  -> normal two-pass SCC evaluation runs
```

The Heavy SCC contains `270` volatile/dynamic members, so it is not considered `reuse_safe`. Formualizer therefore does not assume that the previous converged state is idempotent. The 11 static members visibly change during pass 1 before pass 2 restores the completed state.

Excel’s observable behavior is different: repeated no-edit Calculate calls return unchanged outputs in approximately 100 ms. This is evidence that Excel either retains an equivalent fixed-point state or avoids exposing/repeating the same transient progression in its normal no-op path. The COM experiment cannot identify Excel’s internal mechanism, only the exact output behavior.

### 5. If Excel outputs also advance, how can Excel do it in ~100 ms?

Not applicable to this workbook. Excel outputs do not advance across the five no-op Calculate calls.

The observed result instead supports classification **B**:

```text
Formualizer’s whole-live-SCC iterative-redirty path is
over-conservative for this repeated no-op workload.
```

That conclusion is limited by the separate absolute output mismatches at the F7=300 seed state. It does not prove that every Formualizer formula result is semantically equivalent to Excel; it proves that visible repeated no-op output progression is not required by Excel for this workload.

## A vs B decision

The experiment selects **B**, with an important qualification:

```text
B1: Formualizer schedules/re-evaluates the Heavy SCC on every no-op,
    while Excel produces no output changes and returns in ~100 ms.

B2: Formualizer and Excel also have independent absolute output differences
    at the F7=300 seed state that need separate semantic triage.
```

The next investigation should therefore not begin with a static-remainder certificate alone. It should first explain the seed-state formula mismatches, especially the 11 Z-column formulas and the broad `#REF!`/`#NIMPL` differences. Any future no-op reuse design must preserve the correct engine semantics and must still reject when iterative state could advance.

## Verification constraints

```text
No caching implemented
No fixed-point certificate implemented
No iterative semantics changed
No output-cell special casing added
Full formula-cell snapshots used for Excel
Full formula-vertex snapshots used for Formualizer
```
