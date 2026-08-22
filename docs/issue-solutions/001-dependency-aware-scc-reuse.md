# Issue Solution: Dependency-Aware SCC Reuse

- **Branch:** `fix/dependency-aware-scc-reuse`
- **Status:** Merged into `main`
- **Merge commit:** `ae338959`
- **Implementation commit:** `56d13341`

## Problem

Iterative calculation repeatedly reprocessed the same strongly connected components (SCCs) even when their dependency topology, cycle policy, volatile inputs, and semantic configuration were unchanged. This added avoidable work to repeated recalculation of large workbooks.

A reuse optimization must not reuse stale values after a topology or semantic change. The key issue was therefore not just caching an SCC result, but proving when its dependency graph and reuse assumptions remained valid.

## Solution

The engine now tracks reusable iterative SCC metadata and invalidates it when a relevant change occurs:

- dependency/topology changes;
- structural edits;
- function semantic/provider changes;
- volatile or cycle configuration changes;
- explicit invalidation of iterative members.

Stable SCCs can be reused only when the dependency-aware reuse contract remains valid. Invalidated members are redirtied and evaluated through the normal exact path.

The implementation is engine-wide and does not reference any workbook, sheet, or cell name.

## Validation

The implementation is covered by the evaluator’s SCC, dirty-propagation, structural-edit, and repeated-recalculation tests. The later lookup-cache retirement and allocator telemetry branches were based on this merged topology/reuse foundation.

## Generalization notes

SCC reuse is an optimization only. It must never change convergence policy, member ordering, live-cycle detection, or final values. Any future cache key must include all semantic and topology revisions that can change the SCC result.
