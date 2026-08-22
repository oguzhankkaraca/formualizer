# Issue Solution: Excel COM Oracle and Compatibility Reporting

- **Branch:** `test/excel-oracle-harness`
- **Status:** Merged into `main`
- **Merge commit:** `e9e29d8e`
- **Commits:** `248bb668`, `48b08e8d`, `201187c9`

## Problem

The initial Calc Lens comparison treated Excel cached errors such as `#N/A` as strings while Formualizer errors were structured objects. This classified many matching errors as Formualizer-only defects.

Excel cached values are useful evidence, but they are not sufficient as a deterministic oracle because a workbook may contain stale cached results.

## Solution

The repository now contains:

- a declarative oracle case schema;
- a Microsoft Excel COM generator;
- Excel full-recalculation and provenance snapshots;
- typed error/value normalization;
- a Rust read-only snapshot consumer;
- a generic `compare-excel-workbook` compatibility report runner.

Snapshots include Excel version, executable version, culture, date system, calculation settings, case hash, and workbook hash. Case hashes normalize BOM and line endings so Git CRLF/LF conversion does not invalidate a snapshot.

`FZ_CORPUS_BLESS` cannot rewrite Excel oracle snapshots. Excel-generated XLSX fixtures are sanitized for personal metadata.

## Validation

The initial active error case matches four Excel error kinds and one numeric result. Known unsupported cases are represented as explicit skipped fixtures until their feature branch starts.

## Generalization notes

Every future Excel behavior change must have:

1. a minimal isolated Excel COM case;
2. a provenance snapshot;
3. a failing engine regression before implementation;
4. path/bytes and binding coverage where applicable;
5. a real-workbook blast-radius report.

Production code must never contain workbook-specific formula or cell allowlists.
