# Heavy False-Cycle Root Cause: Dependency Over-Approximation vs Semantic Mismatch

- **Branch:** `investigation/fossil-upstream-integration`
- **Workbook:** `Fossil_EstimatingTemplate_2026-08_21_A.xlsx`
- **Excel oracle:** `docs/issue-solutions/data/excel-circular-set-oracle.json`
- **Mismatch inventory:** `docs/issue-solutions/data/heavy-formualizer-excel-mismatch-inventory.json`
- **Graph artifact:** `docs/issue-solutions/data/heavy-graph-root-cause.json`
- **Raw static graph:** `docs/issue-solutions/data/heavy-static-scc-edge-dump.tsv.zip`
- **Raw runtime graph:** `docs/issue-solutions/data/heavy-scc-edge-dump.tsv.zip`
- **Production behavior:** unchanged; no Engine V2 work and no default Formualizer change.

## Executive finding

The evidence does not support treating Formualizer's 4,829-member SCC as an Excel-equivalent circular calculation region.

Excel's public oracle reports:

```text
Heavy Worksheet.CircularReference seeds with iteration enabled:  0
Heavy Worksheet.CircularReference seeds with iteration disabled: 0
Heavy Excel dependency-trace paths from Heavy seeds:             0
```

The same COM probe detects a disposable synthetic cycle, so the zero Heavy result is meaningful evidence rather than a blanket API failure.

Formualizer's graph analysis shows that the large region is maintained by multiple dependency families, especially named-range and range/virtual expansion. Broad known mismatch-source ablation also shrinks the graph substantially. No isolated Level C semantic correction has yet proved that correcting a specific formula behavior removes the feedback edges.

Best-supported classification:

```text
D. INTERACTION

Semantic mismatches are graph-theoretically involved in the current engine graph,
while named/range/dynamic dependency expansion amplifies the resulting region.
The exact share attributable to actual semantic correction remains unproven.
```

Confidence: **medium**. H3/H2 remain substantially better supported than H1.

## Evidence levels

### Level A — graph causality

The evaluator's actual static graph dump and final runtime edge dump were analyzed by SCC recomputation after edge-family/source ablation. These experiments establish graph causality only:

```text
removed/filtered edges can be necessary to preserve the graph SCC
```

They do not establish that the removed edges are incorrect Excel dependencies.

### Level B — Excel-assisted current-state semantics

Excel was used for circular seed detection, public dependency tracing, recalculated values, calculation settings, and disposable intervention copies. No Heavy seed or trace path was exposed, so no Excel-assisted current-state dependency graph could be constructed without assumptions.

### Level C — actual semantic correction

No targeted semantic correction was attempted. Existing mismatch evidence is broad and no single Excel-observed feedback witness identified a uniquely causal semantic defect.

## 1. Exact SCC structure

Formualizer's verified current-state artifacts report:

```text
total graph vertices:             167,161
total graph SCCs:                 159,844
cyclic SCCs:                           84
largest static SCC:                4,829
runtime-live members:              4,139
main SCC cell members:             4,825
main SCC name members:                 4
CashFlow Inputs cell members:      4,130
CashFlow Engine cell members:        695
```

The diagnostic static graph dump at the actual `static_scc_probe` virtual-dependency boundary contains:

```text
logical internal edges: 13,399
largest SCC:             4,829
cyclic components:           1
cross-sheet cycle:         yes
```

The final runtime-observed graph contains:

```text
expanded internal live edges: 2,076,397
largest runtime-observed SCC: 4,142
cyclic components:                  1
cross-sheet cycle:                yes
```

The prior static origin artifact reports approximately 2.04M static-member live-edge observations. Those are overlapping observations, not unique logical edge count; the raw graph dump records the deduplicated logical graph separately.

### Internal edge taxonomy

Static graph construction records:

| Origin | Logical static edges carrying origin |
| --- | ---: |
| direct cell | 2,102 |
| bounded/virtual range | 8,026 |
| named range | 3,271 |
| dynamic virtual reference | 526 |
| table/whole-row/whole-column/other | 0 observed in this SCC dump |

Origin masks can overlap when one logical edge is reached through more than one construction path.

Runtime evaluator observations record:

| Origin | Expanded runtime edge observations carrying origin |
| --- | ---: |
| direct cell | 1,350 |
| range | 824,154 |
| named range | 2,067,034 |
| dynamic reference | 0 explicit origin-mask observations |
| table/whole-row/whole-column/other | 0 observed |

The zero explicit runtime dynamic-origin count does not mean dynamic formulas are absent. Dynamic formulas are present and usually resolve to direct or named/range targets at runtime; the evaluator's origin mask records the resolved read origin.

## 2. Concrete feedback witnesses

The actual static graph contains cross-sheet feedback in both directions:

```text
CashFlow Inputs!J23 -> CashFlow Engine!J8
CashFlow Engine!J8 -> CashFlow Inputs!AL126
```

The final runtime-observed graph contains:

```text
CashFlow Inputs!J23 -> CashFlow Engine!K65
CashFlow Engine!J8 -> CashFlow Inputs!AH175
```

The complete raw witness records include member indices and evaluator AST debug forms in `heavy-graph-root-cause.json`. The dominant source families in the existing top-edge artifact are `INDEX/MATCH` formulas reading named ranges such as `Cash_Flow_Inputs`, `Key_Project_Milestones`, and related row/column names.

These witnesses prove Formualizer graph feedback. They do not prove Excel uses the same edges.

## 3. Semantic mismatch inventory

Using a fresh Excel COM-recalculated copy with `Inputs!F7=300` and a current Formualizer evaluation:

```text
all formula mismatches:       8,729
main static SCC mismatches:   4,360
```

Main SCC categories:

| Category | Count |
| --- | ---: |
| other type/value mismatch | 4,326 |
| numeric value mismatch | 22 |
| Excel numeric -> Formualizer #VALUE! | 12 |

Examples include:

```text
CashFlow Inputs!J55
  Excel:        "Milestone Date"
  Formualizer:  #SPILL!
  features:     INDIRECT/OFFSET, array/spill

CashFlow Inputs!K55
  Excel:        "Milestones"
  Formualizer:  #NAME?
  features:     dynamic reference, conditional, array/spill, _xlfn.MAP

CashFlow Inputs!V105:...:AQ105
  Excel:        approximately 1
  Formualizer:  0
  feature:      bounded range SUM

CashFlow Inputs!AL126 and CashFlow Inputs!AH175
  Excel:        blank
  Formualizer:  empty string
  features:     conditional/INDEX
```

The mismatch inventory contains address, formula, Excel result, Formualizer result, feature labels, and static-SCC membership for every listed mismatch. A complete runtime-live address set was not available in prior artifacts, so runtime membership is marked unknown except for explicit prior samples. The graph analyzer independently reports 4,360 mismatched addresses in the static main component and 3,679 in the runtime-observed largest component.

### Mismatch formulas in feedback witnesses

The cross-sheet witness endpoints include mismatch cells:

```text
CashFlow Engine!K65
CashFlow Inputs!AL126
CashFlow Inputs!AH175
```

The source endpoints `CashFlow Inputs!J23` and `CashFlow Engine!J8` match the normalized Excel/Formualizer value comparison. Thus known mismatches participate in graph feedback paths as targets, but this does not prove their semantic errors create the cycle.

## 4. Mismatch-source ablation

Removing all outgoing edges whose source is a known main-SCC mismatch produces:

| Graph | Baseline largest SCC | After ablation | Original members retained | Cross-sheet cycle |
| --- | ---: | ---: | ---: | --- |
| Static | 4,829 | 208 | 204 | yes |
| Runtime-observed | 4,142 | 461 | 459 | yes |

This is strong Level A graph causality evidence: mismatch-source edges are broadly necessary for the current large component.

It is not Level C semantic evidence. The ablation removes all outgoing edges from 4,360 mismatch cells, including many formulas whose mismatch may be incidental to the feedback structure.

## 5. Dependency-family ablation

| Graph variant | Largest SCC | Original members retained | Inputs | Engine | Cross-sheet cycle |
| --- | ---: | ---: | ---: | ---: | --- |
| Current static graph | 4,829 | 4,825 | 4,130 | 695 | yes |
| Static without named-range origins | 1 | 1 | 1 | 0 | no |
| Static without range origins | 1 | 1 | 1 | 0 | no |
| Static without dynamic origins | 4,555 | 4,522 | 3,898 | 653 | yes |
| Static direct-cell-only | 1 | 1 | 1 | 0 | no |
| Runtime observed all | 4,142 | 4,138 | 3,613 | 525 | yes |
| Runtime without named-range origins | 1 | 1 | 1 | 0/1 | no |
| Runtime without range origins | 3,597 | 3,597 | 3,072 | 525 | yes |
| Runtime direct-cell-only | 1 | 1 | 1 | 0 | no |
| Runtime without dynamic-origin mask | 4,142 | 4,138 | 3,613 | 525 | yes |

Important interpretation:

```text
Named and range edges are graph-theoretically essential.
Dynamic formulas are graph-theoretically important in static construction,
but the runtime origin mask often records their resolved direct/named/range read.
Direct exact-cell edges alone do not preserve the large cycle.
```

The independent named/range removals show dependency representation is a major amplifier. They do not show that Excel-compatible named/range dependencies should be removed.

## 6. Cross-ablations

Selected combinations:

| Variant | Static largest SCC | Runtime largest SCC |
| --- | ---: | ---: |
| mismatch sources + named origins | 1 | 1 |
| unsupported sources + range origins | 1 | 3,597 |
| conditional mismatch sources + dynamic origins | 254 | 581 |

The smallest observed explanatory combinations are broad, not isolated:

```text
all known mismatch-source edges + named-origin edges -> no large SCC
conditional mismatch sources + dynamic-origin edges -> 254 static / 581 runtime
```

No combination proves a specific semantic bug. It supports interaction rather than a single dominant unsupported function.

## 7. Excel-assisted evidence

Excel oracle facts:

```text
CircularReference seeds with iteration enabled:  0
CircularReference seeds with iteration disabled: 0
ShowPrecedents/ShowDependents paths:             none
NavigateArrow paths from Heavy seeds:            none
```

Ten fresh-copy interventions over the two Formualizer regions and four row bands per region had zero Excel seeds before and after. Because the baseline had no Excel seed, these are not feedback-cut experiments and must not be interpreted as full Excel SCC membership.

No Excel-assisted current-state graph was constructed. Uncertain references were retained rather than guessed.

## 8. Unsupported-function question

The main direct unsupported-function witness is the `_xlfn.MAP` portion of `CashFlow Inputs!K55`. It is a real Excel/Formualizer output mismatch and lies inside the main SCC.

Removing unsupported/#NAME source edges does not materially reduce the current graph:

```text
static:           4,829 -> 4,828
runtime-observed: 4,142 -> 4,142
```

Therefore unsupported functions are not shown to be the dominant graph cause. They are currently **incidental or unproven** with respect to the giant cycle. The function may affect values/branches, but no isolated correction has demonstrated that effect.

## 9. Static, runtime, direct, and Excel-assisted comparison

```text
current static graph:
  largest SCC 4,829; cross-sheet feedback yes

current runtime-live/observed graph:
  largest SCC 4,142; cross-sheet feedback yes

direct exact-cell-only graph:
  largest SCC 1; no cross-sheet cycle

runtime-observed exact graph including resolved named/range reads:
  largest SCC 4,142; cross-sheet feedback yes

Excel-assisted current-state graph:
  not constructed; no Excel seed/path evidence

after targeted semantic correction:
  not available; no Level C correction attempted
```

Using only direct exact-cell references does not preserve the large cycle. Using all evaluator-observed resolved references, including named/range observations, does preserve a large cycle. That is evidence that the named/range/reference model is central to Formualizer's current feedback structure.

## 10. Root-cause classification

### Q1 — Is the SCC mainly conservative dependency over-approximation?

It is a major contributor and graph amplifier. Named/range removal independently destroys the static large SCC, and direct-cell-only edges do not preserve it. Whether those dependencies are incorrect Excel dependencies is not proven.

### Q2 — Is it mainly semantic mismatch?

Known mismatch-source ablation substantially shrinks both static and runtime graphs. This proves graph causality of mismatch-source edges, not that correcting the underlying formulas would remove the edges. Actual semantic causality remains unproven.

### Q3 — Is it an interaction?

Yes is the best-supported classification. The current engine has broad semantic mismatches and a dependency model whose named/range/dynamic expansion connects many members. Either effect can amplify the other.

### Q4 — Would best-established Excel-compatible semantics still produce a large region?

**UNPROVEN.**

Evidence points both ways:

```text
named/range graph ablation alone destroys the large SCC
mismatch-source ablation alone shrinks it to 208/461
no isolated semantic correction has been performed
Excel exposes no Heavy seed/path for a direct counterfactual
```

The unsupported-function subset alone does not matter much, but the broad semantic mismatch set is graph-theoretically important.

## 11. Minimal feedback backbone

A globally minimal feedback-edge set was not attempted. The understandable backbone is:

```text
CashFlow Inputs!J23 -> CashFlow Engine!J8
CashFlow Engine!J8 -> CashFlow Inputs!AL126
```

and in the runtime-observed graph:

```text
CashFlow Inputs!J23 -> CashFlow Engine!K65
CashFlow Engine!J8 -> CashFlow Inputs!AH175
```

These are representative cross-sheet paths, not complete cycle paths. Both static and runtime graphs contain a direct/exact evaluator-supported CashFlow Inputs <-> CashFlow Engine feedback structure when named/range-resolved observations are included.

## 12. Engine V2 implications

Engine V2 should not use the current static SCC partition as its primary scheduling abstraction.

Recommended order:

```text
1. Excel-compatible formula and reference semantics
2. Precise dependency representation and live-reference discovery
3. Demand-driven calculation-chain execution
4. Runtime cycle discovery only after actual feedback is observed
5. Retained exact cyclic workspaces only as a later eligible optimization
```

Before trusting an Engine V2 scheduler benchmark against Excel, these must be correct:

```text
formula semantics
conditional branch semantics
reference-return behavior
INDIRECT/OFFSET target resolution
named/range/open-range representation
array/spill behavior
unsupported/error/coercion behavior
runtime/live dependency discovery
cycle discovery
volatile/external/effect generations
```

Performance optimizations such as retained workspaces should come afterward and only for a certificate-proven region.

## Final answers

1. **Why does Formualizer create the 4,829-member SCC?**

   The current graph combines large named/range/virtual dependency expansion with dynamic-reference and formula-semantic behavior. Known mismatch-source edges are broadly involved, while named/range edges connect the region into one component.

2. **Which edges close the loop?**

   Cross-sheet paths such as `CashFlow Inputs!J23 -> CashFlow Engine!J8 -> CashFlow Inputs!AL126` close representative loops. The runtime graph has analogous paths through `K65` and `AH175`.

3. **One dominant family or several?**

   Several independent mechanisms. Named and range families are individually graph-critical; dynamic and mismatch-source families further shrink the graph when removed.

4. **How much is named/range/open-range expansion?**

   Existing observation counts are approximately 2.03M named-range and 812K range observations. The deduplicated static graph has 3,271 named-origin and 8,026 range-origin logical edges, with overlap. Removing either named or range origins collapses the static SCC to size 1.

5. **How much is static/non-live collection?**

   Static graph size is 4,829 versus prior runtime-live count 4,139; approximately 690 members are static-only by the verified artifact. The runtime-observed graph still has a 4,142-member component, so static-only collection amplifies but does not fully explain the observed feedback.

6. **How much is dynamic-reference behavior?**

   270 members are classified dynamic/volatile. Removing dynamic virtual origins reduces the static largest SCC to 4,555, but the runtime explicit dynamic-origin mask is zero because dynamic formulas resolve to named/direct/range origins. Exact dynamic causal share remains unproven.

7. **How many known mismatches lie inside the main SCC?**

   4,360 known output/type mismatches lie inside the static main SCC. The runtime-observed largest component contains 3,679 known mismatch addresses by graph intersection.

8. **Do mismatch formulas participate in feedback paths?**

   Yes, mismatch endpoints appear in representative feedback paths. Source-level causal responsibility is not isolated.

9. **What happens when mismatch-source edges are removed?**

   Static largest SCC: `4,829 -> 208`. Runtime-observed: `4,142 -> 461`.

10. **What happens when unsupported-function edges are removed?**

    Static: `4,829 -> 4,828`. Runtime-observed: unchanged at `4,142`. Unsupported functions alone are not shown to drive the giant cycle.

11. **Are unsupported functions causal?**

    Currently incidental or unproven. They create real value mismatches but are not graph-dominant in the ablation.

12. **Can a specific wrong semantic behavior be shown to create an incorrect live edge?**

    Not yet. The `K55` `_xlfn.MAP`/spill mismatch is a strong candidate, but no isolated Excel-observed edge correction has been demonstrated.

13. **Can a targeted correction break/shrink the SCC?**

    No Level C correction was attempted. Unproven.

14. **How large is the Excel-assisted current-state graph?**

    It was not constructed because Excel exposed no Heavy seed or trace path. No size is claimed.

15. **Does the direct/runtime-supported CashFlow Inputs <-> CashFlow Engine cycle still exist?**

    Direct exact-cell-only: no large cycle. Runtime-observed exact references including named/range reads: yes, a 4,142-member component with cross-sheet feedback remains.

16. **If known unsupported/incorrect semantics were fixed, would the current engine still create the large SCC?**

    **UNPROVEN.** Broad mismatch-source edge removal shrinks the graph, but edge ablation is not semantic correction and no Level C proof exists.

17. **Which root-cause statement is best supported?**

    **Both dependency over-expansion and semantic mismatches interact.** Confidence medium; the precise semantic-versus-dependency contribution remains unresolved.

18. **What must Engine V2 fix first?**

    Formula/reference semantics and precise dependency/live-reference representation, including named/range/dynamic/conditional/array/error behavior.

19. **Should static SCCs be the primary scheduler abstraction?**

    No. The current static SCC is not Excel-validated and is highly sensitive to virtual dependency expansion.

20. **Should V2 begin with precise/demand-driven evaluation and runtime feedback discovery?**

    Yes. Create cyclic workspaces only after actual runtime feedback is discovered; retain exact fixed-point workspaces later as a proven-safe optimization.

## Scope and safety

```text
No Engine V2 implementation.
No production Formualizer behavior change.
No Fossil-specific production shortcut.
Excel interventions used disposable copies.
Graph ablations are diagnostic causality only.
Excel zero-seed evidence is not treated as proof of zero private Excel behavior.
```
