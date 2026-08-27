# Bounded Engine V2 Architecture POC

## Decision

**REVISE AND REPEAT POC.**

The bounded POC supports the proposed direction on synthetic workbooks and now executes against the supplied real Heavy XLSX, but it does not justify full Engine V2 development yet.

What is proven by this POC:

- A name can remain a typed symbolic definition over canonical cell values.
- Invalidation relations can retain a broad range/name surface without making every relation a runtime-cycle edge.
- `INDEX(name, row, col)` can preserve the source descriptor and selector invalidation while recording only the selected cell for execution and cycle analysis.
- Demand evaluation can discover genuine feedback from actual reads and keep a false static cycle out of the cyclic workspace.
- A conservative iterative fallback can operate on the discovered workspace.
- The real Heavy J11/I65/K65/J23 witness now matches Excel values without restoring the old J11-to-J23 edge.
- Existing production routing and defaults are unchanged.

Why the gate is not `PROCEED`:

- The exact Heavy XLSX is outside the repository checkout, but the supplied Downloads path now loads successfully. The Light workbook and a Light raw dependency dump are still not available for an equivalent independent run.
- POC B uses the existing parser and `LiteralValue`, but its supported evaluator is a bounded adapter-host implementation, not a drop-in execution of the existing formula registry. Existing function implementations need an adapter contract before their state reads can be claimed as V2 reads.
- The earlier full Heavy pass reported 35,857 unsupported formulas on the initial bounded-evaluator run. The known witness is now closed with focused generic semantics, but the full-workbook unsupported inventory remains and the zero-cycle result is not a full Heavy semantic verdict.
- The synthetic Heavy slice is intentionally representative, not full Heavy output parity.
- No retained exact cyclic-state certificate was attempted, as required by scope.

The correct next action for this iteration is to stop after the verified witness. Do not replace V1 or start a broad compatibility rewrite.

## Scope and repository audit

The repository checkout did not contain `AGENTS.md`, `PROJECT_STATE.md`, or architecture files when the first POC run began; the authoritative fork guidance and project memory were subsequently read from `C:\Users\OXK0A0A\Downloads`. The branch was clean at the start:

```text
branch: investigation/fossil-upstream-integration
working tree: clean
```

Checked-in investigation reports are the source of truth for prior Heavy results. In particular:

- `026-heavy-false-cycle-root-cause.md` classifies the current region as interaction between semantic mismatch and named/range expansion, and rejects the current static SCC as an Excel-equivalent region.
- `027-targeted-level-c-semantic-causality.md` reports that the K65 value correction and J8 selected-target counterfactual did not change the 4,829/4,142 topology.
- `012-exact-symbolic-dependency-investigation.md` proves symbolic dirty-query parity on prior controls but also shows that symbolic SCC traversal still visited 3,116,386 logical range neighbors and changed the partition.
- `019-latest-upstream-heavy-baseline.md` records the latest V1 Heavy workload and timing.
- `024-retained-workspace-architecture-blockers.md` rejects unsafe volatile/dynamic retained reuse and identifies the missing generation/certificate contracts.

The new implementation is isolated at `crates/formualizer-engine-v2-poc/`. It is not referenced by any production engine or binding.

## 1. Source-claim matrix

The HyperFormula source references below are pinned to commit `68ae69102969784246bbd29f6646c446f0270bc7`, observed from the current official GitHub repository.

| Topic | Excel documented fact | HyperFormula implementation | Unknown/inference | V2 implication |
| --- | --- | --- | --- | --- |
| Name representation | Open XML `definedName` represents a cell, range, formula, or constant. `Name.RefersTo` exposes/sets an A1 formula string. | `NamedExpressions` maintains workbook and worksheet stores and assigns an internal address to a named expression. | Excel's private in-memory representation is not documented. | Store a name definition and scope; do not infer an independent copied value table. |
| Name scope | Excel documents workbook names and worksheet-specific names. Open XML uses `localSheetId` for local scope. | `NamedExpressions.nearestNamedExpression` checks worksheet scope before workbook scope; names are case-normalized. | Exact precedence edge cases and all external-name behavior remain engine-specific. | Resolve `(scope, normalized name)` at read time and include definition generation in invalidation. |
| Name kind | `RefersToRange` returns a `Range` only when the name refers to a range; it fails for constants/formulas. | HyperFormula accepts named expressions for formula, string, number, and range-like expressions and represents them through named-expression graph/address state. | Excel's exact treatment of formula-valued names in every argument position is not exposed by these APIs. | Preserve `Constant`, `Formula`, `CellReference`, `RangeReference`, and dynamic kinds instead of forcing every name to a range. |
| Name/structural edits | Excel lists adding/editing/deleting a defined name and row/column edits as recalculation triggers; structural edits rebuild dependency information. | `RangeMapping` has row/column movement and truncation operations; named expressions have add/change/remove operations. | Full Excel reference-adjustment rules, tables, spills, and external links need oracle coverage. | Structural generation is a first-class invalidation surface; definitions must be relocated or fail closed. |
| Calculation chain | Excel documents a dependency tree, a calculation chain, and recalculation as separate stages. The Open XML calcChain records prior formula order, not the dependency tree, and does not dictate runtime order. | HyperFormula builds a dependency graph and topological/cycle processing; it also persists/reuses range vertices and graph state internally. | Excel's private chain data structures and heuristics are unknown. | Retain calculation order as a scheduling hint, not as the primary dependency or cycle truth. |
| Dirty propagation | Excel marks changed formulas/values/names and direct/indirect dependents dirty; smart recalculation evaluates the affected set and volatile formulas. | HyperFormula's graph has dirty/volatile nodes and a calculation chain. | Excel's complete invalidation surface for dynamic/reference-returning formulas is not publicly enumerable. | Separate conservative invalidation from exact execution reads. |
| Range representation | Excel public documentation describes ranges and dynamic ranges, but does not document an internal range graph/table. `INDEX` is recommended over volatile `OFFSET` for dynamic ranges in performance guidance. | `RangeMapping` keeps one `RangeVertex` per distinct rectangle; `DependencyGraph` composes a range from a smaller prefix and a tail row; `RangeVertex` caches associative/criterion results. | A HyperFormula range vertex is an implementation node, not proof that Excel has the same node or that values are copied. | Use symbolic rectangle descriptors and indexes for storage; enumerate target identity only when evaluation or cycle proof requires it. |
| Range value reads | A formula that consumes a range must obtain the range's cell values; public docs do not say how Excel stores those values. | `Interpreter` evaluates a range as a range value and aggregate plugins consume it; range graph nodes can cache aggregate results. | Whether Excel caches a particular aggregate, and under what generation contract, is private. | Temporary views/caches need explicit ownership and invalidation; canonical values remain worksheet cells. |
| `INDEX`, `OFFSET`, `INDIRECT` | Excel exposes formula/reference APIs and documents `OFFSET`/`INDIRECT` as volatile; dynamic range guidance discusses `INDEX` and `OFFSET`. | HyperFormula has reference-capable function metadata and an interpreter reference path; parser dependency collection is separate from runtime interpretation. | Exact Excel invalidation versus selected-target execution edges for `INDEX` is not observable through the available public APIs. | Retain source/name/selector/shape invalidation, but use the selected target for execution/cycle edges when the result is a reference/value selection. |
| Static versus actual reads | Excel's documented calculation stages and the available Heavy oracle show that dependency auditing and current branch evaluation are not the same observation. | HyperFormula's official differences page explicitly says parser-time dependency collection can report cycles that do not appear during evaluation; `collectDependencies.ts` walks dependencies during parsing. | Excel's complete branch-sensitive runtime read log is private. | Untaken branches may be invalidation precedents without becoming runtime feedback edges. |
| Cycle detection/handling | Excel detects self-dependence and supports iterative calculation with maximum iterations and maximum acceptable change. | HyperFormula uses dependency graph/topological cycle handling; its docs distinguish parser-time dependency collection from evaluation. | Excel's private circular working-set membership and multi-thread scheduling are unknown. | Discover a cycle from active reads, create a bounded workspace, and initially use the conservative existing solver. |
| Volatile/effect behavior | Excel reevaluates volatile functions on recalculation; Microsoft lists `NOW`, `TODAY`, `RANDBETWEEN`, `OFFSET`, `INDIRECT`, and context-dependent functions. | HyperFormula marks volatile functions dirty on documented volatile actions and separately treats structural functions as structure-dependent. | A single boolean cannot represent clock, random seed, dynamic target, shape, external provider, and structural generations. | Track explicit effect generations and fail closed where a generation contract is absent. |
| Historical patents | Microsoft patent publications describe historical multi-thread recalculation engines and dependency levels. | Not applicable. | A patent is not evidence of the current private Excel implementation or its exact graph. | Use patents only as historical architecture context; validate behavior through current public documentation/oracles. |
| Excel private engine | Public documentation describes observable behavior and file metadata only. Patents describe historical embodiments, not current implementation proof. | HyperFormula is inspectable source, not a model of Excel. | No source proves Excel stores names or ranges as a separate mutable data table, nor that Excel uses HyperFormula's graph strategy. | Keep claims bounded to documented behavior, source facts, synthetic observations, and explicit inference. |

Primary sources:

- [Excel recalculation](https://learn.microsoft.com/en-us/office/client-developer/excel/excel-recalculation)
- [Excel calculation performance](https://learn.microsoft.com/en-us/office/vba/excel/concepts/excel-performance/excel-improving-calculation-performance)
- [Excel performance obstructions](https://learn.microsoft.com/en-us/office/vba/excel/concepts/excel-performance/excel-tips-for-optimizing-performance-obstructions)
- [Open XML DefinedName](https://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.spreadsheet.definedname?view=openxml-3.0.1)
- [Name.RefersTo](https://learn.microsoft.com/en-us/office/vba/api/excel.name.refersto)
- [Name.RefersToRange](https://learn.microsoft.com/en-us/office/vba/api/excel.name.referstorange)
- [Excel named ranges and scope](https://learn.microsoft.com/en-us/office/vba/excel/concepts/cells-and-ranges/refer-to-named-ranges)
- [Open XML calculation chain](https://learn.microsoft.com/en-us/previous-versions/office/developer/office-2010/gg278336(v=office.14))
- [HyperFormula dependency graph](https://hyperformula.handsontable.com/docs/guide/dependency-graph.md)
- [HyperFormula named expressions](https://hyperformula.handsontable.com/docs/guide/named-expressions.md)
- [HyperFormula volatile functions](https://hyperformula.handsontable.com/docs/guide/volatile-functions.md)
- [HyperFormula differences](https://hyperformula.handsontable.com/docs/guide/list-of-differences.md)
- [HyperFormula `DependencyGraph.ts` at the recorded commit](https://raw.githubusercontent.com/handsontable/hyperformula/68ae69102969784246bbd29f6646c446f0270bc7/src/DependencyGraph/DependencyGraph.ts)
- [HyperFormula `RangeMapping.ts` at the recorded commit](https://raw.githubusercontent.com/handsontable/hyperformula/68ae69102969784246bbd29f6646c446f0270bc7/src/DependencyGraph/RangeMapping.ts)
- [HyperFormula `RangeVertex.ts` at the recorded commit](https://raw.githubusercontent.com/handsontable/hyperformula/68ae69102969784246bbd29f6646c446f0270bc7/src/DependencyGraph/RangeVertex.ts)
- [HyperFormula `NamedExpressions.ts` at the recorded commit](https://raw.githubusercontent.com/handsontable/hyperformula/68ae69102969784246bbd29f6646c446f0270bc7/src/NamedExpressions.ts)
- [HyperFormula `Interpreter.ts` at the recorded commit](https://raw.githubusercontent.com/handsontable/hyperformula/68ae69102969784246bbd29f6646c446f0270bc7/src/interpreter/Interpreter.ts)
- [HyperFormula parser dependency collection at the recorded commit](https://raw.githubusercontent.com/handsontable/hyperformula/68ae69102969784246bbd29f6646c446f0270bc7/src/parser/collectDependencies.ts)
- [Historical Microsoft concurrent recalculation patent](https://www.freepatentsonline.com/7533139.html)
- [Historical Microsoft dependency-level recalculation patent](https://www.freepatentsonline.com/8032821.html)

## 2. Current-engine reuse/replacement map

| Existing component | Reuse unchanged | Reuse behind adapter | Must replace for V2 scheduling | Reason |
| --- | --- | --- | --- | --- |
| `formualizer-parse::ASTNode`, `ReferenceType`, parser | Parser and AST shape | Reference resolution and reference-result handling | No | The parser is a useful syntax/IR boundary. It does not define runtime dependency semantics. |
| `formualizer-common::LiteralValue` | Yes | No | No | It is the existing value vocabulary used by the POC. |
| Arrow sheet/value store | Canonical cell ownership | `EvaluationHost`/reference resolver | No | Worksheet cells must remain canonical; V2 must not expose graph-copied mutable values. |
| `RangeView` | Existing aggregate kernels where their read contract is extended | Range view creation and read recording | No | Current `RangeView` is value-oriented; its owned-row path and opaque iteration do not preserve selected reference identity by themselves. |
| `NamedDefinition`, `NamedRange` | Concept and input compatibility | Name registry, scope lookup, generations | Current name-to-SCC scheduling relationship | Current names also have graph vertices/dependent state; V2 needs symbol metadata independent of cycle discovery. |
| `DependencyGraph`/CSR | V1 oracle and diagnostics | Optional import/export bridge | Yes as the primary V2 scheduling abstraction | The checked-in Heavy SCC is sensitive to named/range expansion and is not Excel-validated. |
| `range_deps`, interval/stripe indexes | Range-bound and dirty-query algorithms | Invalidation index | No for invalidation | Prior work proved exact dirty parity on controls; it did not prove symbolic SCC equivalence. |
| `Scheduler`/Tarjan | Comparison oracle and fallback diagnostics | Potential local cycle analysis | Yes as a precomputed whole-workbook partition | V2 schedules requested/dirty work first and only analyzes an active feedback region. |
| `LiveEdgeCollector`, `live_graph` | Edge/SCC algorithms | Active-workspace recorder | No, after changing admission boundary | Existing collector starts from a static SCC; V2 can reuse the local exact-read idea after demand evaluation. |
| Existing iterative solver | No direct reuse in this isolated crate | Workspace state/solver adapter | No new solver is justified | The current solver is coupled to `Engine`/`DependencyGraph`; POC B uses a contract-equivalent local fallback and leaves direct V1 solver reuse to the repeat gate. |
| `Interpreter` and formula registry | Existing semantics as a future oracle | `EvaluationHost`, `ReferenceResolver`, `ReadRecorder`, reference-preserving argument adapter | Not all functions | Existing functions can call context/resolver state directly. `ArgumentHandle` is tied to the current interpreter and current range/value contracts; POC B therefore implements the bounded slice explicitly rather than silently claiming full registry compatibility. |
| `effects.rs`, spill registry/locks | Existing effect vocabulary and ownership concepts | Generation/effect adapter | No | These are useful extension points, but shape/effect generations are not complete V2 contracts yet. |
| Workbook backends and name import | Existing source parsing/import | Typed name/range ingestion adapter | No | `calamine` and workbook loaders provide input evidence; V2 should normalize it into symbolic definitions. |
| Python/JS/Rust public APIs | No change | Future diagnostic/export adapter | Out of scope | No public binding or production route is added by this POC. |

Important adapter findings:

- `EvaluationContext` is the right conceptual seam for existing functions, but several current paths resolve names to `Vec<Vec<LiteralValue>>` and construct value-oriented views.
- The current `Interpreter` has an explicit `evaluate_ast_as_reference` path, which is promising, but function argument handling still needs a host-level read recorder that observes reads inside range consumers and selected reference functions.
- Formula code that reads graph/Arrow state directly, bypasses `EvaluationContext`, or assumes graph-owned values must be moved behind the host before it can participate in exact V2 read claims.

## 3. Engine V2 POC architecture

### Canonical workbook data

```text
Workbook
  sheets -> canonical cell values/formula text/ASTs
  names  -> typed definitions pointing to cells/ranges/formulas
  tables -> grid-backed descriptors and shape generations
  spills -> anchor ownership, shape, and cell projection metadata
```

A name stores a definition and generations. A range stores bounds and sheet identity. Neither object owns an independent mutable copy of worksheet cell values. A temporary array/range materialization is an evaluation-owned snapshot, never a second canonical store, and must be invalidated at the end of the read generation.

### Name model

The implemented bounded model is:

```text
NameDefinitionRecord
  id: NameId
  display_name
  scope: Workbook | Sheet
  scope_sheet: optional sheet name
  definition:
    Constant(LiteralValue)
    Cell(CellId)
    Range(RangeDescriptor)
    Formula(ASTNode)
    Spill(SpillRef)
    DynamicFormula(ASTNode)
  resolved_kind
  definition_generation
  structural_generation
```

Production V2 should use stable workbook/sheet IDs rather than names in keys, but the POC keeps sheet names readable in diagnostics.

Resolution is:

1. normalize the name case-insensitively;
2. check the current sheet's local scope;
3. fall back to workbook scope;
4. return the typed definition, not a copied value table;
5. record the definition ID/generation for invalidation;
6. evaluate a formula/dynamic definition through the same host as a cell formula.

A formula-valued name can produce a scalar or a reference. A range-valued name resolves to a `RangeDescriptor` over worksheet cells. A name-definition change increments its definition generation and dirties all users conservatively.

Structural edits must either transform definitions and formula AST reference anchors according to Excel rules or fail closed. The bounded POC transforms name cell/range/spill coordinates for row insertion and conservatively dirties all formulas on a structural event. It does not claim complete Excel formula relocation coverage.

### First-class reference values

```text
EvaluationResult
  Scalar(LiteralValue)
  Array(Vec<Vec<LiteralValue>>)
  Reference(ReferenceValue)
  Error(ExcelError)

ReferenceValue
  Cell(CellId)
  Range(RangeDescriptor)
  Spill(SpillRef)
  Table(TableDescriptor)
```

Reference identity is retained until the consuming operation requests values. `SUM(IF(flag, B1, D1))` therefore resolves the selected `ReferenceValue` before reading cells; it does not read the inactive branch. `INDEX` returns a selected cell reference in the reference-preserving path and a selected value only when the surrounding operation materializes it.

### Dependency layers

#### Invalidation dependencies

Conservative descriptors answer:

> Which events may require this formula to be reevaluated?

The POC records:

```text
Cell(cell)
Range(rectangle)
Name(name definition ID)
Selector(cell)
Structural(sheet)
Shape(spill)
Effect(effect generation key)
```

A range/name descriptor may cover many formula cells. That is intentional for invalidation and does not imply that every cell was read during the current evaluation.

#### Execution reads

Exact observations answer:

> What value or reference did this formula actually read this time?

The POC records cell, range, name, spill, table, and dynamic target events. A range consumer records each cell value it actually consumes. `INDEX` records the source name/range resolution and selector reads, then reads only the selected target cell.

#### Runtime-cycle dependencies

Exact feedback edges answer:

> Did an active evaluation encounter a formula target already on its call stack?

Only actual formula-to-formula reads are eligible. A broad invalidation range, an inactive IF branch, a name definition, or an unresolved dynamic possibility is not by itself a runtime-cycle edge.

### Selected-reference contract

For:

```excel
=INDEX(Cash_Flow_Inputs,
       MATCH(...),
       MATCH(...))
```

The POC contract is:

| Surface | Recorded dependency |
| --- | --- |
| Source name | `NameDefinition(id, generation)` |
| Source bounds/shape | `RangeDescriptor` and shape/structural generation |
| Row selector | selector cells and any ranges consumed by its `MATCH` |
| Column selector | selector cells and any ranges consumed by its `MATCH` |
| Selected target | exact `CellId` execution read |
| Target value | target cell generation/value generation |
| Runtime cycle | exact selected target only |
| Non-selected source cells | invalidation only; not execution/cycle edges |

This is a hypothesis for Excel-compatible behavior, not a claim about Excel's private graph. Existing Heavy evidence supports testing it: Excel exposes inactive `K64` as a direct precedent for `K65`, while current branch execution selects the false branch; this is exactly why invalidation and runtime-read layers cannot be conflated.

### Range alternatives

| Representation | Storage | Dirty query | Value iteration | Runtime/cycle use |
| --- | --- | --- | --- | --- |
| Expanded cell edges | Large and repetitive | Fast after construction | Direct | Over-expands selected/dynamic reads; current Heavy failure mode |
| Symbolic rectangle | Small per formula/name | Requires interval/stripe index | Iterate canonical cells on demand | Exact target identity still must be recorded for actual reads |
| Interval/stripe index | Reusable reverse query | Good for cell/structural edits | Does not own values | Not enough alone to prove runtime cycles |
| Shared range vertex | Reuses equal rectangles; HyperFormula precedent | Good if invalidation is correct | May cache associative aggregates | Must not merge formula-specific branch/selector semantics |
| Lazy target iterator | No persistent expansion | Query on demand | Efficient for bounded consumers | Can still be expensive at cycle analysis; do not hide expansion cost |
| Generation state | O(1) invalidation checks after indexing | Excellent for unchanged surfaces | Requires exact ownership | Useful later for caches/certificates, not a cycle proof by itself |

The POC stores descriptors and expands only in diagnostic static cycle comparison. Its runtime workspace is built from actual reads. It does not claim that symbolic storage alone makes static SCC traversal cheap: prior checked-in experiments disproved that.

### Demand scheduler

```text
request / edit event
  -> dirty closure through invalidation indexes
  -> retain prior order as a hint
  -> evaluate requested roots and dirty formulas through EvaluationHost
  -> collect exact reads and value/reference deltas
  -> if no feedback: commit acyclic values and stop
  -> if a back-edge is encountered:
       identify active cyclic region
       create deterministic cyclic workspace
       run conservative iterative fallback
       propagate changed outputs to dependents
```

The POC's recursive demand walk is deliberately small. A full scheduler would replace recursion with a bounded work queue, use retained order for acyclic waves, and parallelize only independent read-stable work. It must replan if a dynamic target, spill shape, name generation, or provider effect changes during evaluation.

### Runtime cyclic workspace

A workspace is created only from a runtime SCC in the exact runtime-read graph. It stores:

```text
member identity
observed feedback edges
stable deterministic order
previous values
solver configuration and pass count
convergence/failure status
upstream generation inputs
```

The POC initially uses a small contract-equivalent fixed-point loop with a pass cap because the existing solver is coupled to `Engine`/`DependencyGraph` and is not callable from this isolated crate. It does not use a retained exact state certificate, a tolerance-only acceptance proof, or the entire static SCC as a workspace. The repeat gate must put the existing solver behind a workspace adapter and preserve its error, cap, and iteration semantics.

### Explicit effects and generations

The POC exposes effect keys rather than a single `volatile || dynamic` flag:

```text
RecalcEpoch
Clock
Random
DynamicSelector(cell)
DynamicTarget(cell)
TargetValue(cell)
Shape(spill)
External(provider/context)
Structural(sheet)
```

The POC only exercises recalc epoch, dynamic selector/target, spill shape, name definition, and structural events. Full V2 needs per-effect generation ownership for clock snapshots, random seeds, external/UDF providers, table shape, spill relocation, and semantic configuration.

## 4. POC implementation

Files:

- `crates/formualizer-engine-v2-poc/src/model.rs`: typed definitions, references, dependency descriptors, execution trace.
- `crates/formualizer-engine-v2-poc/src/evaluator.rs`: `EvaluationHost`, `ReferenceResolver`, demand scheduler, reference-preserving evaluator, runtime cycle detection, conservative iterative fallback.
- `crates/formualizer-engine-v2-poc/src/shadow.rs`: POC A XLSX/Calamine shadow reader, checked-in artifact shadow reader, and synthetic shadow model.
- `crates/formualizer-engine-v2-poc/src/tests.rs`: bounded test matrix and controls.
- `crates/formualizer-engine-v2-poc/src/bin/engine-v2-poc.rs`: repeatable diagnostic report command.

Supported executable slice:

```text
direct cell references
bounded ranges
workbook and sheet-local names
constant/cell/range/formula/spill names
INDEX/MATCH/VLOOKUP (including INDEX row/column-zero reference results)
IF/IFS/CHOOSE/IFERROR/AND/OR
SUBSTITUTE/ADDRESS/COLUMN
SUM/MIN/SUMPRODUCT
EDATE with canonical date values
reference-returning IF/CHOOSE
OFFSET/INDIRECT dynamic targets
spill-shape invalidation
runtime cycles and iterative fallback
```

Unsupported functions return an explicit `#NIMPL` error through the POC path. They are not counted as successful V2 formula coverage.

`build_xlsx_shadow_metrics` and `build_xlsx_shadow_pair_report` are the input seam for supplied XLSX files. They use the existing Calamine workbook reader to scan worksheet formulas and defined names into the independent shadow model without constructing the production dependency graph.

POC A has two explicit modes. `--artifacts` reads the checked-in compressed Heavy/runtime dumps and JSON summaries, labelling prior runtime-expanded edges as `legacy_runtime_read_count` rather than V2 exact reads. The default real-workbook mode reads formulas, cell values, and defined names directly from the supplied XLSX through Calamine and constructs the V2 POC model without consulting the prior SCC/runtime-edge artifacts.

## 5. Synthetic test results

Command used:

```powershell
$win = "C:\Users\OXK0A0A\AppData\Local\Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.UCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin"
$env:PATH = "$win;$env:PATH"
cargo +stable-x86_64-pc-windows-gnu test -p formualizer-engine-v2-poc -- --nocapture
```

Result after the bounded fixes:

```text
19 tests passed
0 failed
```

| Case | Expected Excel/V1 behavior used by the control | POC result |
| --- | --- | --- |
| Named range as whole range | Grid-backed range aggregate | Pass: name and range descriptors retained; all consumed cells read |
| Named range through `INDEX` | Selected value changes with selector | Pass |
| Selector changes | Selector dirties consumer and resolves a new target | Pass |
| Non-selected source edit | Source range invalidates consumer, but execution reads selected target | Pass |
| Name-definition change | All name users reevaluate with new definition | Pass |
| Source shape/structural change | Name/range generation dirties users | Pass for bounded row insertion and spill shape |
| Inactive IF branch | Branch value is not read; static dependency remains | Pass |
| Reference-returning IF/CHOOSE | Selected reference identity reaches `SUM` | Pass |
| `OFFSET`/`INDIRECT` target change | Dynamic selector/target is reread and replanned | Pass |
| Spill-shape change | Shape is an invalidation surface, not a copied table | Pass |
| Cross-sheet direct cycle | Runtime back-edge and cyclic workspace | Pass |
| False static cycle | Static candidate but no runtime workspace when branch is inactive | Pass |
| Genuine iterative cycle | Solver fallback converges within the bounded pass policy | Pass |
| Multiple independent cycles | Separate workspaces | Pass |
| Large named-range lookup | Descriptor count scales with consumers, not source area | Pass: 200 descriptors for a 10,000-cell source, not 2,000,000 cell relations |
| Unsupported function | Explicit failure, no silent V2 graph claim | Pass: `RAND()` returns `#NIMPL` |

### Excel micro-cycle control

The existing Excel oracle control was run without touching a saved workbook:

```text
A1 = B1 + 1
B1 = A1 / 2
iteration enabled:  calculation_state 2, CircularReference null, A1 1.999023, B1 0.999512
iteration disabled: calculation_state 0, CircularReference A1
```

The POC detects the same two-member runtime cycle and iterates toward `A1=2`, `B1=1`. The cycle membership and fallback behavior match the control; the bounded POC does not yet reproduce Excel's exact maximum-change stopping point.

## 6. Heavy representative slice

The slice uses the known addresses but does not hard-code their production behavior:

```text
CashFlow Inputs!J23       = SUM('CashFlow Engine'!K65)
CashFlow Engine!K65       = I65
CashFlow Engine!I65       = J11
CashFlow Engine!J11       = INDEX(Cash_Flow_Inputs,1,1)
```

The named source range also contains 4,800 formulas with inactive `IF(FALSE, J23, 0)` branches. This creates a deliberately broad conservative static surface while the active path selects only J23.

Observed diagnostic run:

```text
POC_B heavy_slice formulas=4803
v1_static_scc=4829
v1_runtime_observed_scc=4142
poc_static_cycle_members=4803
poc_runtime_cycle_members=4
poc_workspaces=1
poc_evaluations=4811
solver_passes=2
```

This is not full Heavy parity, but it answers the architecture question: a broad name/range invalidation surface can remain available without making inactive source formulas part of the runtime cyclic workspace. It also demonstrates why static cycle diagnostics must not be mistaken for the V2 scheduling region.

## 7. Heavy and Light shadow results

The repeatable report command is:

```powershell
cargo +stable-x86_64-pc-windows-gnu run -p formualizer-engine-v2-poc --bin engine-v2-poc
```

When the exact workbooks are available, pass them as the two positional arguments:

```powershell
cargo +stable-x86_64-pc-windows-gnu run -p formualizer-engine-v2-poc --bin engine-v2-poc -- .\Heavy.xlsx .\Light.xlsx
```

Current output from the checked-in artifacts:

```text
POC_A heavy source=checked_in_formualizer_artifacts workbook_available=false formulas=Some(4825) names=Some(4) ranges=Some(35) invalidation=Some(5419) persistent=Some(5419) direct_static_edges=Some(2102) legacy_static_edges=Some(13399) legacy_runtime_edges=Some(2076397) v1_static_cycle=Some(4829) v1_runtime_cycle=Some(4142) noop_ms=Some(12628.834) noop_evaluations=Some(14802)
POC_A light source=checked_in_formualizer_artifacts workbook_available=false dirty_closure=Some(0) noop_ms=Some(0.26) noop_evaluations=Some(0)
POC_A limitation heavy_workbook_found=false light_workbook_found=false limitations=2
```

The same binary's XLSX mode was exercised with two repository fixtures:

```text
POC_A xlsx profile=issue162-failure.xlsx formulas=Some(5) names=Some(0) ranges=Some(6) selectors=Some(0) invalidation=Some(6) persistent=Some(6) graph_build_ms=Some(6.2271) memory_bytes_estimate=Some(288)
POC_A xlsx profile=shared_formula_above_master.xlsx formulas=Some(6) names=Some(0) ranges=Some(0) selectors=Some(0) invalidation=Some(12) persistent=Some(12) graph_build_ms=Some(2.7064) memory_bytes_estimate=Some(576)
```

Those are input-seam controls only, not Heavy/Light evidence.

### Real Heavy gate

The no-argument command now resolves the exact requested path:

```text
C:\Users\OXK0A0A\Downloads\Fossil_EstimatingTemplate_2026-08_21_A.xlsx
```

The completed strict run began with:

```text
Heavy source=real_workbook workbook_available=true path=C:\Users\OXK0A0A\Downloads\Fossil_EstimatingTemplate_2026-08_21_A.xlsx worksheets=23
```

The independently built model reported:

```text
formula_count=94932
full_defined_name_count=4127
symbolic_dependency_descriptor_count=83388
persistent_relation_count=488637
invalidation_index_count=220842
model_build_time_ms=26595.195
memory_state_bytes=107601424
opaque_formula_count=0
```

The full requested sequence reported the following. `exact_runtime_reads` is an exact read-event counter, while retained read/edge records are bounded diagnostic samples. `runtime_edges` is the exact observed cell-read edge-event count; the retained set is capped at 100,000 to avoid turning instrumentation into an unbounded memory allocation.

```text
step=initial dirty_candidates=94932 formulas_evaluated=94932 exact_runtime_reads=1465482 runtime_edges=459523 runtime_cycle_count=0 largest_runtime_cyclic_workspace=0 workspace_member_addresses=[] solver_passes=0 wall_time_ms=9262.010 unsupported_formula_count=35857
step=F7=300 dirty_candidates=10908 formulas_evaluated=10908 exact_runtime_reads=123855 runtime_edges=50281 runtime_cycle_count=0 largest_runtime_cyclic_workspace=0 workspace_member_addresses=[] solver_passes=0 wall_time_ms=2482.900 unsupported_formula_count=8359
step=no-op #1 dirty_candidates=0 formulas_evaluated=0 exact_runtime_reads=0 runtime_edges=0 runtime_cycle_count=0 largest_runtime_cyclic_workspace=0 workspace_member_addresses=[] solver_passes=0 wall_time_ms=979.308 unsupported_formula_count=0
step=no-op #2 dirty_candidates=0 formulas_evaluated=0 exact_runtime_reads=0 runtime_edges=0 runtime_cycle_count=0 largest_runtime_cyclic_workspace=0 workspace_member_addresses=[] solver_passes=0 wall_time_ms=967.599 unsupported_formula_count=0
step=same-value F7=300 dirty_candidates=10908 formulas_evaluated=10908 exact_runtime_reads=123855 runtime_edges=50281 runtime_cycle_count=0 largest_runtime_cyclic_workspace=0 workspace_member_addresses=[] solver_passes=0 wall_time_ms=2489.370 unsupported_formula_count=8359
step=F7=301 dirty_candidates=10908 formulas_evaluated=10908 exact_runtime_reads=123855 runtime_edges=50281 runtime_cycle_count=0 largest_runtime_cyclic_workspace=0 workspace_member_addresses=[] solver_passes=0 wall_time_ms=2388.297 unsupported_formula_count=8359
```

The bounded POC answer for the real workbook is therefore:

```text
V1 static SCC:           4829
V1 runtime-observed SCC: approximately 4142
V2 real Heavy workspace: 0 observed by the bounded evaluator
```

That full-workbook sequence predates the witness-only semantic closure below and is not the final semantic verdict. Its zero-cycle result was caused by unsupported dependencies outside the bounded witness path; it must not be used to override the verified witness result.

### Heavy witness semantic closure

The first J11 value divergence was found in the selected target's own formula, not in `INDEX` reference consumption:

```excel
CashFlow Inputs!J9 = INDEX(Assumptions,
    MATCH($C9,Assumptions_R,0),
    MATCH($J$7,Assumptions_C,0))
```

The initial pipeline was:

```text
J9 row MATCH:       27
J9 column MATCH:    #N/A
J9 INDEX reference: not produced
J9 result:          #N/A
J11 result:         #N/A
```

The immediate `Assumptions_C` cells use:

```excel
=SUBSTITUTE(ADDRESS(1,COLUMN(),4),"1","")
```

and were initially `#NIMPL`. The first root-cause classification is **unsupported upstream formula**. Only the witness-required generic semantics were added: `SUBSTITUTE`, `ADDRESS`/`COLUMN`, `IFERROR` error-boundary handling, `INDEX` row/column-zero reference returns, `VLOOKUP`, and date-aware `EDATE`.

The final J11 pipeline is:

```text
MATCH row result:                 Number(9)
MATCH column result:              Number(10)
INDEX selected ReferenceValue:    CashFlow Inputs!J9
read selected target:              Text("SC")
canonical J9 value:                Text("SC")
value returned by INDEX:           Text("SC")
final J11 result:                  Text("SC")
```

The witness chain then evaluates naturally:

```text
J11 = SC       (correct INDEX-selected target value)
I65 = No       (IF false branch)
K65 = ""       (IF false branch; Excel-compatible blank)
J23 = 2025-12-01 (Excel serial 45992)
```

The J23 sub-pipeline is:

```text
J24:                    Date(2032-06-01) / Excel serial 48366
MIN(K29:K112):          -66
MIN(K29:K112) - 12:     -78
EDATE date input:       Date(2032-06-01)
EDATE output:           Date(2025-12-01) / Excel serial 45992
```

The selected-reference regression remains intact: `CashFlow Engine!J11` selects `CashFlow Inputs!J9`, and the focused regression test covers that identity after the semantic fixes.

The complete witness runtime graph, independent of diagnostic trace storage, reports:

```text
runtime_formula_edges_generated: 622
runtime_formula_edges_processed: 604
runtime_formula_edges_retained:  604
call_stack_back_edges:              0
runtime_graph_cyclic_scc_count:     0
largest_runtime_graph_cyclic_scc:   0
```

The known four-edge path remains three present edges plus one absent current-state execution edge:

```text
J23 -> K65: PRESENT
K65 -> I65: PRESENT
I65 -> J11: PRESENT
J11 -> J23: ABSENT; J11 selects J9
```

Thus the complete runtime graph has no cycle for this tested state without relying on a capped diagnostic graph or a call-stack-only definition. The reduced diagnostic-limit control preserves the complete edge count and SCC result.

Interpretation:

- `4825` and `4` are the cell/name members available in the checked-in Heavy SCC dump, not full-workbook formula/name totals.
- `35` is a source-family descriptor proxy from the dump, not a recovered range-geometry count. The raw dump does not encode the original descriptor geometry.
- `2,076,397` is the prior evaluator's expanded runtime edge count. It is comparison evidence only, not a V2 exact-read result.
- Light has a checked-in no-op summary but no full Light workbook or raw dependency dump. Its `0.26 ms`/zero-evaluation result is prior V1 evidence, not a new V2 full-workbook measurement.

The artifact mode remains comparison-only. The real Heavy run below proves the Heavy XLSX ingestion/model-build gate; the Light full-workbook gate is still unavailable, and the real Heavy runtime result remains bounded by unsupported formula coverage.

## 8. Performance comparison

### Prior V1 evidence

| Metric | Heavy V1 artifact | Light V1 artifact |
| --- | ---: | ---: |
| Full-workbook formula vertices | 94,966 | Not available in checked-in Light shadow artifact |
| Main static SCC | 4,829 | No runtime-live main SCC in prior Light run |
| Main runtime-live members | 4,139 | 0 reported |
| Main runtime expanded edges | 2,076,397 | Not available |
| No-op dirty vertices | 20,710 | 0 |
| No-op SCC member evaluations | 14,802 | 0 |
| No-op wall | 12,628.834 ms latest median | 0.26 ms |

The real Heavy POC run is not a clean V1-versus-V2 performance A/B: it uses a bounded evaluator with explicit unsupported formulas and different read instrumentation. Its measurements are execution evidence only:

| Real Heavy POC metric | Initial | F7=300 | No-op #1 | No-op #2 | Same-value F7=300 | F7=301 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Dirty candidates | 94,932 | 10,908 | 0 | 0 | 10,908 | 10,908 |
| Formula evaluation events | 94,932 | 10,908 | 0 | 0 | 10,908 | 10,908 |
| Exact runtime read events | 1,465,482 | 123,855 | 0 | 0 | 123,855 | 123,855 |
| Observed cell-read edge events | 459,523 | 50,281 | 0 | 0 | 50,281 | 50,281 |
| Runtime cycle count | 0 | 0 | 0 | 0 | 0 | 0 |
| Largest runtime workspace | 0 | 0 | 0 | 0 | 0 | 0 |
| Wall time (ms) | 9,262.010 | 2,482.900 | 979.308 | 967.599 | 2,489.370 | 2,388.297 |
| Unsupported formulas | 35,857 | 8,359 | 0 | 0 | 8,359 | 8,359 |

The earlier compact V1 shadow artifact also reports 575,264 expanded graph edges, 88,678 compact records, and a 6.4871x record ratio. That is useful comparison evidence but is not this POC's full-workbook result and had no measured safe wall-time saving.

### New bounded measurements

| Measurement | Result | Meaning |
| --- | ---: | --- |
| `INDEX` source area | 10,000 cells | Synthetic source |
| `INDEX` consumers | 200 | Synthetic consumers |
| POC symbolic range descriptors | 200 | O(consumers), not O(area × consumers) |
| POC persistent relations | 400 | One name + one range relation per consumer in this model |
| Naive source/consumer expansion product | 2,000,000 | Deliberately not materialized |
| POC B clean no-op evaluations | 0 | Demand scheduler does not force clean roots |
| POC B unrelated edit evaluations | 0 | No false dirty closure in control |
| POC B Heavy slice elapsed | Approximately 99 ms in debug GNU run | Diagnostic only; not comparable to full V1 Heavy |
| POC B Heavy slice runtime workspace | 4 members | Materially below V1's 4,142 observed members for this synthetic acceptance workload |

No production performance target is invented. The full Heavy/Light rerun must report graph construction, descriptor indexing, dirty closure, scheduler, adapter, value iteration, cycle discovery, and memory separately.

## 9. Correctness and architecture gates

| Gate | Status | Evidence |
| --- | --- | --- |
| Supported POC formulas match expected Excel/V1 behavior | Pass for bounded controls | 19 synthetic tests and Excel micro-cycle membership control |
| Name scope and bounded structural updates | Partial pass | Workbook/local scope and bounded row/name generation tests pass; full formula AST relocation remains open |
| Selected references resolve correctly | Pass | `INDEX`, `MATCH`, reference-returning IF/CHOOSE, exact target traces |
| Conservative invalidation does not miss tested edits | Pass for bounded controls | Name changes, selector changes, source edits, spill shape, structural event |
| Runtime cycle catches genuine synthetic cycles | Pass | Cross-sheet, same-sheet, multiple independent, iterative controls |
| False broad dependencies do not automatically create runtime cycles | Pass | Inactive IF and Heavy representative slice |
| Existing V1 unchanged | Pass by scope | New crate only; no production route or default config change |
| Names/ranges grid-backed | Pass in model | Definitions hold descriptors; values are read from canonical `CellState` map |
| Invalidation and runtime-cycle layers separate | Pass | Distinct enums/sets and assertions |
| No full cell-edge expansion for named/range storage | Pass in bounded model | Descriptor metrics; expansion is only diagnostic static comparison |
| Runtime workspace based on actual feedback | Pass | Workspace derives from exact runtime edges |
| POC is not SCC-first under new names | Pass in bounded scheduler | Requested/dirty recursion precedes runtime cycle analysis |
| Real Heavy POC A ingestion/model build | **Passed** | Supplied Downloads XLSX loaded independently: 94,932 formulas and 4,127 names |
| Full Heavy/Light POC A comparison | **Not passed** | Light XLSX/raw graph input is still absent; Heavy runtime semantics remain bounded by unsupported formulas |
| Existing formula registry adapter | **Not passed** | Bounded evaluator proves host contract, not full registry reuse |
| Retained exact cyclic state | Out of scope | No certificate claimed |

## 10. Risks and falsification status

1. **Incomplete Light input.** The exact Heavy workbook now loads, but the Light workbook/raw graph input is still unavailable. The real Heavy model build is measured; equivalent Light comparison and full output parity are not.
2. **Function adapter coverage.** Existing formula implementations can access context and value/range APIs directly. A host adapter must intercept every cell, range, name, table, spill, dynamic, and external read before their reads can feed V2 cycle claims.
3. **Static diagnostic expansion.** The POC's static cycle comparison expands symbolic descriptors to formula targets. This is intentionally diagnostic and must not become the V2 runtime path. Prior investigation already measured the cost of lazy symbolic SCC traversal.
4. **Dynamic effects.** The POC's dynamic effect keys are explicit, but the full engine needs owned clock, random, external/provider, and semantic generations.
5. **Structural relocation.** Bounded name ranges shift on row insertion; complete Excel AST translation, tables, spill ownership, and external references are not implemented.
6. **Error/coercion semantics.** The supported evaluator is intentionally small and does not establish parity for the remaining compatibility inventory, arrays, implicit intersection, or all error propagation.
7. **Range consumers.** `SUMPRODUCT` and other aggregate consumers genuinely read all target values. Symbolic descriptors reduce persistent edge storage, not the cost of consuming a large range.
8. **Spills/tables.** The POC models spill shape and table descriptors but does not support all parser syntax or table semantics.
9. **Cycle convergence.** A runtime workspace proves feedback, not convergence. Iteration caps, max-change semantics, error stamping, and exact state ownership must match V1/Excel before production use.
10. **Excel private implementation.** No result here proves how Excel stores names, range nodes, or private circular sets.

The architecture would be falsified by a complete repeat if precise actual reads still produced a comparable Heavy runtime region, if conservative invalidation missed relevant edits, if the adapter required rewriting most formulas before any result, or if genuine cycles could not be solved without restoring static SCC-first scheduling. The bounded controls did not falsify it.

## 11. Full-development gap list

A full implementation would still require:

- remaining formula coverage and Excel output parity;
- full Heavy and Light input/output reruns;
- complete name scope, relative/absolute reference, table, and structural-edit semantics;
- dynamic `INDEX`/`OFFSET`/`INDIRECT`/spill/reference-result coverage;
- explicit volatile/effect generation ownership;
- production `EvaluationHost`/`ReferenceResolver`/`ReadRecorder` integration for all formula implementations;
- iterative workspace completion, error policy, cap/max-change parity, and upstream generation propagation;
- a parallel acyclic scheduler with deterministic apply;
- dynamic replan/fallback when dependencies change during evaluation;
- retained exact cyclic-state certificates only after the runtime model is proven;
- Rust/Python/JS diagnostic and public APIs;
- persistence, durability, serialization, and crash-safe state ownership;
- memory and clean A/B measurements on full Heavy/Light workbooks;
- security and provider/UDF context contracts.

None of those are started by this POC.

## 12. Answers to the central questions

1. **Are named ranges best modeled as symbolic definitions over grid-backed values?**

   Yes for this architecture, and this is consistent with the public Excel/Open XML model. A name is a symbol/definition whose range form ultimately points at worksheet cells. HyperFormula's named-expression/range vertices support the usefulness of separate graph metadata, but do not prove a copied data table. The POC keeps cell values canonical and names descriptive.

2. **Can broad invalidation dependencies be separated safely from exact runtime cycle dependencies?**

   Yes on the bounded synthetic controls. Inactive IF branches and non-selected INDEX source cells invalidate conservatively but do not become runtime reads or cycle edges. Full-workbook safety remains to be proven with the missing inputs and a complete adapter.

3. **Can `INDEX(name, row, col)` use the selected target for execution/cycle purposes while retaining selector/name/structural invalidation?**

   Yes in POC B. The selected cell is the exact execution read; the source name/range and selector ranges remain invalidation descriptors. The result is an architecture control, not a claim about Excel's private invalidation graph.

4. **Can demand-driven feedback discovery avoid the current Heavy SCC?**

   The known Heavy witness now runs with the required bounded semantics and still reports zero runtime cycles because the exact current-state path ends at `CashFlow Inputs!J9`, not `CashFlow Inputs!J23`. The complete witness graph has three of the four historical edges; the missing J11-to-J23 edge is the selected-reference result, not an SCC failure. This closes the witness question without claiming full-workbook cycle parity.

5. **Can this be proven with a bounded POC before full development?**

   Yes for this bounded witness. The first divergence was an unsupported upstream formula path, and the focused generic fixes closed the four-cell witness while preserving selected-reference identity. This does not authorize full V2 development; broader formula coverage and full-workbook parity remain outside the POC.

## 13. Final decision

```text
REVISE AND REPEAT POC
```

Repeat only the bounded gate:

1. provide the exact Light workbook/raw formula input;
2. retain the verified real Heavy XLSX run as the Heavy ingestion baseline;
3. adapt a representative existing formula-function cohort through `EvaluationHost` and prove all state reads are recorded;
4. rerun Heavy with the expanded adapter and compare runtime cycles/workspaces;
5. run the equivalent Light shadow and sequence measurements;
6. make the final `PROCEED TO FULL V2` or `DO NOT PROCEED` decision.

Stop here. Production routing, default behavior, bindings, and broad formula compatibility remain unchanged.
