use formualizer_common::{ExcelError, ExcelErrorKind, LiteralValue};
use formualizer_parse::parser::{ASTNode, ASTNodeType, ReferenceType};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct CellId {
    pub sheet: String,
    pub row: u32,
    pub col: u32,
}

impl CellId {
    pub fn new(sheet: impl Into<String>, row: u32, col: u32) -> Self {
        Self {
            sheet: sheet.into(),
            row,
            col,
        }
    }

    pub fn from_a1(sheet: impl Into<String>, address: &str) -> Result<Self, ExcelError> {
        let (row, col, _, _) = formualizer_common::parse_a1_1based(address)
            .map_err(|_| ExcelError::new(ExcelErrorKind::Ref))?;
        Ok(Self::new(sheet, row, col))
    }
}

impl fmt::Display for CellId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}!{}{}", self.sheet, column_name(self.col), self.row)
    }
}

fn column_name(mut col: u32) -> String {
    if col == 0 {
        return "?".to_string();
    }
    let mut out = String::new();
    while col > 0 {
        let rem = (col - 1) % 26;
        out.push((b'A' + rem as u8) as char);
        col = (col - 1) / 26;
    }
    out.chars().rev().collect()
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct RangeDescriptor {
    pub sheet: String,
    pub start_row: u32,
    pub start_col: u32,
    pub end_row: u32,
    pub end_col: u32,
}

impl RangeDescriptor {
    pub fn new(
        sheet: impl Into<String>,
        start_row: u32,
        start_col: u32,
        end_row: u32,
        end_col: u32,
    ) -> Self {
        let (start_row, end_row) = if start_row <= end_row {
            (start_row, end_row)
        } else {
            (end_row, start_row)
        };
        let (start_col, end_col) = if start_col <= end_col {
            (start_col, end_col)
        } else {
            (end_col, start_col)
        };
        Self {
            sheet: sheet.into(),
            start_row,
            start_col,
            end_row,
            end_col,
        }
    }

    pub fn from_cell(cell: &CellId) -> Self {
        Self::new(cell.sheet.clone(), cell.row, cell.col, cell.row, cell.col)
    }

    pub fn contains(&self, cell: &CellId) -> bool {
        self.sheet == cell.sheet
            && (self.start_row..=self.end_row).contains(&cell.row)
            && (self.start_col..=self.end_col).contains(&cell.col)
    }

    pub fn area(&self) -> usize {
        (self.end_row - self.start_row + 1) as usize * (self.end_col - self.start_col + 1) as usize
    }

    pub fn cells(&self) -> impl Iterator<Item = CellId> + '_ {
        (self.start_row..=self.end_row).flat_map(move |row| {
            (self.start_col..=self.end_col)
                .map(move |col| CellId::new(self.sheet.clone(), row, col))
        })
    }
}

impl fmt::Display for RangeDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}!{}{}:{}{}",
            self.sheet,
            column_name(self.start_col),
            self.start_row,
            column_name(self.end_col),
            self.end_row
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SpillRef {
    pub anchor: CellId,
    pub rows: u32,
    pub cols: u32,
}

impl SpillRef {
    pub fn range(&self) -> RangeDescriptor {
        RangeDescriptor::new(
            self.anchor.sheet.clone(),
            self.anchor.row,
            self.anchor.col,
            self.anchor.row.saturating_add(self.rows.saturating_sub(1)),
            self.anchor.col.saturating_add(self.cols.saturating_sub(1)),
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct TableDescriptor {
    pub name: String,
    pub range: RangeDescriptor,
    pub generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum ReferenceValue {
    Cell(CellId),
    Range(RangeDescriptor),
    Spill(SpillRef),
    Table(TableDescriptor),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum NameScope {
    Workbook,
    Sheet,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct NameId(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub enum NameDefinition {
    Constant(LiteralValue),
    Cell(CellId),
    Range(RangeDescriptor),
    Formula { ast: ASTNode },
    Spill(SpillRef),
    DynamicFormula { ast: ASTNode },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum ResolvedKind {
    Constant,
    Formula,
    CellReference,
    RangeReference,
    TableStructured,
    DynamicReference,
}

impl NameDefinition {
    pub fn resolved_kind(&self) -> ResolvedKind {
        match self {
            Self::Constant(_) => ResolvedKind::Constant,
            Self::Cell(_) => ResolvedKind::CellReference,
            Self::Range(_) | Self::Spill(_) => ResolvedKind::RangeReference,
            Self::Formula { .. } => ResolvedKind::Formula,
            Self::DynamicFormula { .. } => ResolvedKind::DynamicReference,
        }
    }

    pub fn reference(&self) -> Option<ReferenceValue> {
        match self {
            Self::Cell(cell) => Some(ReferenceValue::Cell(cell.clone())),
            Self::Range(range) => Some(ReferenceValue::Range(range.clone())),
            Self::Spill(spill) => Some(ReferenceValue::Spill(spill.clone())),
            Self::Constant(_) | Self::Formula { .. } | Self::DynamicFormula { .. } => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NameDefinitionRecord {
    pub id: NameId,
    pub display_name: String,
    pub scope: NameScope,
    pub scope_sheet: Option<String>,
    pub definition: NameDefinition,
    pub definition_generation: u64,
    pub structural_generation: u64,
    pub resolved_kind: ResolvedKind,
}

#[derive(Clone, Debug, Default)]
pub struct NameRegistry {
    next_id: u32,
    entries: BTreeMap<(Option<String>, String), NameDefinitionRecord>,
    by_id: BTreeMap<NameId, (Option<String>, String)>,
}

impl NameRegistry {
    pub fn shift_rows(&mut self, sheet: &str, before: u32, count: u32) {
        for record in self.entries.values_mut() {
            let shift_cell = |cell: &CellId| {
                if cell.sheet == sheet && cell.row >= before {
                    CellId::new(cell.sheet.clone(), cell.row.saturating_add(count), cell.col)
                } else {
                    cell.clone()
                }
            };
            match &mut record.definition {
                NameDefinition::Cell(cell) => *cell = shift_cell(cell),
                NameDefinition::Range(range)
                    if range.sheet == sheet && range.start_row >= before =>
                {
                    range.start_row = range.start_row.saturating_add(count);
                    range.end_row = range.end_row.saturating_add(count);
                }
                NameDefinition::Range(range) if range.sheet == sheet && range.end_row >= before => {
                    range.end_row = range.end_row.saturating_add(count);
                }
                NameDefinition::Spill(spill) => spill.anchor = shift_cell(&spill.anchor),
                NameDefinition::Constant(_)
                | NameDefinition::Formula { .. }
                | NameDefinition::DynamicFormula { .. } => {}
                _ => {}
            }
            let applies =
                record.scope == NameScope::Workbook || record.scope_sheet.as_deref() == Some(sheet);
            if applies {
                record.structural_generation = record.structural_generation.saturating_add(1);
                record.definition_generation = record.definition_generation.saturating_add(1);
            }
        }
    }

    pub fn define(
        &mut self,
        display_name: impl Into<String>,
        scope: NameScope,
        scope_sheet: Option<String>,
        definition: NameDefinition,
    ) -> NameId {
        let display_name = display_name.into();
        let normalized = display_name.to_ascii_uppercase();
        let key_scope = match scope {
            NameScope::Workbook => None,
            NameScope::Sheet => Some(scope_sheet.clone().unwrap_or_default()),
        };
        let key = (key_scope.clone(), normalized);
        if let Some(existing) = self.entries.get_mut(&key) {
            existing.definition_generation = existing.definition_generation.saturating_add(1);
            existing.definition = definition;
            existing.resolved_kind = existing.definition.resolved_kind();
            existing.display_name = display_name;
            existing.scope_sheet = scope_sheet;
            return existing.id.clone();
        }

        let id = NameId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        let record = NameDefinitionRecord {
            id: id.clone(),
            display_name,
            scope,
            scope_sheet,
            resolved_kind: definition.resolved_kind(),
            definition,
            definition_generation: 1,
            structural_generation: 0,
        };
        self.by_id.insert(id.clone(), key.clone());
        self.entries.insert(key, record);
        id
    }

    pub fn get(&self, id: &NameId) -> Option<&NameDefinitionRecord> {
        self.by_id.get(id).and_then(|key| self.entries.get(key))
    }

    pub fn resolve(&self, name: &str, current_sheet: &str) -> Option<&NameDefinitionRecord> {
        let normalized = name.to_ascii_uppercase();
        self.entries
            .get(&(Some(current_sheet.to_string()), normalized.clone()))
            .or_else(|| self.entries.get(&(None, normalized)))
    }

    pub fn iter(&self) -> impl Iterator<Item = &NameDefinitionRecord> {
        self.entries.values()
    }

    pub fn update_spill(&mut self, spill: &SpillRef) {
        for record in self.entries.values_mut() {
            if let NameDefinition::Spill(existing) = &mut record.definition
                && existing.anchor == spill.anchor
            {
                *existing = spill.clone();
                record.definition_generation = record.definition_generation.saturating_add(1);
                record.resolved_kind = record.definition.resolved_kind();
            }
        }
    }

    pub fn mark_structural_change(&mut self, sheet: &str) {
        for record in self.entries.values_mut() {
            let applies =
                record.scope == NameScope::Workbook || record.scope_sheet.as_deref() == Some(sheet);
            if applies {
                record.structural_generation = record.structural_generation.saturating_add(1);
            }
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum EffectKey {
    RecalcEpoch,
    Clock,
    Random,
    DynamicSelector(CellId),
    DynamicTarget(CellId),
    TargetValue(CellId),
    Shape(SpillRef),
    External(String),
    Structural(String),
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum DependencyDescriptor {
    Cell(CellId),
    Range(RangeDescriptor),
    Name(NameId),
    Selector(CellId),
    Structural(String),
    Shape(SpillRef),
    Effect(EffectKey),
}

pub type InvalidationDependency = DependencyDescriptor;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum ExecutionRead {
    Cell(CellId),
    Range(RangeDescriptor),
    Name(NameId),
    Spill(SpillRef),
    Dynamic {
        selector: Option<CellId>,
        target: ReferenceValue,
    },
    Table(TableDescriptor),
}

#[derive(Clone, Debug, PartialEq)]
pub enum EvaluationResult {
    Scalar(LiteralValue),
    Array(Vec<Vec<LiteralValue>>),
    Reference(ReferenceValue),
    Error(ExcelError),
}

pub type PocValue = EvaluationResult;

impl EvaluationResult {
    pub fn error(kind: ExcelErrorKind) -> Self {
        Self::Error(ExcelError::new(kind))
    }

    pub fn scalar_or_error(self) -> Result<LiteralValue, ExcelError> {
        match self {
            Self::Scalar(value) => Ok(value),
            Self::Array(values) if values.len() == 1 && values[0].len() == 1 => {
                Ok(values[0][0].clone())
            }
            Self::Error(error) => Err(error),
            Self::Reference(_) | Self::Array(_) => Err(ExcelError::new(ExcelErrorKind::Value)),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CellState {
    pub value: LiteralValue,
    pub formula: Option<ASTNode>,
    pub generation: u64,
}

impl Default for CellState {
    fn default() -> Self {
        Self {
            value: LiteralValue::Empty,
            formula: None,
            generation: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FormulaRecord {
    pub address: CellId,
    pub source: String,
    pub ast: ASTNode,
    pub generation: u64,
    pub static_dependencies: Vec<InvalidationDependency>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FormulaReadTrace {
    pub cell_reads: BTreeSet<CellId>,
    pub cell_read_values: BTreeMap<CellId, String>,
    pub formula_reads: BTreeSet<CellId>,
    pub range_reads: BTreeSet<RangeDescriptor>,
    pub range_cells_read: usize,
    pub range_cell_counts: BTreeMap<RangeDescriptor, usize>,
    pub name_resolutions: BTreeSet<NameId>,
    pub selected_references: BTreeSet<ReferenceValue>,
    pub runtime_edges: BTreeSet<(CellId, CellId)>,
    pub unsupported_functions: BTreeSet<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TraceReport {
    pub execution_reads: BTreeSet<ExecutionRead>,
    pub runtime_edges: BTreeSet<(CellId, CellId)>,
    pub runtime_formula_edges: BTreeSet<(CellId, CellId)>,
    pub runtime_cycle_edges: BTreeSet<(CellId, CellId)>,
    pub execution_read_count: usize,
    pub runtime_edge_count: usize,
    pub runtime_formula_edge_events: usize,
    pub runtime_formula_edges_processed: usize,
    pub call_stack_back_edges: usize,
    pub diagnostic_edge_records_dropped: usize,
    pub execution_reads_truncated: bool,
    pub runtime_edges_truncated: bool,
    pub formula_read_traces: BTreeMap<CellId, FormulaReadTrace>,
    pub track_formula_reads: bool,
    pub diagnostic_record_limit: usize,
    pub runtime_cycle_members: BTreeSet<CellId>,
    pub runtime_cycle_paths: Vec<Vec<CellId>>,
    pub invalidation_dependencies: BTreeSet<InvalidationDependency>,
    pub effects: BTreeSet<EffectKey>,
    pub evaluation_counts: BTreeMap<CellId, usize>,
    pub range_cells_read: usize,
    pub name_reads: usize,
    pub dynamic_reads: usize,
    pub unsupported_formula_count: usize,
    pub schedule_steps: usize,
    pub solver_passes: usize,
}

pub type ReadRecorder = TraceReport;

const MAX_RECORDED_READS: usize = 100_000;

impl TraceReport {
    pub(crate) fn enable_formula_read_tracking(&mut self) {
        self.track_formula_reads = true;
    }

    fn formula_trace_mut(&mut self, reader: &CellId) -> &mut FormulaReadTrace {
        self.formula_read_traces.entry(reader.clone()).or_default()
    }

    fn diagnostic_limit(&self) -> usize {
        if self.diagnostic_record_limit == 0 {
            MAX_RECORDED_READS
        } else {
            self.diagnostic_record_limit
        }
    }

    pub(crate) fn record_read(&mut self, read: ExecutionRead) {
        if matches!(read, ExecutionRead::Name(_)) {
            self.name_reads = self.name_reads.saturating_add(1);
        }
        if matches!(read, ExecutionRead::Dynamic { .. }) {
            self.dynamic_reads = self.dynamic_reads.saturating_add(1);
        }
        self.execution_read_count = self.execution_read_count.saturating_add(1);
        if self.execution_reads.len() < self.diagnostic_limit() {
            self.execution_reads.insert(read);
        } else {
            self.execution_reads_truncated = true;
        }
    }

    pub(crate) fn record_cell_read(
        &mut self,
        reader: &CellId,
        target: &CellId,
        value: &LiteralValue,
    ) {
        self.execution_read_count = self.execution_read_count.saturating_add(1);
        if self.execution_reads.len() < self.diagnostic_limit() {
            self.execution_reads
                .insert(ExecutionRead::Cell(target.clone()));
        } else {
            self.execution_reads_truncated = true;
        }
        self.runtime_edge_count = self.runtime_edge_count.saturating_add(1);
        if self.runtime_edges.len() < self.diagnostic_limit() {
            self.runtime_edges.insert((reader.clone(), target.clone()));
        } else {
            self.runtime_edges_truncated = true;
            self.diagnostic_edge_records_dropped =
                self.diagnostic_edge_records_dropped.saturating_add(1);
        }
        if self.track_formula_reads {
            let trace = self.formula_trace_mut(reader);
            trace.cell_reads.insert(target.clone());
            trace
                .cell_read_values
                .insert(target.clone(), format!("{value:?}"));
        }
    }

    pub(crate) fn update_cell_read_value(
        &mut self,
        reader: &CellId,
        target: &CellId,
        value: &LiteralValue,
    ) {
        if self.track_formula_reads {
            self.formula_trace_mut(reader)
                .cell_read_values
                .insert(target.clone(), format!("{value:?}"));
        }
    }

    pub(crate) fn record_runtime_formula_edge(&mut self, reader: &CellId, target: &CellId) {
        self.runtime_formula_edge_events = self.runtime_formula_edge_events.saturating_add(1);
        self.runtime_formula_edges
            .insert((reader.clone(), target.clone()));
        if self.track_formula_reads {
            let trace = self.formula_trace_mut(reader);
            trace.formula_reads.insert(target.clone());
            trace.runtime_edges.insert((reader.clone(), target.clone()));
        }
    }

    pub(crate) fn record_range_read(&mut self, reader: &CellId, range: RangeDescriptor) {
        self.record_read(ExecutionRead::Range(range.clone()));
        if self.track_formula_reads {
            self.formula_trace_mut(reader).range_reads.insert(range);
        }
    }

    pub(crate) fn record_range_cells_read(
        &mut self,
        reader: &CellId,
        range: &RangeDescriptor,
        count: usize,
    ) {
        if self.track_formula_reads {
            let trace = self.formula_trace_mut(reader);
            trace.range_cells_read = trace.range_cells_read.saturating_add(count);
            trace
                .range_cell_counts
                .entry(range.clone())
                .and_modify(|value| *value = value.saturating_add(count))
                .or_insert(count);
        }
    }

    pub(crate) fn record_empty_cell_reads(&mut self, count: usize) {
        self.execution_read_count = self.execution_read_count.saturating_add(count);
        if self.execution_reads.len() >= self.diagnostic_limit() && count > 0 {
            self.execution_reads_truncated = true;
        }
    }

    pub(crate) fn record_name_read(&mut self, reader: &CellId, name: NameId) {
        self.record_read(ExecutionRead::Name(name.clone()));
        if self.track_formula_reads {
            self.formula_trace_mut(reader).name_resolutions.insert(name);
        }
    }

    pub(crate) fn record_selected_reference(&mut self, reader: &CellId, reference: ReferenceValue) {
        if self.track_formula_reads {
            self.formula_trace_mut(reader)
                .selected_references
                .insert(reference);
        }
    }

    pub(crate) fn record_unsupported_function(&mut self, reader: &CellId, name: &str) {
        if self.track_formula_reads {
            self.formula_trace_mut(reader)
                .unsupported_functions
                .insert(name.to_string());
        }
    }

    pub(crate) fn record_cycle(&mut self, edge: (CellId, CellId), path: Vec<CellId>) {
        self.call_stack_back_edges = self.call_stack_back_edges.saturating_add(1);
        self.runtime_cycle_edges.insert(edge);
        for pair in path.windows(2) {
            if let [from, to] = pair {
                self.runtime_cycle_edges.insert((from.clone(), to.clone()));
            }
        }
        for cell in &path {
            self.runtime_cycle_members.insert(cell.clone());
        }
        if !path.is_empty() {
            self.runtime_cycle_paths.push(path);
        }
    }
}

pub fn collect_invalidation_dependencies(
    ast: &ASTNode,
    current_sheet: &str,
    names: &NameRegistry,
) -> Vec<InvalidationDependency> {
    let mut out = BTreeSet::new();
    collect_dependencies_inner(ast, current_sheet, names, DependencyRole::Normal, &mut out);
    out.into_iter().collect()
}

#[derive(Clone, Copy)]
enum DependencyRole {
    Normal,
    Selector,
    Source,
}

fn collect_dependencies_inner(
    node: &ASTNode,
    current_sheet: &str,
    names: &NameRegistry,
    role: DependencyRole,
    out: &mut BTreeSet<InvalidationDependency>,
) {
    match &node.node_type {
        ASTNodeType::Reference { reference, .. } => match reference {
            ReferenceType::Cell {
                sheet, row, col, ..
            } => {
                let cell = CellId::new(sheet.as_deref().unwrap_or(current_sheet), *row, *col);
                if matches!(role, DependencyRole::Selector) {
                    out.insert(DependencyDescriptor::Selector(cell));
                } else {
                    out.insert(DependencyDescriptor::Cell(cell));
                }
            }
            ReferenceType::Range {
                sheet,
                start_row,
                start_col,
                end_row,
                end_col,
                ..
            } => {
                let descriptor = RangeDescriptor::new(
                    sheet.as_deref().unwrap_or(current_sheet),
                    start_row.unwrap_or(1),
                    start_col.unwrap_or(1),
                    end_row.unwrap_or(start_row.unwrap_or(1)),
                    end_col.unwrap_or(start_col.unwrap_or(1)),
                );
                out.insert(DependencyDescriptor::Range(descriptor));
            }
            ReferenceType::NamedRange(name) => {
                if let Some(record) = names.resolve(name, current_sheet) {
                    out.insert(DependencyDescriptor::Name(record.id.clone()));
                    match &record.definition {
                        NameDefinition::Cell(cell) => {
                            out.insert(DependencyDescriptor::Range(RangeDescriptor::from_cell(
                                cell,
                            )));
                        }
                        NameDefinition::Range(range) => {
                            out.insert(DependencyDescriptor::Range(range.clone()));
                        }
                        NameDefinition::Spill(spill) => {
                            out.insert(DependencyDescriptor::Shape(spill.clone()));
                            out.insert(DependencyDescriptor::Range(spill.range()));
                        }
                        NameDefinition::Formula { ast }
                        | NameDefinition::DynamicFormula { ast } => {
                            collect_dependencies_inner(
                                ast,
                                current_sheet,
                                names,
                                DependencyRole::Normal,
                                out,
                            );
                        }
                        NameDefinition::Constant(_) => {}
                    }
                }
            }
            ReferenceType::Table(table) => {
                out.insert(DependencyDescriptor::Effect(EffectKey::External(format!(
                    "table:{}",
                    table.name
                ))));
            }
            ReferenceType::Cell3D { .. }
            | ReferenceType::Range3D { .. }
            | ReferenceType::External(_) => {
                out.insert(DependencyDescriptor::Effect(EffectKey::External(
                    "unsupported_reference".to_string(),
                )));
            }
        },
        ASTNodeType::UnaryOp { expr, .. } => {
            collect_dependencies_inner(expr, current_sheet, names, role, out);
        }
        ASTNodeType::BinaryOp { left, right, .. } => {
            collect_dependencies_inner(left, current_sheet, names, role, out);
            collect_dependencies_inner(right, current_sheet, names, role, out);
        }
        ASTNodeType::Function { name, args } => {
            let name = name.rsplit('.').next().unwrap_or(name).to_ascii_uppercase();
            match name.as_str() {
                "INDEX" => {
                    if let Some(source) = args.first() {
                        collect_dependencies_inner(
                            source,
                            current_sheet,
                            names,
                            DependencyRole::Source,
                            out,
                        );
                    }
                    for selector in args.iter().skip(1) {
                        collect_dependencies_inner(
                            selector,
                            current_sheet,
                            names,
                            DependencyRole::Selector,
                            out,
                        );
                    }
                }
                "OFFSET" => {
                    if let Some(source) = args.first() {
                        collect_dependencies_inner(
                            source,
                            current_sheet,
                            names,
                            DependencyRole::Source,
                            out,
                        );
                    }
                    for selector in args.iter().skip(1) {
                        collect_dependencies_inner(
                            selector,
                            current_sheet,
                            names,
                            DependencyRole::Selector,
                            out,
                        );
                    }
                    out.insert(DependencyDescriptor::Effect(EffectKey::DynamicSelector(
                        CellId::new(current_sheet, 0, 0),
                    )));
                }
                "INDIRECT" => {
                    for selector in args {
                        collect_dependencies_inner(
                            selector,
                            current_sheet,
                            names,
                            DependencyRole::Selector,
                            out,
                        );
                    }
                    out.insert(DependencyDescriptor::Effect(EffectKey::DynamicSelector(
                        CellId::new(current_sheet, 0, 0),
                    )));
                }
                "SUM" | "MIN" | "SUMPRODUCT" => {
                    for arg in args {
                        collect_dependencies_inner(
                            arg,
                            current_sheet,
                            names,
                            DependencyRole::Source,
                            out,
                        );
                    }
                }
                _ => {
                    for arg in args {
                        collect_dependencies_inner(arg, current_sheet, names, role, out);
                    }
                }
            }
        }
        ASTNodeType::Call { callee, args } => {
            collect_dependencies_inner(callee, current_sheet, names, role, out);
            for arg in args {
                collect_dependencies_inner(arg, current_sheet, names, role, out);
            }
        }
        ASTNodeType::Array(rows) => {
            for row in rows {
                for element in row {
                    collect_dependencies_inner(element, current_sheet, names, role, out);
                }
            }
        }
        ASTNodeType::Literal(_) | ASTNodeType::Omitted => {}
    }
}

pub(crate) fn dependency_matches_event(
    dependency: &InvalidationDependency,
    event: &crate::evaluator::ChangeEvent,
) -> bool {
    match (dependency, event) {
        (DependencyDescriptor::Cell(dependency), crate::evaluator::ChangeEvent::Cell(changed))
        | (
            DependencyDescriptor::Selector(dependency),
            crate::evaluator::ChangeEvent::Cell(changed),
        ) => dependency == changed,
        (DependencyDescriptor::Range(range), crate::evaluator::ChangeEvent::Cell(changed)) => {
            range.contains(changed)
        }
        (DependencyDescriptor::Name(dependency), crate::evaluator::ChangeEvent::Name(changed)) => {
            dependency == changed
        }
        (
            DependencyDescriptor::Structural(sheet),
            crate::evaluator::ChangeEvent::Structural(changed),
        ) => sheet == changed,
        (DependencyDescriptor::Shape(spill), crate::evaluator::ChangeEvent::Spill(changed)) => {
            spill.anchor == changed.anchor
        }
        (
            DependencyDescriptor::Effect(EffectKey::DynamicSelector(selector)),
            crate::evaluator::ChangeEvent::Effect(EffectKey::DynamicSelector(changed)),
        ) if selector.row == 0 && selector.col == 0 => true,
        (DependencyDescriptor::Effect(effect), crate::evaluator::ChangeEvent::Effect(changed)) => {
            effect == changed
        }
        _ => false,
    }
}
