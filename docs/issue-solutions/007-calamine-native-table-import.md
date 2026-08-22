# Issue Solution: Native Calamine Table Import

- **Branch:** `feat/calamine-native-tables`
- **Status:** Merged into `main`
- **Merge commit:** `164f41d3`
- **Implementation commit:** `216481f9`

## Problem

Calamine loaded cell values and formulas but did not expose native XLSX table metadata. Structured references such as `SalesTable[Amount]` therefore failed during formula ingest with `Undefined table`, unless an application manually registered known tables.

That application workaround did not work for arbitrary workbooks and was unavailable to the WASM bytes-loading path.

## Solution

Calamine now scans the OOXML package relationships for table parts and imports:

- table name;
- worksheet association;
- table range;
- header-row flag;
- totals-row flag;
- table column names.

The scanner handles both file and bytes input. It reads workbook relationships, small worksheet relationship parts, and table XML parts without decompressing every worksheet cell XML a second time.

Tables are registered after file sheets are adopted and before formula ingest. This makes table references available to planning, dependency tracking, and evaluation.

## Validation

The fixture and tests cover:

- path and bytes adapter metadata;
- structured table calculation;
- table range/header metadata;
- full Calamine regression suite.

Fossil’s two tables were found without manual registration:

```text
Main_GSU_Price_X
SBC_Soils_X
```

## Generalization notes

The scanner uses relationship type `.../table`, not a workbook-specific table filename or directory allowlist. Application-level manual registration remains an idempotent fallback for external callers but is no longer required for XLSX table loading.
