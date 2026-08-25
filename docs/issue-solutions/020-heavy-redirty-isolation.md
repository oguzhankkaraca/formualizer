# Heavy No-op Redirty Isolation

- **Branch:** `investigation/fossil-upstream-integration`
- **Workbook:** `Fossil_EstimatingTemplate_2026-08_21_A.xlsx`
- **Sequence:** initial evaluation, `Inputs!F7=300` recalculation, then one true no-op
- **Raw data:** `docs/issue-solutions/data/heavy-redirty-isolation.json`
- **No production change:** all suppression modes are opt-in diagnostics only.

## Diagnostic modes

| Mode | Environment controls |
| --- | --- |
| Normal | none |
| `no_iterative` | `FZ_DIAGNOSTIC_DISABLE_ITERATIVE_REDIRTY=1` |
| `no_volatile` | `FZ_DIAGNOSTIC_DISABLE_VOLATILE_REDIRTY=1` |
| `no_both` | both controls |

The normal path remains unchanged when no diagnostic environment variable is set.

## A — Normal Heavy no-op

```text
end-to-end wall:                  13,668.660 ms
scheduled SCCs:                         84
SCC member evaluations:             14,802
main SCC:                        4,829 members
main SCC member evaluations:       9,656
main SCC profiled wall:          12,540.124 ms
```

The main SCC accounts for:

```text
91.7% of end-to-end no-op wall time
99.4% of profiled SCC wall time
```

The remaining 83 SCCs contribute approximately `70.2 ms` of profiled SCC time in total. The next-highest individual SCCs are each approximately `3.6–3.8 ms`; they are not the Heavy bottleneck.

Main schedule record:

```text
naturally dirty members:       0
volatile redirty members:   270
iterative redirty members: 4829
schedule reason:          iterative_redirty
```

Main pass transition:

```text
pass 1: 12 members change
        11 are static-remainder members
pass 2: 0 members change
```

The 11 static witnesses are:

```text
Cash Flow Engine!Z33
Cash Flow Engine!Z84:Z86
Cash Flow Engine!Z93:Z97
Cash Flow Engine!Z109:Z110
```

## Dirty provenance

The no-op request has no user edit root. Its diagnostic request roots are:

```text
volatile_redirty:       1,212
iterative_scc_redirty:  4,829
user_edit roots:             0
```

The dirty closure is classified as:

| Provenance | Vertices |
| --- | ---: |
| Volatile only | 14,939 |
| Pre-redirty + volatile | 942 |
| Volatile + iterative | 4,559 |
| Pre-redirty + volatile + iterative | 270 |
| **Total** | **20,710** |

The `pre-redirty` label describes vertices already dirty before the end-of-request redirty stage. It is not a user edit in the no-op request.

This shows that the no-op dirty closure is primarily volatile-covered: all 20,710 vertices are in the volatile closure. Iterative redirty contributes the decisive overlap for the main SCC, but is not required to reach it.

## SCC wall ranking

Normal no-op SCC ranking:

| Rank | Stable ID | Profiled wall | Member evaluations | Schedule reason |
| ---: | ---: | ---: | ---: | --- |
| 1 | `1321560910633541638` | 12,540.124 ms | 9,656 | `iterative_redirty` |
| 2 | `5163784756527881955` | 3.818 ms | 264 | dirty closure |
| 3 | `7705769909603217465` | 3.742 ms | 264 | dirty closure |
| 4 | `5446315020909084425` | 3.735 ms | 264 | dirty closure |
| 5 | `13317632504510465753` | 3.380 ms | 264 | dirty closure |

The full rank is retained in the raw JSON.

## B — Iterative-redirty disabled

```text
no-op wall:                    14,460.509 ms
volatile redirty seeds:          1,212
iterative redirty seeds:              0
dirty closure:                 20,710
scheduled SCCs:                       84
main SCC reached:                    yes
main schedule reason: volatile_redirty
main member evaluations:          9,656
main passes:                           2
completed output SHA: same as normal
```

The main SCC still runs its full two-pass workload. Suppressing iterative redirty does not remove the Heavy no-op cost because the 270 volatile members are sufficient to dirty and schedule the entire SCC.

## C — Volatile-redirty disabled

```text
no-op wall:                    10,892.685 ms
volatile redirty seeds:              0
iterative redirty seeds:          4,829
dirty closure:                 14,214
scheduled SCCs:                       65
main SCC reached:                    yes
main schedule reason: iterative_redirty
main member evaluations:          9,656
main passes:                           2
completed output SHA: same as normal
```

Iterative redirty alone reproduces the same main-SCC evaluation count and the same completed formula-output SHA. It remains approximately 10.1 seconds of main-SCC profiled work in this run.

The invalid mode changes the dirty-set/engine fingerprint because volatile invalidation was suppressed, but the completed formula-output digest matches normal for the observed sequence.

## D — Both suppressed

```text
no-op wall:                       160.089 ms
volatile redirty seeds:                0
iterative redirty seeds:              0
dirty closure:                     1,212
scheduled SCCs:                         0
main SCC reached:                      no
main member evaluations:               0
```

This is a scheduler lower bound only and is not a valid calculation mode. Its completed output digest diverges from normal because both required redirty mechanisms were suppressed.

## Transition trace

The generic transition is:

```text
previous completed SCC state
  -> prior request end redirties volatile and/or iterative members
  -> dirty propagation creates a broad dirty closure
  -> any dirty member makes the SCC an executable scheduling unit
  -> main SCC pass 1 changes 12 members transiently
  -> pass 2 returns to zero changed members
  -> completed formula outputs remain stable
```

Causal isolation identifies the trigger source without hard-coding the Z cells:

```text
iterative disabled, volatile enabled:
  same 12-member pass-1 transition

volatile disabled, iterative enabled:
  same 12-member pass-1 transition

both disabled:
  no main SCC execution
```

Therefore the 11 static changes are not caused by a newly changed volatile value or dynamic target in the no-op. They are evaluation-state/order effects that occur once a dirty member causes the atomic SCC executor to run. The two redirty mechanisms are independent ways to cause that execution.

The existing profile records the first divergent members generically for any SCC. In this workload, the first divergence is always pass 1 and the changed set returns to zero on pass 2.

## Answers

### 1. What percentage of Heavy no-op wall time is the main SCC?

The 4,829-member main SCC consumes approximately:

```text
91.7% of end-to-end no-op wall time
99.4% of profiled SCC time
```

### 2. Without iterative redirty, do volatile/dynamic changes still reach it?

Yes. Volatile redirty alone creates a 20,710-vertex closure and reaches the main SCC. It executes 9,656 main-SCC member evaluations over two passes.

### 3. Without volatile redirty, does iterative redirty alone reproduce the cost?

Yes. Iterative redirty alone creates a 14,214-vertex closure and still reaches the main SCC with 9,656 member evaluations over two passes. Its measured main-SCC cost remains approximately 10 seconds.

### 4. Why does the stable state move during pass 1?

The completed state does not change between no-op requests. Once the SCC is scheduled, however, pass 1 re-evaluates members against the persisted iterative state and current evaluation order. Twelve members temporarily differ; pass 2 observes the updated values and returns to zero changed members.

This transient movement occurs under either redirty mechanism, proving that it is an execution/state-order effect rather than a necessary visible output progression.

### 5. What exact event invalidates the prior state?

The invalidating event is not a topology or live-edge change. It is the dirty-state transition:

```text
end-of-recalc volatile redirty of 1,212 seeds
and/or
end-of-recalc iterative redirty of 4,829 SCC members
```

Those seeds propagate into a dirty closure. Because SCC scheduling is atomic at the SCC-unit level, any dirty member invalidates reuse of the whole 4,829-member SCC.

### 6. What is the fundamental blocker?

The primary blocker is:

```text
SCC execution granularity after any member becomes dirty
```

It is fed independently by two coarse invalidation mechanisms:

```text
- volatile invalidation alone is sufficient;
- iterative invalidation alone is sufficient;
- their interaction broadens the closure but is not required;
- once reached, the SCC executes as a whole over two passes.
```

The correct classification is therefore **coarse SCC execution granularity with two independent redirty sources**, not an interaction that requires both sources simultaneously.

## Scope boundary

This is causal isolation only. No production optimization, cache, SCC reuse strategy, dirty suppression default, or semantic change was implemented.
