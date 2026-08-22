# Excel Oracle Harness

This harness generates minimal XLSX fixtures and expected snapshots with installed Microsoft Excel. Formualizer never writes or blesses `expected.excel.json`.

## Generate or refresh a case

From the repository root on Windows with Microsoft Excel installed:

```powershell
.\tools\excel-oracle\recalculate_excel.ps1 `
  -CasePath .\tests\excel-oracle\error-normalization\case.json
```

Multiple cases can be refreshed in one Excel session sequence:

```powershell
.\tools\excel-oracle\recalculate_excel.ps1 `
  -CasePath `
    .\tests\excel-oracle\error-normalization\case.json, `
    .\tests\excel-oracle\criteria-implicit-intersection\case.json
```

The script replaces only the case's generated `fixture.xlsx` and `expected.excel.json`. The snapshot records Excel version, executable version, culture, calculation settings, date system, case SHA-256, and workbook SHA-256.

Use cell kind `formula_iie` when the case specifically tests legacy implicit-intersection evaluation through Excel `Range.Formula`. The snapshot records both `Formula` and `Formula2`, while the generated XLSX retains the legacy formula text.

## Verify snapshots

Normal Rust CI consumes the committed Excel snapshots without requiring Excel:

```powershell
cargo test -p formualizer-workbook `
  --test excel_oracle `
  --features calamine,json
```

Changing `case.json` or `fixture.xlsx` without refreshing the Excel snapshot fails SHA-256 validation. `FZ_CORPUS_BLESS` does not affect this harness.

A case may contain `skip` while it documents a known unsupported Excel behavior. Remove `skip` in the feature branch's first test commit to expose the failing regression before changing production code.
