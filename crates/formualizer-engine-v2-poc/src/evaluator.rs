use crate::model::{
    CellId, CellState, DependencyDescriptor, EffectKey, EvaluationResult, ExecutionRead,
    FormulaRecord, InvalidationDependency, NameDefinition, NameDefinitionRecord, NameId,
    NameRegistry, NameScope, RangeDescriptor, ReferenceValue, SpillRef, TraceReport,
    collect_invalidation_dependencies, dependency_matches_event,
};
use chrono::{Datelike, Duration, NaiveDate};
use formualizer_common::{
    DateSystem, ExcelError, ExcelErrorKind, LiteralValue, datetime_to_serial, serial_to_datetime,
};
use formualizer_parse::parser::{ASTNode, ASTNodeType, ReferenceType, parse};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::Instant;

const MAX_MATERIALIZED_RANGE_CELLS: usize = 100_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ChangeEvent {
    Cell(CellId),
    Name(NameId),
    Structural(String),
    Spill(SpillRef),
    Effect(EffectKey),
}

#[derive(Clone, Debug, PartialEq)]
enum ResolvedReference {
    Reference(ReferenceValue),
    Constant(LiteralValue),
    Formula(ASTNode),
    DynamicFormula(ASTNode),
}

#[derive(Clone, Debug, PartialEq)]
struct RawCellRead {
    value: LiteralValue,
    formula: Option<ASTNode>,
}

pub trait ReferenceResolver {
    fn resolve_cell_reference(
        &mut self,
        reader: &CellId,
        target: CellId,
    ) -> Result<ReferenceValue, ExcelError>;
    fn resolve_range_reference(
        &mut self,
        reader: &CellId,
        range: RangeDescriptor,
    ) -> Result<ReferenceValue, ExcelError>;
    fn resolve_name_reference(
        &mut self,
        reader: &CellId,
        name: &str,
        current_sheet: &str,
    ) -> Result<ReferenceValue, ExcelError>;
}

pub struct EvaluationHost<'a> {
    cells: &'a BTreeMap<CellId, CellState>,
    names: &'a NameRegistry,
    spills: &'a BTreeMap<CellId, SpillRef>,
    recorder: &'a mut TraceReport,
}

impl<'a> EvaluationHost<'a> {
    fn new(
        cells: &'a BTreeMap<CellId, CellState>,
        names: &'a NameRegistry,
        spills: &'a BTreeMap<CellId, SpillRef>,
        recorder: &'a mut TraceReport,
    ) -> Self {
        Self {
            cells,
            names,
            spills,
            recorder,
        }
    }

    fn raw_cell(&mut self, reader: &CellId, target: &CellId) -> RawCellRead {
        self.recorder
            .effects
            .insert(EffectKey::TargetValue(target.clone()));
        let raw = match self.cells.get(target) {
            Some(state) => RawCellRead {
                value: state.value.clone(),
                formula: state.formula.clone(),
            },
            None => RawCellRead {
                value: LiteralValue::Empty,
                formula: None,
            },
        };
        self.recorder.record_cell_read(reader, target, &raw.value);
        raw
    }

    fn range_reference(&mut self, reader: &CellId, range: RangeDescriptor) -> ReferenceValue {
        self.recorder.record_range_read(reader, range.clone());
        ReferenceValue::Range(range)
    }

    fn name_resolution(
        &mut self,
        reader: &CellId,
        name: &str,
        current_sheet: &str,
    ) -> Result<ResolvedReference, ExcelError> {
        let Some(record) = self.names.resolve(name, current_sheet) else {
            return Err(ExcelError::new(ExcelErrorKind::Name));
        };
        self.recorder.record_name_read(reader, record.id.clone());
        match &record.definition {
            NameDefinition::Constant(value) => Ok(ResolvedReference::Constant(value.clone())),
            NameDefinition::Cell(cell) => Ok(ResolvedReference::Reference(ReferenceValue::Cell(
                cell.clone(),
            ))),
            NameDefinition::Range(range) => Ok(ResolvedReference::Reference(
                self.range_reference(reader, range.clone()),
            )),
            NameDefinition::Spill(spill) => {
                self.recorder
                    .record_read(ExecutionRead::Spill(spill.clone()));
                Ok(ResolvedReference::Reference(ReferenceValue::Spill(
                    spill.clone(),
                )))
            }
            NameDefinition::Formula { ast } => Ok(ResolvedReference::Formula(ast.clone())),
            NameDefinition::DynamicFormula { ast } => {
                Ok(ResolvedReference::DynamicFormula(ast.clone()))
            }
        }
    }

    fn dynamic_reference(
        &mut self,
        selector: Option<CellId>,
        target: ReferenceValue,
    ) -> ReferenceValue {
        if let Some(selector) = &selector {
            self.recorder
                .effects
                .insert(EffectKey::DynamicSelector(selector.clone()));
        } else {
            self.recorder
                .effects
                .insert(EffectKey::DynamicSelector(CellId::new("<literal>", 0, 0)));
        }
        if let ReferenceValue::Cell(cell) = &target {
            self.recorder
                .effects
                .insert(EffectKey::DynamicTarget(cell.clone()));
        }
        self.recorder.record_read(ExecutionRead::Dynamic {
            selector,
            target: target.clone(),
        });
        target
    }

    pub fn resolve_spill_reference(
        &mut self,
        _reader: &CellId,
        anchor: &CellId,
    ) -> Result<ReferenceValue, ExcelError> {
        let spill = self
            .spills
            .get(anchor)
            .cloned()
            .ok_or_else(|| ExcelError::new(ExcelErrorKind::Ref))?;
        self.recorder
            .record_read(ExecutionRead::Spill(spill.clone()));
        Ok(ReferenceValue::Spill(spill))
    }

    fn resolve_ast_reference(
        &mut self,
        reader: &CellId,
        reference: &ReferenceType,
        current_sheet: &str,
    ) -> Result<ResolvedReference, ExcelError> {
        match reference {
            ReferenceType::Cell {
                sheet, row, col, ..
            } => Ok(ResolvedReference::Reference(ReferenceValue::Cell(
                CellId::new(sheet.as_deref().unwrap_or(current_sheet), *row, *col),
            ))),
            ReferenceType::Range {
                sheet,
                start_row,
                start_col,
                end_row,
                end_col,
                ..
            } => {
                let range = RangeDescriptor::new(
                    sheet.as_deref().unwrap_or(current_sheet),
                    start_row.unwrap_or(1),
                    start_col.unwrap_or(1),
                    end_row.unwrap_or(start_row.unwrap_or(1)),
                    end_col.unwrap_or(start_col.unwrap_or(1)),
                );
                Ok(ResolvedReference::Reference(
                    self.range_reference(reader, range),
                ))
            }
            ReferenceType::NamedRange(name) => self.name_resolution(reader, name, current_sheet),
            ReferenceType::Table(table) => Err(ExcelError::new(ExcelErrorKind::NImpl)
                .with_message(format!(
                    "table reference unsupported in POC: {}",
                    table.name
                ))),
            ReferenceType::Cell3D { .. }
            | ReferenceType::Range3D { .. }
            | ReferenceType::External(_) => Err(ExcelError::new(ExcelErrorKind::NImpl)
                .with_message("external or 3D reference unsupported in POC")),
        }
    }
}

impl ReferenceResolver for EvaluationHost<'_> {
    fn resolve_cell_reference(
        &mut self,
        _reader: &CellId,
        target: CellId,
    ) -> Result<ReferenceValue, ExcelError> {
        Ok(ReferenceValue::Cell(target))
    }

    fn resolve_range_reference(
        &mut self,
        reader: &CellId,
        range: RangeDescriptor,
    ) -> Result<ReferenceValue, ExcelError> {
        Ok(self.range_reference(reader, range))
    }

    fn resolve_name_reference(
        &mut self,
        reader: &CellId,
        name: &str,
        current_sheet: &str,
    ) -> Result<ReferenceValue, ExcelError> {
        match self.name_resolution(reader, name, current_sheet)? {
            ResolvedReference::Reference(reference) => Ok(reference),
            ResolvedReference::Constant(_)
            | ResolvedReference::Formula(_)
            | ResolvedReference::DynamicFormula(_) => Err(ExcelError::new(ExcelErrorKind::Value)),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScheduleReport {
    pub requested_count: usize,
    pub requested_cells: Vec<CellId>,
    pub dirty_before: usize,
    pub dirty_after: usize,
    pub evaluated_cells: usize,
    pub evaluation_count: usize,
    pub static_cycle_count: usize,
    pub static_cycle_members: BTreeSet<CellId>,
    pub runtime_cycle_count: usize,
    pub runtime_cycle_members: BTreeSet<CellId>,
    pub cyclic_workspaces: Vec<Vec<CellId>>,
    pub solver_passes: usize,
    pub unsupported_formula_count: usize,
    pub elapsed_ns: u128,
    pub trace: TraceReport,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PocModelStats {
    pub formula_count: usize,
    pub defined_name_count: usize,
    pub symbolic_dependency_descriptor_count: usize,
    pub persistent_relation_count: usize,
    pub invalidation_index_count: usize,
    pub memory_state_bytes: usize,
    pub opaque_formula_count: usize,
}

pub struct PocEngine {
    cells: BTreeMap<CellId, CellState>,
    formulas: BTreeMap<CellId, FormulaRecord>,
    pending_direct_dependents: BTreeMap<CellId, BTreeSet<CellId>>,
    pending_range_dependents: Vec<(RangeDescriptor, CellId)>,
    names: NameRegistry,
    spills: BTreeMap<CellId, SpillRef>,
    dirty: BTreeSet<CellId>,
    active: Vec<CellId>,
    trace: TraceReport,
    data_generation: u64,
    structural_generation: u64,
    recalc_epoch: u64,
    max_iterations: usize,
    iterative: bool,
    static_cycle_diagnostics: bool,
    track_formula_reads: bool,
    diagnostic_trace_limit: usize,
    bulk_loading: bool,
    opaque_formula_count: usize,
}

impl Default for PocEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl PocEngine {
    pub fn new() -> Self {
        Self {
            cells: BTreeMap::new(),
            formulas: BTreeMap::new(),
            pending_direct_dependents: BTreeMap::new(),
            pending_range_dependents: Vec::new(),
            names: NameRegistry::default(),
            spills: BTreeMap::new(),
            dirty: BTreeSet::new(),
            active: Vec::new(),
            trace: TraceReport::default(),
            data_generation: 0,
            structural_generation: 0,
            recalc_epoch: 0,
            max_iterations: 32,
            iterative: true,
            static_cycle_diagnostics: true,
            track_formula_reads: false,
            diagnostic_trace_limit: 100_000,
            bulk_loading: false,
            opaque_formula_count: 0,
        }
    }

    pub fn with_iteration(mut self, max_iterations: usize) -> Self {
        self.max_iterations = max_iterations.max(1);
        self.iterative = true;
        self
    }

    pub fn with_cycle_error(mut self) -> Self {
        self.iterative = false;
        self
    }

    pub fn with_static_cycle_diagnostics(mut self, enabled: bool) -> Self {
        self.static_cycle_diagnostics = enabled;
        self
    }

    pub fn with_formula_read_tracking(mut self, enabled: bool) -> Self {
        self.track_formula_reads = enabled;
        self
    }

    pub fn with_diagnostic_trace_limit(mut self, limit: usize) -> Self {
        self.diagnostic_trace_limit = limit.max(1);
        self
    }

    pub fn set_diagnostic_trace_limit(&mut self, limit: usize) {
        self.diagnostic_trace_limit = limit.max(1);
    }

    pub fn enable_formula_read_tracking(&mut self) {
        self.track_formula_reads = true;
    }

    pub fn begin_bulk_load(&mut self) {
        self.bulk_loading = true;
        self.dirty.clear();
    }

    pub fn finish_bulk_load(&mut self) {
        self.bulk_loading = false;
        self.rebuild_invalidation_indexes();
        self.dirty.extend(self.formulas.keys().cloned());
    }

    pub fn set_formula_error(
        &mut self,
        sheet: impl Into<String>,
        row: u32,
        col: u32,
        source: &str,
        error: ExcelError,
    ) -> CellId {
        let ast = ASTNode::new(ASTNodeType::Literal(LiteralValue::Error(error)), None);
        let cell = self.set_formula(sheet, row, col, source, ast);
        self.opaque_formula_count = self.opaque_formula_count.saturating_add(1);
        cell
    }

    pub fn model_stats(&self) -> PocModelStats {
        let mut symbolic_dependency_descriptor_count = 0usize;
        let mut persistent_relation_count = 0usize;
        let mut name_relation_count = 0usize;
        for record in self.formulas.values() {
            persistent_relation_count =
                persistent_relation_count.saturating_add(record.static_dependencies.len());
            for dependency in &record.static_dependencies {
                match dependency {
                    DependencyDescriptor::Range(_) => {
                        symbolic_dependency_descriptor_count =
                            symbolic_dependency_descriptor_count.saturating_add(1);
                    }
                    DependencyDescriptor::Name(_) => {
                        name_relation_count = name_relation_count.saturating_add(1);
                    }
                    DependencyDescriptor::Cell(_)
                    | DependencyDescriptor::Selector(_)
                    | DependencyDescriptor::Structural(_)
                    | DependencyDescriptor::Shape(_)
                    | DependencyDescriptor::Effect(_) => {}
                }
            }
        }
        let memory_state_bytes = self
            .cells
            .len()
            .saturating_mul(std::mem::size_of::<(CellId, CellState)>())
            .saturating_add(
                self.formulas
                    .len()
                    .saturating_mul(std::mem::size_of::<(CellId, FormulaRecord)>()),
            )
            .saturating_add(
                persistent_relation_count
                    .saturating_mul(std::mem::size_of::<DependencyDescriptor>()),
            )
            .saturating_add(
                self.names
                    .len()
                    .saturating_mul(std::mem::size_of::<NameDefinitionRecord>()),
            );
        PocModelStats {
            formula_count: self.formulas.len(),
            defined_name_count: self.names.len(),
            symbolic_dependency_descriptor_count,
            persistent_relation_count,
            invalidation_index_count: self
                .pending_direct_dependents
                .len()
                .saturating_add(self.pending_range_dependents.len())
                .saturating_add(name_relation_count),
            memory_state_bytes,
            opaque_formula_count: self.opaque_formula_count,
        }
    }

    pub fn set_cell_value(
        &mut self,
        sheet: impl Into<String>,
        row: u32,
        col: u32,
        value: LiteralValue,
    ) -> CellId {
        let cell = CellId::new(sheet, row, col);
        self.data_generation = self.data_generation.saturating_add(1);
        let state = self.cells.entry(cell.clone()).or_default();
        state.value = value;
        state.formula = None;
        state.generation = self.data_generation;
        self.formulas.remove(&cell);
        if !self.bulk_loading {
            self.mark_event(ChangeEvent::Cell(cell.clone()));
        }
        cell
    }

    pub fn set_formula_text(
        &mut self,
        sheet: impl Into<String>,
        row: u32,
        col: u32,
        source: &str,
    ) -> Result<CellId, ExcelError> {
        let ast = parse(source).map_err(|error| {
            ExcelError::new(ExcelErrorKind::Value).with_message(error.to_string())
        })?;
        Ok(self.set_formula(sheet, row, col, source, ast))
    }

    pub fn set_formula(
        &mut self,
        sheet: impl Into<String>,
        row: u32,
        col: u32,
        source: &str,
        ast: ASTNode,
    ) -> CellId {
        let cell = CellId::new(sheet, row, col);
        let had_existing_formula = self.formulas.contains_key(&cell);
        let had_pending_direct = !self.bulk_loading
            && self
                .pending_direct_dependents
                .get(&cell)
                .is_some_and(|dependents| !dependents.is_empty());
        let had_pending_range = !self.bulk_loading
            && self
                .pending_range_dependents
                .iter()
                .any(|(range, _)| range.contains(&cell));
        self.data_generation = self.data_generation.saturating_add(1);
        let dependencies = collect_invalidation_dependencies(&ast, &cell.sheet, &self.names);
        let record = FormulaRecord {
            address: cell.clone(),
            source: source.to_string(),
            ast: ast.clone(),
            generation: self.data_generation,
            static_dependencies: dependencies.clone(),
        };
        self.formulas.insert(cell.clone(), record);
        for dependency in &dependencies {
            match dependency {
                DependencyDescriptor::Cell(target) | DependencyDescriptor::Selector(target) => {
                    self.pending_direct_dependents
                        .entry(target.clone())
                        .or_default()
                        .insert(cell.clone());
                }
                DependencyDescriptor::Range(range) => {
                    self.pending_range_dependents
                        .push((range.clone(), cell.clone()));
                }
                DependencyDescriptor::Name(_)
                | DependencyDescriptor::Structural(_)
                | DependencyDescriptor::Shape(_)
                | DependencyDescriptor::Effect(_) => {}
            }
        }
        let state = self.cells.entry(cell.clone()).or_default();
        state.formula = Some(ast);
        state.value = LiteralValue::Empty;
        state.generation = self.data_generation;
        if !self.bulk_loading && (had_existing_formula || had_pending_direct || had_pending_range) {
            self.mark_event(ChangeEvent::Cell(cell.clone()));
        }
        self.dirty.insert(cell.clone());
        cell
    }

    pub fn define_name(
        &mut self,
        name: impl Into<String>,
        scope: NameScope,
        scope_sheet: Option<String>,
        definition: NameDefinition,
    ) -> NameId {
        let id = self.names.define(name, scope, scope_sheet, definition);
        self.data_generation = self.data_generation.saturating_add(1);
        let affected: Vec<(CellId, ASTNode, String)> = self
            .formulas
            .iter()
            .map(|(cell, record)| (cell.clone(), record.ast.clone(), record.source.clone()))
            .collect();
        for (cell, ast, source) in affected {
            let dependencies = collect_invalidation_dependencies(&ast, &cell.sheet, &self.names);
            if let Some(record) = self.formulas.get_mut(&cell) {
                record.static_dependencies = dependencies;
                record.generation = self.data_generation;
                record.source = source;
            }
        }
        self.mark_event(ChangeEvent::Name(id.clone()));
        id
    }

    pub fn set_spill(&mut self, spill: SpillRef) {
        self.spills.insert(spill.anchor.clone(), spill.clone());
        self.names.update_spill(&spill);
        self.data_generation = self.data_generation.saturating_add(1);
        self.mark_event(ChangeEvent::Spill(spill));
    }

    pub fn insert_rows(&mut self, sheet: &str, before: u32, count: u32) {
        if count == 0 {
            return;
        }
        let mut shifted_cells = BTreeMap::new();
        for (cell, state) in std::mem::take(&mut self.cells) {
            let shifted = if cell.sheet == sheet && cell.row >= before {
                CellId::new(cell.sheet, cell.row.saturating_add(count), cell.col)
            } else {
                cell
            };
            shifted_cells.insert(shifted, state);
        }
        self.cells = shifted_cells;
        let mut shifted_formulas = BTreeMap::new();
        for (cell, mut record) in std::mem::take(&mut self.formulas) {
            let shifted = if cell.sheet == sheet && cell.row >= before {
                CellId::new(cell.sheet, cell.row.saturating_add(count), cell.col)
            } else {
                cell
            };
            record.address = shifted.clone();
            record.static_dependencies =
                collect_invalidation_dependencies(&record.ast, &shifted.sheet, &self.names);
            shifted_formulas.insert(shifted, record);
        }
        self.formulas = shifted_formulas;
        let mut shifted_spills = BTreeMap::new();
        for (anchor, mut spill) in std::mem::take(&mut self.spills) {
            if anchor.sheet == sheet && anchor.row >= before {
                spill.anchor =
                    CellId::new(anchor.sheet, anchor.row.saturating_add(count), anchor.col);
            }
            shifted_spills.insert(spill.anchor.clone(), spill);
        }
        self.spills = shifted_spills;
        self.names.shift_rows(sheet, before, count);
        self.structural_generation = self.structural_generation.saturating_add(1);
        self.data_generation = self.data_generation.saturating_add(1);
        self.mark_event(ChangeEvent::Structural(sheet.to_string()));
    }

    pub fn get_cell_value(&self, cell: &CellId) -> LiteralValue {
        self.cells
            .get(cell)
            .map(|state| state.value.clone())
            .unwrap_or(LiteralValue::Empty)
    }

    pub fn formula(&self, cell: &CellId) -> Option<&FormulaRecord> {
        self.formulas.get(cell)
    }

    pub fn names(&self) -> &NameRegistry {
        &self.names
    }

    pub fn formula_count(&self) -> usize {
        self.formulas.len()
    }

    pub fn dirty_count(&self) -> usize {
        self.dirty.len()
    }

    pub fn find_unrelated_cell(&self, sheet: &str, excluded: &[CellId]) -> Option<CellId> {
        for row in 1..=64 {
            for col in 1..=32 {
                let candidate = CellId::new(sheet, row, col);
                if excluded.iter().any(|cell| cell == &candidate)
                    || self.formulas.contains_key(&candidate)
                {
                    continue;
                }
                let unrelated = self.formulas.values().all(|record| {
                    record.static_dependencies.iter().all(|dependency| {
                        !matches!(
                            dependency,
                            DependencyDescriptor::Cell(cell)
                                | DependencyDescriptor::Selector(cell) if cell == &candidate
                        ) && !matches!(
                            dependency,
                            DependencyDescriptor::Range(range) if range.contains(&candidate)
                        )
                    })
                });
                if unrelated {
                    return Some(candidate);
                }
            }
        }
        None
    }

    pub fn invalidate_effect(&mut self, effect: EffectKey) {
        self.data_generation = self.data_generation.saturating_add(1);
        self.mark_event(ChangeEvent::Effect(effect));
    }

    pub fn trace(&self) -> &TraceReport {
        &self.trace
    }

    pub fn static_dependencies(&self, cell: &CellId) -> &[InvalidationDependency] {
        self.formulas
            .get(cell)
            .map(|record| record.static_dependencies.as_slice())
            .unwrap_or(&[])
    }

    pub fn static_edge_count(&self) -> usize {
        self.static_edges().len()
    }

    pub fn static_cycle_components(&self) -> Vec<Vec<CellId>> {
        let edges = self.static_edges();
        cycle_components(self.formulas.keys().cloned().collect(), &edges)
    }

    pub fn calculate_all(&mut self) -> Result<ScheduleReport, ExcelError> {
        let requested: Vec<CellId> = self.formulas.keys().cloned().collect();
        self.calculate(&requested)
    }

    pub fn calculate_requested(
        &mut self,
        requested: &[CellId],
    ) -> Result<ScheduleReport, ExcelError> {
        self.calculate_with_mode(requested, false, false)
    }

    pub fn calculate_requested_force(
        &mut self,
        requested: &[CellId],
    ) -> Result<ScheduleReport, ExcelError> {
        self.calculate_with_mode(requested, true, false)
    }

    pub fn calculate(&mut self, requested: &[CellId]) -> Result<ScheduleReport, ExcelError> {
        self.calculate_with_mode(requested, false, true)
    }

    fn calculate_with_mode(
        &mut self,
        requested: &[CellId],
        force_roots: bool,
        evaluate_remaining: bool,
    ) -> Result<ScheduleReport, ExcelError> {
        let started = Instant::now();
        self.recalc_epoch = self.recalc_epoch.saturating_add(1);
        self.trace = TraceReport::default();
        self.trace.diagnostic_record_limit = self.diagnostic_trace_limit;
        if self.track_formula_reads {
            self.trace.enable_formula_read_tracking();
        }
        self.trace.effects.insert(EffectKey::RecalcEpoch);
        let requested: Vec<CellId> = if requested.is_empty() {
            self.formulas.keys().cloned().collect()
        } else {
            requested.to_vec()
        };
        let dirty_before = self.dirty.len();
        let static_cycles = if self.static_cycle_diagnostics {
            self.static_cycle_components()
        } else {
            Vec::new()
        };
        let mut report = ScheduleReport {
            requested_count: requested.len(),
            requested_cells: requested.clone(),
            dirty_before,
            static_cycle_count: static_cycles.len(),
            static_cycle_members: static_cycles.iter().flatten().cloned().collect(),
            ..ScheduleReport::default()
        };

        let mut evaluated = BTreeSet::new();
        for root in requested {
            if self.formulas.contains_key(&root) {
                if force_roots {
                    self.dirty.insert(root.clone());
                }
                self.evaluate_cell(&root, false, &mut evaluated)?;
            }
        }

        if evaluate_remaining {
            let mut remaining_roots: Vec<CellId> = self.dirty.iter().cloned().collect();
            for root in remaining_roots.drain(..) {
                self.evaluate_cell(&root, false, &mut evaluated)?;
            }
        }

        let mut workspaces = runtime_cycle_components(&self.trace.runtime_formula_edges);
        if !workspaces.is_empty() && self.iterative {
            let mut pass = 0usize;
            loop {
                pass = pass.saturating_add(1);
                let before = workspaces
                    .iter()
                    .flatten()
                    .map(|cell| (cell.clone(), self.get_cell_value(cell)))
                    .collect::<BTreeMap<_, _>>();
                let mut pass_evaluated = BTreeSet::new();
                for workspace in &workspaces {
                    for cell in workspace {
                        self.dirty.insert(cell.clone());
                        self.evaluate_cell(cell, true, &mut pass_evaluated)?;
                    }
                }
                let changed = workspaces
                    .iter()
                    .flatten()
                    .any(|cell| before.get(cell) != Some(&self.get_cell_value(cell)));
                self.trace.solver_passes = self.trace.solver_passes.saturating_add(1);
                if !changed || pass >= self.max_iterations {
                    break;
                }
            }
            workspaces = runtime_cycle_components(&self.trace.runtime_formula_edges);
        } else if !workspaces.is_empty() && !self.iterative {
            for workspace in &workspaces {
                for cell in workspace {
                    self.cells.entry(cell.clone()).or_default().value =
                        LiteralValue::Error(ExcelError::new(ExcelErrorKind::Circ));
                    self.trace.runtime_cycle_members.insert(cell.clone());
                }
            }
        }
        self.trace
            .runtime_cycle_members
            .extend(workspaces.iter().flatten().cloned());
        self.trace.runtime_formula_edges_processed = self.trace.runtime_formula_edges.len();

        report.evaluated_cells = evaluated.len();
        report.evaluation_count = self.trace.evaluation_counts.values().sum();
        report.runtime_cycle_count = workspaces.len();
        report.runtime_cycle_members = self.trace.runtime_cycle_members.clone();
        report.cyclic_workspaces = workspaces;
        report.solver_passes = self.trace.solver_passes;
        report.unsupported_formula_count = self
            .opaque_formula_count
            .saturating_add(self.trace.unsupported_formula_count);
        report.dirty_after = self.dirty.len();
        report.trace = self.trace.clone();
        report.elapsed_ns = started.elapsed().as_nanos();
        Ok(report)
    }

    fn rebuild_invalidation_indexes(&mut self) {
        self.pending_direct_dependents.clear();
        self.pending_range_dependents.clear();
        for (cell, record) in &self.formulas {
            for dependency in &record.static_dependencies {
                match dependency {
                    DependencyDescriptor::Cell(target) | DependencyDescriptor::Selector(target) => {
                        self.pending_direct_dependents
                            .entry(target.clone())
                            .or_default()
                            .insert(cell.clone());
                    }
                    DependencyDescriptor::Range(range) => {
                        self.pending_range_dependents
                            .push((range.clone(), cell.clone()));
                    }
                    DependencyDescriptor::Name(_)
                    | DependencyDescriptor::Structural(_)
                    | DependencyDescriptor::Shape(_)
                    | DependencyDescriptor::Effect(_) => {}
                }
            }
        }
    }

    fn mark_event(&mut self, event: ChangeEvent) {
        if matches!(event, ChangeEvent::Structural(_)) {
            self.dirty.extend(self.formulas.keys().cloned());
            return;
        }
        let mut affected = BTreeSet::new();
        for (cell, formula) in &self.formulas {
            if formula
                .static_dependencies
                .iter()
                .any(|dependency| dependency_matches_event(dependency, &event))
            {
                affected.insert(cell.clone());
            }
        }
        let mut queue: VecDeque<CellId> = affected.iter().cloned().collect();
        while let Some(changed) = queue.pop_front() {
            if let Some(dependents) = self.pending_direct_dependents.get(&changed) {
                for dependent in dependents {
                    if affected.insert(dependent.clone()) {
                        queue.push_back(dependent.clone());
                    }
                }
            }
            for (range, dependent) in &self.pending_range_dependents {
                if range.contains(&changed) && affected.insert(dependent.clone()) {
                    queue.push_back(dependent.clone());
                }
            }
        }
        self.dirty.extend(affected);
    }

    fn static_edges(&self) -> BTreeSet<(CellId, CellId)> {
        let formulas: BTreeSet<CellId> = self.formulas.keys().cloned().collect();
        let mut edges = BTreeSet::new();
        for (source, record) in &self.formulas {
            for dependency in &record.static_dependencies {
                match dependency {
                    DependencyDescriptor::Cell(target) | DependencyDescriptor::Selector(target) => {
                        if formulas.contains(target) {
                            edges.insert((source.clone(), target.clone()));
                        }
                    }
                    DependencyDescriptor::Range(range) => {
                        for target in formulas.iter().filter(|target| range.contains(target)) {
                            edges.insert((source.clone(), target.clone()));
                        }
                    }
                    DependencyDescriptor::Name(name_id) => {
                        if let Some(record) = self.names.get(name_id) {
                            match &record.definition {
                                NameDefinition::Cell(target) => {
                                    if formulas.contains(target) {
                                        edges.insert((source.clone(), target.clone()));
                                    }
                                }
                                NameDefinition::Range(range) => {
                                    for target in
                                        formulas.iter().filter(|target| range.contains(target))
                                    {
                                        edges.insert((source.clone(), target.clone()));
                                    }
                                }
                                NameDefinition::Spill(spill) => {
                                    for target in formulas
                                        .iter()
                                        .filter(|target| spill.range().contains(target))
                                    {
                                        edges.insert((source.clone(), target.clone()));
                                    }
                                }
                                NameDefinition::Constant(_)
                                | NameDefinition::Formula { .. }
                                | NameDefinition::DynamicFormula { .. } => {}
                            }
                        }
                    }
                    DependencyDescriptor::Structural(_)
                    | DependencyDescriptor::Shape(_)
                    | DependencyDescriptor::Effect(_) => {}
                }
            }
        }
        edges
    }

    fn evaluate_cell(
        &mut self,
        cell: &CellId,
        force: bool,
        evaluated: &mut BTreeSet<CellId>,
    ) -> Result<LiteralValue, ExcelError> {
        if self.active.contains(cell) {
            return Ok(self.get_cell_value(cell));
        }
        let Some(formula) = self.formulas.get(cell).cloned() else {
            return Ok(self.get_cell_value(cell));
        };
        if !force && !self.dirty.contains(cell) {
            return Ok(self.get_cell_value(cell));
        }
        self.active.push(cell.clone());
        self.trace
            .evaluation_counts
            .entry(cell.clone())
            .and_modify(|count| *count = count.saturating_add(1))
            .or_insert(1);
        self.trace
            .invalidation_dependencies
            .extend(formula.static_dependencies.iter().cloned());
        let value = match self
            .eval_node(&formula.ast, cell, &cell.sheet)
            .and_then(|result| self.materialize(result, cell))
        {
            Ok(value) => value,
            Err(error) => {
                if matches!(&error.kind, ExcelErrorKind::NImpl) {
                    self.trace.unsupported_formula_count =
                        self.trace.unsupported_formula_count.saturating_add(1);
                }
                LiteralValue::Error(error)
            }
        };
        self.active.pop();
        if let Some(state) = self.cells.get_mut(cell) {
            state.value = value.clone();
        }
        self.dirty.remove(cell);
        evaluated.insert(cell.clone());
        self.trace.schedule_steps = self.trace.schedule_steps.saturating_add(1);
        Ok(value)
    }

    fn read_cell_value(
        &mut self,
        reader: &CellId,
        target: &CellId,
    ) -> Result<LiteralValue, ExcelError> {
        let raw = {
            let mut host =
                EvaluationHost::new(&self.cells, &self.names, &self.spills, &mut self.trace);
            host.raw_cell(reader, target)
        };
        if raw.formula.is_some() {
            self.trace.record_runtime_formula_edge(reader, target);
        }
        if self.active.contains(target) {
            if let Some(position) = self.active.iter().position(|active| active == target) {
                let mut path = self.active[position..].to_vec();
                path.push(target.clone());
                self.trace
                    .record_cycle((reader.clone(), target.clone()), path);
            }
            return Ok(raw.value);
        }
        if raw.formula.is_some() {
            let mut nested = BTreeSet::new();
            let value = self.evaluate_cell(target, false, &mut nested)?;
            self.trace.update_cell_read_value(reader, target, &value);
            Ok(value)
        } else {
            Ok(raw.value)
        }
    }

    fn range_targets(&self, range: &RangeDescriptor) -> Vec<CellId> {
        let start = CellId::new(range.sheet.clone(), range.start_row, range.start_col);
        let end = CellId::new(range.sheet.clone(), range.end_row, range.end_col);
        self.cells
            .range(start..=end)
            .filter(|(cell, _)| range.contains(cell))
            .map(|(cell, _)| cell.clone())
            .collect()
    }

    fn read_range_values(
        &mut self,
        reader: &CellId,
        range: &RangeDescriptor,
    ) -> Result<Vec<Vec<LiteralValue>>, ExcelError> {
        let rows = (range.end_row - range.start_row + 1) as usize;
        let cols = (range.end_col - range.start_col + 1) as usize;
        let area = rows.saturating_mul(cols);
        if area > MAX_MATERIALIZED_RANGE_CELLS {
            return Err(unsupported(format!(
                "range materialization exceeds POC limit: {range} ({area} cells)"
            )));
        }
        let targets = self.range_targets(range);
        self.trace.record_range_read(reader, range.clone());
        self.trace.range_cells_read = self.trace.range_cells_read.saturating_add(area);
        self.trace.record_range_cells_read(reader, range, area);
        self.trace
            .record_empty_cell_reads(area.saturating_sub(targets.len()));
        let mut values = vec![vec![LiteralValue::Empty; cols]; rows];
        for target in targets {
            let value = self.read_cell_value(reader, &target)?;
            let row = (target.row - range.start_row) as usize;
            let col = (target.col - range.start_col) as usize;
            values[row][col] = value;
        }
        Ok(values)
    }

    fn aggregate_reference_values(
        &mut self,
        reader: &CellId,
        reference: &ReferenceValue,
    ) -> Result<Vec<LiteralValue>, ExcelError> {
        let range = match reference {
            ReferenceValue::Cell(cell) => RangeDescriptor::from_cell(cell),
            ReferenceValue::Range(range) => range.clone(),
            ReferenceValue::Spill(spill) => spill.range(),
            ReferenceValue::Table(table) => table.range.clone(),
        };
        let targets = self.range_targets(&range);
        self.trace.record_range_read(reader, range.clone());
        self.trace.range_cells_read = self.trace.range_cells_read.saturating_add(range.area());
        self.trace
            .record_range_cells_read(reader, &range, range.area());
        self.trace
            .record_empty_cell_reads(range.area().saturating_sub(targets.len()));
        targets
            .into_iter()
            .map(|target| self.read_cell_value(reader, &target))
            .collect()
    }

    fn read_reference(
        &mut self,
        reader: &CellId,
        reference: &ReferenceValue,
    ) -> Result<Vec<Vec<LiteralValue>>, ExcelError> {
        match reference {
            ReferenceValue::Cell(cell) => Ok(vec![vec![self.read_cell_value(reader, cell)?]]),
            ReferenceValue::Range(range) => self.read_range_values(reader, range),
            ReferenceValue::Spill(spill) => self.read_range_values(reader, &spill.range()),
            ReferenceValue::Table(table) => self.read_range_values(reader, &table.range),
        }
    }

    fn eval_node(
        &mut self,
        node: &ASTNode,
        reader: &CellId,
        current_sheet: &str,
    ) -> Result<EvaluationResult, ExcelError> {
        match &node.node_type {
            ASTNodeType::Literal(value) => Ok(EvaluationResult::Scalar(value.clone())),
            ASTNodeType::Omitted => Ok(EvaluationResult::Scalar(LiteralValue::Empty)),
            ASTNodeType::Reference { reference, .. } => {
                let resolved = {
                    let mut host = EvaluationHost::new(
                        &self.cells,
                        &self.names,
                        &self.spills,
                        &mut self.trace,
                    );
                    host.resolve_ast_reference(reader, reference, current_sheet)?
                };
                self.resolved_to_result(resolved, reader, current_sheet)
            }
            ASTNodeType::UnaryOp { op, expr } => {
                let value = self.eval_scalar(expr, reader, current_sheet)?;
                match op.as_str() {
                    "+" => Ok(EvaluationResult::Scalar(value)),
                    "-" => Ok(EvaluationResult::Scalar(LiteralValue::Number(-as_number(
                        &value,
                    )?))),
                    _ => Err(unsupported(format!("unary operator {op}"))),
                }
            }
            ASTNodeType::BinaryOp { op, left, right } => {
                let left = self.eval_scalar(left, reader, current_sheet)?;
                let right = self.eval_scalar(right, reader, current_sheet)?;
                self.eval_binary(op, left, right)
            }
            ASTNodeType::Function { name, args } => {
                self.eval_function(name, args, reader, current_sheet)
            }
            ASTNodeType::Call { .. } => Err(unsupported("generic callable")),
            ASTNodeType::Array(rows) => {
                let mut values = Vec::with_capacity(rows.len());
                for row in rows {
                    let mut row_values = Vec::with_capacity(row.len());
                    for item in row {
                        row_values.push(self.eval_scalar(item, reader, current_sheet)?);
                    }
                    values.push(row_values);
                }
                Ok(EvaluationResult::Array(values))
            }
        }
    }

    fn resolved_to_result(
        &mut self,
        resolved: ResolvedReference,
        reader: &CellId,
        current_sheet: &str,
    ) -> Result<EvaluationResult, ExcelError> {
        match resolved {
            ResolvedReference::Reference(reference) => Ok(EvaluationResult::Reference(reference)),
            ResolvedReference::Constant(value) => Ok(EvaluationResult::Scalar(value)),
            ResolvedReference::Formula(ast) => self.eval_node(&ast, reader, current_sheet),
            ResolvedReference::DynamicFormula(ast) => self.eval_node(&ast, reader, current_sheet),
        }
    }

    fn eval_scalar(
        &mut self,
        node: &ASTNode,
        reader: &CellId,
        current_sheet: &str,
    ) -> Result<LiteralValue, ExcelError> {
        let result = self.eval_node(node, reader, current_sheet)?;
        self.materialize(result, reader)
    }

    fn eval_reference(
        &mut self,
        node: &ASTNode,
        reader: &CellId,
        current_sheet: &str,
    ) -> Result<ReferenceValue, ExcelError> {
        match &node.node_type {
            ASTNodeType::Reference { reference, .. } => {
                let resolved = {
                    let mut host = EvaluationHost::new(
                        &self.cells,
                        &self.names,
                        &self.spills,
                        &mut self.trace,
                    );
                    host.resolve_ast_reference(reader, reference, current_sheet)?
                };
                match resolved {
                    ResolvedReference::Reference(reference) => Ok(reference),
                    ResolvedReference::Formula(ast) | ResolvedReference::DynamicFormula(ast) => {
                        self.eval_reference(&ast, reader, current_sheet)
                    }
                    ResolvedReference::Constant(_) => Err(ExcelError::new(ExcelErrorKind::Value)),
                }
            }
            ASTNodeType::Function { name, args } => {
                let name = normalize_function_name(name);
                match name.as_str() {
                    "IF" => {
                        if args.len() < 2 {
                            return Err(ExcelError::new(ExcelErrorKind::Value));
                        }
                        let condition = self.eval_scalar(&args[0], reader, current_sheet)?;
                        if let LiteralValue::Error(error) = &condition {
                            return Err(error.clone());
                        }
                        let selected = if truthy(&condition) {
                            &args[1]
                        } else {
                            args.get(2).unwrap_or(&args[1])
                        };
                        self.eval_reference(selected, reader, current_sheet)
                    }
                    "CHOOSE" => {
                        if args.is_empty() {
                            return Err(ExcelError::new(ExcelErrorKind::Value));
                        }
                        let index =
                            as_index(&self.eval_scalar(&args[0], reader, current_sheet)?)?;
                        let selected = args
                            .get(index)
                            .ok_or_else(|| ExcelError::new(ExcelErrorKind::Value))?;
                        self.eval_reference(selected, reader, current_sheet)
                    }
                    "INDEX" => self.index_reference(args, reader, current_sheet),
                    "OFFSET" => self.offset_reference(args, reader, current_sheet),
                    "INDIRECT" => self.indirect_reference(args, reader, current_sheet),
                    _ => Err(ExcelError::new(ExcelErrorKind::Value)),
                }
            }
            ASTNodeType::BinaryOp { op, left, right } if op == ":" => {
                let left = self.eval_reference(left, reader, current_sheet)?;
                let right = self.eval_reference(right, reader, current_sheet)?;
                combine_reference(left, right)
            }
            _ => Err(ExcelError::new(ExcelErrorKind::Value)),
        }
    }

    fn eval_function(
        &mut self,
        name: &str,
        args: &[ASTNode],
        reader: &CellId,
        current_sheet: &str,
    ) -> Result<EvaluationResult, ExcelError> {
        let name = normalize_function_name(name);
        match name.as_str() {
            "IF" => {
                if args.len() < 2 {
                    return Err(ExcelError::new(ExcelErrorKind::Value));
                }
                let condition = self.eval_scalar(&args[0], reader, current_sheet)?;
                if let LiteralValue::Error(error) = &condition {
                    return Err(error.clone());
                }
                let selected = if truthy(&condition) {
                    &args[1]
                } else {
                    args.get(2).unwrap_or(&args[1])
                };
                self.eval_node(selected, reader, current_sheet)
            }
            "IFS" => {
                let mut pairs = args.chunks_exact(2);
                for pair in &mut pairs {
                    let condition = self.eval_scalar(&pair[0], reader, current_sheet)?;
                    if let LiteralValue::Error(error) = &condition {
                        return Err(error.clone());
                    }
                    if truthy(&condition) {
                        return self.eval_node(&pair[1], reader, current_sheet);
                    }
                }
                Err(ExcelError::new(ExcelErrorKind::Na))
            }
            "CHOOSE" => {
                if args.is_empty() {
                    return Err(ExcelError::new(ExcelErrorKind::Value));
                }
                let index = as_index(&self.eval_scalar(&args[0], reader, current_sheet)?)?;
                let selected = args
                    .get(index)
                    .ok_or_else(|| ExcelError::new(ExcelErrorKind::Value))?;
                self.eval_node(selected, reader, current_sheet)
            }
            "IFERROR" => {
                if args.len() < 2 {
                    return Err(ExcelError::new(ExcelErrorKind::Value));
                }
                let value = match self.eval_node(&args[0], reader, current_sheet) {
                    Ok(value) => value,
                    Err(_) => {
                        return self.eval_node(
                            args.get(1)
                                .ok_or_else(|| ExcelError::new(ExcelErrorKind::Value))?,
                            reader,
                            current_sheet,
                        );
                    }
                };
                if matches!(value, EvaluationResult::Error(_))
                    || matches!(value, EvaluationResult::Scalar(LiteralValue::Error(_)))
                {
                    self.eval_node(
                        args.get(1)
                            .ok_or_else(|| ExcelError::new(ExcelErrorKind::Value))?,
                        reader,
                        current_sheet,
                    )
                } else {
                    Ok(value)
                }
            }
            "AND" => {
                for arg in args {
                    if !truthy(&self.eval_scalar(arg, reader, current_sheet)?) {
                        return Ok(EvaluationResult::Scalar(LiteralValue::Boolean(false)));
                    }
                }
                Ok(EvaluationResult::Scalar(LiteralValue::Boolean(true)))
            }
            "OR" => {
                for arg in args {
                    if truthy(&self.eval_scalar(arg, reader, current_sheet)?) {
                        return Ok(EvaluationResult::Scalar(LiteralValue::Boolean(true)));
                    }
                }
                Ok(EvaluationResult::Scalar(LiteralValue::Boolean(false)))
            }
            "COLUMN" => {
                let column = if let Some(argument) = args.first() {
                    let reference = self.eval_reference(argument, reader, current_sheet)?;
                    reference_dimensions(&reference).1
                } else {
                    reader.col as usize
                };
                Ok(EvaluationResult::Scalar(LiteralValue::Number(
                    column as f64,
                )))
            }
            "ADDRESS" => {
                if args.len() < 2 {
                    return Err(ExcelError::new(ExcelErrorKind::Value));
                }
                let row = as_index(&self.eval_scalar(&args[0], reader, current_sheet)?)?;
                let column = as_index(&self.eval_scalar(&args[1], reader, current_sheet)?)?;
                let absolute = args
                    .get(2)
                    .map(|arg| self.eval_scalar(arg, reader, current_sheet))
                    .transpose()?
                    .map(|value| as_index(&value))
                    .transpose()?
                    .unwrap_or(1);
                if !(1..=4).contains(&absolute) {
                    return Err(ExcelError::new(ExcelErrorKind::Value));
                }
                let label = column_label(column as u32);
                let address = match absolute {
                    1 => format!("${label}${row}"),
                    2 => format!("{label}${row}"),
                    3 => format!("${label}{row}"),
                    4 => format!("{label}{row}"),
                    _ => unreachable!(),
                };
                Ok(EvaluationResult::Scalar(LiteralValue::Text(address)))
            }
            "SUBSTITUTE" => {
                if args.len() < 3 {
                    return Err(ExcelError::new(ExcelErrorKind::Value));
                }
                let text = value_as_text(&self.eval_scalar(&args[0], reader, current_sheet)?)?;
                let old_text =
                    value_as_text(&self.eval_scalar(&args[1], reader, current_sheet)?)?;
                let new_text =
                    value_as_text(&self.eval_scalar(&args[2], reader, current_sheet)?)?;
                let value = if let Some(instance) = args.get(3) {
                    substitute_instance(
                        &text,
                        &old_text,
                        &new_text,
                        as_index(&self.eval_scalar(instance, reader, current_sheet)?)?,
                    )
                } else {
                    text.replace(&old_text, &new_text)
                };
                Ok(EvaluationResult::Scalar(LiteralValue::Text(value)))
            }
            "EDATE" => {
                if args.len() < 2 {
                    return Err(ExcelError::new(ExcelErrorKind::Value));
                }
                let start = self.eval_scalar(&args[0], reader, current_sheet)?;
                let months = self.eval_scalar(&args[1], reader, current_sheet)?;
                Ok(EvaluationResult::Scalar(edate_value(&start, &months)?))
            }
            "INDEX" => {
                let reference = self.index_reference(args, reader, current_sheet)?;
                Ok(EvaluationResult::Reference(reference))
            }
            "MATCH" => self.match_function(args, reader, current_sheet),
            "VLOOKUP" => self.vlookup_function(args, reader, current_sheet),
            "SUM" => self.aggregate(args, reader, current_sheet, Aggregate::Sum),
            "MIN" => self.aggregate(args, reader, current_sheet, Aggregate::Min),
            "SUMPRODUCT" => self.sumproduct(args, reader, current_sheet),
            "OFFSET" => Ok(EvaluationResult::Reference(self.offset_reference(
                args,
                reader,
                current_sheet,
            )?)),
            "INDIRECT" => Ok(EvaluationResult::Reference(self.indirect_reference(
                args,
                reader,
                current_sheet,
            )?)),
            "ROWS" => {
                let Some(argument) = args.first() else {
                    return Err(ExcelError::new(ExcelErrorKind::Value));
                };
                let reference = self.eval_reference(argument, reader, current_sheet)?;
                Ok(EvaluationResult::Scalar(LiteralValue::Number(
                    reference_dimensions(&reference).0 as f64,
                )))
            }
            "COLUMNS" => {
                let Some(argument) = args.first() else {
                    return Err(ExcelError::new(ExcelErrorKind::Value));
                };
                let reference = self.eval_reference(argument, reader, current_sheet)?;
                Ok(EvaluationResult::Scalar(LiteralValue::Number(
                    reference_dimensions(&reference).1 as f64,
                )))
            }
            _ => {
                self.trace.record_unsupported_function(reader, &name);
                self.trace
                    .effects
                    .insert(EffectKey::External(format!("unsupported_function:{name}")));
                Ok(EvaluationResult::Error(unsupported(format!(
                    "function {name}"
                ))))
            }
        }
    }

    fn index_reference(
        &mut self,
        args: &[ASTNode],
        reader: &CellId,
        current_sheet: &str,
    ) -> Result<ReferenceValue, ExcelError> {
        if args.len() < 2 {
            return Err(ExcelError::new(ExcelErrorKind::Value));
        }
        let source = self.eval_reference(&args[0], reader, current_sheet)?;
        let row = as_index_or_zero(&self.eval_scalar(&args[1], reader, current_sheet)?)?;
        let col = if args.len() >= 3 {
            as_index_or_zero(&self.eval_scalar(&args[2], reader, current_sheet)?)?
        } else {
            1
        };
        let (rows, cols) = reference_dimensions(&source);
        if row > rows || col > cols {
            return Err(ExcelError::new(ExcelErrorKind::Ref));
        }
        let selected = match source {
            ReferenceValue::Cell(cell) => {
                if row > 1 || col > 1 {
                    return Err(ExcelError::new(ExcelErrorKind::Ref));
                }
                ReferenceValue::Cell(cell)
            }
            ReferenceValue::Range(range) => index_range_reference(range, row, col),
            ReferenceValue::Spill(spill) => index_range_reference(spill.range(), row, col),
            ReferenceValue::Table(table) => index_range_reference(table.range, row, col),
        };
        self.trace
            .record_selected_reference(reader, selected.clone());
        Ok(selected)
    }

    fn match_function(
        &mut self,
        args: &[ASTNode],
        reader: &CellId,
        current_sheet: &str,
    ) -> Result<EvaluationResult, ExcelError> {
        if args.len() < 2 {
            return Err(ExcelError::new(ExcelErrorKind::Value));
        }
        let needle = self.eval_scalar(&args[0], reader, current_sheet)?;
        let lookup = self.eval_reference(&args[1], reader, current_sheet)?;
        let exact = args
            .get(2)
            .map(|arg| self.eval_scalar(arg, reader, current_sheet))
            .transpose()?
            .map(|value| as_number(&value).unwrap_or(0.0) == 0.0)
            .unwrap_or(true);
        if let ReferenceValue::Range(range) = &lookup
            && range.area() > MAX_MATERIALIZED_RANGE_CELLS
        {
            let position = self.match_large_range(reader, range, &needle, exact)?;
            return Ok(EvaluationResult::Scalar(LiteralValue::Number(
                position as f64,
            )));
        }
        let values = self.read_reference(reader, &lookup)?;
        let mut position = None;
        if values.len() == 1 {
            for (index, value) in values[0].iter().enumerate() {
                if values_equal(value, &needle) {
                    position = Some(index + 1);
                    break;
                }
                if !exact
                    && as_number(value)
                        .ok()
                        .zip(as_number(&needle).ok())
                        .is_some_and(|(a, b)| a <= b)
                {
                    position = Some(index + 1);
                }
            }
        } else {
            for (index, row) in values.iter().enumerate() {
                if row
                    .first()
                    .is_some_and(|value| values_equal(value, &needle))
                {
                    position = Some(index + 1);
                    break;
                }
            }
        }
        let position = position.ok_or_else(|| ExcelError::new(ExcelErrorKind::Na))?;
        Ok(EvaluationResult::Scalar(LiteralValue::Number(
            position as f64,
        )))
    }

    fn vlookup_function(
        &mut self,
        args: &[ASTNode],
        reader: &CellId,
        current_sheet: &str,
    ) -> Result<EvaluationResult, ExcelError> {
        if args.len() < 3 {
            return Err(ExcelError::new(ExcelErrorKind::Value));
        }
        let needle = self.eval_scalar(&args[0], reader, current_sheet)?;
        let table = self.eval_reference(&args[1], reader, current_sheet)?;
        let column = as_index(&self.eval_scalar(&args[2], reader, current_sheet)?)?;
        let exact = args
            .get(3)
            .map(|arg| self.eval_scalar(arg, reader, current_sheet))
            .transpose()?
            .map(|value| as_number(&value).unwrap_or(0.0) == 0.0)
            .unwrap_or(false);
        let values = self.read_reference(reader, &table)?;
        let width = values.first().map(Vec::len).unwrap_or(0);
        if column > width || width == 0 {
            return Err(ExcelError::new(ExcelErrorKind::Ref));
        }
        let mut position = None;
        for (index, row) in values.iter().enumerate() {
            let Some(value) = row.first() else {
                continue;
            };
            if values_equal(value, &needle) {
                position = Some(index);
                break;
            }
            if !exact
                && as_number(value)
                    .ok()
                    .zip(as_number(&needle).ok())
                    .is_some_and(|(left, right)| left <= right)
            {
                position = Some(index);
            }
        }
        let index = position.ok_or_else(|| ExcelError::new(ExcelErrorKind::Na))?;
        Ok(EvaluationResult::Scalar(
            values[index][column.saturating_sub(1)].clone(),
        ))
    }

    fn match_large_range(
        &mut self,
        reader: &CellId,
        range: &RangeDescriptor,
        needle: &LiteralValue,
        exact: bool,
    ) -> Result<usize, ExcelError> {
        let (rows, cols) = (
            (range.end_row - range.start_row + 1) as usize,
            (range.end_col - range.start_col + 1) as usize,
        );
        if rows > 1 && cols > 1 {
            return Err(unsupported(format!(
                "large two-dimensional MATCH range is unsupported: {range}"
            )));
        }
        let area = range.area();
        let targets = self.range_targets(range);
        self.trace.record_range_read(reader, range.clone());
        self.trace.range_cells_read = self.trace.range_cells_read.saturating_add(area);
        self.trace.record_range_cells_read(reader, range, area);
        self.trace
            .record_empty_cell_reads(area.saturating_sub(targets.len()));
        let mut position = None;
        for target in targets {
            let value = self.read_cell_value(reader, &target)?;
            if let LiteralValue::Error(error) = &value {
                if exact {
                    return Err(error.clone());
                }
                continue;
            }
            let index = if rows == 1 {
                target.col - range.start_col + 1
            } else {
                target.row - range.start_row + 1
            } as usize;
            if values_equal(&value, needle) {
                position = Some(index);
                break;
            }
            if !exact
                && as_number(&value)
                    .ok()
                    .zip(as_number(needle).ok())
                    .is_some_and(|(left, right)| left <= right)
            {
                position = Some(index);
            }
        }
        position.ok_or_else(|| ExcelError::new(ExcelErrorKind::Na))
    }

    fn aggregate(
        &mut self,
        args: &[ASTNode],
        reader: &CellId,
        current_sheet: &str,
        aggregate: Aggregate,
    ) -> Result<EvaluationResult, ExcelError> {
        let mut numbers = Vec::new();
        for arg in args {
            let values = match self.eval_node(arg, reader, current_sheet)? {
                EvaluationResult::Reference(reference) => self
                    .aggregate_reference_values(reader, &reference)?
                    .into_iter()
                    .map(|value| vec![value])
                    .collect::<Vec<_>>(),
                EvaluationResult::Array(values) => values,
                EvaluationResult::Scalar(value) => vec![vec![value]],
                EvaluationResult::Error(error) => return Ok(EvaluationResult::Error(error)),
            };
            for value in values.into_iter().flatten() {
                if let LiteralValue::Error(error) = value {
                    return Ok(EvaluationResult::Error(error));
                }
                if let Ok(number) = as_number(&value) {
                    numbers.push(number);
                }
            }
        }
        let result = match aggregate {
            Aggregate::Sum => numbers.iter().sum(),
            Aggregate::Min => numbers.iter().copied().reduce(f64::min).unwrap_or(0.0),
        };
        Ok(EvaluationResult::Scalar(LiteralValue::Number(result)))
    }

    fn sumproduct(
        &mut self,
        args: &[ASTNode],
        reader: &CellId,
        current_sheet: &str,
    ) -> Result<EvaluationResult, ExcelError> {
        let arrays: Vec<Vec<LiteralValue>> = args
            .iter()
            .map(|arg| self.values_from_arg(arg, reader, current_sheet))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|rows| rows.into_iter().flatten().collect())
            .collect();
        let Some(first) = arrays.first() else {
            return Ok(EvaluationResult::Scalar(LiteralValue::Number(0.0)));
        };
        let mut total = 0.0;
        for index in 0..first.len() {
            let mut product = 1.0;
            for array in &arrays {
                let value = array
                    .get(index)
                    .ok_or_else(|| ExcelError::new(ExcelErrorKind::Value))?;
                product *= as_number(value).unwrap_or(0.0);
            }
            total += product;
        }
        Ok(EvaluationResult::Scalar(LiteralValue::Number(total)))
    }

    fn values_from_arg(
        &mut self,
        arg: &ASTNode,
        reader: &CellId,
        current_sheet: &str,
    ) -> Result<Vec<Vec<LiteralValue>>, ExcelError> {
        match self.eval_node(arg, reader, current_sheet)? {
            EvaluationResult::Reference(reference) => self.read_reference(reader, &reference),
            EvaluationResult::Array(values) => Ok(values),
            EvaluationResult::Scalar(value) => Ok(vec![vec![value]]),
            EvaluationResult::Error(error) => Err(error),
        }
    }

    fn offset_reference(
        &mut self,
        args: &[ASTNode],
        reader: &CellId,
        current_sheet: &str,
    ) -> Result<ReferenceValue, ExcelError> {
        if args.len() < 3 {
            return Err(ExcelError::new(ExcelErrorKind::Value));
        }
        let source = self.eval_reference(&args[0], reader, current_sheet)?;
        let rows = as_signed_number(&self.eval_scalar(&args[1], reader, current_sheet)?)?;
        let cols = as_signed_number(&self.eval_scalar(&args[2], reader, current_sheet)?)?;
        let (height, width) = if args.len() >= 5 {
            (
                as_positive_u32(&self.eval_scalar(&args[3], reader, current_sheet)?)?,
                as_positive_u32(&self.eval_scalar(&args[4], reader, current_sheet)?)?,
            )
        } else {
            let (rows, cols) = reference_dimensions(&source);
            (rows as u32, cols as u32)
        };
        let shifted = shift_reference(source, rows, cols, height, width)?;
        let selector = args
            .iter()
            .skip(1)
            .find_map(|arg| selector_cell(arg, reader, current_sheet));
        let mut host = EvaluationHost::new(&self.cells, &self.names, &self.spills, &mut self.trace);
        Ok(host.dynamic_reference(selector, shifted))
    }

    fn indirect_reference(
        &mut self,
        args: &[ASTNode],
        reader: &CellId,
        current_sheet: &str,
    ) -> Result<ReferenceValue, ExcelError> {
        if args.is_empty() {
            return Err(ExcelError::new(ExcelErrorKind::Value));
        }
        let text = self.eval_scalar(&args[0], reader, current_sheet)?;
        let text = match text {
            LiteralValue::Text(text) => text,
            other => other.to_string(),
        };
        let parsed =
            ReferenceType::from_string(&text).map_err(|_| ExcelError::new(ExcelErrorKind::Ref))?;
        let resolved = {
            let mut host =
                EvaluationHost::new(&self.cells, &self.names, &self.spills, &mut self.trace);
            host.resolve_ast_reference(reader, &parsed, current_sheet)?
        };
        let target = match resolved {
            ResolvedReference::Reference(reference) => reference,
            ResolvedReference::Constant(_)
            | ResolvedReference::Formula(_)
            | ResolvedReference::DynamicFormula(_) => {
                return Err(ExcelError::new(ExcelErrorKind::Ref));
            }
        };
        let selector = selector_cell(&args[0], reader, current_sheet);
        let mut host = EvaluationHost::new(&self.cells, &self.names, &self.spills, &mut self.trace);
        Ok(host.dynamic_reference(selector, target))
    }

    fn materialize(
        &mut self,
        result: EvaluationResult,
        reader: &CellId,
    ) -> Result<LiteralValue, ExcelError> {
        match result {
            EvaluationResult::Scalar(value) => Ok(value),
            EvaluationResult::Array(values) => Ok(LiteralValue::Array(values)),
            EvaluationResult::Reference(reference) => self
                .read_reference(reader, &reference)?
                .first()
                .and_then(|row| row.first())
                .cloned()
                .ok_or_else(|| ExcelError::new(ExcelErrorKind::Ref)),
            EvaluationResult::Error(error) => Err(error),
        }
    }

    fn eval_binary(
        &self,
        op: &str,
        left: LiteralValue,
        right: LiteralValue,
    ) -> Result<EvaluationResult, ExcelError> {
        if let LiteralValue::Error(error) = left {
            return Ok(EvaluationResult::Error(error));
        }
        if let LiteralValue::Error(error) = right {
            return Ok(EvaluationResult::Error(error));
        }
        let result = match op {
            "+" => LiteralValue::Number(as_number(&left)? + as_number(&right)?),
            "-" => LiteralValue::Number(as_number(&left)? - as_number(&right)?),
            "*" => LiteralValue::Number(as_number(&left)? * as_number(&right)?),
            "/" => {
                let denominator = as_number(&right)?;
                if denominator == 0.0 {
                    return Ok(EvaluationResult::Error(ExcelError::new(
                        ExcelErrorKind::Div,
                    )));
                }
                LiteralValue::Number(as_number(&left)? / denominator)
            }
            "^" => LiteralValue::Number(as_number(&left)?.powf(as_number(&right)?)),
            "&" => LiteralValue::Text(format!("{}{}", left, right)),
            "=" => LiteralValue::Boolean(values_equal(&left, &right)),
            "<>" => LiteralValue::Boolean(!values_equal(&left, &right)),
            ">" => LiteralValue::Boolean(as_number(&left)? > as_number(&right)?),
            "<" => LiteralValue::Boolean(as_number(&left)? < as_number(&right)?),
            ">=" => LiteralValue::Boolean(as_number(&left)? >= as_number(&right)?),
            "<=" => LiteralValue::Boolean(as_number(&left)? <= as_number(&right)?),
            _ => return Err(unsupported(format!("binary operator {op}"))),
        };
        Ok(EvaluationResult::Scalar(result))
    }
}

#[derive(Clone, Copy)]
enum Aggregate {
    Sum,
    Min,
}

fn normalize_function_name(name: &str) -> String {
    name.rsplit('.').next().unwrap_or(name).to_ascii_uppercase()
}

fn column_label(mut column: u32) -> String {
    let mut label = String::new();
    while column > 0 {
        let remainder = (column - 1) % 26;
        label.push((b'A' + remainder as u8) as char);
        column = (column - 1) / 26;
    }
    label.chars().rev().collect()
}

fn unsupported(message: impl Into<String>) -> ExcelError {
    ExcelError::new(ExcelErrorKind::NImpl).with_message(message.into())
}

fn value_as_text(value: &LiteralValue) -> Result<String, ExcelError> {
    match value {
        LiteralValue::Error(error) => Err(error.clone()),
        LiteralValue::Text(text) => Ok(text.clone()),
        other => Ok(other.to_string()),
    }
}

fn edate_value(value: &LiteralValue, months: &LiteralValue) -> Result<LiteralValue, ExcelError> {
    let serial = value
        .as_serial_number_for(DateSystem::Excel1900)
        .ok_or_else(|| ExcelError::new(ExcelErrorKind::Value))?;
    let months = as_number(months)? as i64;
    let datetime = serial_to_datetime(serial);
    let date = datetime.date();
    let month_index = i64::from(date.year()) * 12 + i64::from(date.month0()) + months;
    let year = month_index.div_euclid(12) as i32;
    let month = month_index.rem_euclid(12) as u32 + 1;
    let first_of_next_month = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    }
    .ok_or_else(|| ExcelError::new(ExcelErrorKind::Value))?;
    let last_day = (first_of_next_month - Duration::days(1)).day();
    let target = NaiveDate::from_ymd_opt(year, month, date.day().min(last_day))
        .ok_or_else(|| ExcelError::new(ExcelErrorKind::Value))?;
    let result = target.and_time(datetime.time());
    LiteralValue::try_from_serial_number_for(DateSystem::Excel1900, datetime_to_serial(&result))
}

fn substitute_instance(text: &str, old_text: &str, new_text: &str, instance: usize) -> String {
    if old_text.is_empty() || instance == 0 {
        return text.to_string();
    }
    let Some((start, end)) = text
        .match_indices(old_text)
        .nth(instance.saturating_sub(1))
        .map(|(start, _)| (start, start + old_text.len()))
    else {
        return text.to_string();
    };
    format!("{}{}{}", &text[..start], new_text, &text[end..])
}

fn as_number(value: &LiteralValue) -> Result<f64, ExcelError> {
    match value {
        LiteralValue::Number(number) => Ok(*number),
        LiteralValue::Int(number) => Ok(*number as f64),
        LiteralValue::Boolean(value) => Ok(if *value { 1.0 } else { 0.0 }),
        LiteralValue::Empty => Ok(0.0),
        LiteralValue::Text(_) => Err(ExcelError::new(ExcelErrorKind::Value)),
        LiteralValue::Error(error) => Err(error.clone()),
        _ => Err(ExcelError::new(ExcelErrorKind::Value)),
    }
}

fn as_signed_number(value: &LiteralValue) -> Result<i64, ExcelError> {
    Ok(as_number(value)? as i64)
}

fn as_positive_u32(value: &LiteralValue) -> Result<u32, ExcelError> {
    let number = as_number(value)? as i64;
    if number <= 0 {
        return Err(ExcelError::new(ExcelErrorKind::Value));
    }
    Ok(number as u32)
}

fn as_index(value: &LiteralValue) -> Result<usize, ExcelError> {
    let number = as_number(value)? as i64;
    if number <= 0 {
        return Err(ExcelError::new(ExcelErrorKind::Value));
    }
    Ok(number as usize)
}

fn as_index_or_zero(value: &LiteralValue) -> Result<usize, ExcelError> {
    let number = as_number(value)? as i64;
    if number < 0 {
        return Err(ExcelError::new(ExcelErrorKind::Value));
    }
    Ok(number as usize)
}

fn truthy(value: &LiteralValue) -> bool {
    value.is_truthy()
}

fn values_equal(left: &LiteralValue, right: &LiteralValue) -> bool {
    match (left, right) {
        (LiteralValue::Number(left), LiteralValue::Int(right))
        | (LiteralValue::Int(right), LiteralValue::Number(left)) => *left == *right as f64,
        _ => left == right,
    }
}

fn index_range_reference(range: RangeDescriptor, row: usize, col: usize) -> ReferenceValue {
    match (row, col) {
        (0, 0) => ReferenceValue::Range(range),
        (0, col) => ReferenceValue::Range(RangeDescriptor::new(
            range.sheet,
            range.start_row,
            range.start_col + col as u32 - 1,
            range.end_row,
            range.start_col + col as u32 - 1,
        )),
        (row, 0) => ReferenceValue::Range(RangeDescriptor::new(
            range.sheet,
            range.start_row + row as u32 - 1,
            range.start_col,
            range.start_row + row as u32 - 1,
            range.end_col,
        )),
        (row, col) => ReferenceValue::Cell(CellId::new(
            range.sheet,
            range.start_row + row as u32 - 1,
            range.start_col + col as u32 - 1,
        )),
    }
}

fn reference_dimensions(reference: &ReferenceValue) -> (usize, usize) {
    match reference {
        ReferenceValue::Cell(_) => (1, 1),
        ReferenceValue::Range(range) => (
            (range.end_row - range.start_row + 1) as usize,
            (range.end_col - range.start_col + 1) as usize,
        ),
        ReferenceValue::Spill(spill) => (spill.rows as usize, spill.cols as usize),
        ReferenceValue::Table(table) => {
            reference_dimensions(&ReferenceValue::Range(table.range.clone()))
        }
    }
}

fn combine_reference(
    left: ReferenceValue,
    right: ReferenceValue,
) -> Result<ReferenceValue, ExcelError> {
    let left = match left {
        ReferenceValue::Cell(cell) => RangeDescriptor::from_cell(&cell),
        ReferenceValue::Range(range) => range,
        ReferenceValue::Spill(spill) => spill.range(),
        ReferenceValue::Table(table) => table.range,
    };
    let right = match right {
        ReferenceValue::Cell(cell) => RangeDescriptor::from_cell(&cell),
        ReferenceValue::Range(range) => range,
        ReferenceValue::Spill(spill) => spill.range(),
        ReferenceValue::Table(table) => table.range,
    };
    if left.sheet != right.sheet {
        return Err(ExcelError::new(ExcelErrorKind::Ref));
    }
    Ok(ReferenceValue::Range(RangeDescriptor::new(
        left.sheet,
        left.start_row.min(right.start_row),
        left.start_col.min(right.start_col),
        left.end_row.max(right.end_row),
        left.end_col.max(right.end_col),
    )))
}

fn shift_reference(
    source: ReferenceValue,
    row_delta: i64,
    col_delta: i64,
    height: u32,
    width: u32,
) -> Result<ReferenceValue, ExcelError> {
    let base = match source {
        ReferenceValue::Cell(cell) => RangeDescriptor::from_cell(&cell),
        ReferenceValue::Range(range) => range,
        ReferenceValue::Spill(spill) => spill.range(),
        ReferenceValue::Table(table) => table.range,
    };
    let start_row = (base.start_row as i64 + row_delta).max(1) as u32;
    let start_col = (base.start_col as i64 + col_delta).max(1) as u32;
    Ok(ReferenceValue::Range(RangeDescriptor::new(
        base.sheet,
        start_row,
        start_col,
        start_row.saturating_add(height.saturating_sub(1)),
        start_col.saturating_add(width.saturating_sub(1)),
    )))
}

fn selector_cell(node: &ASTNode, reader: &CellId, current_sheet: &str) -> Option<CellId> {
    match &node.node_type {
        ASTNodeType::Reference {
            reference: ReferenceType::Cell {
                sheet, row, col, ..
            },
            ..
        } => Some(CellId::new(
            sheet.as_deref().unwrap_or(current_sheet),
            *row,
            *col,
        )),
        _ => {
            let _ = reader;
            None
        }
    }
}

fn runtime_cycle_components(edges: &BTreeSet<(CellId, CellId)>) -> Vec<Vec<CellId>> {
    let nodes: BTreeSet<CellId> = edges
        .iter()
        .flat_map(|(from, to)| [from.clone(), to.clone()])
        .collect();
    cycle_components(nodes, edges)
}

fn cycle_components(
    nodes: BTreeSet<CellId>,
    edges: &BTreeSet<(CellId, CellId)>,
) -> Vec<Vec<CellId>> {
    let mut adjacency: BTreeMap<CellId, Vec<CellId>> = BTreeMap::new();
    let mut reverse: BTreeMap<CellId, Vec<CellId>> = BTreeMap::new();
    for node in &nodes {
        adjacency.entry(node.clone()).or_default();
        reverse.entry(node.clone()).or_default();
    }
    for (from, to) in edges {
        adjacency.entry(from.clone()).or_default().push(to.clone());
        reverse.entry(to.clone()).or_default().push(from.clone());
    }
    for values in adjacency.values_mut() {
        values.sort();
        values.dedup();
    }
    for values in reverse.values_mut() {
        values.sort();
        values.dedup();
    }

    let mut visited = BTreeSet::new();
    let mut order = Vec::with_capacity(nodes.len());
    for root in &nodes {
        if visited.contains(root) {
            continue;
        }
        visited.insert(root.clone());
        let mut stack = vec![(root.clone(), 0usize)];
        while let Some((node, index)) = stack.last_mut() {
            let children = &adjacency[node];
            if *index < children.len() {
                let child = children[*index].clone();
                *index += 1;
                if visited.insert(child.clone()) {
                    stack.push((child, 0));
                }
            } else {
                order.push(node.clone());
                stack.pop();
            }
        }
    }

    let mut assigned = BTreeSet::new();
    let mut components = Vec::new();
    for root in order.into_iter().rev() {
        if assigned.contains(&root) {
            continue;
        }
        let mut component = Vec::new();
        let mut stack = vec![root.clone()];
        assigned.insert(root);
        while let Some(node) = stack.pop() {
            component.push(node.clone());
            for child in &reverse[&node] {
                if assigned.insert(child.clone()) {
                    stack.push(child.clone());
                }
            }
        }
        component.sort();
        let cyclic = component.len() > 1
            || component
                .first()
                .is_some_and(|node| edges.contains(&(node.clone(), node.clone())));
        if cyclic {
            components.push(component);
        }
    }
    components.sort_by_key(|component| component.first().cloned());
    components
}
