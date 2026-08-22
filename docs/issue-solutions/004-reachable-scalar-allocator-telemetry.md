# Issue Solution: Reachable Scalar Allocator Telemetry

- **Branch:** `perf/port-scalar-allocator-telemetry-8.4`
- **Status:** Merged into `main`
- **Merge commit:** `a65c0b85`
- **Implementation commit:** `a7a1cd6b`

## Problem

The evaluator used scalar arena storage, but baseline telemetry did not distinguish total allocated slots, currently reachable scalar slots, capacity, and reused slots. This made it difficult to tell whether memory growth came from live workbook state, retained arena capacity, or successful slot reuse.

## Solution

Telemetry was added across the storage and evaluation layers:

- scalar arena capacity and reused-slot counters;
- `DataStore` reachable-scalar traversal across value and AST roots;
- scalar capacity/bytes in datastore memory statistics;
- graph baseline counters for value update attempts and persistent commits;
- engine and graph baseline fields for allocated/live/reused scalar slots and bytes.

The reachability calculation follows actual roots instead of assuming that every allocated arena value is live.

## Validation

The allocator telemetry branch passed its focused storage, graph, and evaluator tests and was merged into `main` before the compatibility work.

## Generalization notes

Telemetry is observational. It must not alter allocation, evaluation order, cache retirement, or value semantics. Any future allocator optimization should compare live reachable slots, capacity, reuse, and RSS separately.
