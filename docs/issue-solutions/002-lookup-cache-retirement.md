# Issue Solution: Lookup Cache Retirement After Edits

- **Branch:** `test/lookup-cache-retirement-8.4`
- **Status:** Merged into `main`
- **Merge commit:** `1bdbc885`
- **Implementation commit:** `776acadc`
- **Related production commit:** `4b013270`

## Problem

Lookup index-cache entries were keyed by evaluation snapshots. A direct cell mutation advanced the snapshot identity without retiring stale cache entries. Subsequent lookup evaluation could therefore retain indexes derived from an older workbook state.

The production path was especially vulnerable through direct `Engine::set_cell_value` mutations because that path did not consistently go through the common edit lifecycle.

## Solution

All relevant data edits now use the common edit marker that:

1. advances the active snapshot identity;
2. retires stale lookup-cache entries;
3. preserves the existing dependency dirtying behavior.

The change is located at the engine edit boundary rather than in individual lookup functions, so future lookup implementations inherit the same retirement contract.

## Validation

The lookup retirement regression set contains 15 tests covering cache reuse, mutations, snapshot changes, and stale-entry removal. All focused lookup-cache tests passed before the branch was merged.

## Generalization notes

Cache retirement must happen at the mutation lifecycle boundary. Adding a special case to one lookup function would leave other indexed consumers vulnerable. Structural edits and future table/name updates must also invalidate any cache whose key depends on the changed topology or values.
