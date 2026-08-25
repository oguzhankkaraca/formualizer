# Retained Cyclic-Workspace Architecture: Generality and Safety

- **Branch:** `investigation/fossil-upstream-integration`
- **Heavy probe:** `docs/issue-solutions/data/canonical-retained-workspace-probe.json`
- **Dynamic controls:** `docs/issue-solutions/data/retained-dynamic-controls.json`
- **Generality matrix:** `docs/issue-solutions/data/retained-generality.json`
- **Gated durability:** `docs/issue-solutions/data/gated-durability.json`
- **Historical cost attribution:** `docs/issue-solutions/data/retained-path-cost-attribution.json`
- **Native mutation test:** `diagnostic_exact_reuse_fails_closed_for_real_mutations`
- **Production/default behavior:** unchanged; retained reuse remains diagnostic-only and is currently fail-closed for Heavy.

## Finding

The canonical semantic fixed-point result remains valid:

```text
Heavy static remainder canonical delta: 0
Heavy frontier canonical value delta:   0
```

The retained-workspace experiment is **not currently safe as a production design**. The current dependency planner marks all 269 Heavy frontier members as both dynamic and volatile:

```text
frontier members: 269
volatile members: 269
dynamic members:  269
```

The engine has no per-volatile-generation contract that proves a volatile expression was not semantically refreshed between requests. The diagnostic path therefore rejects Heavy with:

```text
volatile_generation_unproven
```

The previously measured 0.63 s retained Heavy run is historical evidence only and is superseded by this fail-closed result.

## 1. Remaining retained-path cost

The earlier accepted-path instrumentation run, before the volatile gate, separated the approximately 0.4–0.6 s evaluator/API phase as follows:

| Stage | Observed cost |
| --- | ---: |
| Pre-validation SCC setup | ~7.1–7.6 ms |
| Dynamic/frontier formula evaluation | ~19–22 ms for 269 members |
| Canonical value comparison | ~0.009 ms |
| Target fingerprint comparison | ~0.0005 ms |
| Shape comparison | ~0.001 ms |
| Live-edge/origin comparison | ~0.024 ms |
| Generation comparison | ~0.0005 ms |
| Remaining non-main SCC | ~4 ms |
| Full public output digest | ~150–166 ms, outside engine timing |
| Profile ranking | ~0.02–0.7 ms, instrumentation only |

The retained `evaluate_all` wall was approximately 0.43–0.47 s without the post-evaluation digest. The exact checks themselves are not dominant. The remaining evaluator time is inferred to be scheduler/dirty-state/overlay and general request bookkeeping; the available SCC telemetry attributes only ~4 ms to the remaining SCC task. A clean stage-level timer for those outer request phases is still needed before any performance claim.

This distinction follows benchmark discipline: public output digesting and profiling are not removable retained-engine work.

## 2. Is validation incremental?

No. It avoids evaluating the 4,829-member SCC formulas after acceptance, but the current acceptance path still performs SCC-wide metadata work.

Before acceptance it:

```text
constructs the complete member list
reads a complete pre-task snapshot
pre-scans every member for spill/exclusion status
scans every member to build the dynamic frontier
builds frontier flags of length |SCC|
allocates candidate edge/origin arrays of length |SCC|
compares saved member identity against the current member vector
compares saved and current values across all |SCC| members
```

The current complexity is therefore approximately:

```text
setup and state validation: O(|SCC| + edge storage)
member identity check:      O(|SCC|)
pre-task canonical state:   O(|SCC| * recursive value cost)
frontier evaluation:        O(|frontier| * formula cost)
frontier values:            O(|frontier| * recursive array cost)
target fingerprints:        O(|frontier| + recorded reads)
shapes:                     O(|frontier|)
live edges/origins:         O(|SCC| + frontier edge storage) in current implementation
certificate/generations:    O(1)
```

Thus this is **formula-incremental**, not validation-incremental. It currently replaces full SCC evaluation with a full linear SCC scan plus frontier evaluation.

A production design should persist a stable member/topology certificate, canonical state generation, frontier descriptor, target generations, shape generation, and live-edge generation during the successful solve. A no-op should validate changed semantic surfaces and the required frontier only, without reconstructing or comparing all SCC member values.

## 3. Real invalidation tests

The native diagnostic test performed actual engine mutations and asserted that no candidate accepted retained state after:

```text
row insertion that relocates formula ASTs inside an SCC
relevant formula edit
name definition update
native table definition/range update
cycle configuration change
upstream SCC fixed-point/input change feeding a downstream SCC
```

All failed closed. The Python binding does not currently expose row/column insertion/deletion, so those mutations were covered through the native engine API rather than by adding a public binding solely for this investigation.

The Python controls additionally showed table/structural boundary changes rejecting with `boundary_revision_changed`.

## 4. Dynamic and volatile controls

The strengthened controls covered:

| Case | Result |
| --- | --- |
| Dynamic target identity changes but equal target value | `boundary_revision_changed`; no acceptance |
| Target value changes with identity fixed | `boundary_revision_changed`; no acceptance |
| `OFFSET` target formula changes to another equal-valued target | `boundary_revision_changed`; no acceptance |
| Dynamic spill/shape case | no candidate; no acceptance |
| RAND/random state | `volatile_generation_unproven` or normal fallback |
| NOW/clock generation with equal produced value | `volatile_generation_unproven` |
| Volatile UDF returning equal value | `external_or_context_dependent` |
| External/context-dependent UDF | `external_or_context_dependent` |

The key result is that value equality is not used to override target identity, shape, semantic generation, or external context uncertainty.

## 5. Durability and mixed sequences

### Historical pre-gate Heavy run

A previous run performed 100 Heavy no-op requests and observed one stable output hash. However, after the first accepted request, subsequent requests performed zero SCC tasks and zero validation. That run is **not safety evidence** because it accepted a workspace containing volatile frontier members without a volatile-generation proof.

It is retained as a superseded marker in:

```text
docs/issue-solutions/data/retained-durability.json
```

### Valid gated durability

The current fail-closed suite ran 100 requests per synthetic case:

| Case | Output hashes | Accepted | Result |
| --- | ---: | ---: | --- |
| Static exact SCC | 1 | 0 | clean scheduler; no candidate needed |
| Dynamic exact SCC | 1 | 0 | 100 `volatile_generation_unproven` rejections; normal solve |
| Equal-value volatile UDF | 1 | 0 | 100 `external_or_context_dependent` rejections; normal solve |

The mixed dynamic sequence covered:

```text
initial -> no-op
identity change to equal-valued target -> no-op
same-target value change -> no-op
```

Every invalid request was solved normally after rejection, with stable output hashes and no accepted stale workspace.

The sequence demonstrates fail-closed durability, not retained-workspace performance, because no valid volatile/dynamic workspace is currently accepted.

## 6. Generality matrix

The mechanism was exercised on:

```text
small exact iterative SCC
multiple independent SCCs
cross-SCC dependency
INDIRECT dynamic-reference SCC
RAND volatile SCC
Light Fossil
```

Results:

```text
small exact / independent / cross-SCC:
  initial solve completed; subsequent no-ops were cleanly unscheduled

INDIRECT dynamic SCC:
  rejected by volatile-generation gate

RAND SCC:
  rejected by volatile-generation gate

Light Fossil:
  no-op was already cleanly scheduled; retained candidate was not needed
```

The candidate contains no Heavy/Fossil-specific condition. However, the matrix does not yet demonstrate a generally reusable accepted workspace beyond the historical pre-gate Heavy experiment, because static exact SCCs are already skipped by ordinary clean scheduling and dynamic references are currently volatile-gated.

## 7. State required for a production certificate

A production certificate would need at least:

```text
stable SCC member identity and topology generation
formula AST / name / table / symbol semantic generation
mutation/data generation for relevant inputs
cycle configuration and date system
workbook seed and volatile policy
exact convergence proof, including no cap/failure/tolerance-only stop
canonical semantic state generation for the retained workspace
frontier membership and classification generation
per-frontier resolved target identity
per-frontier target value generation
per-frontier spill/array shape generation
per-frontier live-edge identity and origin generation
upstream SCC output/fixed-point generation
external/provider/UDF context/effect generation
```

The retained workspace itself must preserve the complete internal converged values/state needed for output. The certificate should not require reconstructing a raw `LiteralValue` vector or scanning all SCC members on each request.

For volatile functions, equal output is insufficient. Either:

```text
an engine-owned volatile generation must be persisted and compared, or
volatile frontier members must remain on the generic evaluator path
```

The current engine provides neither a sufficiently granular per-expression volatile generation nor a safe retained contract, so the diagnostic gate correctly rejects them.

## 8. Remaining architectural blockers

1. **Volatile-generation contract**

   Heavy's dynamic references are marked volatile. A production design needs a precise generation model for `NOW`, `RAND`, `INDIRECT`, `OFFSET`, volatile UDFs, external sources, and recalc/open policies.

2. **Request lifecycle after acceptance**

   The historical 100-request run showed that accepting once can remove the SCC from later scheduling. A production design must ensure every request that can observe a changed volatile/effect surface either revalidates the frontier or is proven not to need validation.

3. **Full-SCC validation scan**

   Current setup and pre-state checks remain O(|SCC|). Persisted generations/fingerprints and incremental dirty-surface tracking are required.

4. **Outer scheduler attribution**

   Approximately 0.4 s of retained evaluator wall is not explained by frontier formula work or SCC telemetry. It needs clean phase timers before optimizing.

5. **Generation ownership and invalidation completeness**

   Name/table/structural edits, upstream SCC changes, spill relocation, target identity, live-edge origin, external providers, and semantic configuration need one coherent invalidation model rather than independent ad hoc comparisons.

6. **Broader accepted-path evidence**

   The current valid gate accepts no volatile/dynamic workspace. Static exact workspaces are already cleanly skipped. A safe accepted-path matrix requires a non-volatile, semantically dynamic scenario or a real volatile-generation contract.

7. **Public diagnostic parity**

   Rust/Python/JS error metadata parity remains a required pre-production gate, intentionally deferred per task scope. Canonical equality must remain separate from raw diagnostic payload equality.

8. **Clean A/B and bounded-state proof after redesign**

   Any future production candidate still requires clean timing, output/work counters, memory/state-size tracking, repeated mixed durability, and fallback verification.

## Final answers

### 1. What consumes the remaining ~0.63 s?

In the pre-gate accepted experiment, the frontier itself consumed ~21 ms and comparisons were negligible. The evaluator’s remaining ~0.4 s was primarily outer scheduler/dirty/overlay/request bookkeeping and pre-validation setup; the ~150 ms full-output digest was API instrumentation outside engine timing. The current gated Heavy path no longer retains and returns to the normal ~11–14 s SCC solve.

### 2. Is retained-workspace validation truly incremental?

No. Formula execution is incremental after acceptance, but current validation/setup scans and allocates against the full SCC. It is O(|SCC|) metadata validation plus O(|frontier|) frontier evaluation.

### 3. Does any acceptance check scale with total SCC size unnecessarily?

Yes:

```text
member identity comparison
pre-task raw/canonical state comparison
spill/exclusion/frontier classification
frontier flag construction
candidate edge/origin array allocation
live-edge fingerprint inputs
```

### 4. Which state/generations must be persisted?

SCC/member/topology, AST/symbol/name/table, mutation/data, cycle/config/date/seed, exact convergence, canonical state, frontier targets/values/shapes, live edges/origins, upstream fixed-point, and external/provider/volatile/effect generations.

### 5. Do all strengthened adversarial cases fail closed?

Yes in the current diagnostic evidence. Heavy itself fails closed due `volatile_generation_unproven`; real native structural/name/table/formula/config/upstream mutations reject; dynamic equal-value target changes reject; equal-value volatile clock/UDF cases reject; tolerance/capped cases reject.

### 6. Does the mechanism survive 100+ no-op and mixed-request sequences?

The valid gated controls survive 100 requests with stable outputs, zero unsafe acceptance, bounded candidate records, and normal fallback. The historical 100-request Heavy acceptance run is explicitly invalidated because it bypassed volatile generation checks and stopped revalidating after the first acceptance.

### 7. Does it generalize beyond Heavy?

The generic diagnostics run across synthetic independent/cross-SCC cases, dynamic/RAND cases, and Light Fossil without workbook-specific conditions. But there is not yet a generally accepted retained workspace beyond the superseded Heavy proof: static cases are already cleanly skipped, and dynamic/volatile cases are correctly rejected.

### 8. What remains before production design?

Define volatile/effect generations, repair request-lifecycle validation, replace SCC-wide setup with persisted incremental certificates, attribute outer scheduler cost, complete invalidation coverage, and rerun clean A/B plus bounded-state durability on a genuinely eligible non-volatile/dynamic workload. Until then, the retained path must remain diagnostic-only and fail closed.
