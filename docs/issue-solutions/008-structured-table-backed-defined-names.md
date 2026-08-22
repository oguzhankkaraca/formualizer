# Issue Solution: Structured and Table-Backed Defined Names

- **Branch:** `feat/structured-table-defined-names`
- **Status:** Merged into `main` via PR #8
- **PR:** https://github.com/oguzhankkaraca/formualizer/pull/8
- **Commits:** `d4a542f4`, `ab8bc862`, `f9e820f1`, `80018fb1`, `6d4ab4fe`

## Problem

Calamine’s old defined-name conversion accepted only direct cells and finite ranges. Formula-backed names were silently dropped, including names referring to native tables:

```excel
Main_GSU_Price_MVA = Main_GSU_Price_X[[#All],[mva]]
Main_GSU_Price_kV  = Main_GSU_Price_X[#Headers]
```

The direct impact was approximately 1,114 Formualizer-only `#NAME?` results plus downstream value mismatches.

A second compatibility detail was present in Fossil’s OOXML formulas: Excel serialized a bare table data-body reference as `Table[]`. Treating this as `#All` selected header cells such as `Help` instead of the data body.

Finally, `VALUE(range)` was scalar-only, while Excel uses array lifting when the result is consumed as a lookup array.

## Solution

### Stable workbook model

`DefinedNameDefinition::Formula { formula }` preserves raw formula text across Calamine, Umya, and JSON adapters. Formula names are parsed into the engine’s existing `NamedDefinition::Formula` representation.

### General formula-name shape

The engine distinguishes scalar and array-valued formula names using AST shape and function capabilities:

- direct range/table/reference, array literal, `RETURNS_REFERENCE`, and `MAY_SPILL` roots become `NamedArray`;
- scalar formulas such as `=SUM(...)` and `=A1+1` remain `NamedScalar`;
- explicit `@` remains scalar.

### Structured selector composition

Table bounds are composed from row and column selectors instead of special-casing one name:

- `#All`, `#Data`, `#Headers`, `#Totals`;
- single columns;
- column ranges;
- combinations such as `[[#All],[Amount]]`.

### Table default and coercion

- `Table[]` maps to the data body, matching Excel’s legacy OOXML behavior;
- `VALUE(range/array)` maps elementwise and preserves element errors;
- `MATCH` can consume the resulting array.

### Dependencies and updates

Formula-backed names depend on the table vertex. Table updates dirty the name and all dependent formulas. Scope precedence remains sheet-local first, then workbook scope.

## Fossil validation

Current Rust compatibility report after this branch:

```text
Formualizer-only errors: 1,114 -> 1
Value mismatches:         1,060 -> 0
Matches:                 66,351 -> 68,524
```

The remaining one is `CELL("Filename")`, not a structured-name failure. Excel same-error, cached-error, and error-kind clusters are kept separate.

## Test coverage

The Excel fixture covers:

- workbook-scoped formula names;
- sheet-scoped shadowing names;
- `Table[Column]`;
- `Table[[#All],[Column]]`;
- `VALUE` over table-backed ranges;
- exact and approximate `MATCH`;
- path and bytes loading;
- table resize/update invalidation.

The Calamine suite and WASM target check pass. The full formula corpus retains two pre-existing floating-point string snapshot differences in `IMSIN` and `IMCOS`, reproduced identically on `origin/main`.

## Generalization notes

No Fossil name, sheet, cell, formula, or table is referenced in production code. Invalid internal Excel names are not forced through the engine name validator; they remain adapter metadata and are reported under debug loading. Valid formula-backed names are preserved and evaluated through the same graph/table/reference mechanisms as user-created names.
