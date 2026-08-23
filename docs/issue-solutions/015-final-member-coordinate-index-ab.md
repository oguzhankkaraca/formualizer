# Final SCC Member-Coordinate Index A/B Benchmark

- **Branch:** `investigation/fossil-excel-calculation`
- **Samples:** 7 per template, scenario, and mode
- **Order:** seeded randomized/interleaved A/B worker order
- **A:** legacy full-scan collector
- **B:** indexed SCC-local member-coordinate collector
- **Raw data:** `docs/issue-solutions/data/final-member-coordinate-index-benchmark-v2.json`

## Compact Heavy vs Light table

Values are median end-to-end evaluation wall times. Percentages are B relative to A.

| Template / scenario | A legacy | B indexed | B-A | Relative |
|---|---:|---:|---:|---:|
| Heavy / initial | 32,650 ms | 31,081 ms | -1,570 ms | -4.8% |
| Heavy / capacity edit F7 | 14,033 ms | 12,067 ms | -1,967 ms | -14.0% |
| Heavy / no-op | 12,668 ms | 11,309 ms | -1,359 ms | -10.7% |
| Heavy / same-value write | 13,315 ms | 12,311 ms | -1,004 ms | -7.5% |
| Light / initial | 3,832 ms | 3,804 ms | -28 ms | -0.7% |
| Light / capacity edit F6 | 507 ms | 509 ms | +3 ms | +0.5% |
| Light / no-op | 0.774 ms | 0.789 ms | +0.015 ms | +1.9% |
| Light / same-value write | 498 ms | 517 ms | +20 ms | +4.0% |

The p95/min/max values are in the raw JSON. The Light same-value median increase is small in absolute terms and its p95 overlaps the legacy distribution.

## 1. Heavy improvement

Indexed mode improves the Heavy template by the following median amount:

```text
Initial:       4.8%
Capacity edit: 14.0%
No-op:         10.7%
Same-value:     7.5%
```

Main SCC pass-time medians improve by approximately `6–15%`, depending on scenario.

Collector counters show the direct mechanism:

```text
Legacy:  ~1.152B membership checks/pass
Indexed: ~95.7M membership checks/pass
```

That is approximately a `12x` reduction.

## 2. Light regression

The Light template does not show a meaningful regression:

```text
Initial:       -0.7%
Capacity edit: +0.5%
No-op:         +1.9%
Same-value:    +4.0%
```

The largest median difference is approximately `20 ms`. The Light template’s largest profiled SCC is only `125` members, and the indexed collector reduces its measured range membership checks and collector time whenever a collector is exercised.

## 3. Small-SCC index-build overhead

Light indexed coordinate-index build medians:

```text
Initial:        0.225 ms
Capacity edit:  0.160 ms
No-op:          0 ms (no SCC task)
Same-value:     0.265 ms
```

For the two-cell synthetic SCC:

```text
Legacy build: 0 ns
Indexed build: 2.3 µs
```

This overhead is not meaningful relative to Light evaluation time or the Heavy savings.

## 4. Enablement policy

Enable indexed mode by default rather than adding an empirical SCC-size/range-read threshold.

Reason:

```text
- build cost is sub-millisecond for the Light workbook;
- synthetic two-cell build cost is a few microseconds;
- Heavy gains are material;
- Light wall-time changes are within small absolute variance;
- the indexed query is exact and reduces work whenever ranges intersect SCC members.
```

A threshold would add policy complexity without a measured benefit. The legacy mode remains useful as a diagnostic fallback:

```text
FZ_SCC_MEMBER_COORDINATE_INDEX_MODE=legacy
```

## 5. Parity

All required parity gates passed for both templates:

```text
live-edge sets:             equal
origin masks:               equal
live-edge fingerprints:     equal
runtime-live SCC signatures: equal
final formula fingerprints: equal
```

Heavy compare exercised 10 SCC pass records; Light compare exercised 8 pass records. Legacy/indexed formula outputs and runtime signatures were equal across all 7 samples and all four scenarios.

No formula evaluation order, cycle semantics, convergence behavior, dynamic target resolution, or spill semantics were changed by this benchmark task.
