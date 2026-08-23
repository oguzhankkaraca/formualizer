# Heavy vs Light No-op Causality

- **Branch:** `investigation/fossil-excel-calculation`
- **Scope:** Explain why Heavy no-op schedules iterative SCC work while Light no-op returns almost immediately.
- **Semantic changes:** None
- **Raw data:** `docs/issue-solutions/data/heavy-light-noop-causality.json`

## 1. Direct comparison

| Metric | Heavy no-op | Light no-op |
| --- | ---: | ---: |
| End-to-end wall | ~11.4 s | ~0.2–0.8 ms |
| Dirty vertices at request start | 20,710 | 0 |
| Volatile redirty seeds | 1,212 | 0 |
| Iterative redirty seeds | 4,829 | 0 |
| Iterative state values retained | 15,132 | 9,379 |
| SCC units considered | 142 | 78 |
| SCC units reused | 59 | 78 |
| SCC units invalidated | 83 | 0 |
| SCC tasks evaluated | 84 | 0 |
| Main SCC | 4,829 members | no runtime-live main SCC |
| Main SCC naturally dirty members | 0 | — |
| Main SCC iterative-redirty members | 4,829 | — |
| Main SCC volatile/dynamic members | 270 | — |

Light still has static/phantom SCC work after its capacity edit, but no-op has no dirty work and reuses all 78 reusable units. Heavy has a live iterative SCC and must re-evaluate it on every recalc under the current semantic contract.

## 2. Causal chain

```text
Heavy initial evaluation
  -> main 4,829-member static SCC is evaluated
  -> runtime live-edge analysis finds 1 live cycle
  -> CyclePolicy::Iterate marks the SCC as iterating
  -> main SCC contains 270 volatile/dynamic members
  -> reuse_safe = false
  -> members are not put into reusable_iterative_sccs
  -> all 4,829 members are appended to pending_iterative_redirty
  -> redirty_iterative_members propagates dirty state through dependents
  -> after redirty, 20,710 formula vertices are scheduled for the next request

Heavy no-op request
  -> no user edit occurs
  -> dirty_at_request_start = 20,710
  -> 1,212 volatile seeds and 4,829 iterative-SCC seeds are attributed
  -> main SCC has naturally_dirty_member_count = 0
  -> main SCC has iterative_redirty_member_count = 4,829
  -> main SCC reason = iterative_redirty
  -> 84 SCC tasks are evaluated
  -> main 4,829-member SCC runs two full iterative passes
  -> no-op still takes ~11.4 s
```

Light follows a different chain:

```text
Light capacity edit
  -> static/phantom SCC work runs
  -> no live iterative cycle persists
  -> no volatile seeds
  -> no iterative redirty seeds
  -> reusable SCC metadata remains valid

Light no-op request
  -> dirty_at_request_start = 0
  -> SCC units considered = 78
  -> SCC units reused = 78
  -> SCC tasks evaluated = 0
  -> returns in sub-millisecond time
```

## 3. Dirty-root source

Heavy no-op request root attribution:

```text
volatile_redirty:       1,212 seeds
iterative_scc_redirty:  4,829 seeds
```

Representative volatile/dynamic members in the main SCC:

```text
CashFlow Inputs!J55
CashFlow Inputs!K55
CashFlow Inputs!N106:O120
```

The main SCC’s `volatile_member_count` and `dynamic_member_count` are both `270`. These are the formulas that prevent the SCC from being classified as safe for reusable iterative metadata.

The iterative redirty set intentionally contains every member of the live iterative SCC, including named members such as:

```text
cash_flow_inputs
key_project_milestones
milestone_date
```

This is not a user-edit root. It is the Excel-compatible iterative policy root.

## 4. Revisions, state, and topology

Heavy no-op after F7:

```text
request snapshot:    4,165
engine topology:     4,163
graph topology:      4,163
graph symbol:        4,116
```

The no-op request has the same topology and symbol revisions as the preceding F7 request. The dirty work is not caused by a topology or boundary change.

The main SCC’s runtime-live signature remains unchanged:

```text
live cycle count:          1
live cycle members:     4,139
live-edge fingerprint: 1142813687581787051
```

The iterative state is retained, but retention does not imply reuse. Heavy retains state for the iterative work while its 270 volatile/dynamic members make the main SCC ineligible for `reusable_iterative_sccs`. Light retains reusable state for its 78 static/phantom SCC units and therefore clears/reuses them on no-op.

## 5. Minimum semantic condition blocking the Light path

The minimum condition is:

```text
A live CyclePolicy::Iterate SCC contains at least one volatile or dynamic
member, causing reuse_safe = false and forcing the whole SCC into iterative
redirty on the next recalc.
```

For the Heavy main SCC, the condition is present with `270` members. Because the iterative policy redirties all `4,829` members, the SCC is scheduled even when:

```text
no user edit occurred
naturally dirty member count = 0
live-edge fingerprint is unchanged
topology/boundary revisions are unchanged
previous evaluation converged exactly
```

The current semantics deliberately do not assume that a dynamic/volatile iterative SCC can be reused merely because its last observed target topology was unchanged.

## 6. Recommendation

Do not add a no-op shortcut or fixed-point cache yet.

The smallest semantic-safe future direction is to separate:

```text
volatile/dynamic frontier
from
static iterative closure
```

but only after proving that:

```text
- volatile values are sampled according to the existing recalc contract;
- dynamic targets and shapes are unchanged;
- the static closure cannot observe a changed frontier value;
- iterative accumulator state remains equivalent;
- live-edge/runtime SCC membership remains exact.
```

Until that proof exists, the Heavy no-op cost is semantically expected under the current policy: the live iterative SCC must be re-evaluated rather than treated as a reusable clean SCC.

## Final status

```text
Heavy no-op cause:       iterative redirty of live SCC
User edit required:      no
Topology change:         no
Main SCC natural dirty:  no
Volatile/dynamic gate:   270 members; reuse_safe=false
Light no-op cause:       zero redirty roots + 78/78 SCC reuse
Caching implemented:     no
Calculation semantics:   unchanged
```
