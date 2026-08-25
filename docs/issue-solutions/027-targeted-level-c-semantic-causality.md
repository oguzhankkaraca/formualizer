# Targeted Level-C Semantic Causality Proof

- **Branch:** `investigation/fossil-upstream-integration`
- **Workbook:** `Fossil_EstimatingTemplate_2026-08_21_A.xlsx`
- **Graph evidence:** `docs/issue-solutions/data/heavy-graph-root-cause.json`
- **Level-C evidence:** `docs/issue-solutions/data/heavy-targeted-level-c-evidence.json`
- **Excel INDEX targets:** `docs/issue-solutions/data/excel-reference-targets.json`
- **Excel conditional branch:** `docs/issue-solutions/data/excel-conditional-branch.json`
- **Mismatch inventory:** `docs/issue-solutions/data/heavy-formualizer-excel-mismatch-inventory.json`
- **Production behavior:** unchanged. No Engine V2 implementation, optimization, or default behavior change.

## Executive conclusion

No specific semantic mismatch has yet been proven to create the Heavy cycle.

Two targeted Excel-semantic experiments were completed:

```text
1. Conditional branch/value correction for CashFlow Engine!K65
2. Excel-selected INDEX target substitution for CashFlow Engine!J8
```

The conditional correction improves the value representation from Formualizer `Text("")` to Excel blank, but changes no dependency or SCC topology.

The INDEX counterfactual replaces the broad `J8 -> Cash_Flow_Inputs` dependency surface with the exact Excel-selected `CashFlow Inputs!J23` target. It removes 1 static edge / 4,131 runtime-expanded edges and adds one exact target edge, yet the large components remain:

```text
static:           4,829 -> 4,829
runtime-observed: 4,142 -> 4,142
```

This is strong Excel-assisted evidence that the current dependency model can still construct a comparable large cycle after narrowing one high-leverage INDEX dependency. It is Level B rather than full Level C because Excel's invalidation dependency surface is not fully observable and the formula returns a value, not an explicit reference object.

The strongest classification is therefore:

```text
DEPENDENCY_OVER-APPROXIMATION
```

with **medium confidence**, while interaction with semantic mismatches remains possible. The central counterfactual is:

```text
LIKELY YES, but still unproven:
if currently known semantic defects were corrected, the current dependency
model would likely still construct a comparable Heavy SCC.
```

## 1. Complete Formualizer feedback cycle

The static graph contains this complete cycle:

```text
cash_flow_inputs                         [named-range vertex]
  -> CashFlow Inputs!J23                 [range origin, mask 2]
  -> CashFlow Engine!K29                 [range origin, mask 2]
  -> CashFlow Engine!J11                 [direct-cell origin, mask 1]
  -> cash_flow_inputs                    [named-range origin, mask 16]
```

The relevant formulas are:

```text
CashFlow Inputs!J23
=EDATE(J24,MIN('CashFlow Engine'!K29:K112)-12)

CashFlow Engine!K29
=IF($I29="Yes",IFERROR(INDEX(Schedule_Offset,...),""),"")

CashFlow Engine!J11
=INDEX(Cash_Flow_Inputs,
       MATCH($C11,Cash_Flow_Inputs_R,0),
       MATCH($J$6,Cash_Flow_Inputs_C,0))
```

The final runtime-observed graph contains this complete cycle:

```text
CashFlow Inputs!J23                  [range origin, mask 2]
  -> CashFlow Engine!K65
CashFlow Engine!K65                   [direct-cell origin, mask 1]
  -> CashFlow Engine!I65
CashFlow Engine!I65                   [direct-cell origin, mask 1]
  -> CashFlow Engine!J11
CashFlow Engine!J11                   [named-range origin, mask 16]
  -> CashFlow Inputs!J23
```

Relevant formulas:

```text
CashFlow Inputs!J23
=EDATE(J24,MIN('CashFlow Engine'!K29:K112)-12)

CashFlow Engine!K65
=IF(I65="Yes",K64-1,"")

CashFlow Engine!I65
=IF(AND($J$11="CC",$J$14>4),"Yes","No")

CashFlow Engine!J11
=INDEX(Cash_Flow_Inputs,
       MATCH($C11,Cash_Flow_Inputs_R,0),
       MATCH($J$6,Cash_Flow_Inputs_C,0))
```

These are complete Formualizer cycles. They are not claims about Excel's private dependency graph.

## 2. Origin-mask combinations

### Static logical graph

The 13,399 deduplicated static internal edges have these exact mask combinations:

| Origin mask | Count |
| --- | ---: |
| `RANGE` only | 8,026 |
| `NAMED` only | 3,013 |
| `DIRECT` only | 1,834 |
| `DIRECT|DYNAMIC` | 268 |
| `NAMED|DYNAMIC` | 258 |

The complete static cycle uses:

```text
RANGE only
RANGE only
DIRECT only
NAMED only
```

Named and range provenance are independent on this concrete cycle: they occur on different logical edges, not as two bits on the same edge.

### Runtime-observed expanded graph

The 2,076,397 runtime-observed edges have these exact combinations:

| Origin mask | Count |
| --- | ---: |
| `NAMED` only | 1,250,893 |
| `RANGE|NAMED` | 816,128 |
| `RANGE` only | 8,026 |
| `DIRECT` only | 1,337 |
| `DIRECT|NAMED` | 13 |

The complete runtime cycle uses:

```text
RANGE only
DIRECT only
DIRECT only
NAMED only
```

Therefore:

```text
Named and range are independent on the representative direct cycle.
They also overlap heavily in the expanded runtime graph: 816,128 edges carry
both RANGE and NAMED provenance.
```

It is incorrect to describe all named and range causality as independent. The graph proves both independent edge roles and substantial overlapping provenance.

## 3. Targeted conditional semantic correction

### Candidate

```text
CashFlow Engine!K65
=IF(I65="Yes",K64-1,"")
```

Excel COM evaluation reports:

```text
I65 formula:       =IF(AND($J$11="CC",$J$14>4),"Yes","No")
I65 value:         "No"
K65 selected path: false literal ""
K65 Excel value:   ""
DirectPrecedents:  I65, K64
```

The inactive `K64` branch remains visible to Excel's DirectPrecedents API. This is direct evidence that current-state branch execution and dependency invalidation are distinct: Excel selects only the false branch for value evaluation but still exposes the inactive precedent in its dependency audit.

Formualizer also selects the false literal branch and returns `Text("")`, differing only in blank representation.

### Diagnostic correction

The diagnostic correction maps this selected empty string result to canonical blank/`Empty`.

```text
Excel/Formualizer value parity: improves for K65
Dependency edges changed:       no
Static SCC:                     4,829 -> 4,829
Runtime SCC:                    4,142 -> 4,142
Cross-sheet cycle:              remains
```

Conclusion:

```text
The tested conditional semantic mismatch is not causal for the giant cycle.
No Excel-unrequired conditional edge was proven: Excel itself exposes I65,K64.
```

This is a narrow value-semantic Level-C correction, not a dependency deletion.

## 4. Targeted INDEX/reference correction

### Candidate

```text
CashFlow Engine!J8
=INDEX(Cash_Flow_Inputs,
       MATCH($C8,Cash_Flow_Inputs_R,0),
       MATCH($J$6,Cash_Flow_Inputs_C,0))
```

Excel COM resolves the selected reference as:

```text
CashFlow Inputs!J23
```

The current Formualizer graph has:

```text
static J8 -> cash_flow_inputs named vertex
runtime J8 -> 4,132 resolved named/range-expanded targets
```

The diagnostic counterfactual replaces the selected source's outgoing dependency surface with:

```text
J8 -> CashFlow Inputs!J23
```

Observed topology:

| Variant | Static largest SCC | Runtime largest SCC | Cross-sheet cycle |
| --- | ---: | ---: | --- |
| Baseline | 4,829 | 4,142 | Yes |
| J8 exact selected target | 4,829 | 4,142 | Yes |

The counterfactual removes one static edge / 4,131 runtime-expanded edges and adds one exact target edge. The large cycle remains unchanged.

This result is not called a full Level-C semantic correction because:

```text
J8 returns a value, not an explicit reference object.
Excel's invalidation surface is not fully observable through COM.
Replacing dependency edges is a semantic counterfactual, not a changed evaluator.
```

It is nevertheless strong Excel-assisted evidence against the claim that this one broad INDEX expansion alone is the root cause.

## 5. Before/after targeted corrections

| Correction | Level | Static SCC before → after | Runtime SCC before → after | Cycle result |
| --- | --- | ---: | ---: | --- |
| `K65` empty-string → blank | C value semantics | 4,829 → 4,829 | 4,142 → 4,142 | Remains |
| `J8` broad INDEX surface → Excel-selected J23 | B counterfactual | 4,829 → 4,829 | 4,142 → 4,142 | Remains |
| Both together | C+B diagnostic combination | 4,829 → 4,829 | 4,142 → 4,142 | Remains |

No selected correction materially improved topology.

## 6. Relation to broad mismatch ablation

The broad Level-A mismatch-source ablation remains:

```text
static:           4,829 -> 208
runtime-observed: 4,142 -> 461
```

That result means mismatch-source edges are graph-theoretically important in the current Formualizer graph. It does not mean that correcting all those formulas would remove those edges.

The targeted results are different:

```text
unsupported/#NAME source removal: no material topology change
K65 conditional/value correction: no topology change
J8 Excel-selected INDEX target:   no topology change
```

Therefore the evidence does not justify claiming that semantic mismatch is the primary cause of the giant SCC. The broad mismatch set may interact with dependency expansion, but no individual semantic correction has demonstrated that causal effect.

## Final answers

### 1. Complete Formualizer feedback cycle

Yes:

```text
CashFlow Inputs!J23
-> CashFlow Engine!K65
-> CashFlow Engine!I65
-> CashFlow Engine!J11
-> CashFlow Inputs!J23
```

Origin masks are `RANGE`, `DIRECT`, `DIRECT`, `NAMED`.

### 2. Which edges carry which combinations?

For that cycle:

```text
J23 -> K65: RANGE only
K65 -> I65: DIRECT only
I65 -> J11: DIRECT only
J11 -> J23: NAMED only
```

### 3. Are named and range causality independent?

Both patterns exist:

```text
representative cycle: independent edge roles
runtime graph:       substantial RANGE|NAMED overlap (816,128 edges)
```

They must not be treated as wholly independent mechanisms.

### 4. Can a conditional mismatch be proven to create an Excel-unrequired edge?

No. For K65, Excel selects the false branch but DirectPrecedents still exposes I65 and K64. The tested correction only changes blank representation and does not change the graph.

### 5. Can an INDEX/reference-return mismatch be proven to create an Excel-unrequired edge?

Not conclusively. Excel selects `CashFlow Inputs!J23` for J8, and narrowing J8 to that exact target is a valid Level-B counterfactual. It leaves the large SCC unchanged, but Excel's complete invalidation surface is not observable enough to call the original named-range edge incorrect.

### 6. Did a proven semantic mismatch correction change SCC topology?

No.

```text
K65 correction: no topology change
J8 selected-target counterfactual: no topology change
combined: no topology change
```

### 7. Before/after sizes

```text
K65 value correction:  static 4,829 -> 4,829; runtime 4,142 -> 4,142
J8 target correction:  static 4,829 -> 4,829; runtime 4,142 -> 4,142
combined:             static 4,829 -> 4,829; runtime 4,142 -> 4,142
```

### 8. Does a large cycle remain after the strongest targeted corrections?

Yes. The static and runtime-observed CashFlow Inputs ↔ CashFlow Engine cycles remain.

### 9. If known causally relevant semantic defects were corrected, would the current engine still construct a comparable Heavy SCC?

**LIKELY YES, but still unproven.**

The strongest evidence is the J8 Excel-selected-target substitution: a broad INDEX dependency surface was replaced with one Excel-observed target and the SCC did not shrink. The conditional correction also improved value parity without changing topology.

A complete correction of all known semantic mismatches has not been implemented, so this cannot be classified PROVEN YES.

### 10. Primary root cause classification

**DEPENDENCY OVER-APPROXIMATION**, medium confidence.

The current dependency model is independently sufficient in the tested J8 counterfactual, and named/range expansion is essential to the graph. Semantic mismatches remain a plausible amplifier/interaction, but no specific correction has broken or materially shrunk the cycle.

### 11. Engine V2 ordering

Engine V2 should fix correctness before optimization, in this order:

```text
1. Excel-compatible formula and reference semantics
2. Exact named/range/INDEX/INDIRECT/OFFSET/conditional/spill dependency semantics
3. Demand-driven runtime reference discovery
4. Runtime cycle creation only after actual feedback is observed
5. Retained cyclic workspaces only as a later proven-safe optimization
```

Static SCCs should not be the primary scheduling abstraction for this workload. The current static graph is highly sensitive to virtual dependency expansion and is not validated by Excel circular-reference evidence.

## What remains unproven

```text
Excel's complete private dependency graph
Excel's complete circular working-set membership
Whether Excel's invalidation graph uses exact INDEX targets or source ranges
Whether a broader set of semantic corrections changes SCC topology
Whether the current Formualizer dependency model remains large after all
known semantic defects are corrected together
```

## Scope and safety

```text
No Engine V2 implementation.
No production/default Formualizer changes.
No broad compatibility project.
All semantic corrections were diagnostic simulations only.
All graph removals/counterfactuals are not production fixes.
```
