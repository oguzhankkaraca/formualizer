# Issue Solution: Concrete Bounds for Named References

- **Branch:** `fix/port-named-range-bounds-8.4`
- **Status:** Merged into `main`
- **Merge commit:** `42ea6dd9`
- **Implementation commit:** `044f7813`

## Problem

Named ranges could be evaluated as values, but the planning/reference layer could not consistently recover their concrete sheet and cell bounds. Reference-consuming formulas such as `INDEX`/`MATCH` therefore lacked the same concrete-bound behavior available for direct cell and range references.

The missing behavior was in the forwarding chain from evaluation context through function context to the engine, not in one Fossil-specific formula.

## Solution

The following layers now forward and resolve concrete named-reference bounds:

- `EvaluationContext`;
- `FunctionContext`;
- engine reference resolution;
- reference functions and planning paths.

The resolver returns concrete sheet, start row/column, and end row/column when the name is backed by a direct cell or finite range. Unsupported dynamic/reference-producing definitions remain conservative rather than being guessed.

## Validation

The branch added four named-range `INDEX`/`MATCH` regressions and passed the named-range test suite. The implementation was later used by the structured/table-backed name work.

## Generalization notes

Concrete bounds are metadata and must not require materializing the referenced values. Dynamic names, external references, and unresolved formulas must fail closed and remain on the exact fallback path until their semantics are independently proven.
