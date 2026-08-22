# formualizer-bench-core

Shared benchmark suite contract types for scenario definitions and benchmark result records.

This crate is intentionally lightweight and runtime-agnostic so it can be used by:
- Rust benchmark runners
- Python/Node adapters via JSON/YAML schema interchange
- CI report tooling

## Corpus generator

The crate includes a corpus generation binary:

```bash
cargo run -p formualizer-bench-core --features xlsx --bin generate-corpus -- \
  --scenarios benchmarks/scenarios.yaml
```

## Excel cache compatibility report

Compare typed Excel cached formula values with a Formualizer evaluation without treating every spreadsheet error as an engine defect:

```bash
cargo run -p formualizer-bench-core --features formualizer_runner \
  --bin compare-excel-workbook -- \
  --workbook path/to/workbook.xlsx \
  --mode iterate \
  --output target/compatibility-report.json
```

The report separates matching errors, Formualizer-only errors, different error kinds, Excel errors that become values, ordinary value mismatches, and matches. Excel caches are evidence rather than a replacement for the provenance-backed Excel COM fixtures under `tests/excel-oracle`.
