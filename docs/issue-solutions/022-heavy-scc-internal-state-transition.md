# Heavy SCC Internal State Transition

- **Branch:** `investigation/fossil-upstream-integration`
- **Workbook:** `Fossil_EstimatingTemplate_2026-08_21_A.xlsx`
- **Sequence:** initial evaluation, `Inputs!F7=300` calculation, true no-op
- **Raw trace:** `docs/issue-solutions/data/scc-state-transition.json`
- **Scope:** Diagnostic explanation only; no production optimization or semantic fix.

## Result

The apparent fixed-point contradiction is caused by an internal state-representation boundary plus the first-pass operator:

```text
same-request replay:
  internal error payloads are reconstructed during pass 1
  pass 2 settles to zero changed members

request boundary:
  the pre-pass snapshot loses error message/context payloads
  while the persisted internal state retained them

next no-op:
  pass 1 reconstructs the same 12 payloads
  pass 2 settles to zero changed members
```

The completed public formula output remains stable. The internal `LiteralValue` equality used for pass-change accounting does not remain pointwise stable through one sweep.

## Experiment 1 — same-request extra pass

The diagnostic extra pass runs before request-level redirty and restores the captured engine state afterward.

After the F7 calculation:

```text
public/committed changed members: 0
internal changed members:        12
internal pass count:              2
internal pass 1 changes:         12
internal pass 2 changes:          0
```

The same result occurs when the extra pass is applied twice consecutively without a request boundary:

```text
T(x*):    internal changed members = 12, then settles
T(T(x*)): internal changed members = 12, then settles
```

The public before/after state fingerprint is unchanged because the Arrow/overlay read path normalizes the error payloads.

## Experiment 2 — state across the request boundary

Immediately before the next no-op SCC evaluation, the exact-state probe finds 13 differences between the prior saved internal state and the pre-task snapshot:

```text
Cash Flow Inputs!J55
Cash Flow Inputs!K55
Cash Flow Engine!Z33
Cash Flow Engine!Z84:Z86
Cash Flow Engine!Z93:Z97
Cash Flow Engine!Z109:Z110
```

The differences are representation-level:

```text
J55:
  #CIRC! + "Circular dependency detected"
  -> #CIRC! with no message

K55:
  #NAME? + "Unknown function: _xlfn.MAP"
  -> #NAME? with no message

Z33, Z84:Z86, Z93:Z97, Z109:Z110:
  #VALUE! + "Cannot convert to number (strict)"
  -> #VALUE! with no message
```

The following did not change:

```text
iterative state value count
live-edge identity/fingerprint
dynamic targets and shapes
boundary/topology revisions
semantic/config revisions
evaluation order
```

The persisted iterative state is not lost numerically. The observable pre-task snapshot is reconstructed through the committed/Arrow value path, which does not preserve the evaluator’s error message/context fields.

## Experiment 3 — first-pass semantics

The first and later passes use different comparison state:

| Pass | State/comparison behavior |
| --- | --- |
| Pass 1 | `prev_pass=None`; `last_value` starts from the pre-task snapshot; no convergence comparison runs; `changed` compares against that snapshot. |
| Pass 2+ | `prev_pass=Some(previous full-pass values)`; `changed` is reset; convergence compares the prior full pass with the current pass. |

Both pass types use the same deterministic member order:

```text
cells sorted by sheet/row/column
name members after cells, lexicographically
```

So the operator state changes, but the evaluation order does not.

For the Heavy no-op:

```text
pass 1: first_pass_no_prev_pass, 4,828 evaluated members, 12 changes
pass 2: iterative_pass_with_prev_pass, 4,828 evaluated members, 0 changes
```

The one-member difference from the 4,829 SCC size is the existing in-SCC array/spill exclusion behavior.

## Experiment 4 — deterministic replay

The diagnostic replay results are deterministic:

```text
same-request extra pass #1: 12 internal changes, final public state unchanged
same-request extra pass #2: 12 internal changes, final public state unchanged
```

After request initialization, the exact probe reports:

```text
pre-evaluation state differences: 13
frontier values unchanged:        true
frontier target fingerprints:     true
frontier shapes unchanged:        true
live-edge identities unchanged:   true
boundary revisions unchanged:     true
semantic revisions unchanged:     true
```

The regular no-op still executes the normal full SCC path because the internal first-sweep state is not pointwise equal and no static-remainder certificate exists.

## Experiment 5 — first divergence trace

The first changed member in deterministic member order is:

```text
Cash Flow Inputs!K55
```

Formula family:

```text
SUBSTITUTE(VSTACK(...MAP(...)), ...)
```

Internal transition:

```text
#NAME? with no message
->
#NAME? with message "Unknown function: _xlfn.MAP"
```

It has no precedent reads because function resolution fails before reading arguments. This is part of the existing formula-semantics mismatch and was not fixed.

The first static remainder witness is:

```text
Cash Flow Engine!Z33
```

Formula:

```text
IF(V33="","",SUMPRODUCT(W33+INT(SEQUENCE(V33,1,0,1)/X33)*Y33))
```

Internal transition:

```text
#VALUE! with no message
->
#VALUE! with message "Cannot convert to number (strict)"
```

Read trace:

```text
V33: same-SCC target member, observed as target_member=Some(4180)
W33: non-SCC direct-cell read
V33: same-SCC target member, read again
X33: non-SCC direct-cell read
```

Because `V33` precedes `Z33` in the deterministic order, the same-pass committed value is visible to the later `Z33` evaluation. The formula then reconstructs the strict conversion error payload. The underlying error kind remains `#VALUE!`; the first-pass change is in error message/context representation.

The other 10 static witnesses have the same error-payload pattern.

## Answers

### 1. Is the completed Heavy SCC an exact fixed point of another immediate same-request SCC pass?

Not pointwise under internal `LiteralValue` equality: each immediate full replay reconstructs 12 error-bearing members during pass 1. The completed two-pass public output is stable and the replay is deterministic.

### 2. What state changes across the request boundary before the next no-op pass?

Thirteen internal error payloads lose their message/context fields in the pre-task snapshot. Error kinds, iterative-state count, live edges, targets, shapes, and revisions remain unchanged.

### 3. Are first-pass and later-pass evaluation semantics identical?

No. Pass 1 has `prev_pass=None`, no convergence comparison, and compares against the pre-task snapshot. Later passes compare against the prior full-pass buffer. Evaluation order is the same.

### 4. Is the same evaluation order retained?

Yes. The cell/name ordering is deterministic and unchanged between passes and replays. Order change is not the cause.

### 5. What is the first actual semantic divergence causing the 12-member transient movement?

The first internal divergence is `K55` error-payload reconstruction for unresolved `_xlfn.MAP`. The first static witness is `Z33`, whose `SEQUENCE`/strict numeric-coercion path reconstructs a `#VALUE!` message after reading current-pass `V33` and non-SCC direct cells.

No numeric or error-kind change is observed in the completed output. The broad Excel/Formualizer semantic mismatch remains a separate correctness gate.

### 6. Could a retained cyclic workspace enter the no-op request without losing its converged state?

A retained workspace could preserve the numeric/error-kind state and iterative-state count, but the current engine does not preserve the exact internal error payloads through the overlay snapshot boundary, and every new SCC task starts with `prev_pass=None`.

A future retained workspace would need to preserve at least:

```text
internal member LiteralValues including error payloads
committed/pre-task snapshot representation
iterative seed values
previous-pass buffer semantics
live-edge/target/shape state
convergence state
```

Even then, this experiment does not prove that whole-SCC reuse is safe for all formulas. It only establishes that the current 12-member transient movement is deterministic and representation/operator driven, not visible numeric iterative progression.

## Final mechanism

```text
F7 completes
  -> internal evaluator values retain detailed error payloads
  -> same-request replay reconstructs 12 payloads, then settles

request boundary / pre-task snapshot
  -> overlay path returns same error kinds without message/context
  -> SCC task starts with prev_pass=None

no-op pass 1
  -> K55 and 11 Z formulas reconstruct error payloads
  -> 12 members report changed

no-op pass 2
  -> prev_pass is populated
  -> 0 members report changed
  -> completed public outputs remain stable
```

No production fix, cache, workspace retention, or semantic change was implemented.
