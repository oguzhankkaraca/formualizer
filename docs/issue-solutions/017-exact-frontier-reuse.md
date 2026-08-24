# Exact Frontier Validation for Heavy Live Iterative SCC

- **Branch:** `investigation/fossil-excel-calculation`
- **Status:** Diagnostic exact-validation path complete; no reuse accepted for Heavy
- **No tolerance proof:** The candidate never uses `<= maxChange`.
- **No production caching:** No cache or semantic shortcut was enabled.
- **Raw data:** `docs/issue-solutions/data/exact-frontier-reuse-experiment.json`

## Frontier classification

Heavy main SCC frontier classification:

```text
volatile members:                 270
dynamic members:                  270
volatile-only:                      0
dynamic-only:                       0
volatile + dynamic:               270
INDIRECT/OFFSET target formulas:  270
  target-only:                    268
  target + dynamic shape:           2
dynamic-shape-only:                 0
external/UDF/context frontier:     0
non-cell frontier members:         0
```

The two dynamic-shape members are:

```text
Cash Flow Inputs!J55
Cash Flow Inputs!K55
```

They use the `OFFSET/FILTER/UNIQUE/VSTACK/MAP` family. The other 268 frontier formulas are predominantly `INDIRECT($J$19)` / `INDEX/MATCH` target formulas.

The graph flags mark all 270 as both volatile and dynamic. This is why the existing `reuse_safe` condition rejects the entire SCC from reusable iterative metadata.

## Exact candidate

Opt-in environment:

```text
FZ_DIAGNOSTIC_EXACT_SCC_REUSE=1
```

For an SCC with prior state, the diagnostic candidate:

1. checks exact member/frontier identity;
2. checks data, topology, graph-symbol, function-semantic, provider, cycle, date-system, seed, volatile-level, and deterministic-mode revisions;
3. re-evaluates only frontier members against the existing converged state without committing their values;
4. fingerprints all observed scalar/range/name read target identities;
5. compares frontier values exactly;
6. compares dynamic target/read fingerprints exactly;
7. compares dynamic value shapes exactly;
8. compares frontier live-edge identities exactly;
9. requires a whole-SCC static-remainder fixed-point witness;
10. otherwise rejects and falls through to the normal full SCC path.

The candidate does not use `maxChange` as a proof.

## Heavy sequence result

| Phase | Frontier validation | Exact frontier state | Static remainder | Decision |
| --- | ---: | --- | ---: | --- |
| Initial | 0 members; no prior state | not applicable | 4,559 | reject: `no_prior_state` |
| F7 edit | no validation; revision changed | not applicable | 4,559 | reject: `boundary_revision_changed` |
| No-op | 269 members; 16.6 ms | values/targets/shapes/live-edge IDs unchanged | 4,559 | reject: `static_remainder_progression_unproven` |

Heavy no-op exact candidate record:

```text
frontier_member_count:                       269
static_remainder_member_count:              4559
frontier_evaluations:                         269
frontier_values_unchanged:                 true
dynamic_targets_unchanged:                 true
frontier_shapes_unchanged:                 true
live_edge_identities_unchanged:            true
boundary_revisions_unchanged:              true
semantic_revisions_unchanged:              true
static_remainder_fixed_point_witness:      false
static_remainder_changed_count_previous:     11
accepted:                                   false
avoided_member_evaluations:                   0
```

The previous F7 recalc changed all 4,559 static-remainder members during its first pass. More importantly, the subsequent normal Heavy no-op first pass still changed 11 static-remainder members even though every frontier invariant tested by the candidate was unchanged.

This directly proves:

```text
unchanged volatile/dynamic frontier
+ unchanged targets/shapes/live edges/revisions
!= unchanged whole-SCC state
```

The normal no-op full path produced the same aggregate formula-value fingerprint as the control:

```text
[20710, 1026544198047018979]
```

The exact candidate did not skip that path, so iterative-state progression semantics were preserved.

## Required rejection controls

| Control | Result |
| --- | --- |
| Changed volatile value / volatile UDF | fail-closed rejection: `external_or_context_dependent` or exact frontier value change |
| INDIRECT/OFFSET target change | rejection through data/boundary revision and/or changed live target; cycle removal also prevents candidate reuse |
| FILTER/spill shape change | exact shape comparison rejects; no reuse without whole-SCC witness |
| Named-range change | graph-symbol revision mismatch rejects |
| Boundary/input change | data snapshot/topology revision mismatch rejects |
| Semantic/config change | semantic/config revision mismatch rejects |
| External/UDF/context state | fail-closed context gate rejects |
| Static remainder can progress | `static_remainder_progression_unproven` rejects |

## Cost and result

The Heavy no-op frontier validation itself costs approximately:

```text
16.6 ms
269 frontier evaluations
0 avoided full-SCC evaluations
```

The diagnostic path’s end-to-end no-op was approximately `13.1 s`, versus the normal control at approximately `11.4 s`. This is expected: the diagnostic path adds exact read fingerprinting and frontier evaluation, then deliberately runs the normal full SCC after rejecting reuse.

There is no safe sub-second result from this candidate.

## Final conclusion

The minimum semantic condition preventing Heavy from taking Light’s no-op path is not merely the presence of volatile/dynamic members. It is the absence of an exact whole-SCC progression proof after those members are validated.

Heavy’s frontier can be exactly unchanged while the 4,559-member static remainder advances on the next normal recalculation. Therefore:

```text
frontier-only exact validation is insufficient
whole-SCC reuse is not proven
no full-SCC evaluation may be skipped
```

Under the current general spreadsheet semantics, the safe proof boundary is either:

```text
run the full SCC pass required by the existing convergence contract
```

or introduce a formally verified static-remainder fixed-point certificate. The latter is a future architectural/cache project and is intentionally not implemented here.
