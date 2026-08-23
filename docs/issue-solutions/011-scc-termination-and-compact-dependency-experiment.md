# SCC Confirmation Termination and Compact Dependency Experiments

- **Branch:** `investigation/fossil-excel-calculation`
- **Status:** Diagnostic experiments complete; no production semantic changes
- **Parent evidence:** [`010`](010-excel-vs-formualizer-fossil-calculation-investigation.md)
- **Raw data:** `docs/issue-solutions/data/`

## Executive result

The two architectural experiments do not justify a production cache yet.

```text
A. Tolerance-only early SCC termination: unsafe in general.
B. Compact dependency shadow: promising representation, parity incomplete.
```

The correct root cause of the second SCC pass is now known: the current convergence contract deliberately has no predecessor full-pass value for pass 1. It must run pass 2 before it can apply the normal `values_converged(previous_pass, current_pass)` test. A prior recalc’s converged state is not a substitute for the immediately preceding pass: small residual changes can accumulate or follow a different trajectory.

## A. Why pass 2 is currently required

The current SCC evaluator does this:

```text
pass 1: evaluate every evaluable SCC member
         prev_pass = None

classify live graph

if a live cycle exists:
    set iterating = true
    if prev_pass exists:
        compare previous full pass with current full pass
        stop if all members converge
    otherwise:
        evaluate another full pass
        set prev_pass = values from pass 1

pass 2: evaluate every member again
         compare pass 1 values with pass 2 values
```

The first pass has no previous **full iteration pass**. The pre-task snapshot is not equivalent: it is the state before the current recalculation and may be a partially converged state, a capped state, or a state produced under a different dirty/boundary condition.

For the Fossil main SCC:

| Phase | Pass | Evaluated | Changed | Max delta | Time |
| --- | ---: | ---: | ---: | ---: | ---: |
| F7 | 1 | 4,828 | 12 | 0 for normal iteration comparison | 156 ms |
| F7 | 2 | 4,828 | 0 | 0 | ~5.2 s |
| No-op | 1 | 4,828 | 12 | 0 for normal iteration comparison | 156 ms |
| No-op | 2 | 4,828 | 0 | 0 | ~5.1 s |

The zero-change second pass is therefore expensive confirmation work, but it is also where the current general convergence proof is made.

## A. Diagnostic early-termination path

An opt-in path was added behind:

```text
FZ_DIAGNOSTIC_EARLY_SCC_TERMINATION=1
```

The default path is unchanged. The candidate checks:

```text
- prior final SCC state exists
- member identity/order unchanged
- data/boundary revision unchanged
- cycle configuration unchanged
- final live-edge fingerprint unchanged
- dynamic output shapes unchanged
- prior final values -> current pass-1 values converge under maxChange
```

When all checks pass, the diagnostic path stops after pass 1 and records how many member evaluations it would avoid. It does not use a hard-coded workbook target or cell list.

### Fossil result

| Phase | Normal wall | Early wall | Early accepted | Avoided member evaluations | Formula fingerprint |
| --- | ---: | ---: | ---: | ---: | --- |
| Initial | 30,062 ms | 34,156 ms | 0 | 0 | equal |
| F7=300 | 11,912 ms | 15,437 ms | 0 | 0 | equal |
| No-op | 11,227 ms | 7,638 ms | 84 SCCs | 7,400 total; 4,828 main | equal |
| Same-value write | 12,051 ms | 14,774 ms | 0 | 0 | equal |
| F7=301 | 10,978 ms | 14,830 ms | 0 | 0 | equal |
| F7 back to 300 | 10,872 ms | 13,969 ms | 0 | 0 | equal |

The no-op diagnostic run saved approximately:

```text
3,590 ms
31.97% of wall time
4,828 main-SCC member evaluations
7,400 SCC member evaluations in total
```

This is an upper-bound opportunity for a future safe design, not a production benefit estimate. The diagnostic path performs extra state/invariant work and is intentionally not a benchmark replacement.

### Falsification result

The same invariant set is not sufficient for a general early stop. These controls accepted early termination but diverged from the normal full path:

| Control | Normal no-op | Early no-op | Candidate delta |
| --- | --- | --- | ---: |
| Active IF cycle | `B1=1.999755859375`, `C1=0.9998779296875` | `B1=1.99951171875`, `C1=0.999755859375` | 0.00048828125 |
| Two-cell cycle | `A1=1.999755859375`, `B1=0.9998779296875` | `A1=1.99951171875`, `B1=0.999755859375` | 0.00048828125 |
| Same-sheet cycle | `B1=19.999847412109375`, `C1=9.999923706054688` | `B1=19.99969482421875`, `C1=9.999847412109375` | 0.00030517578125 |
| Cross-sheet cycle | `Sheet1!A1=1.999755859375`, `Sheet2!A1=0.9998779296875` | `Sheet1!A1=1.99951171875`, `Sheet2!A1=0.999755859375` | 0.00048828125 |

In all four cases:

```text
topology unchanged
shape unchanged
boundary/config unchanged
prior-state delta <= maxChange
```

Yet the next normal iteration changes the output again. Therefore:

> `previous final state -> current pass-1 delta <= maxChange` does not prove that current pass 1 is a fixed point.

### A verdict

```text
Tolerance-only early termination: REJECTED as a general optimization.
```

A safe general early stop requires a stronger fixed-point witness than a tolerance comparison against a prior recalc. For arbitrary spreadsheet formulas, the ordinary witness is another evaluation pass. A specialized early stop could be safe only for a proven deterministic/idempotent subset with exact state equality and complete dependency/boundary proof; that is narrower than the current mixed SCC.

The named-range definition control also passed: after a name definition update, the candidate rejects with `boundary_revision_changed`. The native test is:

```text
crates/formualizer-eval/src/engine/tests/scc_runtime_cycles.rs
```

Raw A results:

```text
docs/issue-solutions/data/early-scc-termination-experiment.json

docs/issue-solutions/data/fossil-scc-iteration-trace.json
```

## B. Compact range/name dependency prototype

### Current graph shape

The current engine already has two dependency layers:

```text
expanded/direct CSR graph
formula_to_range_deps symbolic range descriptors
stripe_to_dependents compressed candidate index
named dependency reverse records
sheet interval indexes
```

The prototype exposes this layout without changing evaluation or dirty propagation.

### Shadow statistics after initial Fossil evaluation

```text
expanded CSR graph edges:       575,264
formula vertices:                94,966
range-bearing formulas:           9,126
symbolic range records:          17,936
stripe membership records:       21,021
named dependency records:        49,449
dynamic descriptors:                270
compact shadow records:          88,676
estimated compact bytes:      1,578,328
```

The symbolic shadow record count is approximately 6.49x smaller than the expanded CSR edge count. The byte estimate covers the prototype descriptors/index entries only; it is not a replacement for the full engine RSS measurement.

### Parity validation

The prototype validator classified the current CSR formula dependency edges as:

```text
expanded formula edges: 573,502
direct cell edges:      523,841
named edges:             49,449
symbolic range edges in CSR: 0
unclassified edges:         212
```

The zero symbolic-range count is informative rather than a success: range dependencies are held in the separate compressed index and are not represented by the ordinary `get_dependencies` CSR list. The 212 unclassified edges mean the shadow model is not yet an exact replacement dependency model.

The prototype is therefore:

```text
useful for representation design: yes
complete exact invalidation driver: no
connected to evaluation: no
connected to dirty propagation: no
measured wall-time saving: 0
eligible for production: no
```

Raw B result:

```text
docs/issue-solutions/data/compact-dependency-prototype.json
```

### Does compact representation change SCC structure?

Not by itself.

A symbolic rectangle/name record can reduce storage and candidate traversal cost, but it represents the same logical dependency relation. If a range semantically connects a formula to cells in a cycle, the cycle remains logically present. SCC membership changes only when the engine has a correctness-proof mechanism for:

- active/live conditional reads;
- dynamic target resolution;
- range overlap and interval reachability;
- named-range expansion;
- spill/array relationships;
- cross-sheet boundaries.

Therefore compact dependencies are primarily a representation/traversal optimization until a range-aware SCC algorithm is implemented and parity-tested.

## Build and edit-loop investigation

### Actual dependency chain

The Python wrapper depends on this local chain:

```text
formualizer-python
  -> formualizer
      -> formualizer-eval
      -> formualizer-workbook
      -> formualizer-sheetport
          -> formualizer-common
          -> formualizer-parse
```

`formualizer-eval` is the central engine crate. Changes to its public telemetry structs or evaluator code are visible through `formualizer`, `formualizer-workbook`, `formualizer-sheetport`, and the Python wrapper. A binding runtime test therefore eventually needs a native extension rebuild.

### Measured loop costs

| Command/path | Measured result |
| --- | ---: |
| `cargo check -p formualizer-eval` after a warm incremental edit | ~0.5–12 s |
| `cargo check -p formualizer-python --no-default-features` with the correct 64-bit `PYO3_PYTHON` | ~6.6 s |
| `maturin develop --release --no-default-features` after evaluator/API edits | ~4.5–4.9 min on this Windows/MinGW environment |
| no-source-change incremental `maturin develop --release` | ~0.8 s |
| failed default Python check | default `allocator-jemalloc` attempted `tikv-jemalloc-sys`; shell path/build environment failure |

The largest cost is not `cargo check`; it is a release native extension rebuild after an API/evaluator change. The default Python package feature includes `allocator-jemalloc`, while the working investigation command intentionally uses `--no-default-features`.

### Lowest-risk edit-loop policy

```text
1. Eval-only Rust edit:
   cargo check -p formualizer-eval

2. Eval Rust tests:
   cargo test -p formualizer-eval --lib <target>

3. Binding source/API edit:
   cargo check -p formualizer-python --no-default-features
   with VIRTUAL_ENV/PYO3_PYTHON set to the 64-bit test venv

4. Python runtime behavior test:
   maturin develop --release --no-default-features

5. Full release build:
   keep the existing final benchmark/release command unchanged
```

Do not rebuild the binding for evaluator-only compile checks. Do not change the linker or allocator policy to hide a build failure.

### Diagnostics placement recommendation

Frequently changing diagnostics currently live in `formualizer-eval/src/engine/eval.rs`, so their public type/API changes can force downstream recompilation. A lower-risk future cleanup is:

```text
stable engine-side telemetry hook/trait
small diagnostics data crate or generated adapter
binding conversion layer that changes less often
```

This can reduce downstream source invalidation for diagnostic schema changes, but it will not eliminate the need to rebuild `formualizer-eval` when evaluator instrumentation itself changes. It should be considered only as a build-loop cleanup, not as a calculation optimization.

## Architecture recommendation

### Root cause of unnecessary second pass

The pass-2 cost comes from two independent facts:

1. pass 1 has no previous full-pass baseline under the current convergence contract;
2. the mixed SCC is very large and dense, so confirming pass 2 evaluates thousands of members and expands many range/name reads.

Dynamic/volatile invalidation makes the SCC reach this path, but dynamic-call caching alone does not remove the topology/evaluation cost.

### Minimal retrofit architecture

Do not add fixed-point caching yet. First implement:

1. a complete compact dependency shadow with exact direct/range/name/dynamic/conditional/spill classification;
2. a parity validator that compares compact predicted dirty sets with the current expanded/control sets for every edit control;
3. a range-aware SCC discovery experiment that reports whether symbolic traversal changes cost without changing logical membership;
4. a deterministic exact-state/idempotence control for early termination, separate from tolerance-only convergence.

The first production-adjacent change should be the compact dependency representation only after its dirty-set parity reaches zero unclassified cases. It should initially remain behind a diagnostic flag and continue using the existing evaluator as the result oracle.

### Ideal greenfield architecture

```text
Formula IR with typed dependency descriptors
  -> compact direct/range/name/table/dynamic indexes
  -> retained calculation order and dirty closure
  -> range-aware live dependency/SCC analysis
  -> serial circular workspace with explicit boundary revisions
  -> parallel acyclic layers and inner range kernels
  -> exact semantic invalidation contract
```

The circular workspace should cache metadata/state only after proving:

```text
same logical topology
same dynamic targets/shapes
same external boundary revisions
same semantic configuration
same deterministic/volatile contract
same iterative state/convergence contract
```

### Exact next implementation step

```text
Complete the compact dependency shadow parity validator.
```

Specifically, eliminate the 212 unclassified dependency edges and add explicit conditional/spill/dynamic provenance. Then compare compact predicted dirty sets and SCC candidates with the expanded control on the existing Fossil edit controls. Do not enable an evaluator fast path and do not add fixed-point caching until that validator is exact.

## Raw experiment data

```text
docs/issue-solutions/data/early-scc-termination-experiment.json
docs/issue-solutions/data/compact-dependency-prototype.json
docs/issue-solutions/data/build-loop-measurements.json
```

## Final gate

This branch is intentionally stopped before production optimization:

```text
early termination under tolerance: disproven
compact dependency representation: promising but parity incomplete
mixed-SCC caching: not implemented
production cycle semantics: unchanged
main merge: not performed
```
