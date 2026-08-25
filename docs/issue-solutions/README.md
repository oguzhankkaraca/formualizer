# Formualizer Issue Solutions

This directory records the branch-level solutions ported to Formualizer v0.8.4, their validation evidence, and the remaining compatibility backlog.

## Branch status

| Branch | Status | Main reference |
| --- | --- | --- |
| `fix/dependency-aware-scc-reuse` | Merged | [`001`](001-dependency-aware-scc-reuse.md) |
| `test/lookup-cache-retirement-8.4` | Merged | [`002`](002-lookup-cache-retirement.md) |
| `fix/port-named-range-bounds-8.4` | Merged | [`003`](003-named-range-concrete-bounds.md) |
| `perf/port-scalar-allocator-telemetry-8.4` | Merged | [`004`](004-reachable-scalar-allocator-telemetry.md) |
| `test/excel-oracle-harness` | Merged | [`005`](005-excel-oracle-harness.md) |
| `fix/criteria-implicit-intersection` | Merged | [`006`](006-criteria-implicit-intersection.md) |
| `feat/calamine-native-tables` | Merged | [`007`](007-calamine-native-table-import.md) |
| `feat/structured-table-defined-names` | Merged via PR #8 | [`008`](008-structured-table-backed-defined-names.md) |
| `ui/formualizer-canvas` | Investigation in progress | [`009`](009-fossil-performance-investigation.md) |
| `investigation/fossil-excel-calculation` | Exact symbolic dependency investigation; no production optimization | [`010`](010-excel-vs-formualizer-fossil-calculation-investigation.md), [`011`](011-scc-termination-and-compact-dependency-experiment.md), [`012`](012-exact-symbolic-dependency-investigation.md), [`013`](013-fossil-scc-pass-profile.md), [`014`](014-scc-member-coordinate-index.md), [`015`](015-final-member-coordinate-index-ab.md), [`016`](016-heavy-light-noop-causality.md), [`017`](017-exact-frontier-reuse.md), [`018`](018-excel-formualizer-repeated-noop.md), [`019`](019-latest-upstream-heavy-baseline.md), [`020`](020-heavy-redirty-isolation.md), [`021`](021-heavy-scc-state-transition.md), [`022`](022-heavy-scc-internal-state-transition.md), [`023`](023-canonical-semantic-fixed-point.md), [`024`](024-retained-workspace-architecture-blockers.md), [`025`](025-excel-circular-set-oracle.md), [`026`](026-heavy-false-cycle-root-cause.md) |

## Remaining problems

See [`remaining-compatibility-issues.md`](remaining-compatibility-issues.md) for the three isolated roots that are intentionally queued for later:

1. IF condition error propagation — 21 error-kind mismatches.
2. SUMIF error-range propagation — 399 Excel `#REF!` to Formualizer value transitions.
3. `CELL("Filename")` workbook/host context — one Formualizer-only error.

The UI track is now in progress on `ui/formualizer-canvas`; the Fossil performance investigation and Excel reverse-engineering evidence are recorded in [`009`](009-fossil-performance-investigation.md) and [`010`](010-excel-vs-formualizer-fossil-calculation-investigation.md).

## Rules

- Production fixes must be general Excel semantics, never workbook-specific patches.
- Excel COM recalculated fixtures are the behavior oracle; cached workbook values are evidence only.
- Every solution records a failing regression before implementation and a post-fix validation result.
- Performance work starts only after correctness fingerprints are stable.
