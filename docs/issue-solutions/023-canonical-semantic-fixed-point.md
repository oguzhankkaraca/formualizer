# Canonical Semantic Fixed-Point Evidence

- **Branch:** `investigation/fossil-upstream-integration`
- **Workbook:** `Fossil_EstimatingTemplate_2026-08_21_A.xlsx`
- **Input:** `Inputs!F7 = 300`
- **State trace:** `docs/issue-solutions/data/scc-state-transition.json`
- **Retained proof:** `docs/issue-solutions/data/canonical-retained-workspace-probe.json`
- **Controls:** `docs/issue-solutions/data/canonical-retained-workspace-controls.json`
- **Production behavior:** unchanged; all reuse is diagnostic opt-in only.

## 1. Canonical semantic equality

`LiteralValue` raw equality includes the complete `ExcelError` payload, including message/context. That payload is observable:

```text
Rust:  ExcelError fields and Display
Python: LiteralValue.error_kind, error_message, repr, to_python() error dict
JS/WASM: BindingValue.Error includes code/message; display is also exposed
```

Therefore error message/context cannot be silently discarded from public diagnostics. The investigation defines a separate canonical spreadsheet-semantic equality:

| Value | Canonical comparison |
| --- | --- |
| Number / Int / Date / DateTime / Time / Duration | Exact numeric-class serial equality; `-0` and `0` are equal; identical NaN bit patterns are equal. |
| String | Exact, case-sensitive string equality. |
| Boolean | Exact boolean equality. |
| Blank | `Empty` equals `Empty` only. |
| Error | `ExcelErrorKind` only; message/context/extra are diagnostic metadata. |
| Array | Exact row count, column count, and recursive canonical element equality. |
| Reference identity | Not represented by `LiteralValue`; compared separately using resolved read-target fingerprints and live-edge identities. |
| Spill shape | Compared separately and exactly from array dimensions/shape records. |
| External/effect state | Compared through provider/semantic generations and fail-closed context handling. |

This comparator is diagnostic-only. Raw equality and raw `changed` telemetry remain available.

## 2. Heavy replay: raw versus canonical

For the completed F7 state, same-request replay, and next no-op:

```text
same-request extra T(x*):
  raw internal changes:       12
  canonical changes:           0
  completed public changes:    0

same-request extra T(T(x*)):
  raw internal changes:       12
  canonical changes:           0
  completed public changes:    0

next no-op pass 1:
  raw changes:                12
  canonical changes:           0

next no-op pass 2:
  raw changes:                 0
  canonical changes:           0
```

The 12 raw changes are:

```text
Cash Flow Inputs!K55
Cash Flow Engine!Z33
Cash Flow Engine!Z84:Z86
Cash Flow Engine!Z93:Z97
Cash Flow Engine!Z109:Z110
```

The 11 static-remainder members have raw error-message changes but zero canonical semantic changes. The dynamic `K55` member likewise changes only its `#NAME?` message payload.

## 3. Request-boundary state

The exact state probe reports 13 raw state differences between the prior saved internal state and the pre-task snapshot:

```text
Cash Flow Inputs!J55
Cash Flow Inputs!K55
Cash Flow Engine!Z33
Cash Flow Engine!Z84:Z86
Cash Flow Engine!Z93:Z97
Cash Flow Engine!Z109:Z110
```

Canonical state differences across the same comparison:

```text
0
```

The persisted iterative state count, dynamic targets, shapes, live-edge identity, topology, and semantic/config revisions are unchanged.

The prior state is therefore canonically stable even though raw internal error metadata is normalized at the overlay/pre-task snapshot boundary.

## 4. Dynamic/volatile frontier

The exact diagnostic frontier contains 269 evaluable members in this state; one existing array/spill member is excluded by the normal SCC pre-scan.

The canonical frontier validation reports:

```text
frontier canonical values unchanged: true
frontier raw values unchanged:       true
resolved target fingerprints:        unchanged
spill/array shapes:                  unchanged
live-edge identities:                unchanged
boundary revisions:                 unchanged
semantic/config revisions:           unchanged
```

The frontier produces no canonical semantic delta on a true no-op.

## 5. Static remainder reconsideration

The previous rejection was based on raw `LiteralValue` equality:

```text
raw static remainder changes:       11
canonical static remainder changes:  0
```

The previous `static_remainder_progression_unproven` blocker was therefore caused by representation-level error metadata, not spreadsheet-value progression.

The normal convergence state was also successfully exact-stable under canonical semantics. No tolerance-based reuse proof was used.

## 6. Diagnostic retained-workspace experiment

The normal control and canonical retained diagnostic were run independently:

| Metric | Normal | Canonical retained |
| --- | ---: | ---: |
| No-op wall | 14,731.445 ms | 631.342 ms |
| SCC tasks | 84 | 1 non-main task |
| SCC member evaluations | 14,802 | 264 |
| Main SCC evaluations | 9,656 | 0 after acceptance |
| Main candidate | not applicable | accepted |
| Full formula output count | 94,966 | 94,966 |
| Full public output SHA | `c69041...` | exactly equal |
| Avoided main evaluations | 0 | 4,828 |

The canonical candidate accepted the main SCC with:

```text
frontier values unchanged:        true
frontier targets unchanged:       true
frontier shapes unchanged:        true
live-edge identities unchanged:   true
boundary revisions unchanged:     true
semantic revisions unchanged:     true
static remainder canonical delta: 0
previous exact convergence:       true
```

The retained path was not enabled by default and was not used in normal production-mode calculations.

## 7. Adversarial controls

All controls fail closed:

| Control | Result |
| --- | --- |
| RAND/random state change | Reject: `frontier_value_changed` |
| NOW/clock change | Reject: `semantic_revision_changed` |
| Dynamic target identity change | Reject or candidate SCC removed; no acceptance |
| Dynamic target shape change | No candidate; no acceptance |
| INDIRECT/OFFSET target change | Reject through target/revision path or candidate removal |
| Name/table definition change | Reject: `boundary_revision_changed` |
| Structural row/column-equivalent boundary change | Reject: `boundary_revision_changed` |
| Upstream SCC fixed-point/input change | Reject or candidate SCC removed; no acceptance |
| Volatile/external UDF | Reject: `external_or_context_dependent` |
| Tolerance-only convergence | Reject: no exact static witness |
| Capped iteration | Reject: no exact prior convergence witness |

The controls were designed so that an absent candidate record is also a safe rejection; no control treats missing instrumentation as permission to reuse.

## Answers

### 1. Is Heavy at an exact canonical semantic fixed point after F7?

Yes, for the observed F7 state and canonical semantics:

```text
static remainder canonical delta: 0
frontier canonical delta:         0
full previous convergence:        exact
```

Raw error metadata still moves during individual sweeps.

### 2. Are the 12 raw pass-1 changes entirely non-semantic diagnostic metadata?

Yes for the observed Heavy no-op:

```text
raw changes:       12
canonical changes:  0
```

They are same-kind error payload/message reconstructions.

### 3. Does the dynamic/volatile frontier produce any canonical delta on true no-op?

No. All evaluated frontier members pass canonical value, target, shape, live-edge, boundary, and semantic-generation comparisons.

### 4. Was the previous static-remainder blocker invalid under correct equality?

Yes. The previous blocker was based on raw error metadata. The static remainder has zero canonical semantic changes.

### 5. What validity key was sufficient for diagnostic retained-workspace acceptance?

The sufficient observed key was the conjunction of:

```text
same SCC member/frontier identity
canonical pre-evaluation state unchanged
canonical frontier values unchanged
exact dynamic read-target fingerprints unchanged
exact shapes unchanged
exact frontier live-edge identity unchanged
same boundary/topology/symbol revisions
same semantic/provider/config revisions
previous full SCC converged exactly and was not capped
static remainder canonical first-pass delta = 0
context/external state deemed safe
```

With that key, retaining the prior workspace produced an identical full public formula-output SHA and reduced the main-SCC work from 9,656 member evaluations to zero.

This is diagnostic evidence, not a production cache contract.

### 6. Which adversarial cases invalidate it?

Any of these invalidate acceptance:

```text
volatile value/generation change
clock or semantic-generation change
dynamic target identity/value/shape change
name, table, row, column, or boundary revision change
external/UDF/context state uncertainty
canonical pre-task state change
non-exact/tolerance-only convergence
capped or failed iteration
any canonical static-remainder delta
any live-edge identity change
```

## Final conclusion

The Heavy no-op state is canonically fixed for the observed workbook state, despite raw internal error-payload changes. The diagnostic retained-workspace experiment successfully reproduces the normal full public output while avoiding the main SCC evaluation.

However, this does not authorize production reuse yet. The exact contract still needs broader validation across real dynamic targets, spill transitions, external state, structural edits, and iterative-state progression cases. No production/default behavior was changed.
