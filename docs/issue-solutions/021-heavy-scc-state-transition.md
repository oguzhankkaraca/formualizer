# Heavy SCC State Transition and First-pass Movement

- **Branch:** `investigation/fossil-upstream-integration`
- **Workbook:** `Fossil_EstimatingTemplate_2026-08_21_A.xlsx`
- **Input state:** initial evaluation, then `Inputs!F7=300` and recalculation
- **Raw state trace:** `docs/issue-solutions/data/scc-state-transition.json`
- **No production fix:** all probes are diagnostic-only and restore state where required.

## Executive result

The apparent contradiction is caused by two different notions of state:

```text
public/overlay formula output state:
  stable after a completed F7 calculation and repeated no-op requests

internal LiteralValue state used by SCC pass comparisons:
  error payloads/messages are regenerated during the first sweep
```

The same-request extra pass does not produce a public completed-output change, but its internal first pass still changes 12 members. The next request additionally reconstructs its pre-pass snapshot from the committed/Arrow overlay, which has normalized away error messages for 13 members.

## Experiment 1 — same-request extra pass

The probe runs a full diagnostic SCC evaluation at the end of the current request, before normal redirty, then restores all captured state.

After the F7 calculation:

```text
public before/after changed members: 0
internal first-pass changed members: 12
internal second-pass changed members: 0
pass count:                          2
state fingerprint before/after:      equal
```

The 12 internal changes are:

```text
Cash Flow Inputs!K55
Cash Flow Engine!Z33
Cash Flow Engine!Z84:Z86
Cash Flow Engine!Z93:Z97
Cash Flow Engine!Z109:Z110
```

The first same-request pass changes error payloads, not numeric values. The public overlay read after the full extra pass is unchanged because the overlay representation normalizes those errors.

The probe was repeated twice without a request boundary (`T(x*)`, then `T(T(x*))`). Both applications produced the same result:

```text
public changed members: 0
internal changed members: 12 during the first sweep
state fingerprint before/after: equal
```

This rules out random scheduling drift in the observed run.

## Experiment 2 — cyclic state across the request boundary

The exact-state probe compares the previously saved internal SCC state with the SCC snapshot immediately before the next no-op SCC evaluation.

The no-op pre-evaluation snapshot differs in 13 members:

```text
Cash Flow Inputs!J55
Cash Flow Inputs!K55
Cash Flow Engine!Z33
Cash Flow Engine!Z84:Z86
Cash Flow Engine!Z93:Z97
Cash Flow Engine!Z109:Z110
```

Observed transitions:

```text
J55:
  #CIRC! with message "Circular dependency detected"
  -> #CIRC! with no message

K55:
  #NAME? with message "Unknown function: _xlfn.MAP"
  -> #NAME? with no message

Z33, Z84:Z86, Z93:Z97, Z109:Z110:
  #VALUE! with message "Cannot convert to number (strict)"
  -> #VALUE! with no message
```

The following state remained unchanged across the boundary:

```text
iterative state value count: 15,132
live-edge topology/fingerprint: unchanged
boundary/topology revisions: unchanged
semantic/config revisions: unchanged
```

The pre-pass snapshot is created through the normal cell-read/overlay path. The overlay stores the error kind but not the internal error message/context retained by the evaluator’s prior `last_value` state. Thus the 13 differences are error-payload representation differences, not numeric or dependency-topology changes.

## Experiment 3 — first versus later pass semantics

The evaluator does not use one identical operator for every sweep.

### First SCC pass

```text
last_value = pre-task snapshot
prev_pass = None
changed is compared against the pre-task snapshot
no convergence test runs
members are evaluated and committed in member order
```

The first pass therefore compares newly produced internal `LiteralValue`s against the overlay-derived snapshot. An error with the same kind but a newly reconstructed message compares unequal.

### Later iterative passes

Before pass 2:

```text
prev_pass = Some(last_value.clone())
changed is reset
members are evaluated again in the same member order
convergence compares pass 1 values with pass 2 values
```

The normal convergence test is used to decide whether the iterative solver stops. It is not used as a no-op reuse proof.

The Heavy no-op profile confirms:

```text
pass 1: operator = first_pass_no_prev_pass
         4,828 evaluated members
         12 changed members

pass 2: operator = iterative_pass_with_prev_pass
         4,828 evaluated members
          0 changed members
```

One member is excluded because of the existing in-SCC array/spill handling, so the pass count is 4,828 for the no-op task even though the SCC contains 4,829 members.

Evaluation order is retained: cell members are sorted by sheet/row/column, followed by name members. The pass-2 order is not the source of the difference; the operator state (`prev_pass=None` versus `Some`) and the snapshot/commit boundary are.

## Experiment 4 — deterministic replay

The same-request probe applies the full SCC operator twice with state restoration between applications:

```text
T(x*):
  public final state unchanged
  internal first sweep changes 12 error-bearing members
  internal settle sweep changes 0

T(T(x*)):
  same result
```

After normal request initialization, the exact-state probe finds the 13-member representation mismatch before any frontier formula is evaluated. The following no-op then reconstructs 12 of those payloads during its first pass and settles on pass 2.

This distinguishes the hypotheses:

```text
A. x* is not a fixed point of a single internal first-sweep operator: true
   for error payload equality, but not for public numeric/error-kind output.

B. request initialization modifies the observed internal snapshot: true.

C. first-pass operator differs from later-pass operator: true (`prev_pass=None`).

D. evaluation order changes: not observed; order remains deterministic.

E. hidden dynamic/reference state changes: not observed for live-edge identity,
   target fingerprints, or shapes.
```

## Experiment 5 — first divergence trace

The first changed member in member-evaluation order is:

```text
Cash Flow Inputs!K55
```

Its internal transition is:

```text
#NAME? with no message
->
#NAME? with message "Unknown function: _xlfn.MAP"
```

It has no precedent read trace because function resolution fails before reads occur. This is part of the known broad formula-semantics mismatch and is not fixed here.

The first static-remainder witness is:

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

Representative reads captured for Z33:

```text
scalar sheet=7 row=32 col=21 origin=direct_cell target_member=Some(4180)
scalar sheet=7 row=32 col=22 origin=direct_cell target_member=None
scalar sheet=7 row=32 col=21 origin=direct_cell target_member=Some(4180)
scalar sheet=7 row=32 col=23 origin=direct_cell target_member=None
```

The target member is the same-SCC `V33` dependency. The other reads are non-SCC direct-cell reads. Since `V33` precedes `Z33` in the deterministic cell order, the read is visible from the current pass’s committed state. The formula then produces the strict conversion error associated with the current implementation’s `SEQUENCE`/numeric coercion path.

The remaining 10 Z witnesses have the same pattern: same-kind `#VALUE!` representation before/after, with strict-conversion error payload reconstructed during pass 1.

## Answers

### 1. Is the completed Heavy SCC an exact fixed point of another immediate same-request SCC pass?

Not as an internal single-sweep `LiteralValue` state: the first sweep changes 12 error-bearing members. The full two-pass SCC operator returns to the same completed public output state and repeats deterministically.

So:

```text
single internal first sweep: not exact
completed two-pass public state: stable
```

### 2. What state changes across the request boundary?

The persisted internal state and the overlay-derived pre-pass snapshot differ in 13 members. The differences are error messages/context, while error kinds remain the same. Iterative-state count, dynamic targets, shapes, live edges, and revisions remain unchanged.

### 3. Are first-pass and later-pass semantics identical?

No.

The first pass has `prev_pass=None`, performs no convergence comparison, and compares against the pre-task snapshot. Later passes have `prev_pass=Some(...)`, reset `changed`, and compare against the previous full-pass values.

### 4. Is the same evaluation order retained?

Yes. The deterministic member order is retained across pass 1, pass 2, same-request replay, and the next no-op. The observed divergence is not caused by a reordered SCC.

### 5. What is the first actual semantic divergence causing the 12-member movement?

The first internal divergence is an error-payload reconstruction at `Cash Flow Inputs!K55` (`#NAME?` message for `_xlfn.MAP`). The first static witness is `Cash Flow Engine!Z33`, where strict numeric-conversion error context is reconstructed after reading current-pass `V33` and direct non-SCC cells.

The formula error kinds do not change. The known broad Excel/Formualizer semantic mismatch remains a separate correctness gate.

### 6. Could a retained cyclic workspace enter the no-op request without losing its converged state?

The numeric/error-kind state and iterative-state count are retained, but the current public/overlay path does not preserve the exact internal error payloads. The request also initializes each SCC task with `prev_pass=None`.

A retained workspace that preserved the complete internal `last_value`, error payloads, snapshot representation, and convergence buffers could avoid this representation-level first-pass movement. However, this is not yet a safe no-op reuse proof: the first-sweep operator still reports internal changes, and the broader formula-state mismatches remain unresolved.

No reuse or caching fix is implemented by this investigation.

## Final mechanism

```text
F7 completed internal state
  -> same-request full SCC replay deterministically reconstructs 12 error payloads
  -> final public state remains unchanged

request boundary
  -> overlay-derived snapshot normalizes 13 error payloads
  -> iterative task starts with prev_pass=None

no-op pass 1
  -> K55 and 11 Z formulas reconstruct internal error messages
  -> 12 members report changed

no-op pass 2
  -> prev_pass is populated
  -> no members report changed
  -> completed public outputs remain stable
```

This is a state-representation/operator-boundary effect, not evidence of visible numeric iterative progression. The broad formula semantic mismatch should be triaged separately before any production fixed-point or workspace-retention design.
