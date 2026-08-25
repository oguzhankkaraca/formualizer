# Latest Upstream Heavy Fossil Baseline

- **Branch:** `investigation/fossil-upstream-integration`
- **Upstream:** `60c0afad109de3ed05b72d38a9008c17c755fa85`
- **Previous reference:** `352b0ce747ed3dc2beebacf938dc792d5bcc4c28`
- **Workbook:** `Fossil_EstimatingTemplate_2026-08_21_A.xlsx`
- **Input:** `Inputs!F7 = 300`
- **Excel oracle:** Existing full capture reused; Excel was not rerun.
- **Latest raw:** `docs/issue-solutions/data/latest-upstream-heavy-baseline.json`
- **Comparison raw:** `docs/issue-solutions/data/latest-upstream-heavy-comparison.json`

The latest run used the established runtime/iterate configuration with 100 maximum iterations, `maxChange=0.001`, and parallel evaluation enabled. No new optimization, cache, SCC reuse strategy, or formula fix was implemented.

## Before / after

Previous performance values are the prior 7-sample indexed medians. Latest values are one updated sequence with five no-op samples; the no-op row uses the latest five-sample median.

| Metric | Previous Heavy | Latest upstream integration | Delta |
| --- | ---: | ---: | ---: |
| Formualizer formula vertices | 94,966 | 94,966 | 0 |
| Common Excel/Formualizer addresses | 94,932 | 94,932 | 0 |
| Strict Excel output differences | 17,219 | 17,218 | -1 |
| Material/type differences | 12,138 | 12,137 | -1 |
| Excel numeric → Formualizer `#VALUE!` | 543 | 262 | -281 |
| Excel numeric → Formualizer `#REF!` | 5,165 | 5,186 | +21 |
| Excel numeric → Formualizer `#NIMPL` | 942 | 942 | 0 |
| Other material/type differences | 5,488 | 5,747 | +259 |
| Cyclic SCC count | 84 | 84 | 0 |
| Largest static SCC | 4,829 | 4,829 | 0 |
| Main runtime-live SCC | 4,829 | 4,829 | 0 |
| Main runtime-live members | 4,139 | 4,139 | 0 |
| Volatile members | 270 | 270 | 0 |
| Dynamic members | 270 | 270 | 0 |
| Volatile/dynamic overlap | 270 | 270 | 0 |
| Main live-edge fingerprint | 1,142,813,681,167,587,051 | 1,142,813,681,167,587,051 | 0 |
| No-op SCC tasks | 84 | 84 | 0 |
| No-op SCC member evaluations | 14,802 | 14,802 | 0 |
| No-op dirty vertices | 20,710 | 20,710 | 0 |
| No-op volatile redirty seeds | 1,212 | 1,212 | 0 |
| No-op iterative-redirty members | 4,829 | 4,829 | 0 |
| Initial wall time | 31,081 ms | 33,001 ms | **+6.2%** |
| F7 edit wall time | 12,067 ms | 12,917 ms | **+7.0%** |
| True no-op wall time | 11,309 ms | 12,629 ms median | **+11.7%** |
| Same-value F7 wall time | 12,311 ms | 13,043 ms | **+5.9%** |

The performance comparison is directional rather than a fresh matched sample-size benchmark: the prior values are randomized 7-sample medians, while the latest values are one sequence with five repeated no-op observations.

## Correctness

The latest full F7 snapshot was compared against the existing full Excel seed snapshot:

```text
Excel formula cells:          94,932
Formualizer formula vertices: 94,966
Common addresses:             94,932
Strict differences:           17,218
Material/type differences:    12,137
```

The F7 formula-value fingerprint changed relative to the previous branch state:

```text
previous: [20710, 1026544198047018979]
latest:   [20710, 10208017083477216623]
```

Upstream changed the distribution of errors but did not materially restore Excel parity:

```text
Excel numeric -> Formualizer #VALUE!:  543 -> 262
Excel numeric -> Formualizer #REF!:    5165 -> 5186
Excel numeric -> Formualizer #NIMPL:    942 -> 942
```

The other 11 Z cells remain incompatible:

| Formula | Excel | Latest Formualizer |
| --- | ---: | --- |
| `CashFlow Engine!Z33` | `0.78` | `#VALUE!` |
| `CashFlow Engine!Z84` | `0.12999999999999998` | `#VALUE!` |
| `CashFlow Engine!Z85` | `0.55100000000000016` | `#VALUE!` |
| `CashFlow Engine!Z86` | `0.09` | `#VALUE!` |
| `CashFlow Engine!Z93` | `0.63000000000000023` | `#VALUE!` |
| `CashFlow Engine!Z94` | `0.63000000000000023` | `#VALUE!` |
| `CashFlow Engine!Z95` | `0.63000000000000023` | `#VALUE!` |
| `CashFlow Engine!Z96` | `0.63000000000000023` | `#VALUE!` |
| `CashFlow Engine!Z97` | `0.063` | `#VALUE!` |
| `CashFlow Engine!Z109` | `2.0500000000000003` | `#VALUE!` |
| `CashFlow Engine!Z110` | `1.6800000000000004` | `#VALUE!` |

## SCC and dependency state

The latest upstream changes did not materially alter the Heavy SCC:

```text
static cyclic SCCs:          84
largest static SCC:        4829 members
main runtime-live SCC:     4829 members
runtime-live members:      4139
volatile members:            270
dynamic members:             270
volatile/dynamic overlap:   270
```

The no-op request remains structurally identical:

```text
dirty vertices at request start: 20,710
volatile redirty seeds:           1,212
iterative redirty members:        4,829
SCC tasks:                           84
SCC member evaluations:          14,802
SCC units considered:              142
SCC units reused:                   59
SCC units invalidated:              83
iterative state values retained: 15,132
```

The main SCC still uses two passes on each F7/no-op/same-value request. The live-edge fingerprint remains unchanged.

## Profiling state

Latest no-op median profile:

```text
scalar reads:             63,666
range reads:             477,678
requested range cells: 397,903,718
named reads:             378,516
internal SCC targets:  120,652,254
range membership checks:191,364,812
collector time:          1,337 ms
coordinate-index build:  0.129 ms
```

The coordinate-index build remains negligible. The dominant cost is repeated full SCC work: the main SCC and related SCC tasks re-evaluate thousands of members over two passes while the 20,710-vertex dirty closure is maintained.

## Answers

1. **Did Excel parity materially improve?**

   No. Strict and material differences each decreased by only one address. The error distribution changed, but broad parity remains unresolved.

2. **Did any of the 11 Z cells become Excel-compatible?**

   No. All 11 remain `#VALUE!` in latest Formualizer and numeric in Excel.

3. **Did the large SCC or runtime-live SCC materially change?**

   No. Size, live-member count, cyclic SCC count, and live-edge fingerprint are unchanged.

4. **Did volatile/dynamic classification change?**

   No. The classification remains 270 volatile, 270 dynamic, with all 270 overlapping.

5. **Did F7 performance improve or regress?**

   It regressed directionally from the previous indexed median: `12,067 ms -> 12,917 ms`, approximately `+7.0%`.

6. **Did true no-op performance improve or regress?**

   It regressed directionally from `11,309 ms` to a latest five-sample median of `12,629 ms`, approximately `+11.7%`.

7. **What now dominates the Heavy cost?**

   Repeated full iterative SCC scheduling/evaluation remains dominant. The no-op still starts with 20,710 dirty vertices, invalidates 83 SCC units, evaluates 84 SCC tasks, and performs 14,802 SCC member evaluations. Coordinate-index construction is not the cause.

8. **What should be investigated next?**

   First triage the unchanged absolute seed-state semantic mismatches, especially the 11 Z formulas and the broad `#REF!`/`#NIMPL` distribution. Upstream did not change the SCC workload, and the latest baseline provides no evidence that a new SCC topology or classification explains the regression.

   After those formula semantics are understood, revisit the no-op scheduling contract. Any future reuse/certificate work must remain exact and must not assume that unchanged frontier state proves the static remainder cannot progress.

## Scope confirmation

```text
No new optimization implemented
No caching strategy implemented
No SCC reuse strategy enabled
No formula fix implemented
Existing Excel oracle reused
Original investigation branch untouched
```
