# SCC-Local Member-Coordinate Index

- **Branch:** `investigation/fossil-excel-calculation`
- **Scope:** Replace the full SCC-member scan in `LiveEdgeCollector::record_rect_with_origin` with an SCC-local sheet/row/coordinate index.
- **Semantic changes:** None.
- **Raw benchmark:** `docs/issue-solutions/data/member-coordinate-index-benchmark.json`
- **Raw collector profile:** `docs/issue-solutions/data/member-coordinate-index-profile.json`

## 1. Membership checks before vs after

Fossil main SCC, per pass:

```text
legacy full scan:  1,152,398,175 checks
indexed lookup:       95,682,406–95,731,828 checks
```

This is approximately:

```text
12.0x fewer membership checks
```

The index is built once per SCC and partitioned by sheet with row-sorted coordinate entries. Range reads binary-search the row bounds and inspect only row candidates before applying the exact column bounds.

## 2. Collector time before vs after

Diagnostic profile, Fossil main SCC:

```text
legacy:  ~1.19–1.21 s/pass
indexed: ~0.65–0.72 s/pass
```

The indexed collector reduces the measured collector path by approximately:

```text
~0.5 s/pass
```

The existing exact target set is preserved; only the member candidate lookup changes.

## 3. SCC pass time before vs after

Indexed-first benchmark:

| Scenario | Legacy | Indexed | Change |
| --- | ---: | ---: | ---: |
| Initial | 38,745 ms | 30,706 ms | -8,039 ms / -20.8% |
| F7 | 16,124 ms | 12,070 ms | -4,055 ms / -25.1% |
| No-op | 15,342 ms | 11,340 ms | -4,003 ms / -26.1% |

Main SCC pass timings from the mode profile also improved consistently when the indexed mode ran first:

```text
indexed initial passes: 5.25–5.82 s
legacy initial passes:  7.25–7.49 s
```

The earlier legacy-first run showed normal process/CPU variance, so the indexed-first run is the recorded benchmark order. The membership-check and collector counters independently confirm the mechanism of the improvement.

## 4. End-to-end F7/no-op improvement

```text
F7:
  legacy:  16.124 s
  indexed: 12.070 s
  saving:   4.055 s / 25.1%

No-op:
  legacy:  15.342 s
  indexed: 11.340 s
  saving:   4.003 s / 26.1%
```

Initial evaluation also improved by approximately `8.0 s` in the captured indexed-first run.

## 5. Parity result

The old and indexed collectors were run side-by-side in `compare` mode from the same formula execution.

Required parity passed:

```text
live edge sets:              equal
live-edge fingerprints:      equal
edge-origin masks:           equal
runtime-live SCC signature:  equal
final formula fingerprints:  equal
```

The Fossil main SCC compare records were equal for all initial/F7/no-op passes. The synthetic two-cell SCC also passed. Self-edge/range behavior remains covered by the existing live-edge SCC tests, and the indexed collector is now the default mode; `legacy` and `compare` remain available through:

```text
FZ_SCC_MEMBER_COORDINATE_INDEX_MODE=legacy
FZ_SCC_MEMBER_COORDINATE_INDEX_MODE=compare
```

### Recommendation

Parity passed and the speedup is meaningful. The smallest production cut-over is the already-contained default change to the indexed collector constructor. No formula order, iterative convergence, cycle policy, dynamic target resolution, or runtime-live SCC semantics need to change.
