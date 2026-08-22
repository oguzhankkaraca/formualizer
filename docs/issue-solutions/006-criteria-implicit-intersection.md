# Issue Solution: Legacy Implicit Intersection in Criteria Arguments

- **Branch:** `fix/criteria-implicit-intersection`
- **Status:** Merged into `main`
- **Merge commit:** `205912b1`
- **Implementation commit:** `f7225fa0`

## Problem

Legacy Excel formulas silently intersect a multi-cell reference when it is supplied to a scalar criteria position. For example:

```excel
=SUMIF($E:$E,$H:$H,P:P)
```

At a formula cell on row `r`, Excel uses the criteria cell from row `r` in `$H:$H`. Formualizer treated the entire criteria range as an array, producing zero or an error instead of the scalar criterion.

The same behavior applied to `COUNTIF`, `AVERAGEIF`, `SUMIFS`, `COUNTIFS`, and `AVERAGEIFS` criteria-expression arguments.

## Solution

The existing explicit `@` implicit-intersection implementation was exposed through a reusable `ArgumentHandle` scalar accessor. Criteria parsing now uses that accessor only for criteria-expression positions.

The global `ArgumentHandle::value()` behavior was not changed, so range-consuming and dynamic-array functions do not receive an unrelated scalarization.

Direct references resolve the intersected cell without eagerly materializing the entire whole-column range.

## Validation

The Excel oracle covers:

- column-vector intersection by formula row;
- row-vector intersection by formula column;
- scalar and multi-criteria aggregate families;
- explicit `@` controls;
- path/bytes evaluation.

Fossil results after the fix:

```text
Engine errors:            6,544 -> 6,502
Formualizer-only errors:  1,156 -> 1,114
Non-error mismatches:     1,849 -> 1,222
Matches:                 65,520 -> 66,189
```

Heavy `Inputs!F6=300` recalculation remained effectively unchanged at approximately 607 ms versus 611 ms median in the branch measurement.

## Generalization notes

Implicit intersection is a context/argument-role rule, not a SUMIF special case. New scalar argument consumers should use the shared boundary only when their Excel schema is proven to require legacy intersection.
