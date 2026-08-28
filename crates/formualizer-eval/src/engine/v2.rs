use crate::engine::{CycleConfig, DateSystem, VertexId};
use crate::reference::{CellRef, SheetId};
use formualizer_common::{CoordHashMap, ExcelError, LiteralValue, PackedSheetCell};
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

fn requested_from_value(value: Option<&str>) -> bool {
    value.is_some_and(|value| matches!(value, "1" | "true" | "TRUE" | "on" | "ON"))
}

pub(crate) fn requested() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        false
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let value = std::env::var("FORMUALIZER_ENGINE_V2").ok();
        requested_from_value(value.as_deref())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct ReadRange {
    pub sheet: String,
    pub start_row: u32,
    pub start_col: u32,
    pub end_row: u32,
    pub end_col: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct ReadCell {
    pub sheet: String,
    pub row: u32,
    pub col: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) enum EffectKind {
    RecalcEpoch,
    Clock,
    Random,
    DynamicSelector,
    DynamicTarget,
    SpillShape,
    TableShape,
    ExternalProvider,
    StructuralGeneration,
    DateSystem,
    PlacementContext,
}

impl EffectKind {
    fn index(self) -> usize {
        match self {
            Self::RecalcEpoch => 0,
            Self::Clock => 1,
            Self::Random => 2,
            Self::DynamicSelector => 3,
            Self::DynamicTarget => 4,
            Self::SpillShape => 5,
            Self::TableShape => 6,
            Self::ExternalProvider => 7,
            Self::StructuralGeneration => 8,
            Self::DateSystem => 9,
            Self::PlacementContext => 10,
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct ReferenceObservationRecord {
    pub sheet: String,
    pub start_row: u32,
    pub start_col: u32,
    pub end_row: u32,
    pub end_col: u32,
    pub rows: usize,
    pub cols: usize,
    pub generation: crate::traits::ReferenceGeneration,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RawReadSet {
    pub selected_targets: BTreeSet<ReadCell>,
    pub ranges: BTreeSet<ReadRange>,
    pub names: BTreeSet<String>,
    pub tables: BTreeSet<String>,
    pub external: BTreeSet<String>,
    pub effects: BTreeSet<EffectKind>,
    pub cell_events: Vec<PackedSheetCell>,
    pub cell_events_sorted: bool,
    pub reference_observations: BTreeSet<ReferenceObservationRecord>,
    pub cell_read_events: usize,
    pub range_read_events: usize,
    pub logical_range_positions: usize,
    pub physical_cells_fetched: usize,
    pub observation_recording_ns: u128,
    pub range_read_materialization_ns: u128,
    pub recorder_raw_events: [usize; RECORDER_OPERATION_COUNT],
    pub recorder_sampled_elapsed_ns: [u128; RECORDER_OPERATION_COUNT],
    pub effect_raw_events: [usize; EFFECT_KIND_COUNT],
    pub range_materialization_events: usize,
}

pub(crate) trait RangeReadObserver: Send + Sync {
    fn cell_read(&self, cell: PackedSheetCell);
    fn cell_rows_read(
        &self,
        sheet_id: SheetId,
        start_row: u32,
        end_row: u32,
        column_runs: &[(u32, u32)],
    ) {
        for row in start_row..=end_row {
            for &(start_col, end_col) in column_runs {
                for col in start_col..=end_col {
                    if let Some(cell) = PackedSheetCell::try_new(sheet_id, row, col) {
                        self.cell_read(cell);
                    }
                }
            }
        }
    }
    fn range_consumed(
        &self,
        sheet: &str,
        start_row: u32,
        start_col: u32,
        end_row: u32,
        end_col: u32,
        logical_positions: usize,
        physical_cells: usize,
    );
    fn range_materialized(&self, elapsed_ns: u128);
    fn range_materialization_sample_interval(&self) -> Option<u32> {
        None
    }
}

pub(crate) const RECORDER_OPERATION_COUNT: usize = 8;
pub(crate) const RECORDER_OPERATION_SAMPLE_INTERVAL: usize = 4096;
pub(crate) const EFFECT_KIND_COUNT: usize = 11;
pub(crate) const RANGE_MATERIALIZATION_SAMPLE_INTERVAL: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum V2RecorderOperation {
    ScalarCellRead,
    RangeObservation,
    SelectedReference,
    ReferenceGeneration,
    NameSymbol,
    Table,
    Provider,
    SemanticEffect,
}

impl V2RecorderOperation {
    fn index(self) -> usize {
        match self {
            Self::ScalarCellRead => 0,
            Self::RangeObservation => 1,
            Self::SelectedReference => 2,
            Self::ReferenceGeneration => 3,
            Self::NameSymbol => 4,
            Self::Table => 5,
            Self::Provider => 6,
            Self::SemanticEffect => 7,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RawReadAttribution {
    pub observation_recording_ns: u128,
    pub range_read_materialization_ns: u128,
    pub recorder_raw_events: [usize; RECORDER_OPERATION_COUNT],
    pub recorder_sampled_elapsed_ns: [u128; RECORDER_OPERATION_COUNT],
    pub effect_raw_events: [usize; EFFECT_KIND_COUNT],
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct V2FormulaEdgeExtractionAttribution {
    pub scalar_events: usize,
    pub scalar_event_coordinates_inspected: usize,
    pub scalar_unique_coordinates_inspected: usize,
    pub range_observations_inspected: usize,
    pub range_coordinates_expanded: usize,
    pub sheet_lookups_attempted: usize,
    pub sheet_lookups_succeeded: usize,
    pub dependency_owner_lookups_attempted: usize,
    pub dependency_owner_lookup_hits: usize,
    pub dependency_owner_lookup_misses: usize,
    pub formula_membership_lookups: usize,
    pub formula_vertex_resolutions: usize,
    pub non_formula_vertex_resolutions: usize,
    pub raw_formula_edge_candidates: usize,
    pub formula_edge_insert_attempts: usize,
    pub duplicate_formula_edge_candidates: usize,
    pub unique_formula_edges: usize,
    pub exact_cell_insert_attempts: usize,
    pub exact_cells_produced: usize,
    pub name_resolution_attempts: usize,
    pub name_resolution_hits: usize,
    pub table_resolution_attempts: usize,
    pub table_resolution_hits: usize,
    pub generation_revision_lookups: usize,
    pub formula_owner_index_builds: usize,
    pub formula_owner_index_entries: usize,
    pub formula_owner_index_build_ns: u128,
    pub formula_owner_entries_by_sheet: BTreeMap<SheetId, usize>,
    pub owner_probe_results: Vec<(PackedSheetCell, bool)>,
    pub scalar_event_scan_ns: u128,
    pub scalar_exact_cell_edge_ns: u128,
    pub name_resolution_ns: u128,
    pub table_resolution_ns: u128,
    pub other_ns: u128,
}

impl V2FormulaEdgeExtractionAttribution {
    fn accumulate(&mut self, other: Self) {
        self.scalar_events = self.scalar_events.saturating_add(other.scalar_events);
        self.scalar_event_coordinates_inspected = self
            .scalar_event_coordinates_inspected
            .saturating_add(other.scalar_event_coordinates_inspected);
        self.scalar_unique_coordinates_inspected = self
            .scalar_unique_coordinates_inspected
            .saturating_add(other.scalar_unique_coordinates_inspected);
        self.range_observations_inspected = self
            .range_observations_inspected
            .saturating_add(other.range_observations_inspected);
        self.range_coordinates_expanded = self
            .range_coordinates_expanded
            .saturating_add(other.range_coordinates_expanded);
        self.sheet_lookups_attempted = self
            .sheet_lookups_attempted
            .saturating_add(other.sheet_lookups_attempted);
        self.sheet_lookups_succeeded = self
            .sheet_lookups_succeeded
            .saturating_add(other.sheet_lookups_succeeded);
        self.dependency_owner_lookups_attempted = self
            .dependency_owner_lookups_attempted
            .saturating_add(other.dependency_owner_lookups_attempted);
        self.dependency_owner_lookup_hits = self
            .dependency_owner_lookup_hits
            .saturating_add(other.dependency_owner_lookup_hits);
        self.dependency_owner_lookup_misses = self
            .dependency_owner_lookup_misses
            .saturating_add(other.dependency_owner_lookup_misses);
        self.formula_membership_lookups = self
            .formula_membership_lookups
            .saturating_add(other.formula_membership_lookups);
        self.formula_vertex_resolutions = self
            .formula_vertex_resolutions
            .saturating_add(other.formula_vertex_resolutions);
        self.non_formula_vertex_resolutions = self
            .non_formula_vertex_resolutions
            .saturating_add(other.non_formula_vertex_resolutions);
        self.raw_formula_edge_candidates = self
            .raw_formula_edge_candidates
            .saturating_add(other.raw_formula_edge_candidates);
        self.formula_edge_insert_attempts = self
            .formula_edge_insert_attempts
            .saturating_add(other.formula_edge_insert_attempts);
        self.duplicate_formula_edge_candidates = self
            .duplicate_formula_edge_candidates
            .saturating_add(other.duplicate_formula_edge_candidates);
        self.unique_formula_edges = self
            .unique_formula_edges
            .saturating_add(other.unique_formula_edges);
        self.exact_cell_insert_attempts = self
            .exact_cell_insert_attempts
            .saturating_add(other.exact_cell_insert_attempts);
        self.exact_cells_produced = self
            .exact_cells_produced
            .saturating_add(other.exact_cells_produced);
        self.name_resolution_attempts = self
            .name_resolution_attempts
            .saturating_add(other.name_resolution_attempts);
        self.name_resolution_hits = self
            .name_resolution_hits
            .saturating_add(other.name_resolution_hits);
        self.table_resolution_attempts = self
            .table_resolution_attempts
            .saturating_add(other.table_resolution_attempts);
        self.table_resolution_hits = self
            .table_resolution_hits
            .saturating_add(other.table_resolution_hits);
        self.generation_revision_lookups = self
            .generation_revision_lookups
            .saturating_add(other.generation_revision_lookups);
        self.formula_owner_index_builds = self
            .formula_owner_index_builds
            .saturating_add(other.formula_owner_index_builds);
        self.formula_owner_index_entries = self
            .formula_owner_index_entries
            .saturating_add(other.formula_owner_index_entries);
        self.formula_owner_index_build_ns = self
            .formula_owner_index_build_ns
            .saturating_add(other.formula_owner_index_build_ns);
        self.scalar_event_scan_ns = self
            .scalar_event_scan_ns
            .saturating_add(other.scalar_event_scan_ns);
        self.scalar_exact_cell_edge_ns = self
            .scalar_exact_cell_edge_ns
            .saturating_add(other.scalar_exact_cell_edge_ns);
        self.name_resolution_ns = self
            .name_resolution_ns
            .saturating_add(other.name_resolution_ns);
        self.table_resolution_ns = self
            .table_resolution_ns
            .saturating_add(other.table_resolution_ns);
        self.other_ns = self.other_ns.saturating_add(other.other_ns);
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct V2ReadFinalizationAttribution {
    pub recorder_extraction_ns: u128,
    pub cloning_copying_ns: u128,
    pub sorting_ns: u128,
    pub deduplication_ns: u128,
    pub range_canonicalization_ns: u128,
    pub formula_edge_extraction_ns: u128,
    pub formula_edge: V2FormulaEdgeExtractionAttribution,
    pub reference_generation_canonicalization_ns: u128,
    pub selected_reference_handling_ns: u128,
    pub spill_shape_metadata_ns: u128,
    pub semantic_effect_metadata_ns: u128,
    pub summary_construction_ns: u128,
    pub other_ns: u128,
    pub raw_entries_before: usize,
    pub unique_entries_after: usize,
    pub duplicate_entries_removed: usize,
    pub elements_copied: usize,
}

impl V2ReadFinalizationAttribution {
    pub(crate) fn elapsed_ns(&self) -> u128 {
        self.recorder_extraction_ns
            .saturating_add(self.cloning_copying_ns)
            .saturating_add(self.sorting_ns)
            .saturating_add(self.deduplication_ns)
            .saturating_add(self.range_canonicalization_ns)
            .saturating_add(self.formula_edge_extraction_ns)
            .saturating_add(self.reference_generation_canonicalization_ns)
            .saturating_add(self.selected_reference_handling_ns)
            .saturating_add(self.spill_shape_metadata_ns)
            .saturating_add(self.semantic_effect_metadata_ns)
            .saturating_add(self.summary_construction_ns)
            .saturating_add(self.other_ns)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct V2RecorderAttribution {
    pub raw_events: [usize; RECORDER_OPERATION_COUNT],
    pub sampled_elapsed_ns: [u128; RECORDER_OPERATION_COUNT],
    pub unique_entries: [usize; RECORDER_OPERATION_COUNT],
    pub effect_raw_events: [usize; EFFECT_KIND_COUNT],
    pub effect_unique_entries: [usize; EFFECT_KIND_COUNT],
    pub formula_edge_raw_events: usize,
    pub formula_edge_unique_entries: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum V2FormulaAttributionCategory {
    OutsideWorkspace,
    RetainedDirtyUpstream,
    ExactScc,
    Downstream,
}

impl Default for V2FormulaAttributionCategory {
    fn default() -> Self {
        Self::OutsideWorkspace
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct V2FormulaAttribution {
    pub invocations: usize,
    pub exact_read_sets_produced: usize,
    pub logical_range_positions: usize,
    pub physical_cells_fetched: usize,
    pub formula_execution_ns: u128,
    pub observation_recording_ns: u128,
    pub range_read_materialization_ns: u128,
    pub exact_read_canonicalization_ns: u128,
    pub recorder: V2RecorderAttribution,
    pub finalization: V2ReadFinalizationAttribution,
    pub finalization_samples: Vec<(VertexId, usize, u128)>,
    pub formula_edge_samples: Vec<(VertexId, usize, usize, u128, usize)>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct V2OwnerSheetAttribution {
    pub probes: usize,
    pub unique_coordinates: usize,
    pub repeated_coordinates: usize,
    pub repeated_positive_probes: usize,
    pub repeated_negative_probes: usize,
    pub hits: usize,
    pub misses: usize,
    pub formula_owners: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct V2OwnerReadSetAttribution {
    pub category: V2FormulaAttributionCategory,
    pub vertex: VertexId,
    pub size: usize,
    pub hits: usize,
    pub misses: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct V2OwnerReuseAttribution {
    seen: CoordHashMap<PackedSheetCell, (bool, u32)>,
    pub probes: usize,
    pub unique_coordinates: usize,
    pub repeated_coordinates: usize,
    pub repeated_positive_probes: usize,
    pub repeated_negative_probes: usize,
    pub unique_positive_coordinates: usize,
    pub unique_negative_coordinates: usize,
    pub per_sheet: BTreeMap<SheetId, V2OwnerSheetAttribution>,
    pub read_sets: Vec<V2OwnerReadSetAttribution>,
    pub read_set_cells: Vec<Vec<PackedSheetCell>>,
}

impl V2OwnerReuseAttribution {
    fn record(
        &mut self,
        category: V2FormulaAttributionCategory,
        vertex: VertexId,
        probes: &[(PackedSheetCell, bool)],
        formula_owners: &BTreeMap<SheetId, usize>,
    ) {
        for (&sheet, &owners) in formula_owners {
            self.per_sheet.entry(sheet).or_default().formula_owners = owners;
        }
        let mut hits = 0usize;
        for &(coordinate, positive) in probes {
            self.probes = self.probes.saturating_add(1);
            let sheet = self.per_sheet.entry(coordinate.sheet_id()).or_default();
            sheet.probes = sheet.probes.saturating_add(1);
            if positive {
                hits = hits.saturating_add(1);
                sheet.hits = sheet.hits.saturating_add(1);
            } else {
                sheet.misses = sheet.misses.saturating_add(1);
            }
            match self.seen.entry(coordinate) {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert((positive, 1));
                    self.unique_coordinates = self.unique_coordinates.saturating_add(1);
                    sheet.unique_coordinates = sheet.unique_coordinates.saturating_add(1);
                    if positive {
                        self.unique_positive_coordinates =
                            self.unique_positive_coordinates.saturating_add(1);
                    } else {
                        self.unique_negative_coordinates =
                            self.unique_negative_coordinates.saturating_add(1);
                    }
                }
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    let (first_positive, count) = entry.get_mut();
                    debug_assert_eq!(*first_positive, positive);
                    if *count == 1 {
                        self.repeated_coordinates = self.repeated_coordinates.saturating_add(1);
                        sheet.repeated_coordinates = sheet.repeated_coordinates.saturating_add(1);
                    }
                    *count = count.saturating_add(1);
                    if positive {
                        self.repeated_positive_probes =
                            self.repeated_positive_probes.saturating_add(1);
                        sheet.repeated_positive_probes =
                            sheet.repeated_positive_probes.saturating_add(1);
                    } else {
                        self.repeated_negative_probes =
                            self.repeated_negative_probes.saturating_add(1);
                        sheet.repeated_negative_probes =
                            sheet.repeated_negative_probes.saturating_add(1);
                    }
                }
            }
        }
        self.read_sets.push(V2OwnerReadSetAttribution {
            category,
            vertex,
            size: probes.len(),
            hits,
            misses: probes.len().saturating_sub(hits),
        });
        self.read_set_cells
            .push(probes.iter().map(|(coordinate, _)| *coordinate).collect());
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct V2ExclusiveAttribution {
    pub outside_workspace: V2FormulaAttribution,
    pub retained_dirty_upstream: V2FormulaAttribution,
    pub exact_scc: V2FormulaAttribution,
    pub downstream: V2FormulaAttribution,
    pub owner_reuse: V2OwnerReuseAttribution,
    pub retained_state_scan_ns: u128,
    pub demand_scheduling_ns: u128,
    pub retained_plan_validation_ns: u128,
    pub contract_validation_ns: u128,
    pub adjacency_replacement_ns: u128,
    pub cleanup_ns: u128,
    pub explicit_residual_ns: u128,
}

impl V2ExclusiveAttribution {
    fn formula_mut(&mut self, category: V2FormulaAttributionCategory) -> &mut V2FormulaAttribution {
        match category {
            V2FormulaAttributionCategory::OutsideWorkspace => &mut self.outside_workspace,
            V2FormulaAttributionCategory::RetainedDirtyUpstream => {
                &mut self.retained_dirty_upstream
            }
            V2FormulaAttributionCategory::ExactScc => &mut self.exact_scc,
            V2FormulaAttributionCategory::Downstream => &mut self.downstream,
        }
    }

    pub(crate) fn record_invocation(
        &mut self,
        category: V2FormulaAttributionCategory,
        formula_execution_ns: u128,
        reads: RawReadAttribution,
    ) {
        let formula = self.formula_mut(category);
        formula.invocations = formula.invocations.saturating_add(1);
        formula.formula_execution_ns = formula
            .formula_execution_ns
            .saturating_add(formula_execution_ns);
        formula.observation_recording_ns = formula
            .observation_recording_ns
            .saturating_add(reads.observation_recording_ns);
        formula.range_read_materialization_ns = formula
            .range_read_materialization_ns
            .saturating_add(reads.range_read_materialization_ns);
        for index in 0..RECORDER_OPERATION_COUNT {
            formula.recorder.raw_events[index] =
                formula.recorder.raw_events[index].saturating_add(reads.recorder_raw_events[index]);
            formula.recorder.sampled_elapsed_ns[index] = formula.recorder.sampled_elapsed_ns[index]
                .saturating_add(reads.recorder_sampled_elapsed_ns[index]);
        }
        for index in 0..EFFECT_KIND_COUNT {
            formula.recorder.effect_raw_events[index] = formula.recorder.effect_raw_events[index]
                .saturating_add(reads.effect_raw_events[index]);
        }
    }

    pub(crate) fn record_exact_read(
        &mut self,
        category: V2FormulaAttributionCategory,
        vertex: VertexId,
        reads: &ExactReadSet,
        finalization: V2ReadFinalizationAttribution,
    ) {
        let finalization_ns = finalization.elapsed_ns();
        if !finalization.formula_edge.owner_probe_results.is_empty() {
            self.owner_reuse.record(
                category,
                vertex,
                &finalization.formula_edge.owner_probe_results,
                &finalization.formula_edge.formula_owner_entries_by_sheet,
            );
        }
        let formula = self.formula_mut(category);
        formula.exact_read_sets_produced = formula.exact_read_sets_produced.saturating_add(1);
        formula.logical_range_positions = formula
            .logical_range_positions
            .saturating_add(reads.logical_range_positions);
        formula.physical_cells_fetched = formula
            .physical_cells_fetched
            .saturating_add(reads.physical_cells_fetched);
        formula.exact_read_canonicalization_ns = formula
            .exact_read_canonicalization_ns
            .saturating_add(finalization_ns);
        formula.finalization_samples.push((
            vertex,
            finalization.raw_entries_before,
            finalization_ns,
        ));
        formula.formula_edge_samples.push((
            vertex,
            finalization.formula_edge.scalar_events,
            finalization.formula_edge.dependency_owner_lookups_attempted,
            finalization.formula_edge_extraction_ns,
            finalization.formula_edge.unique_formula_edges,
        ));
        formula.recorder.unique_entries[0] =
            formula.recorder.unique_entries[0].saturating_add(reads.cells.len());
        formula.recorder.unique_entries[1] =
            formula.recorder.unique_entries[1].saturating_add(reads.ranges.len());
        formula.recorder.unique_entries[2] =
            formula.recorder.unique_entries[2].saturating_add(reads.selected_targets.len());
        formula.recorder.unique_entries[3] =
            formula.recorder.unique_entries[3].saturating_add(reads.reference_observations.len());
        formula.recorder.unique_entries[4] =
            formula.recorder.unique_entries[4].saturating_add(reads.names.len());
        formula.recorder.unique_entries[5] =
            formula.recorder.unique_entries[5].saturating_add(reads.tables.len());
        formula.recorder.unique_entries[6] =
            formula.recorder.unique_entries[6].saturating_add(reads.external.len());
        formula.recorder.unique_entries[7] =
            formula.recorder.unique_entries[7].saturating_add(reads.effects.len());
        for effect in &reads.effects {
            let index = effect.index();
            formula.recorder.effect_unique_entries[index] =
                formula.recorder.effect_unique_entries[index].saturating_add(1);
        }
        formula.recorder.formula_edge_raw_events = formula
            .recorder
            .formula_edge_raw_events
            .saturating_add(reads.formula_edge_events);
        formula.recorder.formula_edge_unique_entries = formula
            .recorder
            .formula_edge_unique_entries
            .saturating_add(reads.formula_edges.len());
        formula.finalization.recorder_extraction_ns = formula
            .finalization
            .recorder_extraction_ns
            .saturating_add(finalization.recorder_extraction_ns);
        formula.finalization.cloning_copying_ns = formula
            .finalization
            .cloning_copying_ns
            .saturating_add(finalization.cloning_copying_ns);
        formula.finalization.sorting_ns = formula
            .finalization
            .sorting_ns
            .saturating_add(finalization.sorting_ns);
        formula.finalization.deduplication_ns = formula
            .finalization
            .deduplication_ns
            .saturating_add(finalization.deduplication_ns);
        formula.finalization.range_canonicalization_ns = formula
            .finalization
            .range_canonicalization_ns
            .saturating_add(finalization.range_canonicalization_ns);
        formula.finalization.formula_edge_extraction_ns = formula
            .finalization
            .formula_edge_extraction_ns
            .saturating_add(finalization.formula_edge_extraction_ns);
        formula
            .finalization
            .formula_edge
            .accumulate(finalization.formula_edge);
        formula
            .finalization
            .reference_generation_canonicalization_ns = formula
            .finalization
            .reference_generation_canonicalization_ns
            .saturating_add(finalization.reference_generation_canonicalization_ns);
        formula.finalization.selected_reference_handling_ns = formula
            .finalization
            .selected_reference_handling_ns
            .saturating_add(finalization.selected_reference_handling_ns);
        formula.finalization.spill_shape_metadata_ns = formula
            .finalization
            .spill_shape_metadata_ns
            .saturating_add(finalization.spill_shape_metadata_ns);
        formula.finalization.semantic_effect_metadata_ns = formula
            .finalization
            .semantic_effect_metadata_ns
            .saturating_add(finalization.semantic_effect_metadata_ns);
        formula.finalization.summary_construction_ns = formula
            .finalization
            .summary_construction_ns
            .saturating_add(finalization.summary_construction_ns);
        formula.finalization.other_ns = formula
            .finalization
            .other_ns
            .saturating_add(finalization.other_ns);
        formula.finalization.raw_entries_before = formula
            .finalization
            .raw_entries_before
            .saturating_add(finalization.raw_entries_before);
        formula.finalization.unique_entries_after = formula
            .finalization
            .unique_entries_after
            .saturating_add(finalization.unique_entries_after);
        formula.finalization.duplicate_entries_removed = formula
            .finalization
            .duplicate_entries_removed
            .saturating_add(finalization.duplicate_entries_removed);
        formula.finalization.elements_copied = formula
            .finalization
            .elements_copied
            .saturating_add(finalization.elements_copied);
    }

    pub(crate) fn exclusive_children_ns(&self) -> u128 {
        self.retained_state_scan_ns
            .saturating_add(self.demand_scheduling_ns)
            .saturating_add(self.retained_plan_validation_ns)
            .saturating_add(self.contract_validation_ns)
            .saturating_add(self.adjacency_replacement_ns)
            .saturating_add(self.cleanup_ns)
            .saturating_add(self.outside_workspace.formula_execution_ns)
            .saturating_add(self.retained_dirty_upstream.formula_execution_ns)
            .saturating_add(self.exact_scc.formula_execution_ns)
            .saturating_add(self.downstream.formula_execution_ns)
            .saturating_add(self.outside_workspace.exact_read_canonicalization_ns)
            .saturating_add(self.retained_dirty_upstream.exact_read_canonicalization_ns)
            .saturating_add(self.exact_scc.exact_read_canonicalization_ns)
            .saturating_add(self.downstream.exact_read_canonicalization_ns)
    }
}

#[derive(Default)]
struct V2ReadRecorderState {
    source: Option<VertexId>,
    current: RawReadSet,
    completed: BTreeMap<VertexId, RawReadSet>,
}

pub(crate) struct V2ReadRecorder {
    active: AtomicBool,
    state: Mutex<V2ReadRecorderState>,
    attribution_enabled: bool,
    owner_reuse_enabled: bool,
    recorder_sample_counters: [std::sync::atomic::AtomicUsize; RECORDER_OPERATION_COUNT],
    range_materialization_sample_counter: std::sync::atomic::AtomicUsize,
}

impl Default for V2ReadRecorder {
    fn default() -> Self {
        Self {
            active: AtomicBool::new(false),
            state: Mutex::new(V2ReadRecorderState::default()),
            attribution_enabled: std::env::var_os("FZ_TRACE_V2_ATTRIBUTION").is_some(),
            owner_reuse_enabled: std::env::var_os("FZ_TRACE_V2_OWNER_REUSE").is_some()
                || std::env::var_os("FZ_BENCH_V2_OWNER_RESOLVERS").is_some(),
            recorder_sample_counters: std::array::from_fn(|_| {
                std::sync::atomic::AtomicUsize::new(0)
            }),
            range_materialization_sample_counter: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

impl V2ReadRecorder {
    pub(crate) fn attribution_enabled(&self) -> bool {
        self.attribution_enabled
    }

    pub(crate) fn owner_reuse_enabled(&self) -> bool {
        self.attribution_enabled && self.owner_reuse_enabled
    }

    pub(crate) fn begin_attribution_request(&self) {
        for counter in &self.recorder_sample_counters {
            counter.store(0, Ordering::Relaxed);
        }
        self.range_materialization_sample_counter
            .store(0, Ordering::Relaxed);
    }

    pub(crate) fn begin(self: &Arc<Self>, source: VertexId) -> V2ReadGuard {
        let mut state = self.state.lock().expect("V2 read recorder poisoned");
        if let Some(previous) = state.source.take() {
            let reads = std::mem::take(&mut state.current);
            state.completed.insert(previous, reads);
        }
        state.completed.remove(&source);
        state.source = Some(source);
        state.current = RawReadSet::default();
        drop(state);
        self.active.store(true, Ordering::Release);
        V2ReadGuard {
            recorder: Arc::clone(self),
        }
    }

    fn clear(&self) {
        self.active.store(false, Ordering::Release);
        let mut state = self.state.lock().expect("V2 read recorder poisoned");
        if let Some(source) = state.source.take() {
            let reads = std::mem::take(&mut state.current);
            state.completed.insert(source, reads);
        }
    }

    pub(crate) fn take(&self, source: VertexId) -> RawReadSet {
        let mut state = self.state.lock().expect("V2 read recorder poisoned");
        if state.source == Some(source) {
            self.active.store(false, Ordering::Release);
            state.source = None;
            return std::mem::take(&mut state.current);
        }
        state.completed.remove(&source).unwrap_or_default()
    }

    pub(crate) fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    pub(crate) fn discard_all(&self) {
        self.active.store(false, Ordering::Release);
        *self.state.lock().expect("V2 read recorder poisoned") = V2ReadRecorderState::default();
    }

    #[cfg(test)]
    pub(crate) fn retained_entry_count(&self) -> usize {
        let state = self.state.lock().expect("V2 read recorder poisoned");
        state
            .completed
            .len()
            .saturating_add(usize::from(state.source.is_some()))
    }

    pub(crate) fn attribution_stats(&self, source: VertexId) -> RawReadAttribution {
        let state = self.state.lock().expect("V2 read recorder poisoned");
        let reads = if state.source == Some(source) {
            Some(&state.current)
        } else {
            state.completed.get(&source)
        };
        reads
            .map(|reads| RawReadAttribution {
                observation_recording_ns: reads.observation_recording_ns,
                range_read_materialization_ns: reads.range_read_materialization_ns,
                recorder_raw_events: reads.recorder_raw_events,
                recorder_sampled_elapsed_ns: reads.recorder_sampled_elapsed_ns,
                effect_raw_events: reads.effect_raw_events,
            })
            .unwrap_or_default()
    }

    fn with_current(&self, operation: V2RecorderOperation, f: impl FnOnce(&mut RawReadSet)) {
        if !self.is_active() {
            return;
        }
        let operation_index = operation.index();
        let sample = self.attribution_enabled
            && self.recorder_sample_counters[operation_index]
                .fetch_add(1, Ordering::Relaxed)
                .saturating_add(1)
                % RECORDER_OPERATION_SAMPLE_INTERVAL
                == 0;
        let mut state = self.state.lock().expect("V2 read recorder poisoned");
        if state.source.is_none() {
            return;
        }
        let reads = &mut state.current;
        reads.recorder_raw_events[operation_index] =
            reads.recorder_raw_events[operation_index].saturating_add(1);
        let started = sample.then(Instant::now);
        f(reads);
        if let Some(started) = started {
            let sampled_ns = started
                .elapsed()
                .as_nanos()
                .saturating_mul(RECORDER_OPERATION_SAMPLE_INTERVAL as u128);
            reads.recorder_sampled_elapsed_ns[operation_index] =
                reads.recorder_sampled_elapsed_ns[operation_index].saturating_add(sampled_ns);
            reads.observation_recording_ns =
                reads.observation_recording_ns.saturating_add(sampled_ns);
        }
    }

    pub(crate) fn next_range_materialization_sample_interval(&self) -> Option<u32> {
        if !self.attribution_enabled || !self.is_active() {
            return None;
        }
        let sample = self
            .range_materialization_sample_counter
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1)
            % RANGE_MATERIALIZATION_SAMPLE_INTERVAL
            == 0;
        sample.then_some(RANGE_MATERIALIZATION_SAMPLE_INTERVAL as u32)
    }

    pub(crate) fn record_range_materialization(&self, elapsed_ns: u128) {
        if !self.is_active() {
            return;
        }
        let mut state = self.state.lock().expect("V2 read recorder poisoned");
        if state.source.is_none() {
            return;
        }
        state.current.range_read_materialization_ns = state
            .current
            .range_read_materialization_ns
            .saturating_add(elapsed_ns);
    }

    pub(crate) fn record_cell(&self, cell: PackedSheetCell) {
        self.with_current(V2RecorderOperation::ScalarCellRead, |reads| {
            reads.cell_events_sorted = reads.cell_events.is_empty()
                || (reads.cell_events_sorted
                    && reads
                        .cell_events
                        .last()
                        .is_none_or(|previous| *previous <= cell));
            reads.cell_events.push(cell);
            reads.cell_read_events = reads.cell_read_events.saturating_add(1);
            reads.physical_cells_fetched = reads.physical_cells_fetched.saturating_add(1);
        });
    }

    fn record_cell_rows(
        &self,
        sheet_id: SheetId,
        start_row: u32,
        end_row: u32,
        column_runs: &[(u32, u32)],
    ) {
        if start_row > end_row || column_runs.is_empty() || !self.is_active() {
            return;
        }
        let started = self.attribution_enabled.then(Instant::now);
        let mut state = self.state.lock().expect("V2 read recorder poisoned");
        if state.source.is_none() {
            return;
        }
        let reads = &mut state.current;
        let before = reads.cell_events.len();
        if let Some(first) = PackedSheetCell::try_new(sheet_id, start_row, column_runs[0].0) {
            reads.cell_events_sorted = reads.cell_events.is_empty()
                || (reads.cell_events_sorted
                    && reads
                        .cell_events
                        .last()
                        .is_none_or(|previous| *previous <= first));
        }
        let rows = (end_row - start_row + 1) as usize;
        let columns = column_runs.iter().fold(0usize, |total, (start, end)| {
            total.saturating_add(end.saturating_sub(*start).saturating_add(1) as usize)
        });
        reads.cell_events.reserve(rows.saturating_mul(columns));
        for row in start_row..=end_row {
            for &(start_col, end_col) in column_runs {
                for col in start_col..=end_col {
                    if let Some(cell) = PackedSheetCell::try_new(sheet_id, row, col) {
                        reads.cell_events.push(cell);
                    }
                }
            }
        }
        let added = reads.cell_events.len().saturating_sub(before);
        reads.cell_read_events = reads.cell_read_events.saturating_add(added);
        reads.physical_cells_fetched = reads.physical_cells_fetched.saturating_add(added);
        if self.attribution_enabled {
            let operation_index = V2RecorderOperation::ScalarCellRead.index();
            reads.recorder_raw_events[operation_index] =
                reads.recorder_raw_events[operation_index].saturating_add(added);
            self.recorder_sample_counters[operation_index].fetch_add(added, Ordering::Relaxed);
            if let Some(started) = started {
                let elapsed = started.elapsed().as_nanos();
                reads.recorder_sampled_elapsed_ns[operation_index] =
                    reads.recorder_sampled_elapsed_ns[operation_index].saturating_add(elapsed);
                reads.observation_recording_ns =
                    reads.observation_recording_ns.saturating_add(elapsed);
            }
        }
    }

    pub(crate) fn record_name(&self, name: impl Into<String>) {
        self.with_current(V2RecorderOperation::NameSymbol, |reads| {
            reads.names.insert(name.into());
        });
    }

    pub(crate) fn record_table(&self, name: impl Into<String>) {
        self.with_current(V2RecorderOperation::Table, |reads| {
            reads.tables.insert(name.into());
        });
    }

    pub(crate) fn record_external(&self, name: impl Into<String>) {
        self.with_current(V2RecorderOperation::Provider, |reads| {
            reads.external.insert(name.into());
        });
    }

    pub(crate) fn record_effect(&self, effect: EffectKind) {
        let effect_index = effect.index();
        self.with_current(V2RecorderOperation::SemanticEffect, |reads| {
            reads.effects.insert(effect);
            reads.effect_raw_events[effect_index] =
                reads.effect_raw_events[effect_index].saturating_add(1);
        });
    }

    pub(crate) fn record_selected_cell(&self, sheet: &str, row: u32, col: u32) {
        self.with_current(V2RecorderOperation::SelectedReference, |reads| {
            reads.selected_targets.insert(ReadCell {
                sheet: sheet.to_string(),
                row,
                col,
            });
        });
    }

    pub(crate) fn record_reference_observation(&self, observation: ReferenceObservationRecord) {
        self.with_current(V2RecorderOperation::ReferenceGeneration, |reads| {
            reads.reference_observations.insert(observation);
        });
    }

    pub(crate) fn record_range_consumed(
        &self,
        sheet: &str,
        start_row: u32,
        start_col: u32,
        end_row: u32,
        end_col: u32,
        logical_positions: usize,
        physical_cells: usize,
    ) {
        self.with_current(V2RecorderOperation::RangeObservation, |reads| {
            reads.ranges.insert(ReadRange {
                sheet: sheet.to_string(),
                start_row,
                start_col,
                end_row,
                end_col,
            });
            reads.range_read_events = reads.range_read_events.saturating_add(1);
            reads.logical_range_positions = reads
                .logical_range_positions
                .saturating_add(logical_positions);
            reads.physical_cells_fetched =
                reads.physical_cells_fetched.saturating_add(physical_cells);
        });
    }
}

impl RangeReadObserver for V2ReadRecorder {
    fn cell_read(&self, cell: PackedSheetCell) {
        self.record_cell(cell);
    }

    fn cell_rows_read(
        &self,
        sheet_id: SheetId,
        start_row: u32,
        end_row: u32,
        column_runs: &[(u32, u32)],
    ) {
        self.record_cell_rows(sheet_id, start_row, end_row, column_runs);
    }

    fn range_consumed(
        &self,
        sheet: &str,
        start_row: u32,
        start_col: u32,
        end_row: u32,
        end_col: u32,
        logical_positions: usize,
        physical_cells: usize,
    ) {
        self.record_range_consumed(
            sheet,
            start_row,
            start_col,
            end_row,
            end_col,
            logical_positions,
            physical_cells,
        );
    }

    fn range_materialized(&self, elapsed_ns: u128) {
        self.record_range_materialization(elapsed_ns);
    }

    fn range_materialization_sample_interval(&self) -> Option<u32> {
        self.next_range_materialization_sample_interval()
    }
}

pub(crate) struct V2ReadGuard {
    recorder: Arc<V2ReadRecorder>,
}

impl Drop for V2ReadGuard {
    fn drop(&mut self) {
        self.recorder.clear();
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ExactReadSet {
    pub cells: Vec<PackedSheetCell>,
    pub selected_targets: BTreeSet<CellRef>,
    pub ranges: BTreeSet<ReadRange>,
    pub names: BTreeSet<String>,
    pub tables: BTreeSet<String>,
    pub external: BTreeSet<String>,
    pub effects: BTreeSet<EffectKind>,
    pub formula_edges: Vec<VertexId>,
    pub formula_edge_events: usize,
    pub reference_observations: BTreeSet<ReferenceObservationRecord>,
    pub logical_range_positions: usize,
    pub physical_cells_fetched: usize,
    pub diagnostic_records_retained: usize,
}

impl ExactReadSet {
    pub(crate) fn contains_cell(&self, cell: &CellRef) -> bool {
        PackedSheetCell::try_new(cell.sheet_id, cell.coord.row(), cell.coord.col())
            .is_some_and(|cell| self.cells.binary_search(&cell).is_ok())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct V2Metrics {
    pub dirty_roots: usize,
    pub dirty_candidates: usize,
    pub formulas_evaluated: usize,
    pub formulas_evaluated_inside_workspaces: usize,
    pub formulas_evaluated_outside_workspaces: usize,
    pub runtime_formula_edge_events: usize,
    pub runtime_formula_edges_processed: usize,
    pub runtime_formula_edges_retained: usize,
    pub unique_current_runtime_formula_edges: usize,
    pub stale_edges_removed: usize,
    pub diagnostic_records_retained: usize,
    pub diagnostic_records_dropped: usize,
    pub logical_range_positions: usize,
    pub physical_cells_fetched: usize,
    pub active_cyclic_workspace_members: usize,
    pub workspace_members_evaluated: usize,
    pub workspace_units: usize,
    pub solver_passes: usize,
    pub schedule_units: usize,
    pub fallback_mode_activations: usize,
    pub queue_steps: usize,
    pub demand_subgraph_ns: u128,
    pub schedule_demand_subgraph_ns: u128,
    pub scoped_admission_ns: u128,
    pub dirty_seed_selection_ns: u128,
    pub schedule_construction_ns: u128,
    pub acyclic_formula_evaluation_ns: u128,
    pub workspace_construction_ns: u128,
    pub iterative_solver_execution_ns: u128,
    pub exact_read_finalization_ns: u128,
    pub exact_edge_replacement_ns: u128,
    pub generation_reference_validation_ns: u128,
    pub spill_effect_commit_ns: u128,
    pub schedule_ns: u128,
    pub formula_ns: u128,
    pub workspace_ns: u128,
    pub cleanup_ns: u128,
    pub elapsed_ns: u128,
    pub kernel_named_phase_ns: u128,
    pub kernel_unattributed_ns: u128,
    pub kernel_top_level_named_phase_ns: u128,
    pub kernel_top_level_unattributed_ns: u128,
    pub exclusive_attribution: V2ExclusiveAttribution,
    pub retained_state_scan_ns: u128,
    pub retained_state_scan_read_sets: usize,
    pub retained_state_scan_edges: usize,
    pub demand_nodes_visited: usize,
    pub demand_explicit_edges_visited: usize,
    pub demand_virtual_edges_visited: usize,
    pub demand_dedup_entries: usize,
    pub demand_allocation_ns: u128,
    pub demand_dependency_traversal_ns: u128,
    pub demand_virtual_traversal_ns: u128,
    pub virtual_demand: V2VirtualDemandAttribution,
    pub demand_closures_built: usize,
    pub demand_closure_reuse_hits: usize,
    pub demand_closure_reuse_rejections: usize,
    pub demand_closure_reuse_rejection_reasons: BTreeMap<String, usize>,
    pub demand_reuse_consumption_ns: u128,
    pub workspace_retained_plan_candidates: usize,
    pub workspace_retained_plan_hits: usize,
    pub workspace_retained_plan_rejections: usize,
    pub workspace_retained_plan_rejection_reasons: BTreeMap<String, usize>,
    pub discovery_evaluations_avoided: usize,
    pub dirty_upstream_evaluations: usize,
    pub clean_upstream_cache_reuses: usize,
    pub scc_discovery_evaluations_avoided: usize,
    pub downstream_discovery_evaluations_avoided: usize,
    pub retained_plan_runtime_invalidations: usize,
    pub retained_plan_reopens: usize,
    pub retained_plan_runtime_invalidation_reasons: BTreeMap<String, usize>,
    pub retained_classification_effective_dirty_members: usize,
    pub retained_classification_clean_members: usize,
    pub retained_classification_exact_scc_members: usize,
    pub retained_classification_upstream_members: usize,
    pub retained_classification_downstream_members: usize,
    pub retained_classification_unrelated_members: usize,
    pub retained_classification_missing_reads: usize,
    pub retained_classification_invalid_members: usize,
    pub retained_classification_reusable_values: usize,
    pub admission_demand_nodes_visited: usize,
    pub admission_demand_explicit_edges_visited: usize,
    pub admission_demand_virtual_edges_visited: usize,
    pub admission_demand_allocation_ns: u128,
    pub admission_demand_dependency_traversal_ns: u128,
    pub admission_demand_virtual_traversal_ns: u128,
    pub schedule_demand_nodes_visited: usize,
    pub schedule_demand_explicit_edges_visited: usize,
    pub schedule_demand_virtual_edges_visited: usize,
    pub schedule_demand_allocation_ns: u128,
    pub schedule_demand_dependency_traversal_ns: u128,
    pub schedule_demand_virtual_traversal_ns: u128,
    pub validation_read_sets_examined: usize,
    pub validation_runtime_formula_edges_examined: usize,
    pub validation_runtime_formula_edges_invalidated: usize,
    pub validation_runtime_formula_edges_unchanged: usize,
    pub validation_runtime_formula_ns: u128,
    pub validation_reference_observations_examined: usize,
    pub validation_reference_observations_invalidated: usize,
    pub validation_reference_observations_unchanged: usize,
    pub validation_reference_ns: u128,
    pub validation_topology_checks: usize,
    pub validation_topology_invalidated: usize,
    pub validation_topology_ns: u128,
    pub validation_symbol_name_entries: usize,
    pub validation_symbol_name_unchanged: usize,
    pub validation_symbol_name_invalidated: usize,
    pub validation_table_shape_entries: usize,
    pub validation_table_shape_unchanged: usize,
    pub validation_table_shape_invalidated: usize,
    pub validation_spill_shape_entries: usize,
    pub validation_spill_shape_unchanged: usize,
    pub validation_spill_shape_invalidated: usize,
    pub validation_provider_effect_entries: usize,
    pub validation_provider_effect_unchanged: usize,
    pub validation_provider_effect_invalidated: usize,
    pub validation_selected_reference_entries: usize,
    pub validation_selected_reference_unchanged: usize,
    pub validation_selected_reference_invalidated: usize,
    pub validation_range_reference_entries: usize,
    pub validation_range_reference_unchanged: usize,
    pub validation_range_reference_invalidated: usize,
    pub validation_metadata_ns: u128,
    pub exact_read_sets_finalized: usize,
    pub exact_read_sets_changed: usize,
    pub exact_read_sets_unchanged: usize,
    pub diagnostic_read_set_compare_ns: u128,
    pub exact_edges_examined: usize,
    pub exact_edges_removed: usize,
    pub exact_edges_inserted: usize,
    pub exact_edges_unchanged: usize,
    pub reverse_buckets_touched: usize,
    pub exact_edge_remove_ns: u128,
    pub exact_edge_insert_ns: u128,
    pub exact_edge_compare_ns: u128,
    pub exact_edge_canonicalize_ns: u128,
    pub exact_edge_sets_compared: usize,
    pub exact_identical_edge_sets: usize,
    pub exact_changed_edge_sets: usize,
    pub exact_reverse_buckets_untouched: usize,
    pub exact_reverse_buckets_mutated: usize,
    pub exact_full_replacement_fallback_count: usize,
    pub exact_full_replacement_fallback_reasons: BTreeMap<String, usize>,
    pub runtime_contract_validation_candidates: usize,
    pub runtime_contract_validation_cache_hits: usize,
    pub runtime_contract_validation_cache_misses: usize,
    pub runtime_contract_edges_skipped: usize,
    pub runtime_contract_edges_examined: usize,
    pub runtime_contract_certificates_invalidated: usize,
    pub runtime_contract_certificate_invalidation_reasons: BTreeMap<String, usize>,
    pub workspace_profiles: Vec<V2WorkspaceDiagnostic>,
    pub conservative_dirty_formula_count: usize,
    pub effective_dirty_formula_count: usize,
    pub pruned_dirty_formula_count: usize,
    pub conservative_workspace_candidate_count: usize,
    pub effective_workspace_count: usize,
    pub pruned_workspace_count: usize,
    pub exact_pruning_accepted_count: usize,
    pub exact_pruning_rejected_count: usize,
    pub exact_reverse_propagation_vertices_visited: usize,
    pub exact_reverse_read_formulas_reached: usize,
    pub exact_formula_edge_formulas_reached: usize,
    pub runtime_expansion_reopen_count: usize,
    pub pruning_rejection_reasons: BTreeMap<String, usize>,
    pub conservative_workspace_member_count: usize,
    pub exact_scc_member_count: usize,
    pub non_feedback_workspace_member_count: usize,
    pub workspace_discovery_formula_evaluations: usize,
    pub workspace_exact_scc_formula_evaluations: usize,
    pub workspace_upstream_formula_evaluations: usize,
    pub workspace_downstream_formula_evaluations: usize,
    pub outside_acyclic_formula_evaluations: usize,
    pub outside_acyclic_formula_evaluation_ns: u128,
    pub workspace_discovery_formula_evaluation_ns: u128,
    pub workspace_exact_scc_formula_evaluation_ns: u128,
    pub workspace_upstream_formula_evaluation_ns: u128,
    pub workspace_downstream_formula_evaluation_ns: u128,
    pub scc_preparation_formula_evaluations: usize,
    pub scc_preparation_ns: u128,
    pub repeated_non_feedback_evaluations: usize,
    pub repeated_non_feedback_evaluations_avoided: usize,
    pub workspaces_using_exact_scc_kernel: usize,
    pub workspaces_using_full_conservative_solver: usize,
    pub workspace_kernel_fallback_reasons: BTreeMap<String, usize>,
    pub exact_scc_rebuild_count: usize,
    pub exact_scc_expansion_count: usize,
    pub workspace_reopen_count: usize,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct EdgeReplacementStats {
    pub old_edges: usize,
    pub new_edges: usize,
    pub edges_examined: usize,
    pub removed: usize,
    pub inserted: usize,
    pub unchanged: usize,
    pub edge_sets_compared: usize,
    pub identical_edge_sets: usize,
    pub changed_edge_sets: usize,
    pub reverse_buckets_touched: usize,
    pub reverse_buckets_untouched: usize,
    pub reverse_buckets_mutated: usize,
    pub full_replacement_fallback: bool,
    pub compare_ns: u128,
    pub remove_ns: u128,
    pub insert_ns: u128,
    pub canonicalize_ns: u128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct V2WorkspaceClassification {
    pub stable_id: u64,
    pub conservative_members: usize,
    pub stage1_effective_dirty_members: usize,
    pub stage1_clean_members: usize,
    pub dirty_upstream_members: usize,
    pub clean_upstream_members: usize,
    pub exact_scc_members: usize,
    pub upstream_members: usize,
    pub downstream_members: usize,
    pub exact_scc_components: Vec<Vec<VertexId>>,
    pub upstream_order: Vec<VertexId>,
    pub downstream_order: Vec<VertexId>,
    pub unrelated_conservative_members: usize,
    pub exact_read_state_missing: usize,
    pub contract_certificate_valid: usize,
    pub contract_certificate_missing: usize,
    pub topology_sensitive_members: usize,
    pub generation_revision_invalid_members: usize,
    pub cached_value_reusable_members: usize,
    pub retained_plan_candidate: bool,
    pub retained_plan_valid: bool,
    pub retained_plan_rejection_reason: Option<&'static str>,
}

pub(crate) struct V2State {
    pub current_reads: BTreeMap<VertexId, ExactReadSet>,
    pub reverse_runtime: BTreeMap<VertexId, BTreeSet<VertexId>>,
    pub current_edges: BTreeSet<(VertexId, VertexId)>,
    pub last_effective_dirty: BTreeSet<VertexId>,
    pub topology_revision: Option<u64>,
    pub symbol_revision: Option<u64>,
    pub semantic_revision: Option<u64>,
    pub needs_full_rebuild: bool,
    pub metrics: V2Metrics,
    #[cfg(test)]
    pub fail_after_formula_commits: Option<usize>,
}

impl Default for V2State {
    fn default() -> Self {
        Self {
            current_reads: BTreeMap::new(),
            reverse_runtime: BTreeMap::new(),
            current_edges: BTreeSet::new(),
            last_effective_dirty: BTreeSet::new(),
            topology_revision: None,
            symbol_revision: None,
            semantic_revision: None,
            needs_full_rebuild: true,
            metrics: V2Metrics::default(),
            #[cfg(test)]
            fail_after_formula_commits: None,
        }
    }
}

impl V2State {
    fn clear_graph(&mut self) {
        self.current_reads.clear();
        self.reverse_runtime.clear();
        self.current_edges.clear();
        self.last_effective_dirty.clear();
        self.needs_full_rebuild = true;
    }

    pub(crate) fn synchronize_revisions(
        &mut self,
        topology_revision: u64,
        symbol_revision: u64,
        semantic_revision: u64,
    ) -> bool {
        let changed = self.topology_revision != Some(topology_revision)
            || self.symbol_revision != Some(symbol_revision)
            || self.semantic_revision != Some(semantic_revision);
        if changed {
            self.clear_graph();
            self.topology_revision = Some(topology_revision);
            self.symbol_revision = Some(symbol_revision);
            self.semantic_revision = Some(semantic_revision);
        }
        changed
    }

    pub(crate) fn revisions_match(
        &self,
        topology_revision: u64,
        symbol_revision: u64,
        semantic_revision: u64,
    ) -> bool {
        self.topology_revision == Some(topology_revision)
            && self.symbol_revision == Some(symbol_revision)
            && self.semantic_revision == Some(semantic_revision)
    }

    pub(crate) fn reset_exact_state(&mut self) {
        self.clear_graph();
        self.topology_revision = None;
        self.symbol_revision = None;
        self.semantic_revision = None;
    }

    pub(crate) fn synchronize_vertices(&mut self, vertices: &[VertexId]) {
        let live: FxHashSet<VertexId> = vertices.iter().copied().collect();
        let stale: Vec<VertexId> = self
            .current_reads
            .keys()
            .copied()
            .filter(|vertex| !live.contains(vertex))
            .collect();
        for vertex in stale {
            self.remove_read_set(vertex);
        }
    }

    fn remove_read_set(&mut self, vertex: VertexId) {
        if let Some(old) = self.current_reads.remove(&vertex) {
            for dependency in old.formula_edges {
                self.current_edges.remove(&(vertex, dependency));
                if let Some(readers) = self.reverse_runtime.get_mut(&dependency) {
                    readers.remove(&vertex);
                    if readers.is_empty() {
                        self.reverse_runtime.remove(&dependency);
                    }
                }
            }
        }
    }

    pub(crate) fn replace_read_set(&mut self, vertex: VertexId, reads: ExactReadSet) -> usize {
        self.replace_read_set_with_stats(vertex, reads).removed
    }

    pub(crate) fn replace_read_set_with_stats(
        &mut self,
        vertex: VertexId,
        reads: ExactReadSet,
    ) -> EdgeReplacementStats {
        let compare_started = Instant::now();
        let new_edges_len = reads.formula_edges.len();
        let (old_edges_len, removed_edges, inserted_edges, unchanged) = {
            let old_edges = self
                .current_reads
                .get(&vertex)
                .map(|old| old.formula_edges.as_slice())
                .unwrap_or_default();
            let mut removed = Vec::new();
            let mut inserted = Vec::new();
            let mut unchanged = 0usize;
            let mut old_index = 0usize;
            let mut new_index = 0usize;
            while old_index < old_edges.len() && new_index < reads.formula_edges.len() {
                match old_edges[old_index].cmp(&reads.formula_edges[new_index]) {
                    std::cmp::Ordering::Less => {
                        removed.push(old_edges[old_index]);
                        old_index = old_index.saturating_add(1);
                    }
                    std::cmp::Ordering::Greater => {
                        inserted.push(reads.formula_edges[new_index]);
                        new_index = new_index.saturating_add(1);
                    }
                    std::cmp::Ordering::Equal => {
                        unchanged = unchanged.saturating_add(1);
                        old_index = old_index.saturating_add(1);
                        new_index = new_index.saturating_add(1);
                    }
                }
            }
            removed.extend_from_slice(&old_edges[old_index..]);
            inserted.extend_from_slice(&reads.formula_edges[new_index..]);
            (old_edges.len(), removed, inserted, unchanged)
        };
        let identical_edge_sets = removed_edges.is_empty() && inserted_edges.is_empty();
        let compare_ns = compare_started.elapsed().as_nanos();

        let remove_started = Instant::now();
        for dependency in &removed_edges {
            self.current_edges.remove(&(vertex, *dependency));
            if let Some(readers) = self.reverse_runtime.get_mut(dependency) {
                readers.remove(&vertex);
                if readers.is_empty() {
                    self.reverse_runtime.remove(dependency);
                }
            }
        }
        let remove_ns = remove_started.elapsed().as_nanos();

        let insert_started = Instant::now();
        for dependency in &inserted_edges {
            self.current_edges.insert((vertex, *dependency));
            self.reverse_runtime
                .entry(*dependency)
                .or_default()
                .insert(vertex);
        }
        let insert_ns = insert_started.elapsed().as_nanos();

        let canonicalize_started = Instant::now();
        self.current_reads.insert(vertex, reads);
        let canonicalize_ns = canonicalize_started.elapsed().as_nanos();
        EdgeReplacementStats {
            old_edges: old_edges_len,
            new_edges: new_edges_len,
            edges_examined: old_edges_len.saturating_add(new_edges_len),
            removed: removed_edges.len(),
            inserted: inserted_edges.len(),
            unchanged,
            edge_sets_compared: 1,
            identical_edge_sets: usize::from(identical_edge_sets),
            changed_edge_sets: usize::from(!identical_edge_sets),
            reverse_buckets_touched: removed_edges.len().saturating_add(inserted_edges.len()),
            reverse_buckets_untouched: unchanged,
            reverse_buckets_mutated: removed_edges.len().saturating_add(inserted_edges.len()),
            full_replacement_fallback: false,
            compare_ns,
            remove_ns,
            insert_ns,
            canonicalize_ns,
        }
    }

    pub(crate) fn readers_of(&self, dependency: VertexId) -> Vec<VertexId> {
        self.reverse_runtime
            .get(&dependency)
            .map(|readers| readers.iter().copied().collect())
            .unwrap_or_default()
    }
}

#[derive(Clone, Debug)]
pub(crate) enum V2ScheduleUnit {
    Acyclic(Vec<VertexId>),
    Workspace(Vec<VertexId>),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct V2VirtualDemandAttribution {
    pub expansion_requests: usize,
    pub expansion_calls: usize,
    pub sources_with_edges: usize,
    pub unique_sources: usize,
    pub unique_targets: usize,
    pub range_source_lookups: usize,
    pub range_sources_with_dependencies: usize,
    pub range_dependency_records: usize,
    pub range_expansions: usize,
    pub dynamic_source_checks: usize,
    pub dynamic_expansion_calls: usize,
    pub sheet_identity_resolutions: usize,
    pub coordinates_examined: usize,
    pub vertex_grid_lookups: usize,
    pub formula_owner_graph_lookups: usize,
    pub raw_edges_emitted: usize,
    pub unique_source_target_pairs: usize,
    pub duplicate_source_target_pairs: usize,
    pub closure_membership_probes: usize,
    pub closure_new_targets: usize,
    pub stack_pushes: usize,
    pub temporary_vec_allocations: usize,
    pub temporary_map_allocations: usize,
    pub source_lookup_ns: u128,
    pub range_resolution_ns: u128,
    pub expansion_materialization_ns: u128,
    pub identity_conversion_ns: u128,
    pub target_lookup_filter_ns: u128,
    pub dynamic_evaluation_ns: u128,
    pub builder_dedup_ns: u128,
    pub builder_map_ns: u128,
    pub closure_source_lookup_ns: u128,
    pub closure_publish_ns: u128,
    pub closure_membership_ns: u128,
}

impl V2VirtualDemandAttribution {
    pub(crate) fn accumulate(&mut self, other: Self) {
        self.expansion_requests = self
            .expansion_requests
            .saturating_add(other.expansion_requests);
        self.expansion_calls = self.expansion_calls.saturating_add(other.expansion_calls);
        self.sources_with_edges = self
            .sources_with_edges
            .saturating_add(other.sources_with_edges);
        self.unique_sources = self.unique_sources.saturating_add(other.unique_sources);
        self.unique_targets = self.unique_targets.saturating_add(other.unique_targets);
        self.range_source_lookups = self
            .range_source_lookups
            .saturating_add(other.range_source_lookups);
        self.range_sources_with_dependencies = self
            .range_sources_with_dependencies
            .saturating_add(other.range_sources_with_dependencies);
        self.range_dependency_records = self
            .range_dependency_records
            .saturating_add(other.range_dependency_records);
        self.range_expansions = self.range_expansions.saturating_add(other.range_expansions);
        self.dynamic_source_checks = self
            .dynamic_source_checks
            .saturating_add(other.dynamic_source_checks);
        self.dynamic_expansion_calls = self
            .dynamic_expansion_calls
            .saturating_add(other.dynamic_expansion_calls);
        self.sheet_identity_resolutions = self
            .sheet_identity_resolutions
            .saturating_add(other.sheet_identity_resolutions);
        self.coordinates_examined = self
            .coordinates_examined
            .saturating_add(other.coordinates_examined);
        self.vertex_grid_lookups = self
            .vertex_grid_lookups
            .saturating_add(other.vertex_grid_lookups);
        self.formula_owner_graph_lookups = self
            .formula_owner_graph_lookups
            .saturating_add(other.formula_owner_graph_lookups);
        self.raw_edges_emitted = self
            .raw_edges_emitted
            .saturating_add(other.raw_edges_emitted);
        self.unique_source_target_pairs = self
            .unique_source_target_pairs
            .saturating_add(other.unique_source_target_pairs);
        self.duplicate_source_target_pairs = self
            .duplicate_source_target_pairs
            .saturating_add(other.duplicate_source_target_pairs);
        self.closure_membership_probes = self
            .closure_membership_probes
            .saturating_add(other.closure_membership_probes);
        self.closure_new_targets = self
            .closure_new_targets
            .saturating_add(other.closure_new_targets);
        self.stack_pushes = self.stack_pushes.saturating_add(other.stack_pushes);
        self.temporary_vec_allocations = self
            .temporary_vec_allocations
            .saturating_add(other.temporary_vec_allocations);
        self.temporary_map_allocations = self
            .temporary_map_allocations
            .saturating_add(other.temporary_map_allocations);
        self.source_lookup_ns = self.source_lookup_ns.saturating_add(other.source_lookup_ns);
        self.range_resolution_ns = self
            .range_resolution_ns
            .saturating_add(other.range_resolution_ns);
        self.expansion_materialization_ns = self
            .expansion_materialization_ns
            .saturating_add(other.expansion_materialization_ns);
        self.identity_conversion_ns = self
            .identity_conversion_ns
            .saturating_add(other.identity_conversion_ns);
        self.target_lookup_filter_ns = self
            .target_lookup_filter_ns
            .saturating_add(other.target_lookup_filter_ns);
        self.dynamic_evaluation_ns = self
            .dynamic_evaluation_ns
            .saturating_add(other.dynamic_evaluation_ns);
        self.builder_dedup_ns = self.builder_dedup_ns.saturating_add(other.builder_dedup_ns);
        self.builder_map_ns = self.builder_map_ns.saturating_add(other.builder_map_ns);
        self.closure_source_lookup_ns = self
            .closure_source_lookup_ns
            .saturating_add(other.closure_source_lookup_ns);
        self.closure_publish_ns = self
            .closure_publish_ns
            .saturating_add(other.closure_publish_ns);
        self.closure_membership_ns = self
            .closure_membership_ns
            .saturating_add(other.closure_membership_ns);
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct V2DemandStats {
    pub nodes_visited: usize,
    pub explicit_edges_visited: usize,
    pub virtual_edges_visited: usize,
    pub dedup_entries: usize,
    pub allocation_ns: u128,
    pub dependency_traversal_ns: u128,
    pub virtual_traversal_ns: u128,
    pub virtual_detail: V2VirtualDemandAttribution,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct V2RuntimeContractStats {
    pub candidates: usize,
    pub cache_hits: usize,
    pub cache_misses: usize,
    pub edges_skipped: usize,
    pub edges_examined: usize,
    pub certificates_invalidated: usize,
    pub invalidation_reasons: BTreeMap<String, usize>,
}

#[derive(Clone, Debug)]
pub(crate) struct V2DemandClosure {
    pub roots: Vec<VertexId>,
    pub vertices: Vec<VertexId>,
    pub virtual_dependencies: FxHashMap<VertexId, Vec<VertexId>>,
    pub topology_revision: u64,
    pub symbol_revision: u64,
    pub semantic_revision: u64,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct V2DemandClosureStats {
    pub closures_built: usize,
    pub reuse_hits: usize,
    pub reuse_rejections: usize,
    pub rejection_reasons: BTreeMap<String, usize>,
}

#[derive(Clone, Debug)]
pub(crate) struct V2ScheduleResult {
    pub units: Vec<V2ScheduleUnit>,
    pub demand_subgraph_ns: u128,
    pub construction_ns: u128,
    pub demand_reuse_consumption_ns: u128,
    pub demand_stats: V2DemandStats,
}

#[derive(Clone, Debug)]
pub(crate) struct V2VertexResult {
    pub value: LiteralValue,
    pub reads: ExactReadSet,
    pub formula_evaluation_ns: u128,
    pub exact_read_finalization_ns: u128,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum V2RequestKind {
    Full,
    Targeted,
}

#[derive(Clone, Debug)]
pub(crate) struct V2WorkspaceResult {
    pub stable_id: u64,
    pub stamped: usize,
    pub evaluated_vertices: usize,
    pub formula_evaluations: usize,
    pub solver_passes: usize,
    pub active_cyclic_members: usize,
    pub actual_cyclic_components: Vec<Vec<VertexId>>,
    pub pass_formula_evaluations: Vec<usize>,
    pub workspace_construction_ns: u128,
    pub iterative_solver_execution_ns: u128,
    pub exact_read_finalization_ns: u128,
    pub elapsed_ns: u128,
    pub stage2_used_exact_scc_kernel: bool,
    pub stage2_used_full_conservative_solver: bool,
    pub discovery_formula_evaluations: usize,
    pub exact_scc_formula_evaluations: usize,
    pub upstream_formula_evaluations: usize,
    pub downstream_formula_evaluations: usize,
    pub discovery_formula_evaluation_ns: u128,
    pub exact_scc_formula_evaluation_ns: u128,
    pub upstream_formula_evaluation_ns: u128,
    pub downstream_formula_evaluation_ns: u128,
    pub downstream_spill_effect_commit_ns: u128,
    pub repeated_non_feedback_evaluations: usize,
    pub repeated_non_feedback_evaluations_avoided: usize,
    pub exact_scc_rebuild_count: usize,
    pub exact_scc_expansion_count: usize,
    pub workspace_reopen_count: usize,
    pub retained_plan_runtime_invalidations: usize,
    pub retained_plan_reopens: usize,
    pub retained_plan_validation_ns: u128,
    pub retained_plan_runtime_invalidation_reason: Option<&'static str>,
    pub kernel_fallback_reason: Option<&'static str>,
    pub members: Vec<(VertexId, LiteralValue, ExactReadSet)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct V2WorkspaceDiagnostic {
    pub stable_id: u64,
    pub members: Vec<VertexId>,
    pub dirty_members: Vec<VertexId>,
    pub actual_cyclic_components: Vec<Vec<VertexId>>,
    pub pass_formula_evaluations: Vec<usize>,
    pub elapsed_ns: u128,
    pub classification: Option<V2WorkspaceClassification>,
}

#[derive(Clone, Debug)]
pub(crate) struct V2RunResult {
    pub computed_vertices: usize,
    pub cycle_errors: usize,
    pub elapsed: std::time::Duration,
}

pub(crate) trait V2Host {
    fn v2_begin_request(&mut self);
    fn v2_detailed_attribution_enabled(&self) -> bool;
    fn v2_set_formula_attribution_category(&mut self, category: V2FormulaAttributionCategory);
    fn v2_take_formula_attribution(&mut self) -> V2ExclusiveAttribution;
    fn v2_finish_request(&mut self, kind: V2RequestKind);
    fn v2_abort_request(&mut self);
    fn v2_dirty_vertices(&self) -> Vec<VertexId>;
    fn v2_user_dirty_roots(&self) -> Vec<VertexId>;
    fn v2_force_requested_roots(&self) -> bool;
    fn v2_exact_read_intersects_mutations(&self, reads: &ExactReadSet) -> bool;
    fn v2_exact_read_generation_valid_for_pruning(&self, reads: &ExactReadSet) -> bool;
    fn v2_runtime_contract_certificate_valid(&self, vertex: VertexId) -> bool;
    fn v2_is_volatile(&self, vertex: VertexId) -> bool;
    fn v2_admission_phase_timings(&self) -> (u128, u128);
    fn v2_take_demand_stats(&mut self) -> V2DemandStats;
    fn v2_take_demand_closure_stats(&mut self) -> V2DemandClosureStats;
    fn v2_begin_runtime_contract_validation(&mut self);
    fn v2_take_runtime_contract_stats(&mut self) -> V2RuntimeContractStats;
    fn v2_schedule(
        &mut self,
        roots: Option<&[VertexId]>,
        full_rebuild: bool,
    ) -> Result<V2ScheduleResult, ExcelError>;
    fn v2_formula_vertices(&self) -> Vec<VertexId>;
    fn v2_is_formula(&self, vertex: VertexId) -> bool;
    fn v2_is_dirty(&self, vertex: VertexId) -> bool;
    fn v2_current_value(&self, vertex: VertexId) -> LiteralValue;
    fn v2_evaluate_vertex(&mut self, vertex: VertexId) -> Result<V2VertexResult, ExcelError>;
    fn v2_runtime_reads_valid(&mut self, reads: &ExactReadSet) -> bool;
    fn v2_reference_observations_valid(&self, reads: &ExactReadSet) -> bool;
    fn v2_commit_vertex(
        &mut self,
        vertex: VertexId,
        value: &LiteralValue,
    ) -> Result<(), ExcelError>;
    fn v2_evaluate_workspace(
        &mut self,
        members: &[VertexId],
    ) -> Result<V2WorkspaceResult, ExcelError>;
    fn v2_evaluate_workspace_retained_plan(
        &mut self,
        members: &[VertexId],
        plan: &V2WorkspaceClassification,
        state: &V2State,
    ) -> Result<V2WorkspaceResult, ExcelError>;
    fn v2_clear_dirty(&mut self, vertices: &[VertexId]);
    fn v2_resource_checkpoint(&mut self, work: u64) -> Result<(), ExcelError>;
    fn v2_values_equal(&self, left: &LiteralValue, right: &LiteralValue) -> bool;
    fn v2_topology_revision(&self) -> u64;
    fn v2_symbol_revision(&self) -> u64;
    fn v2_semantic_revision(&self) -> u64;
    fn v2_cycle_config(&self) -> CycleConfig;
    fn v2_date_system(&self) -> DateSystem;
}

fn revisions_match<H: V2Host>(
    host: &H,
    state: &V2State,
    topology_revision: u64,
    symbol_revision: u64,
    semantic_revision: u64,
) -> bool {
    state.revisions_match(topology_revision, symbol_revision, semantic_revision)
        && host.v2_topology_revision() == topology_revision
        && host.v2_symbol_revision() == symbol_revision
        && host.v2_semantic_revision() == semantic_revision
}

fn revision_error() -> ExcelError {
    revision_error_with_message("Engine V2 validity changed during evaluation")
}

fn revision_error_with_message(message: &'static str) -> ExcelError {
    ExcelError::new(formualizer_common::ExcelErrorKind::NImpl).with_message(message)
}

fn runtime_contract_error() -> ExcelError {
    ExcelError::new(formualizer_common::ExcelErrorKind::NImpl)
        .with_message("Engine V2 runtime demand expanded into an unsupported formula")
}

fn exact_read_has_mandatory_effect(reads: &ExactReadSet) -> bool {
    reads.effects.iter().any(|effect| {
        matches!(
            effect,
            EffectKind::RecalcEpoch
                | EffectKind::Clock
                | EffectKind::Random
                | EffectKind::ExternalProvider
        )
    })
}

fn add_pruning_rejection(metrics: &mut V2Metrics, reason: &'static str) {
    *metrics
        .pruning_rejection_reasons
        .entry(reason.to_string())
        .or_default() += 1;
}

fn record_demand_stats(metrics: &mut V2Metrics, stats: V2DemandStats) {
    metrics.demand_nodes_visited = metrics
        .demand_nodes_visited
        .saturating_add(stats.nodes_visited);
    metrics.demand_explicit_edges_visited = metrics
        .demand_explicit_edges_visited
        .saturating_add(stats.explicit_edges_visited);
    metrics.demand_virtual_edges_visited = metrics
        .demand_virtual_edges_visited
        .saturating_add(stats.virtual_edges_visited);
    metrics.demand_dedup_entries = metrics
        .demand_dedup_entries
        .saturating_add(stats.dedup_entries);
    metrics.demand_allocation_ns = metrics
        .demand_allocation_ns
        .saturating_add(stats.allocation_ns);
    metrics.demand_dependency_traversal_ns = metrics
        .demand_dependency_traversal_ns
        .saturating_add(stats.dependency_traversal_ns);
    metrics.demand_virtual_traversal_ns = metrics
        .demand_virtual_traversal_ns
        .saturating_add(stats.virtual_traversal_ns);
    metrics.virtual_demand.accumulate(stats.virtual_detail);
}

fn record_demand_closure_stats(metrics: &mut V2Metrics, stats: V2DemandClosureStats) {
    metrics.demand_closures_built = stats.closures_built;
    metrics.demand_closure_reuse_hits = stats.reuse_hits;
    metrics.demand_closure_reuse_rejections = stats.reuse_rejections;
    metrics.demand_closure_reuse_rejection_reasons = stats.rejection_reasons;
}

fn record_runtime_contract_stats(metrics: &mut V2Metrics, stats: V2RuntimeContractStats) {
    metrics.runtime_contract_validation_candidates = stats.candidates;
    metrics.runtime_contract_validation_cache_hits = stats.cache_hits;
    metrics.runtime_contract_validation_cache_misses = stats.cache_misses;
    metrics.runtime_contract_edges_skipped = stats.edges_skipped;
    metrics.runtime_contract_edges_examined = stats.edges_examined;
    metrics.runtime_contract_certificates_invalidated = stats.certificates_invalidated;
    metrics.runtime_contract_certificate_invalidation_reasons = stats.invalidation_reasons;
}

fn record_validation_metadata(metrics: &mut V2Metrics, reads: &ExactReadSet) {
    metrics.validation_read_sets_examined = metrics.validation_read_sets_examined.saturating_add(1);
    metrics.validation_runtime_formula_edges_examined = metrics
        .validation_runtime_formula_edges_examined
        .saturating_add(reads.formula_edges.len());
    metrics.validation_reference_observations_examined = metrics
        .validation_reference_observations_examined
        .saturating_add(reads.reference_observations.len());
    metrics.validation_symbol_name_entries = metrics
        .validation_symbol_name_entries
        .saturating_add(reads.names.len());
    metrics.validation_symbol_name_unchanged = metrics
        .validation_symbol_name_unchanged
        .saturating_add(reads.names.len());
    metrics.validation_table_shape_entries = metrics
        .validation_table_shape_entries
        .saturating_add(reads.tables.len());
    metrics.validation_table_shape_unchanged = metrics
        .validation_table_shape_unchanged
        .saturating_add(reads.tables.len());
    let spill_entries = usize::from(reads.effects.contains(&EffectKind::SpillShape));
    metrics.validation_spill_shape_entries = metrics
        .validation_spill_shape_entries
        .saturating_add(spill_entries);
    metrics.validation_spill_shape_unchanged = metrics
        .validation_spill_shape_unchanged
        .saturating_add(spill_entries);
    let provider_entries = reads.external.len().saturating_add(usize::from(
        reads.effects.contains(&EffectKind::ExternalProvider),
    ));
    metrics.validation_provider_effect_entries = metrics
        .validation_provider_effect_entries
        .saturating_add(provider_entries);
    metrics.validation_provider_effect_unchanged = metrics
        .validation_provider_effect_unchanged
        .saturating_add(provider_entries);
    metrics.validation_selected_reference_entries = metrics
        .validation_selected_reference_entries
        .saturating_add(reads.selected_targets.len());
    metrics.validation_selected_reference_unchanged = metrics
        .validation_selected_reference_unchanged
        .saturating_add(reads.selected_targets.len());
    let range_entries = reads
        .ranges
        .len()
        .saturating_add(reads.reference_observations.len());
    metrics.validation_range_reference_entries = metrics
        .validation_range_reference_entries
        .saturating_add(range_entries);
    metrics.validation_range_reference_unchanged = metrics
        .validation_range_reference_unchanged
        .saturating_add(range_entries);
}

fn classify_workspace_plan<H: V2Host>(
    host: &H,
    state: &V2State,
    members: &[VertexId],
    prior_profiles: &[V2WorkspaceDiagnostic],
    contract_validity: &mut Vec<u8>,
) -> V2WorkspaceClassification {
    let prior = prior_profiles
        .iter()
        .find(|profile| profile.members.as_slice() == members);
    let stable_id = prior.map_or(0, |profile| profile.stable_id);
    let retained_topology = prior
        .and_then(|profile| profile.classification.as_ref())
        .filter(|classification| {
            classification.retained_plan_valid
                && classification.exact_scc_components
                    == prior
                        .map(|profile| profile.actual_cyclic_components.clone())
                        .unwrap_or_default()
        });
    let local_members = members.iter().copied().collect::<BTreeSet<_>>();
    let exact_scc_components = prior
        .map(|profile| profile.actual_cyclic_components.clone())
        .unwrap_or_default();
    let exact_scc = exact_scc_components
        .iter()
        .flatten()
        .copied()
        .collect::<BTreeSet<_>>();
    let (upstream, downstream, upstream_order, downstream_order, unrelated) =
        if let Some(classification) = retained_topology {
            (
                classification.upstream_order.iter().copied().collect(),
                classification.downstream_order.iter().copied().collect(),
                classification.upstream_order.clone(),
                classification.downstream_order.clone(),
                classification.unrelated_conservative_members,
            )
        } else {
            let mut upstream = BTreeSet::new();
            let mut upstream_queue = exact_scc.iter().copied().collect::<VecDeque<_>>();
            while let Some(reader) = upstream_queue.pop_front() {
                let Some(reads) = state.current_reads.get(&reader) else {
                    continue;
                };
                for dependency in &reads.formula_edges {
                    if local_members.contains(dependency)
                        && !exact_scc.contains(dependency)
                        && upstream.insert(*dependency)
                    {
                        upstream_queue.push_back(*dependency);
                    }
                }
            }
            let mut reverse = BTreeMap::<VertexId, Vec<VertexId>>::new();
            for reader in members {
                if let Some(reads) = state.current_reads.get(reader) {
                    for dependency in &reads.formula_edges {
                        if local_members.contains(dependency) {
                            reverse.entry(*dependency).or_default().push(*reader);
                        }
                    }
                }
            }
            let mut downstream = BTreeSet::new();
            let mut downstream_queue = exact_scc.iter().copied().collect::<VecDeque<_>>();
            while let Some(dependency) = downstream_queue.pop_front() {
                for reader in reverse.get(&dependency).into_iter().flatten() {
                    if !exact_scc.contains(reader) && downstream.insert(*reader) {
                        downstream_queue.push_back(*reader);
                    }
                }
            }
            let topological_order = |members_to_order: &BTreeSet<VertexId>| {
                let mut indegree = BTreeMap::<VertexId, usize>::new();
                let mut readers = BTreeMap::<VertexId, Vec<VertexId>>::new();
                for member in members_to_order {
                    indegree.insert(*member, 0);
                }
                for member in members_to_order {
                    if let Some(reads) = state.current_reads.get(member) {
                        for dependency in &reads.formula_edges {
                            if members_to_order.contains(dependency) {
                                *indegree.entry(*member).or_default() += 1;
                                readers.entry(*dependency).or_default().push(*member);
                            }
                        }
                    }
                }
                let mut ready = indegree
                    .iter()
                    .filter_map(|(member, degree)| (*degree == 0).then_some(*member))
                    .collect::<BTreeSet<_>>();
                let mut ordered = Vec::with_capacity(members_to_order.len());
                while let Some(member) = ready.pop_first() {
                    ordered.push(member);
                    for reader in readers.get(&member).into_iter().flatten() {
                        if let Some(degree) = indegree.get_mut(reader) {
                            *degree = degree.saturating_sub(1);
                            if *degree == 0 {
                                ready.insert(*reader);
                            }
                        }
                    }
                }
                for member in members_to_order {
                    if !ordered.contains(member) {
                        ordered.push(*member);
                    }
                }
                ordered
            };
            let upstream_order = topological_order(&upstream);
            let downstream_order = topological_order(&downstream);
            let unrelated = local_members
                .difference(&exact_scc)
                .filter(|member| !upstream.contains(member) && !downstream.contains(member))
                .count();
            (
                upstream,
                downstream,
                upstream_order,
                downstream_order,
                unrelated,
            )
        };
    let mut stage1_effective_dirty_members = 0usize;
    let mut stage1_clean_members = 0usize;
    let mut dirty_upstream_members = 0usize;
    let mut clean_upstream_members = 0usize;
    let mut exact_read_state_missing = 0usize;
    let mut contract_certificate_valid = 0usize;
    let mut contract_certificate_missing = 0usize;
    let mut topology_sensitive_members = 0usize;
    let mut generation_revision_invalid_members = 0usize;
    let mut cached_value_reusable_members = 0usize;
    for member in members {
        let dirty = state.last_effective_dirty.contains(member);
        if dirty {
            stage1_effective_dirty_members = stage1_effective_dirty_members.saturating_add(1);
        } else {
            stage1_clean_members = stage1_clean_members.saturating_add(1);
        }
        if upstream.contains(member) {
            if dirty {
                dirty_upstream_members = dirty_upstream_members.saturating_add(1);
            } else {
                clean_upstream_members = clean_upstream_members.saturating_add(1);
            }
        }
        let Some(reads) = state.current_reads.get(member) else {
            exact_read_state_missing = exact_read_state_missing.saturating_add(1);
            continue;
        };
        let contract_valid = reads.formula_edges.iter().all(|dependency| {
            let index = dependency.0 as usize;
            if contract_validity.len() <= index {
                contract_validity.resize(index.saturating_add(1), 0);
            }
            match contract_validity[index] {
                1 => true,
                2 => false,
                _ => {
                    let valid = host.v2_runtime_contract_certificate_valid(*dependency);
                    contract_validity[index] = if valid { 1 } else { 2 };
                    valid
                }
            }
        });
        if contract_valid {
            contract_certificate_valid = contract_certificate_valid.saturating_add(1);
        } else {
            contract_certificate_missing = contract_certificate_missing.saturating_add(1);
        }
        let topology_sensitive = !reads.reference_observations.is_empty()
            || !reads.selected_targets.is_empty()
            || !reads.names.is_empty()
            || !reads.tables.is_empty()
            || reads.effects.iter().any(|effect| {
                matches!(
                    effect,
                    EffectKind::DynamicSelector
                        | EffectKind::DynamicTarget
                        | EffectKind::SpillShape
                        | EffectKind::TableShape
                        | EffectKind::StructuralGeneration
                )
            });
        if topology_sensitive {
            topology_sensitive_members = topology_sensitive_members.saturating_add(1);
        }
        if topology_sensitive && !host.v2_exact_read_generation_valid_for_pruning(reads) {
            generation_revision_invalid_members =
                generation_revision_invalid_members.saturating_add(1);
        }
        if upstream.contains(member)
            && !dirty
            && contract_valid
            && !(topology_sensitive && !host.v2_exact_read_generation_valid_for_pruning(reads))
        {
            cached_value_reusable_members = cached_value_reusable_members.saturating_add(1);
        }
    }
    let retained_plan_candidate = prior.is_some() && !exact_scc.is_empty();
    let mut retained_plan_valid = retained_plan_candidate;
    let mut rejection_reason = None;
    if !retained_plan_candidate {
        retained_plan_valid = false;
        rejection_reason = Some("missing_retained_workspace_plan");
    } else if state.needs_full_rebuild {
        retained_plan_valid = false;
        rejection_reason = Some("exact_state_requires_rebuild");
    } else if exact_read_state_missing != 0 {
        retained_plan_valid = false;
        rejection_reason = Some("exact_read_state_missing");
    } else if contract_certificate_missing != 0 {
        retained_plan_valid = false;
        rejection_reason = Some("contract_certificate_missing");
    } else if generation_revision_invalid_members != 0 {
        retained_plan_valid = false;
        rejection_reason = Some("generation_revision_invalid");
    } else if members.iter().any(|member| {
        state.last_effective_dirty.contains(member)
            && state.current_reads.get(member).is_some_and(|reads| {
                (!reads.reference_observations.is_empty()
                    || !reads.selected_targets.is_empty()
                    || reads.effects.iter().any(|effect| {
                        matches!(
                            effect,
                            EffectKind::DynamicSelector | EffectKind::DynamicTarget
                        )
                    }))
                    && host.v2_exact_read_intersects_mutations(reads)
            })
    }) {
        retained_plan_valid = false;
        rejection_reason = Some("topology_sensitive_dirty_member");
    }
    V2WorkspaceClassification {
        stable_id,
        conservative_members: members.len(),
        stage1_effective_dirty_members,
        stage1_clean_members,
        dirty_upstream_members,
        clean_upstream_members,
        exact_scc_members: exact_scc.len(),
        upstream_members: upstream.len(),
        downstream_members: downstream.len(),
        exact_scc_components,
        upstream_order,
        downstream_order,
        unrelated_conservative_members: unrelated,
        exact_read_state_missing,
        contract_certificate_valid,
        contract_certificate_missing,
        topology_sensitive_members,
        generation_revision_invalid_members,
        cached_value_reusable_members,
        retained_plan_candidate,
        retained_plan_valid,
        retained_plan_rejection_reason: rejection_reason,
    }
}

fn record_workspace_classification(
    metrics: &mut V2Metrics,
    classification: &V2WorkspaceClassification,
) {
    metrics.workspace_retained_plan_candidates =
        metrics.workspace_retained_plan_candidates.saturating_add(1);
    if classification.retained_plan_valid {
        metrics.workspace_retained_plan_hits =
            metrics.workspace_retained_plan_hits.saturating_add(1);
        metrics.discovery_evaluations_avoided =
            metrics.discovery_evaluations_avoided.saturating_add(
                classification
                    .conservative_members
                    .saturating_sub(classification.upstream_members),
            );
        metrics.dirty_upstream_evaluations = metrics
            .dirty_upstream_evaluations
            .saturating_add(classification.dirty_upstream_members);
        metrics.clean_upstream_cache_reuses = metrics
            .clean_upstream_cache_reuses
            .saturating_add(classification.cached_value_reusable_members);
        metrics.scc_discovery_evaluations_avoided = metrics
            .scc_discovery_evaluations_avoided
            .saturating_add(classification.exact_scc_members);
        metrics.downstream_discovery_evaluations_avoided = metrics
            .downstream_discovery_evaluations_avoided
            .saturating_add(classification.downstream_members);
    } else {
        metrics.workspace_retained_plan_rejections =
            metrics.workspace_retained_plan_rejections.saturating_add(1);
        if let Some(reason) = classification.retained_plan_rejection_reason {
            *metrics
                .workspace_retained_plan_rejection_reasons
                .entry(reason.to_string())
                .or_default() += 1;
        }
    }
    metrics.retained_classification_effective_dirty_members = metrics
        .retained_classification_effective_dirty_members
        .saturating_add(classification.stage1_effective_dirty_members);
    metrics.retained_classification_clean_members = metrics
        .retained_classification_clean_members
        .saturating_add(classification.stage1_clean_members);
    metrics.retained_classification_exact_scc_members = metrics
        .retained_classification_exact_scc_members
        .saturating_add(classification.exact_scc_members);
    metrics.retained_classification_upstream_members = metrics
        .retained_classification_upstream_members
        .saturating_add(classification.upstream_members);
    metrics.retained_classification_downstream_members = metrics
        .retained_classification_downstream_members
        .saturating_add(classification.downstream_members);
    metrics.retained_classification_unrelated_members = metrics
        .retained_classification_unrelated_members
        .saturating_add(classification.unrelated_conservative_members);
    metrics.retained_classification_missing_reads = metrics
        .retained_classification_missing_reads
        .saturating_add(classification.exact_read_state_missing);
    metrics.retained_classification_invalid_members = metrics
        .retained_classification_invalid_members
        .saturating_add(classification.generation_revision_invalid_members)
        .saturating_add(classification.contract_certificate_missing);
    metrics.retained_classification_reusable_values = metrics
        .retained_classification_reusable_values
        .saturating_add(classification.cached_value_reusable_members);
}

fn prune_schedule<H: V2Host>(
    host: &H,
    state: &mut V2State,
    units: Vec<V2ScheduleUnit>,
    roots: Option<&[VertexId]>,
) -> Vec<V2ScheduleUnit> {
    let conservative_formulas = units
        .iter()
        .flat_map(|unit| match unit {
            V2ScheduleUnit::Acyclic(vertices) | V2ScheduleUnit::Workspace(vertices) => {
                vertices.iter().copied()
            }
        })
        .collect::<BTreeSet<_>>();
    let conservative_dirty = conservative_formulas
        .iter()
        .copied()
        .filter(|vertex| host.v2_is_dirty(*vertex))
        .collect::<BTreeSet<_>>();
    state.last_effective_dirty.clear();
    state.metrics.conservative_dirty_formula_count = conservative_dirty.len();
    state.metrics.conservative_workspace_candidate_count = units
        .iter()
        .filter(|unit| matches!(unit, V2ScheduleUnit::Workspace(_)))
        .count();

    if roots.is_none() {
        return units;
    }

    if state.needs_full_rebuild || state.current_reads.is_empty() {
        add_pruning_rejection(&mut state.metrics, "no_retained_exact_state");
        state.metrics.exact_pruning_rejected_count = 1;
        state
            .last_effective_dirty
            .extend(conservative_dirty.iter().copied());
        state.metrics.effective_dirty_formula_count = conservative_dirty.len();
        state.metrics.pruned_dirty_formula_count = 0;
        state.metrics.effective_workspace_count =
            state.metrics.conservative_workspace_candidate_count;
        return units;
    }

    let user_dirty_roots = host.v2_user_dirty_roots();
    let mut invalid_vertices = BTreeSet::new();
    let mut missing_exact_state = false;
    let mut rejected_formula_count = 0usize;
    for vertex in &conservative_dirty {
        let Some(reads) = state.current_reads.get(vertex) else {
            add_pruning_rejection(&mut state.metrics, "no_retained_exact_state");
            missing_exact_state = true;
            rejected_formula_count = rejected_formula_count.saturating_add(1);
            continue;
        };
        if !host.v2_exact_read_generation_valid_for_pruning(reads) {
            add_pruning_rejection(&mut state.metrics, "generation_invalid");
            invalid_vertices.insert(*vertex);
            rejected_formula_count = rejected_formula_count.saturating_add(1);
            continue;
        }
        if !reads.names.is_empty()
            || !reads.tables.is_empty()
            || !reads.external.is_empty()
            || reads.effects.contains(&EffectKind::StructuralGeneration)
        {
            add_pruning_rejection(&mut state.metrics, "incomplete_observation");
            invalid_vertices.insert(*vertex);
            rejected_formula_count = rejected_formula_count.saturating_add(1);
        }
    }

    if missing_exact_state {
        state.metrics.exact_pruning_rejected_count = 1;
        state
            .last_effective_dirty
            .extend(conservative_dirty.iter().copied());
        state.metrics.effective_dirty_formula_count = conservative_dirty.len();
        state.metrics.pruned_dirty_formula_count = 0;
        state.metrics.effective_workspace_count =
            state.metrics.conservative_workspace_candidate_count;
        return units;
    }

    let mut effective = invalid_vertices.clone();
    let mut propagation_queue = invalid_vertices.into_iter().collect::<VecDeque<_>>();
    if host.v2_force_requested_roots() {
        for root in roots.into_iter().flatten().copied() {
            if host.v2_is_formula(root) && effective.insert(root) {
                propagation_queue.push_back(root);
            }
        }
    }
    for root in user_dirty_roots {
        if host.v2_is_formula(root) && effective.insert(root) {
            propagation_queue.push_back(root);
        }
    }
    for (reader, reads) in &state.current_reads {
        if !conservative_formulas.contains(reader) {
            continue;
        }
        let mutation_hit = host.v2_exact_read_intersects_mutations(reads);
        let mandatory_seed = host.v2_is_volatile(*reader) || exact_read_has_mandatory_effect(reads);
        if mutation_hit || mandatory_seed {
            if mutation_hit {
                state.metrics.exact_reverse_read_formulas_reached = state
                    .metrics
                    .exact_reverse_read_formulas_reached
                    .saturating_add(1);
            }
            if effective.insert(*reader) {
                propagation_queue.push_back(*reader);
            }
        }
    }

    let mut visited = BTreeSet::new();
    let mut exact_edge_formulas_reached = BTreeSet::new();
    while let Some(dependency) = propagation_queue.pop_front() {
        if !visited.insert(dependency) {
            continue;
        }
        state.metrics.exact_reverse_propagation_vertices_visited = state
            .metrics
            .exact_reverse_propagation_vertices_visited
            .saturating_add(1);
        for reader in state
            .reverse_runtime
            .get(&dependency)
            .into_iter()
            .flatten()
            .copied()
        {
            exact_edge_formulas_reached.insert(reader);
            if effective.insert(reader) {
                propagation_queue.push_back(reader);
            }
        }
    }

    state.metrics.exact_formula_edge_formulas_reached = exact_edge_formulas_reached.len();
    state.last_effective_dirty.extend(effective.iter().copied());
    state.metrics.exact_pruning_accepted_count = 1;
    state.metrics.exact_pruning_rejected_count = usize::from(rejected_formula_count > 0);
    state.metrics.effective_dirty_formula_count =
        effective.intersection(&conservative_formulas).count();
    state.metrics.pruned_dirty_formula_count = conservative_dirty.difference(&effective).count();

    let mut effective_workspace_count = 0usize;
    let filtered = units
        .into_iter()
        .filter_map(|unit| match unit {
            V2ScheduleUnit::Acyclic(vertices) => {
                let retained = vertices
                    .into_iter()
                    .filter(|vertex| effective.contains(vertex))
                    .collect::<Vec<_>>();
                (!retained.is_empty()).then_some(V2ScheduleUnit::Acyclic(retained))
            }
            V2ScheduleUnit::Workspace(vertices) => {
                if vertices.iter().any(|vertex| effective.contains(vertex)) {
                    effective_workspace_count = effective_workspace_count.saturating_add(1);
                    Some(V2ScheduleUnit::Workspace(vertices))
                } else {
                    None
                }
            }
        })
        .collect::<Vec<_>>();
    state.metrics.effective_workspace_count = effective_workspace_count;
    state.metrics.pruned_workspace_count = state
        .metrics
        .conservative_workspace_candidate_count
        .saturating_sub(effective_workspace_count);
    filtered
}

pub(crate) fn run<H: V2Host>(
    host: &mut H,
    state: &mut V2State,
    roots: Option<&[VertexId]>,
) -> Result<V2RunResult, ExcelError> {
    let started = Instant::now();
    let request_kind = if roots.is_some() {
        V2RequestKind::Targeted
    } else {
        V2RequestKind::Full
    };
    let topology_revision = host.v2_topology_revision();
    let symbol_revision = host.v2_symbol_revision();
    let semantic_revision = host.v2_semantic_revision();
    let prior_workspace_profiles = state.metrics.workspace_profiles.clone();
    let retained_scan_started = Instant::now();
    let retained_scan_read_sets = state.current_reads.len();
    state.synchronize_revisions(topology_revision, symbol_revision, semantic_revision);
    let formulas = host.v2_formula_vertices();
    state.synchronize_vertices(&formulas);
    let retained_scan_ns = retained_scan_started.elapsed().as_nanos();
    state.metrics = V2Metrics::default();
    state.metrics.retained_state_scan_ns = retained_scan_ns;
    state.metrics.exclusive_attribution.retained_state_scan_ns = retained_scan_ns;
    state.metrics.retained_state_scan_read_sets = retained_scan_read_sets;
    state.metrics.retained_state_scan_edges = 0;
    let demand_scheduling_started = Instant::now();
    let admission_demand_stats = host.v2_take_demand_stats();
    state.metrics.admission_demand_nodes_visited = admission_demand_stats.nodes_visited;
    state.metrics.admission_demand_explicit_edges_visited =
        admission_demand_stats.explicit_edges_visited;
    state.metrics.admission_demand_virtual_edges_visited =
        admission_demand_stats.virtual_edges_visited;
    state.metrics.admission_demand_allocation_ns = admission_demand_stats.allocation_ns;
    state.metrics.admission_demand_dependency_traversal_ns =
        admission_demand_stats.dependency_traversal_ns;
    state.metrics.admission_demand_virtual_traversal_ns =
        admission_demand_stats.virtual_traversal_ns;
    record_demand_stats(&mut state.metrics, admission_demand_stats);
    let (scoped_admission_ns, admission_demand_ns) = host.v2_admission_phase_timings();
    state.metrics.scoped_admission_ns = scoped_admission_ns;
    state.metrics.demand_subgraph_ns = state
        .metrics
        .demand_subgraph_ns
        .saturating_add(admission_demand_ns);
    let dirty_started = Instant::now();
    let dirty_candidates = host.v2_dirty_vertices();
    state.metrics.dirty_candidates = dirty_candidates.len();
    state.metrics.dirty_seed_selection_ns = dirty_started.elapsed().as_nanos();
    host.v2_resource_checkpoint(0)?;
    let schedule = host.v2_schedule(roots, state.needs_full_rebuild)?;
    let demand_closure_stats = host.v2_take_demand_closure_stats();
    record_demand_closure_stats(&mut state.metrics, demand_closure_stats);
    state.metrics.demand_reuse_consumption_ns = schedule.demand_reuse_consumption_ns;
    state.metrics.schedule_demand_nodes_visited = schedule.demand_stats.nodes_visited;
    state.metrics.schedule_demand_explicit_edges_visited =
        schedule.demand_stats.explicit_edges_visited;
    state.metrics.schedule_demand_virtual_edges_visited =
        schedule.demand_stats.virtual_edges_visited;
    state.metrics.schedule_demand_allocation_ns = schedule.demand_stats.allocation_ns;
    state.metrics.schedule_demand_dependency_traversal_ns =
        schedule.demand_stats.dependency_traversal_ns;
    state.metrics.schedule_demand_virtual_traversal_ns = schedule.demand_stats.virtual_traversal_ns;
    record_demand_stats(&mut state.metrics, schedule.demand_stats);
    state.metrics.schedule_demand_subgraph_ns = schedule.demand_subgraph_ns;
    state.metrics.demand_subgraph_ns = state
        .metrics
        .demand_subgraph_ns
        .saturating_add(schedule.demand_subgraph_ns);
    state.metrics.schedule_construction_ns = schedule.construction_ns;
    state.metrics.schedule_ns = schedule.construction_ns;
    let schedule_units = prune_schedule(host, state, schedule.units, roots);
    state.metrics.exclusive_attribution.demand_scheduling_ns =
        demand_scheduling_started.elapsed().as_nanos();
    state.metrics.schedule_units = schedule_units.len();
    state.metrics.workspace_units = schedule_units
        .iter()
        .filter(|unit| matches!(unit, V2ScheduleUnit::Workspace(_)))
        .count();
    state.metrics.dirty_roots = schedule_units
        .iter()
        .map(|unit| match unit {
            V2ScheduleUnit::Acyclic(vertices) | V2ScheduleUnit::Workspace(vertices) => {
                vertices.len()
            }
        })
        .sum();
    let mut workspace_plan_classifications = BTreeMap::new();
    let retained_plan_validation_started = Instant::now();
    let mut retained_contract_validity = Vec::new();
    if roots.is_some() {
        for unit in &schedule_units {
            if let V2ScheduleUnit::Workspace(members) = unit {
                let classification = classify_workspace_plan(
                    host,
                    state,
                    members,
                    &prior_workspace_profiles,
                    &mut retained_contract_validity,
                );
                record_workspace_classification(&mut state.metrics, &classification);
                workspace_plan_classifications.insert(members.clone(), classification);
            }
        }
    }
    state
        .metrics
        .exclusive_attribution
        .retained_plan_validation_ns = retained_plan_validation_started.elapsed().as_nanos();
    let mut evaluated = BTreeSet::new();
    let mut computed_vertices = 0usize;
    let mut cycle_errors = 0usize;

    host.v2_begin_request();
    host.v2_begin_runtime_contract_validation();
    let detailed_read_set_metrics = host.v2_detailed_attribution_enabled();
    let result = (|| {
        for unit in schedule_units {
            match unit {
                V2ScheduleUnit::Acyclic(vertices) => {
                    for vertex in vertices {
                        if !host.v2_is_formula(vertex) {
                            continue;
                        }
                        state.metrics.queue_steps = state.metrics.queue_steps.saturating_add(1);
                        let formula_started = Instant::now();
                        host.v2_resource_checkpoint(1)?;
                        host.v2_set_formula_attribution_category(
                            V2FormulaAttributionCategory::OutsideWorkspace,
                        );
                        let evaluated_vertex = host.v2_evaluate_vertex(vertex)?;
                        state.metrics.outside_acyclic_formula_evaluations = state
                            .metrics
                            .outside_acyclic_formula_evaluations
                            .saturating_add(1);
                        state.metrics.outside_acyclic_formula_evaluation_ns = state
                            .metrics
                            .outside_acyclic_formula_evaluation_ns
                            .saturating_add(evaluated_vertex.formula_evaluation_ns);
                        state.metrics.acyclic_formula_evaluation_ns = state
                            .metrics
                            .acyclic_formula_evaluation_ns
                            .saturating_add(evaluated_vertex.formula_evaluation_ns);
                        state.metrics.exact_read_finalization_ns = state
                            .metrics
                            .exact_read_finalization_ns
                            .saturating_add(evaluated_vertex.exact_read_finalization_ns);
                        let value = evaluated_vertex.value;
                        let reads = evaluated_vertex.reads;
                        let validation_started = Instant::now();
                        let metadata_started = Instant::now();
                        record_validation_metadata(&mut state.metrics, &reads);
                        state.metrics.validation_metadata_ns = state
                            .metrics
                            .validation_metadata_ns
                            .saturating_add(metadata_started.elapsed().as_nanos());
                        let runtime_started = Instant::now();
                        let runtime_valid = host.v2_runtime_reads_valid(&reads);
                        let runtime_ns = runtime_started.elapsed().as_nanos();
                        state.metrics.validation_runtime_formula_ns = state
                            .metrics
                            .validation_runtime_formula_ns
                            .saturating_add(runtime_ns);
                        if runtime_valid {
                            state.metrics.validation_runtime_formula_edges_unchanged = state
                                .metrics
                                .validation_runtime_formula_edges_unchanged
                                .saturating_add(reads.formula_edges.len());
                        } else {
                            state.metrics.validation_runtime_formula_edges_invalidated = state
                                .metrics
                                .validation_runtime_formula_edges_invalidated
                                .saturating_add(reads.formula_edges.len());
                            return Err(runtime_contract_error());
                        }
                        let reference_started = Instant::now();
                        let reference_valid = host.v2_reference_observations_valid(&reads);
                        state.metrics.validation_reference_ns = state
                            .metrics
                            .validation_reference_ns
                            .saturating_add(reference_started.elapsed().as_nanos());
                        if reference_valid {
                            state.metrics.validation_reference_observations_unchanged = state
                                .metrics
                                .validation_reference_observations_unchanged
                                .saturating_add(reads.reference_observations.len());
                        } else {
                            state.metrics.validation_reference_observations_invalidated = state
                                .metrics
                                .validation_reference_observations_invalidated
                                .saturating_add(reads.reference_observations.len());
                            return Err(revision_error());
                        }
                        let topology_started = Instant::now();
                        let topology_valid = revisions_match(
                            host,
                            state,
                            topology_revision,
                            symbol_revision,
                            semantic_revision,
                        );
                        state.metrics.validation_topology_checks =
                            state.metrics.validation_topology_checks.saturating_add(1);
                        state.metrics.validation_topology_ns = state
                            .metrics
                            .validation_topology_ns
                            .saturating_add(topology_started.elapsed().as_nanos());
                        if !topology_valid {
                            state.metrics.validation_topology_invalidated = state
                                .metrics
                                .validation_topology_invalidated
                                .saturating_add(1);
                            return Err(revision_error());
                        }
                        state.metrics.generation_reference_validation_ns = state
                            .metrics
                            .generation_reference_validation_ns
                            .saturating_add(validation_started.elapsed().as_nanos());
                        state.metrics.exclusive_attribution.contract_validation_ns = state
                            .metrics
                            .exclusive_attribution
                            .contract_validation_ns
                            .saturating_add(validation_started.elapsed().as_nanos());
                        let commit_started = Instant::now();
                        host.v2_commit_vertex(vertex, &value)?;
                        state.metrics.spill_effect_commit_ns = state
                            .metrics
                            .spill_effect_commit_ns
                            .saturating_add(commit_started.elapsed().as_nanos());
                        let post_commit_validation_started = Instant::now();
                        let post_commit_topology_valid = revisions_match(
                            host,
                            state,
                            topology_revision,
                            symbol_revision,
                            semantic_revision,
                        );
                        state.metrics.validation_topology_checks =
                            state.metrics.validation_topology_checks.saturating_add(1);
                        state.metrics.validation_topology_ns = state
                            .metrics
                            .validation_topology_ns
                            .saturating_add(post_commit_validation_started.elapsed().as_nanos());
                        if !post_commit_topology_valid {
                            state.metrics.validation_topology_invalidated = state
                                .metrics
                                .validation_topology_invalidated
                                .saturating_add(1);
                            return Err(revision_error());
                        }
                        state.metrics.generation_reference_validation_ns = state
                            .metrics
                            .generation_reference_validation_ns
                            .saturating_add(post_commit_validation_started.elapsed().as_nanos());
                        state.metrics.runtime_formula_edge_events = state
                            .metrics
                            .runtime_formula_edge_events
                            .saturating_add(reads.formula_edge_events);
                        state.metrics.runtime_formula_edges_processed = state
                            .metrics
                            .runtime_formula_edges_processed
                            .saturating_add(reads.formula_edges.len());
                        state.metrics.logical_range_positions = state
                            .metrics
                            .logical_range_positions
                            .saturating_add(reads.logical_range_positions);
                        state.metrics.physical_cells_fetched = state
                            .metrics
                            .physical_cells_fetched
                            .saturating_add(reads.physical_cells_fetched);
                        state.metrics.diagnostic_records_retained = state
                            .metrics
                            .diagnostic_records_retained
                            .saturating_add(reads.diagnostic_records_retained);
                        let read_set_compare_started = detailed_read_set_metrics.then(Instant::now);
                        let read_set_changed = detailed_read_set_metrics.then(|| {
                            state
                                .current_reads
                                .get(&vertex)
                                .is_none_or(|previous| previous != &reads)
                        });
                        if let Some(started) = read_set_compare_started {
                            state.metrics.diagnostic_read_set_compare_ns = state
                                .metrics
                                .diagnostic_read_set_compare_ns
                                .saturating_add(started.elapsed().as_nanos());
                        }
                        let edge_stats = state.replace_read_set_with_stats(vertex, reads);
                        state.metrics.exclusive_attribution.adjacency_replacement_ns = state
                            .metrics
                            .exclusive_attribution
                            .adjacency_replacement_ns
                            .saturating_add(
                                edge_stats
                                    .compare_ns
                                    .saturating_add(edge_stats.remove_ns)
                                    .saturating_add(edge_stats.insert_ns)
                                    .saturating_add(edge_stats.canonicalize_ns),
                            );
                        state.metrics.exact_read_sets_finalized =
                            state.metrics.exact_read_sets_finalized.saturating_add(1);
                        if let Some(read_set_changed) = read_set_changed {
                            if read_set_changed {
                                state.metrics.exact_read_sets_changed =
                                    state.metrics.exact_read_sets_changed.saturating_add(1);
                            } else {
                                state.metrics.exact_read_sets_unchanged =
                                    state.metrics.exact_read_sets_unchanged.saturating_add(1);
                            }
                        }
                        state.metrics.exact_edges_examined = state
                            .metrics
                            .exact_edges_examined
                            .saturating_add(edge_stats.edges_examined);
                        state.metrics.exact_edges_removed = state
                            .metrics
                            .exact_edges_removed
                            .saturating_add(edge_stats.removed);
                        state.metrics.exact_edges_inserted = state
                            .metrics
                            .exact_edges_inserted
                            .saturating_add(edge_stats.inserted);
                        state.metrics.exact_edges_unchanged = state
                            .metrics
                            .exact_edges_unchanged
                            .saturating_add(edge_stats.unchanged);
                        state.metrics.exact_edge_sets_compared = state
                            .metrics
                            .exact_edge_sets_compared
                            .saturating_add(edge_stats.edge_sets_compared);
                        state.metrics.exact_identical_edge_sets = state
                            .metrics
                            .exact_identical_edge_sets
                            .saturating_add(edge_stats.identical_edge_sets);
                        state.metrics.exact_changed_edge_sets = state
                            .metrics
                            .exact_changed_edge_sets
                            .saturating_add(edge_stats.changed_edge_sets);
                        state.metrics.exact_reverse_buckets_untouched = state
                            .metrics
                            .exact_reverse_buckets_untouched
                            .saturating_add(edge_stats.reverse_buckets_untouched);
                        state.metrics.exact_reverse_buckets_mutated = state
                            .metrics
                            .exact_reverse_buckets_mutated
                            .saturating_add(edge_stats.reverse_buckets_mutated);
                        if edge_stats.full_replacement_fallback {
                            state.metrics.exact_full_replacement_fallback_count = state
                                .metrics
                                .exact_full_replacement_fallback_count
                                .saturating_add(1);
                        }
                        state.metrics.reverse_buckets_touched = state
                            .metrics
                            .reverse_buckets_touched
                            .saturating_add(edge_stats.reverse_buckets_touched);
                        state.metrics.exact_edge_compare_ns = state
                            .metrics
                            .exact_edge_compare_ns
                            .saturating_add(edge_stats.compare_ns);
                        state.metrics.exact_edge_remove_ns = state
                            .metrics
                            .exact_edge_remove_ns
                            .saturating_add(edge_stats.remove_ns);
                        state.metrics.exact_edge_insert_ns = state
                            .metrics
                            .exact_edge_insert_ns
                            .saturating_add(edge_stats.insert_ns);
                        state.metrics.exact_edge_canonicalize_ns = state
                            .metrics
                            .exact_edge_canonicalize_ns
                            .saturating_add(edge_stats.canonicalize_ns);
                        state.metrics.exact_edge_replacement_ns =
                            state.metrics.exact_edge_replacement_ns.saturating_add(
                                edge_stats
                                    .compare_ns
                                    .saturating_add(edge_stats.remove_ns)
                                    .saturating_add(edge_stats.insert_ns)
                                    .saturating_add(edge_stats.canonicalize_ns),
                            );
                        state.metrics.stale_edges_removed = state
                            .metrics
                            .stale_edges_removed
                            .saturating_add(edge_stats.removed);
                        state.metrics.formulas_evaluated =
                            state.metrics.formulas_evaluated.saturating_add(1);
                        state.metrics.formulas_evaluated_outside_workspaces = state
                            .metrics
                            .formulas_evaluated_outside_workspaces
                            .saturating_add(1);
                        computed_vertices = computed_vertices.saturating_add(1);
                        evaluated.insert(vertex);
                        state.metrics.formula_ns = state
                            .metrics
                            .formula_ns
                            .saturating_add(formula_started.elapsed().as_nanos());
                        #[cfg(test)]
                        if state
                            .fail_after_formula_commits
                            .is_some_and(|limit| computed_vertices >= limit)
                        {
                            return Err(ExcelError::new(formualizer_common::ExcelErrorKind::NImpl)
                                .with_message("Injected Engine V2 failure after formula commit"));
                        }
                    }
                }
                V2ScheduleUnit::Workspace(members) => {
                    if members.is_empty() {
                        continue;
                    }
                    state.metrics.queue_steps =
                        state.metrics.queue_steps.saturating_add(members.len());
                    let workspace_started = Instant::now();
                    let dirty_members = members
                        .iter()
                        .copied()
                        .filter(|member| host.v2_is_dirty(*member))
                        .collect::<Vec<_>>();
                    host.v2_resource_checkpoint(members.len() as u64)?;
                    let workspace = if roots.is_some()
                        && let Some(plan) = workspace_plan_classifications
                            .get(&members)
                            .filter(|plan| plan.retained_plan_valid)
                    {
                        host.v2_evaluate_workspace_retained_plan(&members, plan, state)?
                    } else {
                        host.v2_evaluate_workspace(&members)?
                    };
                    state
                        .metrics
                        .workspace_profiles
                        .push(V2WorkspaceDiagnostic {
                            stable_id: workspace.stable_id,
                            members: members.clone(),
                            dirty_members,
                            actual_cyclic_components: workspace.actual_cyclic_components.clone(),
                            pass_formula_evaluations: workspace.pass_formula_evaluations.clone(),
                            elapsed_ns: workspace.elapsed_ns,
                            classification: if workspace.retained_plan_runtime_invalidations == 0
                                && workspace.workspace_reopen_count == 0
                            {
                                workspace_plan_classifications.get(&members).cloned()
                            } else {
                                None
                            },
                        });
                    state.metrics.conservative_workspace_member_count = state
                        .metrics
                        .conservative_workspace_member_count
                        .saturating_add(members.len());
                    state.metrics.exact_scc_member_count = state
                        .metrics
                        .exact_scc_member_count
                        .saturating_add(workspace.active_cyclic_members);
                    state.metrics.non_feedback_workspace_member_count = state
                        .metrics
                        .non_feedback_workspace_member_count
                        .saturating_add(
                            members
                                .len()
                                .saturating_sub(workspace.active_cyclic_members),
                        );
                    state.metrics.workspace_discovery_formula_evaluations = state
                        .metrics
                        .workspace_discovery_formula_evaluations
                        .saturating_add(workspace.discovery_formula_evaluations);
                    state.metrics.workspace_exact_scc_formula_evaluations = state
                        .metrics
                        .workspace_exact_scc_formula_evaluations
                        .saturating_add(workspace.exact_scc_formula_evaluations);
                    state.metrics.workspace_upstream_formula_evaluations = state
                        .metrics
                        .workspace_upstream_formula_evaluations
                        .saturating_add(workspace.upstream_formula_evaluations);
                    state.metrics.workspace_downstream_formula_evaluations = state
                        .metrics
                        .workspace_downstream_formula_evaluations
                        .saturating_add(workspace.downstream_formula_evaluations);
                    state.metrics.workspace_discovery_formula_evaluation_ns = state
                        .metrics
                        .workspace_discovery_formula_evaluation_ns
                        .saturating_add(workspace.discovery_formula_evaluation_ns);
                    state.metrics.workspace_exact_scc_formula_evaluation_ns = state
                        .metrics
                        .workspace_exact_scc_formula_evaluation_ns
                        .saturating_add(workspace.exact_scc_formula_evaluation_ns);
                    state.metrics.workspace_upstream_formula_evaluation_ns = state
                        .metrics
                        .workspace_upstream_formula_evaluation_ns
                        .saturating_add(workspace.upstream_formula_evaluation_ns);
                    state.metrics.workspace_downstream_formula_evaluation_ns = state
                        .metrics
                        .workspace_downstream_formula_evaluation_ns
                        .saturating_add(workspace.downstream_formula_evaluation_ns);
                    state.metrics.repeated_non_feedback_evaluations = state
                        .metrics
                        .repeated_non_feedback_evaluations
                        .saturating_add(workspace.repeated_non_feedback_evaluations);
                    state.metrics.repeated_non_feedback_evaluations_avoided = state
                        .metrics
                        .repeated_non_feedback_evaluations_avoided
                        .saturating_add(workspace.repeated_non_feedback_evaluations_avoided);
                    if workspace.stage2_used_exact_scc_kernel {
                        state.metrics.workspaces_using_exact_scc_kernel = state
                            .metrics
                            .workspaces_using_exact_scc_kernel
                            .saturating_add(1);
                    }
                    if workspace.stage2_used_full_conservative_solver {
                        state.metrics.workspaces_using_full_conservative_solver = state
                            .metrics
                            .workspaces_using_full_conservative_solver
                            .saturating_add(1);
                        if let Some(reason) = workspace.kernel_fallback_reason {
                            *state
                                .metrics
                                .workspace_kernel_fallback_reasons
                                .entry(reason.to_string())
                                .or_default() += 1;
                        }
                    }
                    state.metrics.exact_scc_rebuild_count = state
                        .metrics
                        .exact_scc_rebuild_count
                        .saturating_add(workspace.exact_scc_rebuild_count);
                    state.metrics.exact_scc_expansion_count = state
                        .metrics
                        .exact_scc_expansion_count
                        .saturating_add(workspace.exact_scc_expansion_count);
                    state.metrics.workspace_reopen_count = state
                        .metrics
                        .workspace_reopen_count
                        .saturating_add(workspace.workspace_reopen_count);
                    state.metrics.retained_plan_runtime_invalidations = state
                        .metrics
                        .retained_plan_runtime_invalidations
                        .saturating_add(workspace.retained_plan_runtime_invalidations);
                    state.metrics.retained_plan_reopens = state
                        .metrics
                        .retained_plan_reopens
                        .saturating_add(workspace.retained_plan_reopens);
                    state
                        .metrics
                        .exclusive_attribution
                        .retained_plan_validation_ns = state
                        .metrics
                        .exclusive_attribution
                        .retained_plan_validation_ns
                        .saturating_add(workspace.retained_plan_validation_ns);
                    if let Some(reason) = workspace.retained_plan_runtime_invalidation_reason {
                        *state
                            .metrics
                            .retained_plan_runtime_invalidation_reasons
                            .entry(reason.to_string())
                            .or_default() += 1;
                    }
                    state.metrics.acyclic_formula_evaluation_ns = state
                        .metrics
                        .acyclic_formula_evaluation_ns
                        .saturating_add(workspace.discovery_formula_evaluation_ns)
                        .saturating_add(workspace.downstream_formula_evaluation_ns);
                    state.metrics.spill_effect_commit_ns = state
                        .metrics
                        .spill_effect_commit_ns
                        .saturating_add(workspace.downstream_spill_effect_commit_ns);
                    state.metrics.workspace_construction_ns = state
                        .metrics
                        .workspace_construction_ns
                        .saturating_add(workspace.workspace_construction_ns);
                    state.metrics.scc_preparation_ns = state
                        .metrics
                        .scc_preparation_ns
                        .saturating_add(workspace.workspace_construction_ns);
                    state.metrics.iterative_solver_execution_ns = state
                        .metrics
                        .iterative_solver_execution_ns
                        .saturating_add(workspace.iterative_solver_execution_ns);
                    state.metrics.exact_read_finalization_ns = state
                        .metrics
                        .exact_read_finalization_ns
                        .saturating_add(workspace.exact_read_finalization_ns);
                    let validation_started = Instant::now();
                    let mut runtime_invalid = 0usize;
                    let mut reference_invalid = 0usize;
                    for (_, _, reads) in &workspace.members {
                        let metadata_started = Instant::now();
                        record_validation_metadata(&mut state.metrics, reads);
                        state.metrics.validation_metadata_ns = state
                            .metrics
                            .validation_metadata_ns
                            .saturating_add(metadata_started.elapsed().as_nanos());
                        let runtime_started = Instant::now();
                        let runtime_valid = host.v2_runtime_reads_valid(reads);
                        state.metrics.validation_runtime_formula_ns = state
                            .metrics
                            .validation_runtime_formula_ns
                            .saturating_add(runtime_started.elapsed().as_nanos());
                        if runtime_valid {
                            state.metrics.validation_runtime_formula_edges_unchanged = state
                                .metrics
                                .validation_runtime_formula_edges_unchanged
                                .saturating_add(reads.formula_edges.len());
                        } else {
                            runtime_invalid = runtime_invalid.saturating_add(1);
                            state.metrics.validation_runtime_formula_edges_invalidated = state
                                .metrics
                                .validation_runtime_formula_edges_invalidated
                                .saturating_add(reads.formula_edges.len());
                        }
                        let reference_started = Instant::now();
                        let reference_valid = host.v2_reference_observations_valid(reads);
                        state.metrics.validation_reference_ns = state
                            .metrics
                            .validation_reference_ns
                            .saturating_add(reference_started.elapsed().as_nanos());
                        if reference_valid {
                            state.metrics.validation_reference_observations_unchanged = state
                                .metrics
                                .validation_reference_observations_unchanged
                                .saturating_add(reads.reference_observations.len());
                        } else {
                            reference_invalid = reference_invalid.saturating_add(1);
                            state.metrics.validation_reference_observations_invalidated = state
                                .metrics
                                .validation_reference_observations_invalidated
                                .saturating_add(reads.reference_observations.len());
                        }
                    }
                    if runtime_invalid != 0 {
                        return Err(runtime_contract_error());
                    }
                    if reference_invalid != 0 {
                        return Err(revision_error_with_message(
                            "Engine V2 workspace reference validity changed during evaluation",
                        ));
                    }
                    let topology_started = Instant::now();
                    let topology_valid = revisions_match(
                        host,
                        state,
                        topology_revision,
                        symbol_revision,
                        semantic_revision,
                    );
                    state.metrics.validation_topology_checks =
                        state.metrics.validation_topology_checks.saturating_add(1);
                    state.metrics.validation_topology_ns = state
                        .metrics
                        .validation_topology_ns
                        .saturating_add(topology_started.elapsed().as_nanos());
                    if !topology_valid {
                        state.metrics.validation_topology_invalidated = state
                            .metrics
                            .validation_topology_invalidated
                            .saturating_add(1);
                        return Err(revision_error_with_message(
                            "Engine V2 workspace revisions changed during evaluation",
                        ));
                    }
                    state.metrics.generation_reference_validation_ns = state
                        .metrics
                        .generation_reference_validation_ns
                        .saturating_add(validation_started.elapsed().as_nanos());
                    state.metrics.exclusive_attribution.contract_validation_ns = state
                        .metrics
                        .exclusive_attribution
                        .contract_validation_ns
                        .saturating_add(validation_started.elapsed().as_nanos());
                    cycle_errors = cycle_errors.saturating_add(usize::from(workspace.stamped > 0));
                    computed_vertices =
                        computed_vertices.saturating_add(workspace.evaluated_vertices);
                    state.metrics.solver_passes = state
                        .metrics
                        .solver_passes
                        .saturating_add(workspace.solver_passes);
                    state.metrics.active_cyclic_workspace_members = state
                        .metrics
                        .active_cyclic_workspace_members
                        .saturating_add(workspace.active_cyclic_members);
                    for (member, _value, reads) in workspace.members {
                        state.metrics.runtime_formula_edge_events = state
                            .metrics
                            .runtime_formula_edge_events
                            .saturating_add(reads.formula_edge_events);
                        state.metrics.runtime_formula_edges_processed = state
                            .metrics
                            .runtime_formula_edges_processed
                            .saturating_add(reads.formula_edges.len());
                        state.metrics.logical_range_positions = state
                            .metrics
                            .logical_range_positions
                            .saturating_add(reads.logical_range_positions);
                        state.metrics.physical_cells_fetched = state
                            .metrics
                            .physical_cells_fetched
                            .saturating_add(reads.physical_cells_fetched);
                        state.metrics.diagnostic_records_retained = state
                            .metrics
                            .diagnostic_records_retained
                            .saturating_add(reads.diagnostic_records_retained);
                        let read_set_compare_started = detailed_read_set_metrics.then(Instant::now);
                        let read_set_changed = detailed_read_set_metrics.then(|| {
                            state
                                .current_reads
                                .get(&member)
                                .is_none_or(|previous| previous != &reads)
                        });
                        if let Some(started) = read_set_compare_started {
                            state.metrics.diagnostic_read_set_compare_ns = state
                                .metrics
                                .diagnostic_read_set_compare_ns
                                .saturating_add(started.elapsed().as_nanos());
                        }
                        let edge_stats = state.replace_read_set_with_stats(member, reads);
                        state.metrics.exclusive_attribution.adjacency_replacement_ns = state
                            .metrics
                            .exclusive_attribution
                            .adjacency_replacement_ns
                            .saturating_add(
                                edge_stats
                                    .compare_ns
                                    .saturating_add(edge_stats.remove_ns)
                                    .saturating_add(edge_stats.insert_ns)
                                    .saturating_add(edge_stats.canonicalize_ns),
                            );
                        state.metrics.exact_read_sets_finalized =
                            state.metrics.exact_read_sets_finalized.saturating_add(1);
                        if let Some(read_set_changed) = read_set_changed {
                            if read_set_changed {
                                state.metrics.exact_read_sets_changed =
                                    state.metrics.exact_read_sets_changed.saturating_add(1);
                            } else {
                                state.metrics.exact_read_sets_unchanged =
                                    state.metrics.exact_read_sets_unchanged.saturating_add(1);
                            }
                        }
                        state.metrics.exact_edges_examined = state
                            .metrics
                            .exact_edges_examined
                            .saturating_add(edge_stats.edges_examined);
                        state.metrics.exact_edges_removed = state
                            .metrics
                            .exact_edges_removed
                            .saturating_add(edge_stats.removed);
                        state.metrics.exact_edges_inserted = state
                            .metrics
                            .exact_edges_inserted
                            .saturating_add(edge_stats.inserted);
                        state.metrics.exact_edges_unchanged = state
                            .metrics
                            .exact_edges_unchanged
                            .saturating_add(edge_stats.unchanged);
                        state.metrics.exact_edge_sets_compared = state
                            .metrics
                            .exact_edge_sets_compared
                            .saturating_add(edge_stats.edge_sets_compared);
                        state.metrics.exact_identical_edge_sets = state
                            .metrics
                            .exact_identical_edge_sets
                            .saturating_add(edge_stats.identical_edge_sets);
                        state.metrics.exact_changed_edge_sets = state
                            .metrics
                            .exact_changed_edge_sets
                            .saturating_add(edge_stats.changed_edge_sets);
                        state.metrics.exact_reverse_buckets_untouched = state
                            .metrics
                            .exact_reverse_buckets_untouched
                            .saturating_add(edge_stats.reverse_buckets_untouched);
                        state.metrics.exact_reverse_buckets_mutated = state
                            .metrics
                            .exact_reverse_buckets_mutated
                            .saturating_add(edge_stats.reverse_buckets_mutated);
                        if edge_stats.full_replacement_fallback {
                            state.metrics.exact_full_replacement_fallback_count = state
                                .metrics
                                .exact_full_replacement_fallback_count
                                .saturating_add(1);
                        }
                        state.metrics.reverse_buckets_touched = state
                            .metrics
                            .reverse_buckets_touched
                            .saturating_add(edge_stats.reverse_buckets_touched);
                        state.metrics.exact_edge_compare_ns = state
                            .metrics
                            .exact_edge_compare_ns
                            .saturating_add(edge_stats.compare_ns);
                        state.metrics.exact_edge_remove_ns = state
                            .metrics
                            .exact_edge_remove_ns
                            .saturating_add(edge_stats.remove_ns);
                        state.metrics.exact_edge_insert_ns = state
                            .metrics
                            .exact_edge_insert_ns
                            .saturating_add(edge_stats.insert_ns);
                        state.metrics.exact_edge_canonicalize_ns = state
                            .metrics
                            .exact_edge_canonicalize_ns
                            .saturating_add(edge_stats.canonicalize_ns);
                        state.metrics.exact_edge_replacement_ns =
                            state.metrics.exact_edge_replacement_ns.saturating_add(
                                edge_stats
                                    .compare_ns
                                    .saturating_add(edge_stats.remove_ns)
                                    .saturating_add(edge_stats.insert_ns)
                                    .saturating_add(edge_stats.canonicalize_ns),
                            );
                        state.metrics.stale_edges_removed = state
                            .metrics
                            .stale_edges_removed
                            .saturating_add(edge_stats.removed);
                        evaluated.insert(member);
                    }
                    state.metrics.formulas_evaluated = state
                        .metrics
                        .formulas_evaluated
                        .saturating_add(workspace.formula_evaluations);
                    state.metrics.formulas_evaluated_inside_workspaces = state
                        .metrics
                        .formulas_evaluated_inside_workspaces
                        .saturating_add(workspace.formula_evaluations);
                    state.metrics.workspace_members_evaluated = state
                        .metrics
                        .workspace_members_evaluated
                        .saturating_add(workspace.evaluated_vertices);
                    state.metrics.workspace_ns = state
                        .metrics
                        .workspace_ns
                        .saturating_add(workspace_started.elapsed().as_nanos());
                }
            }
            state.metrics.unique_current_runtime_formula_edges = state.current_edges.len();
            state.metrics.runtime_formula_edges_retained = state.current_edges.len();
        }

        let cleanup_started = Instant::now();
        let final_validation_started = Instant::now();
        let final_topology_valid = revisions_match(
            host,
            state,
            topology_revision,
            symbol_revision,
            semantic_revision,
        );
        state.metrics.validation_topology_checks =
            state.metrics.validation_topology_checks.saturating_add(1);
        let final_validation_ns = final_validation_started.elapsed().as_nanos();
        state.metrics.validation_topology_ns = state
            .metrics
            .validation_topology_ns
            .saturating_add(final_validation_ns);
        state.metrics.generation_reference_validation_ns = state
            .metrics
            .generation_reference_validation_ns
            .saturating_add(final_validation_ns);
        if !final_topology_valid {
            state.metrics.validation_topology_invalidated = state
                .metrics
                .validation_topology_invalidated
                .saturating_add(1);
            return Err(revision_error());
        }
        let evaluated_vertices: Vec<VertexId> = evaluated.into_iter().collect();
        host.v2_clear_dirty(&evaluated_vertices);
        state.needs_full_rebuild = false;
        let retained_refresh_started = Instant::now();
        if roots.is_some() {
            let profiles_to_refresh = state
                .metrics
                .workspace_profiles
                .iter()
                .enumerate()
                .filter(|(_, profile)| {
                    profile
                        .classification
                        .as_ref()
                        .is_none_or(|classification| {
                            !classification.retained_plan_valid
                                || classification.exact_scc_components
                                    != profile.actual_cyclic_components
                        })
                })
                .map(|(index, profile)| (index, profile.clone()))
                .collect::<Vec<_>>();
            let mut contract_validity = Vec::new();
            for (index, profile) in profiles_to_refresh {
                let classification = classify_workspace_plan(
                    host,
                    state,
                    &profile.members,
                    std::slice::from_ref(&profile),
                    &mut contract_validity,
                );
                if classification.retained_plan_valid {
                    state.metrics.workspace_profiles[index].classification = Some(classification);
                }
            }
        }
        let retained_refresh_ns = retained_refresh_started.elapsed().as_nanos();
        state
            .metrics
            .exclusive_attribution
            .retained_plan_validation_ns = state
            .metrics
            .exclusive_attribution
            .retained_plan_validation_ns
            .saturating_add(retained_refresh_ns);
        host.v2_finish_request(request_kind);
        state.metrics.cleanup_ns = cleanup_started
            .elapsed()
            .as_nanos()
            .saturating_sub(retained_refresh_ns);
        state.metrics.exclusive_attribution.cleanup_ns = state.metrics.cleanup_ns;
        state.metrics.unique_current_runtime_formula_edges = state.current_edges.len();
        state.metrics.runtime_formula_edges_retained = state.current_edges.len();
        state.metrics.elapsed_ns = started.elapsed().as_nanos();
        state.metrics.kernel_named_phase_ns = state
            .metrics
            .retained_state_scan_ns
            .saturating_add(state.metrics.schedule_demand_subgraph_ns)
            .saturating_add(state.metrics.dirty_seed_selection_ns)
            .saturating_add(state.metrics.schedule_construction_ns)
            .saturating_add(state.metrics.acyclic_formula_evaluation_ns)
            .saturating_add(state.metrics.workspace_construction_ns)
            .saturating_add(state.metrics.iterative_solver_execution_ns)
            .saturating_add(state.metrics.exact_read_finalization_ns)
            .saturating_add(state.metrics.exact_edge_replacement_ns)
            .saturating_add(state.metrics.generation_reference_validation_ns)
            .saturating_add(state.metrics.spill_effect_commit_ns)
            .saturating_add(state.metrics.cleanup_ns);
        state.metrics.kernel_unattributed_ns = state
            .metrics
            .elapsed_ns
            .saturating_sub(state.metrics.kernel_named_phase_ns);
        state.metrics.kernel_top_level_named_phase_ns = state
            .metrics
            .retained_state_scan_ns
            .saturating_add(state.metrics.schedule_demand_subgraph_ns)
            .saturating_add(state.metrics.dirty_seed_selection_ns)
            .saturating_add(state.metrics.schedule_construction_ns)
            .saturating_add(state.metrics.formula_ns)
            .saturating_add(state.metrics.workspace_ns)
            .saturating_add(state.metrics.cleanup_ns);
        state.metrics.kernel_top_level_unattributed_ns = state
            .metrics
            .elapsed_ns
            .saturating_sub(state.metrics.kernel_top_level_named_phase_ns);
        let mut exclusive_attribution = host.v2_take_formula_attribution();
        exclusive_attribution.retained_state_scan_ns = state.metrics.retained_state_scan_ns;
        exclusive_attribution.demand_scheduling_ns =
            state.metrics.exclusive_attribution.demand_scheduling_ns;
        exclusive_attribution.retained_plan_validation_ns = state
            .metrics
            .exclusive_attribution
            .retained_plan_validation_ns;
        exclusive_attribution.contract_validation_ns =
            state.metrics.exclusive_attribution.contract_validation_ns;
        exclusive_attribution.adjacency_replacement_ns =
            state.metrics.exclusive_attribution.adjacency_replacement_ns;
        exclusive_attribution.cleanup_ns = state.metrics.cleanup_ns;
        exclusive_attribution.explicit_residual_ns = state
            .metrics
            .elapsed_ns
            .saturating_sub(exclusive_attribution.exclusive_children_ns());
        state.metrics.exclusive_attribution = exclusive_attribution;
        Ok(V2RunResult {
            computed_vertices,
            cycle_errors,
            elapsed: started.elapsed(),
        })
    })();
    let runtime_contract_stats = host.v2_take_runtime_contract_stats();
    record_runtime_contract_stats(&mut state.metrics, runtime_contract_stats);
    if result.is_err() {
        state.metrics.elapsed_ns = started.elapsed().as_nanos();
        host.v2_abort_request();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reads(edges: &[VertexId]) -> ExactReadSet {
        ExactReadSet {
            formula_edges: edges.iter().copied().collect(),
            ..ExactReadSet::default()
        }
    }

    #[test]
    fn feature_gate_values_are_explicit_and_fail_closed() {
        for value in [
            Some("1"),
            Some("true"),
            Some("TRUE"),
            Some("on"),
            Some("ON"),
        ] {
            assert!(requested_from_value(value));
        }
        for value in [
            None,
            Some(""),
            Some("0"),
            Some("false"),
            Some("True"),
            Some("yes"),
        ] {
            assert!(!requested_from_value(value));
        }
    }

    #[test]
    fn replace_read_set_keeps_forward_and_reverse_indexes_exact() {
        let reader = VertexId(1);
        let a = VertexId(2);
        let b = VertexId(3);
        let c = VertexId(4);
        let mut state = V2State::default();

        assert_eq!(state.replace_read_set(reader, reads(&[a, b])), 0);
        assert_eq!(
            state.current_edges,
            [(reader, a), (reader, b)].into_iter().collect()
        );
        assert_eq!(state.readers_of(a), vec![reader]);
        assert_eq!(state.readers_of(b), vec![reader]);

        assert_eq!(state.replace_read_set(reader, reads(&[b, c])), 1);
        assert_eq!(
            state.current_edges,
            [(reader, b), (reader, c)].into_iter().collect()
        );
        assert!(state.readers_of(a).is_empty());
        assert_eq!(state.readers_of(b), vec![reader]);
        assert_eq!(state.readers_of(c), vec![reader]);

        for _ in 0..100 {
            assert_eq!(state.replace_read_set(reader, reads(&[b, c])), 0);
            assert_eq!(state.current_edges.len(), 2);
            assert_eq!(state.reverse_runtime.len(), 2);
        }

        assert_eq!(state.replace_read_set(reader, reads(&[])), 2);
        assert!(state.current_edges.is_empty());
        assert!(state.reverse_runtime.is_empty());
        assert_eq!(state.current_reads.len(), 1);

        state.synchronize_vertices(&[]);
        assert!(state.current_reads.is_empty());
    }

    #[test]
    fn delta_replacement_mutates_only_changed_adjacency() {
        let reader = VertexId(1);
        let a = VertexId(2);
        let b = VertexId(3);
        let c = VertexId(4);
        let mut state = V2State::default();

        let first = state.replace_read_set_with_stats(reader, reads(&[a, b]));
        assert_eq!(first.removed, 0);
        assert_eq!(first.inserted, 2);
        assert_eq!(first.unchanged, 0);
        assert_eq!(first.reverse_buckets_mutated, 2);

        let identical = state.replace_read_set_with_stats(reader, reads(&[a, b]));
        assert_eq!(identical.identical_edge_sets, 1);
        assert_eq!(identical.changed_edge_sets, 0);
        assert_eq!(identical.removed, 0);
        assert_eq!(identical.inserted, 0);
        assert_eq!(identical.reverse_buckets_touched, 0);
        assert_eq!(identical.reverse_buckets_untouched, 2);
        assert_eq!(identical.reverse_buckets_mutated, 0);

        let added = state.replace_read_set_with_stats(reader, reads(&[a, b, c]));
        assert_eq!(added.changed_edge_sets, 1);
        assert_eq!(added.removed, 0);
        assert_eq!(added.inserted, 1);
        assert_eq!(added.unchanged, 2);
        assert_eq!(added.reverse_buckets_untouched, 2);
        assert_eq!(added.reverse_buckets_mutated, 1);

        let removed = state.replace_read_set_with_stats(reader, reads(&[b, c]));
        assert_eq!(removed.removed, 1);
        assert_eq!(removed.inserted, 0);
        assert_eq!(removed.unchanged, 2);
        assert_eq!(removed.reverse_buckets_untouched, 2);
        assert_eq!(removed.reverse_buckets_mutated, 1);
        assert!(state.readers_of(a).is_empty());
        assert_eq!(state.readers_of(b), vec![reader]);
        assert_eq!(state.readers_of(c), vec![reader]);
    }

    #[test]
    #[ignore = "Stage 3H recorder representation microbenchmark"]
    fn stage3h_recorder_representation_microbenchmark() {
        use formualizer_common::{CoordHashMap, PackedSheetCell};
        use std::hint::black_box;

        const FORMULAS: usize = 1_768;
        const EVENTS: usize = 1_016;
        const UNIQUE: usize = 507;
        let sheet = "CashFlow Inputs";

        let started = Instant::now();
        let mut string_entries = 0usize;
        for formula in 0..FORMULAS {
            let source = Mutex::new(Some(VertexId(formula as u32 + 1)));
            let sets = Mutex::new(BTreeMap::<VertexId, BTreeMap<ReadCell, usize>>::new());
            sets.lock()
                .unwrap()
                .insert(VertexId(formula as u32 + 1), BTreeMap::new());
            for event in 0..EVENTS {
                let source = source.lock().unwrap().unwrap();
                let mut sets = sets.lock().unwrap();
                let reads = sets.get_mut(&source).unwrap();
                let cell = ReadCell {
                    sheet: sheet.to_string(),
                    row: (event % UNIQUE) as u32,
                    col: ((event % UNIQUE) % 31) as u32,
                };
                *reads.entry(cell).or_default() += 1;
            }
            string_entries += sets
                .lock()
                .unwrap()
                .values()
                .map(BTreeMap::len)
                .sum::<usize>();
        }
        let string_mutex_btree_ns = started.elapsed().as_nanos();
        black_box(string_entries);

        let started = Instant::now();
        let mut packed_btree_entries = 0usize;
        for _ in 0..FORMULAS {
            let reads = Mutex::new(BTreeMap::<PackedSheetCell, usize>::new());
            for event in 0..EVENTS {
                let cell = PackedSheetCell::try_new(
                    7,
                    (event % UNIQUE) as u32,
                    ((event % UNIQUE) % 31) as u32,
                )
                .unwrap();
                *reads.lock().unwrap().entry(cell).or_default() += 1;
            }
            packed_btree_entries += reads.lock().unwrap().len();
        }
        let packed_mutex_btree_ns = started.elapsed().as_nanos();
        black_box(packed_btree_entries);

        let started = Instant::now();
        let mut packed_fx_entries = 0usize;
        for _ in 0..FORMULAS {
            let reads = Mutex::new(FxHashMap::<PackedSheetCell, usize>::default());
            for event in 0..EVENTS {
                let cell = PackedSheetCell::try_new(
                    7,
                    (event % UNIQUE) as u32,
                    ((event % UNIQUE) % 31) as u32,
                )
                .unwrap();
                *reads.lock().unwrap().entry(cell).or_default() += 1;
            }
            packed_fx_entries += reads.lock().unwrap().len();
        }
        let packed_mutex_fx_ns = started.elapsed().as_nanos();
        black_box(packed_fx_entries);

        let started = Instant::now();
        let mut packed_coord_entries = 0usize;
        for _ in 0..FORMULAS {
            let reads = Mutex::new(CoordHashMap::<PackedSheetCell, usize>::default());
            for event in 0..EVENTS {
                let cell = PackedSheetCell::try_new(
                    7,
                    (event % UNIQUE) as u32,
                    ((event % UNIQUE) % 31) as u32,
                )
                .unwrap();
                *reads.lock().unwrap().entry(cell).or_default() += 1;
            }
            packed_coord_entries += reads.lock().unwrap().len();
        }
        let packed_mutex_coord_ns = started.elapsed().as_nanos();
        black_box(packed_coord_entries);

        let started = Instant::now();
        let mut packed_vec_entries = 0usize;
        for _ in 0..FORMULAS {
            let reads = Mutex::new(Vec::<PackedSheetCell>::with_capacity(EVENTS));
            for event in 0..EVENTS {
                let cell = PackedSheetCell::try_new(
                    7,
                    (event % UNIQUE) as u32,
                    ((event % UNIQUE) % 31) as u32,
                )
                .unwrap();
                reads.lock().unwrap().push(cell);
            }
            let mut reads = reads.into_inner().unwrap();
            reads.sort_unstable();
            packed_vec_entries += reads.windows(2).filter(|pair| pair[0] != pair[1]).count() + 1;
            black_box(reads);
        }
        let packed_mutex_vec_sort_ns = started.elapsed().as_nanos();
        black_box(packed_vec_entries);

        let started = Instant::now();
        let mut string_tree_entries = 0usize;
        for _ in 0..FORMULAS {
            let mut reads = BTreeMap::<ReadCell, usize>::new();
            for event in 0..EVENTS {
                let cell = ReadCell {
                    sheet: sheet.to_string(),
                    row: (event % UNIQUE) as u32,
                    col: ((event % UNIQUE) % 31) as u32,
                };
                *reads.entry(cell).or_default() += 1;
            }
            string_tree_entries += reads.len();
            black_box(reads);
        }
        let string_btree_ns = started.elapsed().as_nanos();
        black_box(string_tree_entries);

        let started = Instant::now();
        let mut packed_tree_entries = 0usize;
        for _ in 0..FORMULAS {
            let mut reads = BTreeMap::<PackedSheetCell, usize>::new();
            for event in 0..EVENTS {
                let cell = PackedSheetCell::try_new(
                    7,
                    (event % UNIQUE) as u32,
                    ((event % UNIQUE) % 31) as u32,
                )
                .unwrap();
                *reads.entry(cell).or_default() += 1;
            }
            packed_tree_entries += reads.len();
            black_box(reads);
        }
        let packed_btree_ns = started.elapsed().as_nanos();
        black_box(packed_tree_entries);

        let started = Instant::now();
        let mut packed_local_fx_entries = 0usize;
        for _ in 0..FORMULAS {
            let mut reads = FxHashMap::<PackedSheetCell, usize>::default();
            for event in 0..EVENTS {
                let cell = PackedSheetCell::try_new(
                    7,
                    (event % UNIQUE) as u32,
                    ((event % UNIQUE) % 31) as u32,
                )
                .unwrap();
                *reads.entry(cell).or_default() += 1;
            }
            packed_local_fx_entries += reads.len();
            black_box(reads);
        }
        let packed_fx_ns = started.elapsed().as_nanos();
        black_box(packed_local_fx_entries);

        let started = Instant::now();
        let mut packed_local_coord_entries = 0usize;
        for _ in 0..FORMULAS {
            let mut reads = CoordHashMap::<PackedSheetCell, usize>::default();
            for event in 0..EVENTS {
                let cell = PackedSheetCell::try_new(
                    7,
                    (event % UNIQUE) as u32,
                    ((event % UNIQUE) % 31) as u32,
                )
                .unwrap();
                *reads.entry(cell).or_default() += 1;
            }
            packed_local_coord_entries += reads.len();
            black_box(reads);
        }
        let packed_coord_ns = started.elapsed().as_nanos();
        black_box(packed_local_coord_entries);

        let started = Instant::now();
        let mut packed_local_vec_entries = 0usize;
        for _ in 0..FORMULAS {
            let mut reads = Vec::<PackedSheetCell>::with_capacity(EVENTS);
            for event in 0..EVENTS {
                reads.push(
                    PackedSheetCell::try_new(
                        7,
                        (event % UNIQUE) as u32,
                        ((event % UNIQUE) % 31) as u32,
                    )
                    .unwrap(),
                );
            }
            reads.sort_unstable();
            packed_local_vec_entries +=
                reads.windows(2).filter(|pair| pair[0] != pair[1]).count() + 1;
            black_box(reads);
        }
        let packed_vec_sort_ns = started.elapsed().as_nanos();
        black_box(packed_local_vec_entries);

        println!(
            "stage3h_recorder_microbenchmark_ns=string_mutex_btree:{string_mutex_btree_ns} packed_mutex_btree:{packed_mutex_btree_ns} packed_mutex_fx:{packed_mutex_fx_ns} packed_mutex_coord:{packed_mutex_coord_ns} packed_mutex_vec_sort:{packed_mutex_vec_sort_ns} string_btree:{string_btree_ns} packed_btree:{packed_btree_ns} packed_fx:{packed_fx_ns} packed_coord:{packed_coord_ns} packed_vec_sort:{packed_vec_sort_ns}"
        );
        assert_eq!(string_entries, FORMULAS * UNIQUE);
        assert_eq!(packed_btree_entries, string_entries);
        assert_eq!(packed_fx_entries, string_entries);
        assert_eq!(packed_coord_entries, string_entries);
        assert_eq!(packed_vec_entries, string_entries);
        assert_eq!(string_tree_entries, string_entries);
        assert_eq!(packed_tree_entries, string_entries);
        assert_eq!(packed_local_fx_entries, string_entries);
        assert_eq!(packed_local_coord_entries, string_entries);
        assert_eq!(packed_local_vec_entries, string_entries);
    }
}
