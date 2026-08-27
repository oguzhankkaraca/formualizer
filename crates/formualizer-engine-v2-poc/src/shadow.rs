use crate::evaluator::{PocEngine, PocModelStats};
use crate::model::{
    CellId, DependencyDescriptor, FormulaRecord, NameDefinition, NameRegistry, NameScope,
    RangeDescriptor, ReferenceValue, TraceReport, collect_invalidation_dependencies,
};
use formualizer_common::{ExcelError, ExcelErrorKind, LiteralValue};
use formualizer_parse::parser::{ASTNode, ASTNodeType, parse};
use formualizer_workbook::{
    CalamineAdapter, DefinedNameDefinition, DefinedNameScope, SpreadsheetReader,
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Instant;
use zip::ZipArchive;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum ShadowRelation {
    DirectCell,
    SymbolicRange,
    NameDefinition,
    Selector,
    Shape,
    DynamicEffect,
    LegacyRuntimeRead,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShadowMetrics {
    pub profile: String,
    pub input_source: String,
    pub workbook_available: bool,
    pub formula_vertices: Option<usize>,
    pub name_definition_count: Option<usize>,
    pub symbolic_range_descriptor_count: Option<usize>,
    pub selector_descriptor_count: Option<usize>,
    pub invalidation_dependency_count: Option<usize>,
    pub persistent_relation_count: Option<usize>,
    pub direct_static_edge_count: Option<usize>,
    pub legacy_static_edge_count: Option<usize>,
    pub legacy_runtime_read_edge_count: Option<usize>,
    pub static_cycle_candidate_size: Option<usize>,
    pub legacy_runtime_cycle_size: Option<usize>,
    pub legacy_runtime_cycle_members: Option<usize>,
    pub dirty_closure: Option<usize>,
    pub no_op_schedule_ms: Option<f64>,
    pub no_op_evaluations: Option<usize>,
    pub graph_build_time_ms: Option<f64>,
    pub memory_bytes_estimate: Option<usize>,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct ShadowModel {
    formulas: BTreeMap<CellId, FormulaRecord>,
    names: NameRegistry,
    runtime_reads: BTreeSet<(CellId, CellId)>,
}

impl ShadowModel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn define_name(
        &mut self,
        name: impl Into<String>,
        scope: NameScope,
        scope_sheet: Option<String>,
        definition: NameDefinition,
    ) {
        self.names.define(name, scope, scope_sheet, definition);
        self.refresh_dependencies();
    }

    pub fn add_formula_text(
        &mut self,
        sheet: impl Into<String>,
        row: u32,
        col: u32,
        source: &str,
    ) -> Result<CellId, String> {
        let ast = parse(source).map_err(|error| error.to_string())?;
        Ok(self.add_formula(sheet, row, col, source, ast))
    }

    pub fn add_formula(
        &mut self,
        sheet: impl Into<String>,
        row: u32,
        col: u32,
        source: &str,
        ast: ASTNode,
    ) -> CellId {
        let address = CellId::new(sheet, row, col);
        let static_dependencies =
            collect_invalidation_dependencies(&ast, &address.sheet, &self.names);
        self.formulas.insert(
            address.clone(),
            FormulaRecord {
                address: address.clone(),
                source: source.to_string(),
                ast,
                generation: 1,
                static_dependencies,
            },
        );
        address
    }

    pub fn record_runtime_read(&mut self, reader: CellId, target: CellId) {
        self.runtime_reads.insert((reader, target));
    }

    pub fn formula_count(&self) -> usize {
        self.formulas.len()
    }

    pub fn name_count(&self) -> usize {
        self.names.len()
    }

    pub fn formula(&self, address: &CellId) -> Option<&FormulaRecord> {
        self.formulas.get(address)
    }

    pub fn build_metrics(&self, profile: impl Into<String>) -> ShadowMetrics {
        let mut direct = 0usize;
        let mut ranges = 0usize;
        let mut names = 0usize;
        let mut selectors = 0usize;
        let mut effects = 0usize;
        let mut records = 0usize;
        for formula in self.formulas.values() {
            records = records.saturating_add(formula.static_dependencies.len());
            for dependency in &formula.static_dependencies {
                match dependency {
                    DependencyDescriptor::Cell(_) => direct = direct.saturating_add(1),
                    DependencyDescriptor::Range(_) => ranges = ranges.saturating_add(1),
                    DependencyDescriptor::Name(_) => names = names.saturating_add(1),
                    DependencyDescriptor::Selector(_) => selectors = selectors.saturating_add(1),
                    DependencyDescriptor::Structural(_)
                    | DependencyDescriptor::Shape(_)
                    | DependencyDescriptor::Effect(_) => effects = effects.saturating_add(1),
                }
            }
        }
        ShadowMetrics {
            profile: profile.into(),
            input_source: "synthetic_formula_model".to_string(),
            workbook_available: true,
            formula_vertices: Some(self.formulas.len()),
            name_definition_count: Some(self.names.len()),
            symbolic_range_descriptor_count: Some(ranges),
            selector_descriptor_count: Some(selectors),
            invalidation_dependency_count: Some(records),
            persistent_relation_count: Some(records),
            direct_static_edge_count: Some(direct),
            legacy_static_edge_count: None,
            legacy_runtime_read_edge_count: Some(self.runtime_reads.len()),
            static_cycle_candidate_size: None,
            legacy_runtime_cycle_size: None,
            legacy_runtime_cycle_members: None,
            dirty_closure: None,
            no_op_schedule_ms: None,
            no_op_evaluations: None,
            graph_build_time_ms: None,
            memory_bytes_estimate: None,
            notes: vec![format!("{effects} effect/shape descriptors retained")],
        }
    }

    fn refresh_dependencies(&mut self) {
        for (address, formula) in &mut self.formulas {
            formula.static_dependencies =
                collect_invalidation_dependencies(&formula.ast, &address.sheet, &self.names);
        }
    }
}

pub fn build_xlsx_shadow_metrics(path: impl AsRef<Path>) -> Result<ShadowMetrics, String> {
    let path = path.as_ref();
    let started = Instant::now();
    let mut adapter = CalamineAdapter::open_path(path)
        .map_err(|error| format!("open {}: {error}", path.display()))?;
    let sheets = adapter
        .sheet_names()
        .map_err(|error| format!("list sheets in {}: {error}", path.display()))?;
    if sheets.is_empty() {
        return Err(format!("{} has no worksheets", path.display()));
    }
    let definitions = adapter
        .defined_names()
        .map_err(|error| format!("read names from {}: {error}", path.display()))?;
    let mut model = ShadowModel::new();
    let mut name_parse_failures = 0usize;
    for defined_name in definitions {
        let scope = match defined_name.scope {
            DefinedNameScope::Workbook => NameScope::Workbook,
            DefinedNameScope::Sheet => NameScope::Sheet,
        };
        let definition = match defined_name.definition {
            DefinedNameDefinition::Range { address } => {
                NameDefinition::Range(crate::model::RangeDescriptor::new(
                    address.sheet,
                    address.start_row,
                    address.start_col,
                    address.end_row,
                    address.end_col,
                ))
            }
            DefinedNameDefinition::Literal { value } => NameDefinition::Constant(value),
            DefinedNameDefinition::Formula { formula } => {
                let source = if formula.trim_start().starts_with('=') {
                    formula
                } else {
                    format!("={formula}")
                };
                match parse(&source) {
                    Ok(ast) => NameDefinition::Formula { ast },
                    Err(_) => {
                        name_parse_failures = name_parse_failures.saturating_add(1);
                        continue;
                    }
                }
            }
        };
        model.define_name(
            defined_name.name,
            scope,
            defined_name.scope_sheet,
            definition,
        );
    }
    let mut formula_parse_failures = 0usize;
    for sheet in &sheets {
        let data = adapter
            .read_sheet(sheet)
            .map_err(|error| format!("read sheet {sheet} from {}: {error}", path.display()))?;
        for ((row, col), cell) in data.cells {
            let Some(formula) = cell.formula else {
                continue;
            };
            let source = if formula.trim_start().starts_with('=') {
                formula
            } else {
                format!("={formula}")
            };
            if model.add_formula_text(sheet, row, col, &source).is_err() {
                formula_parse_failures = formula_parse_failures.saturating_add(1);
            }
        }
    }
    let mut metrics = model.build_metrics(
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "xlsx-shadow".to_string()),
    );
    metrics.input_source = "xlsx_calamine_plus_v2_shadow".to_string();
    metrics.graph_build_time_ms = Some(started.elapsed().as_secs_f64() * 1_000.0);
    metrics.memory_bytes_estimate = metrics
        .persistent_relation_count
        .map(|count| count.saturating_mul(std::mem::size_of::<DependencyDescriptor>()));
    metrics
        .notes
        .push(format!("{} worksheets scanned", sheets.len()));
    metrics
        .notes
        .push(format!("{} defined names scanned", model.name_count()));
    if name_parse_failures > 0 {
        metrics.notes.push(format!(
            "{name_parse_failures} formula-backed names could not be parsed"
        ));
    }
    if formula_parse_failures > 0 {
        metrics.notes.push(format!(
            "{formula_parse_failures} formula cells could not be parsed"
        ));
    }
    Ok(metrics)
}

pub fn build_xlsx_shadow_pair_report(
    heavy_path: impl AsRef<Path>,
    light_path: impl AsRef<Path>,
) -> Result<ArtifactShadowReport, String> {
    let heavy_path = heavy_path.as_ref();
    let light_path = light_path.as_ref();
    let heavy = build_xlsx_shadow_metrics(heavy_path)?;
    let light = build_xlsx_shadow_metrics(light_path)?;
    Ok(ArtifactShadowReport {
        heavy,
        light,
        heavy_workbook_found: true,
        light_workbook_found: true,
        artifact_backed: false,
        artifact_paths: vec![heavy_path.display().to_string(), light_path.display().to_string()],
        limitations: vec![
            "The XLSX shadow pass constructs invalidation descriptors independently; runtime reads require an evaluation trace or a later V1 replay.".to_string(),
        ],
    })
}

pub struct XlsxPocModel {
    pub path: String,
    pub engine: PocEngine,
    pub model_stats: PocModelStats,
    pub model_build_time_ms: f64,
    pub worksheets: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RealSequenceStep {
    pub label: String,
    pub dirty_candidates: usize,
    pub formulas_evaluated: usize,
    pub evaluation_events: usize,
    pub exact_runtime_reads: usize,
    pub retained_runtime_read_records: usize,
    pub runtime_read_records_truncated: bool,
    pub runtime_edges: usize,
    pub runtime_formula_edges_generated: usize,
    pub runtime_formula_edges_processed: usize,
    pub runtime_formula_edges_retained: usize,
    pub retained_runtime_edge_records: usize,
    pub diagnostic_edge_records_dropped: usize,
    pub runtime_edge_records_truncated: bool,
    pub call_stack_back_edges: usize,
    pub runtime_cycle_count: usize,
    pub largest_runtime_cyclic_workspace: usize,
    pub workspace_member_addresses: Vec<String>,
    pub solver_passes: usize,
    pub wall_time_ms: f64,
    pub unsupported_formula_count: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RealHeavyPocReport {
    pub source: String,
    pub path: String,
    pub workbook_available: bool,
    pub model_stats: PocModelStats,
    pub model_build_time_ms: f64,
    pub worksheets: Vec<String>,
    pub steps: Vec<RealSequenceStep>,
}

pub fn load_xlsx_poc_model(path: impl AsRef<Path>) -> Result<XlsxPocModel, String> {
    let path = path.as_ref();
    let started = Instant::now();
    let mut adapter = CalamineAdapter::open_path(path)
        .map_err(|error| format!("open {}: {error}", path.display()))?;
    let worksheets = adapter
        .sheet_names()
        .map_err(|error| format!("list sheets in {}: {error}", path.display()))?;
    if worksheets.is_empty() {
        return Err(format!("{} has no worksheets", path.display()));
    }
    let definitions = adapter
        .defined_names()
        .map_err(|error| format!("read defined names from {}: {error}", path.display()))?;
    let mut engine = PocEngine::new()
        .with_iteration(100)
        .with_static_cycle_diagnostics(false);
    engine.begin_bulk_load();
    for defined_name in definitions {
        let name = defined_name.name;
        let scope = match defined_name.scope {
            DefinedNameScope::Workbook => NameScope::Workbook,
            DefinedNameScope::Sheet => NameScope::Sheet,
        };
        let definition = match defined_name.definition {
            DefinedNameDefinition::Range { address } => {
                NameDefinition::Range(crate::model::RangeDescriptor::new(
                    address.sheet,
                    address.start_row,
                    address.start_col,
                    address.end_row,
                    address.end_col,
                ))
            }
            DefinedNameDefinition::Literal { value } => NameDefinition::Constant(value),
            DefinedNameDefinition::Formula { formula } => {
                let source = if formula.trim_start().starts_with('=') {
                    formula
                } else {
                    format!("={formula}")
                };
                match parse(&source) {
                    Ok(ast) => NameDefinition::Formula { ast },
                    Err(error) => NameDefinition::Formula {
                        ast: parse_error_ast(format!("name {name}: {error}")),
                    },
                }
            }
        };
        engine.define_name(name, scope, defined_name.scope_sheet, definition);
    }

    for sheet in &worksheets {
        let data = adapter
            .read_sheet(sheet)
            .map_err(|error| format!("read sheet {sheet} from {}: {error}", path.display()))?;
        for ((row, col), cell) in &data.cells {
            if cell.formula.is_none()
                && let Some(value) = &cell.value
            {
                engine.set_cell_value(sheet, *row, *col, value.clone());
            }
        }
        for ((row, col), cell) in data.cells {
            let Some(formula) = cell.formula else {
                continue;
            };
            if formula.trim().is_empty() {
                continue;
            }
            let source = if formula.trim_start().starts_with('=') {
                formula.clone()
            } else {
                format!("={formula}")
            };
            match parse(&source) {
                Ok(ast) => {
                    engine.set_formula(sheet, row, col, &source, ast);
                }
                Err(error) => {
                    engine.set_formula_error(
                        sheet,
                        row,
                        col,
                        &source,
                        ExcelError::new(ExcelErrorKind::NImpl)
                            .with_message(format!("formula parse unsupported: {error}")),
                    );
                }
            }
        }
    }
    engine.finish_bulk_load();
    let model_stats = engine.model_stats();
    Ok(XlsxPocModel {
        path: path.display().to_string(),
        engine,
        model_stats,
        model_build_time_ms: started.elapsed().as_secs_f64() * 1_000.0,
        worksheets,
    })
}

pub fn run_real_heavy_poc(path: impl AsRef<Path>) -> Result<RealHeavyPocReport, String> {
    run_real_template_poc(path, 7, "F7", false)
}

pub fn run_real_light_poc(path: impl AsRef<Path>) -> Result<RealHeavyPocReport, String> {
    run_real_template_poc(path, 6, "F6", true)
}

fn run_real_template_poc(
    path: impl AsRef<Path>,
    input_row: u32,
    input_label: &str,
    include_unrelated: bool,
) -> Result<RealHeavyPocReport, String> {
    let path = path.as_ref();
    let mut model = load_xlsx_poc_model(path)?;
    if model.model_stats.formula_count == 0 {
        return Err(format!("{} contains no formula cells", path.display()));
    }
    if !model.worksheets.iter().any(|sheet| sheet == "Inputs") {
        return Err(format!("{} has no worksheet named Inputs", path.display()));
    }
    let mut steps = Vec::with_capacity(if include_unrelated { 7 } else { 6 });
    steps.push(run_sequence_step("initial", &mut model.engine)?);
    model
        .engine
        .set_cell_value("Inputs", input_row, 6, LiteralValue::Number(300.0));
    steps.push(run_sequence_step(
        &format!("{input_label}=300"),
        &mut model.engine,
    )?);
    steps.push(run_sequence_step("no-op #1", &mut model.engine)?);
    steps.push(run_sequence_step("no-op #2", &mut model.engine)?);
    model
        .engine
        .set_cell_value("Inputs", input_row, 6, LiteralValue::Number(300.0));
    steps.push(run_sequence_step(
        &format!("same-value {input_label}=300"),
        &mut model.engine,
    )?);
    model
        .engine
        .set_cell_value("Inputs", input_row, 6, LiteralValue::Number(301.0));
    steps.push(run_sequence_step(
        &format!("{input_label}=301"),
        &mut model.engine,
    )?);
    if include_unrelated {
        let input_cell = CellId::new("Inputs", input_row, 6);
        let unrelated = model
            .engine
            .find_unrelated_cell("Inputs", &[input_cell])
            .ok_or_else(|| "could not identify a safe unrelated Inputs cell".to_string())?;
        model.engine.set_cell_value(
            unrelated.sheet.clone(),
            unrelated.row,
            unrelated.col,
            LiteralValue::Number(0.0),
        );
        steps.push(run_sequence_step("unrelated edit", &mut model.engine)?);
    }
    Ok(RealHeavyPocReport {
        source: "real_workbook".to_string(),
        path: model.path,
        workbook_available: true,
        model_stats: model.model_stats,
        model_build_time_ms: model.model_build_time_ms,
        worksheets: model.worksheets,
        steps,
    })
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WitnessCellAudit {
    pub address: String,
    pub formula: String,
    pub evaluation_status: String,
    pub result_or_error: String,
    pub branch_selected: String,
    pub unsupported_functions: Vec<String>,
    pub exact_cell_reads: Vec<String>,
    pub exact_cell_read_values: Vec<String>,
    pub exact_cell_read_formulas: Vec<String>,
    pub exact_formula_reads: Vec<String>,
    pub range_reads: Vec<String>,
    pub range_cells_read: usize,
    pub name_resolutions: Vec<String>,
    pub selected_references: Vec<String>,
    pub emitted_runtime_formula_edges: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ValuePipelineStage {
    pub label: String,
    pub result_type: String,
    pub result_value: String,
    pub error: Option<String>,
    pub reference_identity: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WitnessEdgeAudit {
    pub from: String,
    pub to: String,
    pub present: bool,
    pub reason: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HeavyWitnessAudit {
    pub path: String,
    pub source: String,
    pub workbook_available: bool,
    pub f7_state: String,
    pub cells: Vec<WitnessCellAudit>,
    pub edges: Vec<WitnessEdgeAudit>,
    pub runtime_formula_edges_generated: usize,
    pub runtime_formula_edges_processed: usize,
    pub runtime_formula_edges_retained: usize,
    pub diagnostic_edge_records_stored: usize,
    pub diagnostic_edge_records_dropped: usize,
    pub call_stack_back_edges: usize,
    pub runtime_graph_cyclic_scc_count: usize,
    pub largest_runtime_graph_cyclic_scc: usize,
    pub largest_runtime_graph_cyclic_scc_members: Vec<String>,
    pub j23_required_range: String,
    pub j23_required_range_consumed_cells: usize,
    pub j23_edate_min_supported: bool,
    pub j11_selected_target: Option<String>,
    pub diagnostic_limit_control_passed: bool,
    pub diagnostic_limit_control_default_cycle_count: usize,
    pub diagnostic_limit_control_reduced_cycle_count: usize,
    pub diagnostic_limit_control_default_edge_count: usize,
    pub diagnostic_limit_control_reduced_edge_count: usize,
    pub diagnostic_limit_control_reduced_records_stored: usize,
    pub diagnostic_limit_control_reduced_records_dropped: usize,
    pub j11_value_pipeline: Vec<ValuePipelineStage>,
    pub j9_value_pipeline: Vec<ValuePipelineStage>,
    pub j9: WitnessCellAudit,
    pub witness_chain: Vec<WitnessCellAudit>,
    pub j23_upstream_audits: Vec<WitnessCellAudit>,
    pub j23_value_pipeline: Vec<ValuePipelineStage>,
    pub first_value_divergence: String,
}

pub fn run_real_heavy_witness_audit(path: impl AsRef<Path>) -> Result<HeavyWitnessAudit, String> {
    let path = path.as_ref();
    let mut model = load_xlsx_poc_model(path)?;
    if !model.worksheets.iter().any(|sheet| sheet == "Inputs") {
        return Err(format!("{} has no worksheet named Inputs", path.display()));
    }
    let f7 = CellId::new("Inputs", 7, 6);
    model
        .engine
        .set_cell_value("Inputs", 7, 6, LiteralValue::Number(300.0));
    model.engine.enable_formula_read_tracking();
    let witness = vec![
        CellId::new("CashFlow Inputs", 23, 10),
        CellId::new("CashFlow Engine", 65, 11),
        CellId::new("CashFlow Engine", 65, 9),
        CellId::new("CashFlow Engine", 11, 10),
    ];
    let report = model
        .engine
        .calculate_requested_force(&witness)
        .map_err(|error| format!("witness calculation: {error}"))?;
    let cells = witness
        .iter()
        .map(|cell| witness_cell_audit(&model.engine, &report.trace, cell))
        .collect::<Vec<_>>();
    let edge_pairs = [
        (witness[0].clone(), witness[1].clone()),
        (witness[1].clone(), witness[2].clone()),
        (witness[2].clone(), witness[3].clone()),
        (witness[3].clone(), witness[0].clone()),
    ];
    let edges = edge_pairs
        .iter()
        .map(|(from, to)| witness_edge_audit(&report.trace, from, to))
        .collect::<Vec<_>>();
    let required_range = RangeDescriptor::new("CashFlow Engine", 29, 11, 112, 11);
    let j23_trace = report
        .trace
        .formula_read_traces
        .get(&witness[0])
        .cloned()
        .unwrap_or_default();
    let j23_required_range_consumed_cells = j23_trace
        .range_cell_counts
        .get(&required_range)
        .copied()
        .unwrap_or(0);
    let j23_edate_min_supported = j23_required_range_consumed_cells == required_range.area()
        && !j23_trace
            .unsupported_functions
            .iter()
            .any(|name| name == "EDATE" || name == "MIN");
    let j11_selected_target = report
        .trace
        .formula_read_traces
        .get(&witness[3])
        .and_then(|trace| {
            trace
                .selected_references
                .iter()
                .find_map(|reference| match reference {
                    ReferenceValue::Cell(cell) => Some(cell.to_string()),
                    _ => None,
                })
        });
    let default_cycle_count = report.runtime_cycle_count;
    let default_edges = report.trace.runtime_formula_edges.len();
    let default_members = report.runtime_cycle_members.clone();
    if edges.iter().all(|edge| edge.present) && default_cycle_count == 0 {
        return Err(
            "complete runtime graph contains all Heavy witness edges but cycle detection reported zero cycles"
                .to_string(),
        );
    }

    let mut reduced_model = load_xlsx_poc_model(path)?;
    reduced_model.engine.set_diagnostic_trace_limit(1);
    reduced_model
        .engine
        .set_cell_value("Inputs", 7, 6, LiteralValue::Number(300.0));
    let reduced_report = reduced_model
        .engine
        .calculate_requested_force(&witness)
        .map_err(|error| format!("reduced-trace witness calculation: {error}"))?;
    let reduced_cycle_count = reduced_report.runtime_cycle_count;
    let reduced_edges = reduced_report.trace.runtime_formula_edges.len();
    let reduced_members = reduced_report.runtime_cycle_members.clone();
    let diagnostic_limit_control_passed = default_cycle_count == reduced_cycle_count
        && default_edges == reduced_edges
        && default_members == reduced_members;

    let semantic = semantic_witness_result(path)?;
    Ok(HeavyWitnessAudit {
        path: path.display().to_string(),
        source: "real_workbook".to_string(),
        workbook_available: true,
        f7_state: format!("{}={:?}", f7, LiteralValue::Number(300.0)),
        cells,
        edges,
        runtime_formula_edges_generated: report.trace.runtime_formula_edge_events,
        runtime_formula_edges_processed: report.trace.runtime_formula_edges_processed,
        runtime_formula_edges_retained: report.trace.runtime_formula_edges.len(),
        diagnostic_edge_records_stored: report.trace.runtime_edges.len(),
        diagnostic_edge_records_dropped: report.trace.diagnostic_edge_records_dropped,
        call_stack_back_edges: report.trace.call_stack_back_edges,
        runtime_graph_cyclic_scc_count: default_cycle_count,
        largest_runtime_graph_cyclic_scc: report
            .cyclic_workspaces
            .iter()
            .map(Vec::len)
            .max()
            .unwrap_or(0),
        largest_runtime_graph_cyclic_scc_members: report
            .cyclic_workspaces
            .iter()
            .max_by_key(|workspace| workspace.len())
            .map(|workspace| workspace.iter().map(ToString::to_string).collect())
            .unwrap_or_default(),
        j23_required_range: required_range.to_string(),
        j23_required_range_consumed_cells,
        j23_edate_min_supported,
        j11_selected_target,
        diagnostic_limit_control_passed,
        diagnostic_limit_control_default_cycle_count: default_cycle_count,
        diagnostic_limit_control_reduced_cycle_count: reduced_cycle_count,
        diagnostic_limit_control_default_edge_count: default_edges,
        diagnostic_limit_control_reduced_edge_count: reduced_edges,
        diagnostic_limit_control_reduced_records_stored: reduced_report.trace.runtime_edges.len(),
        diagnostic_limit_control_reduced_records_dropped: reduced_report
            .trace
            .diagnostic_edge_records_dropped,
        j11_value_pipeline: semantic.j11_value_pipeline,
        j9_value_pipeline: semantic.j9_value_pipeline,
        j9: semantic.j9,
        witness_chain: semantic.witness_chain,
        j23_upstream_audits: semantic.j23_upstream_audits,
        j23_value_pipeline: semantic.j23_value_pipeline,
        first_value_divergence: semantic.first_value_divergence,
    })
}

struct SemanticWitnessResult {
    j11_value_pipeline: Vec<ValuePipelineStage>,
    j9_value_pipeline: Vec<ValuePipelineStage>,
    j9: WitnessCellAudit,
    witness_chain: Vec<WitnessCellAudit>,
    j23_upstream_audits: Vec<WitnessCellAudit>,
    j23_value_pipeline: Vec<ValuePipelineStage>,
    first_value_divergence: String,
}

fn semantic_witness_result(path: &Path) -> Result<SemanticWitnessResult, String> {
    let mut model = load_xlsx_poc_model(path)?;
    model
        .engine
        .set_cell_value("Inputs", 7, 6, LiteralValue::Number(300.0));
    model.engine.enable_formula_read_tracking();
    let scratch_sheet = "__V2 Semantic Audit";
    let row_match = model
        .engine
        .set_formula_text(
            scratch_sheet,
            1,
            1,
            "=MATCH('CashFlow Engine'!C11,Cash_Flow_Inputs_R,0)",
        )
        .map_err(|error| error.to_string())?;
    let column_match = model
        .engine
        .set_formula_text(
            scratch_sheet,
            2,
            1,
            "=MATCH('CashFlow Engine'!J6,Cash_Flow_Inputs_C,0)",
        )
        .map_err(|error| error.to_string())?;
    let selected_index = model
        .engine
        .set_formula_text(scratch_sheet, 3, 1, "=INDEX(Cash_Flow_Inputs,A1,A2)")
        .map_err(|error| error.to_string())?;
    let j24 = model
        .engine
        .set_formula_text(scratch_sheet, 4, 1, "='CashFlow Inputs'!J24")
        .map_err(|error| error.to_string())?;
    let min_k29_k112 = model
        .engine
        .set_formula_text(scratch_sheet, 5, 1, "=MIN('CashFlow Engine'!K29:K112)")
        .map_err(|error| error.to_string())?;
    let subtract_12 = model
        .engine
        .set_formula_text(scratch_sheet, 6, 1, "=A5-12")
        .map_err(|error| error.to_string())?;
    let edate_result = model
        .engine
        .set_formula_text(scratch_sheet, 7, 1, "=EDATE(A4,A6)")
        .map_err(|error| error.to_string())?;
    let assumptions_row_match = model
        .engine
        .set_formula_text(
            scratch_sheet,
            8,
            1,
            "=MATCH('CashFlow Inputs'!C9,Assumptions_R,0)",
        )
        .map_err(|error| error.to_string())?;
    let assumptions_column_match = model
        .engine
        .set_formula_text(
            scratch_sheet,
            9,
            1,
            "=MATCH('CashFlow Inputs'!J7,Assumptions_C,0)",
        )
        .map_err(|error| error.to_string())?;
    let assumptions_index = model
        .engine
        .set_formula_text(scratch_sheet, 10, 1, "=INDEX(Assumptions,A8,A9)")
        .map_err(|error| error.to_string())?;
    let j11 = CellId::new("CashFlow Engine", 11, 10);
    let j9 = CellId::new("CashFlow Inputs", 9, 10);
    let i65 = CellId::new("CashFlow Engine", 65, 9);
    let k65 = CellId::new("CashFlow Engine", 65, 11);
    let j23 = CellId::new("CashFlow Inputs", 23, 10);

    let row_report = model
        .engine
        .calculate_requested_force(&[row_match.clone()])
        .map_err(|error| error.to_string())?;
    let column_report = model
        .engine
        .calculate_requested_force(&[column_match.clone()])
        .map_err(|error| error.to_string())?;
    let index_report = model
        .engine
        .calculate_requested_force(&[selected_index.clone()])
        .map_err(|error| error.to_string())?;
    let assumptions_row_report = model
        .engine
        .calculate_requested_force(&[assumptions_row_match.clone()])
        .map_err(|error| error.to_string())?;
    let assumptions_column_report = model
        .engine
        .calculate_requested_force(&[assumptions_column_match.clone()])
        .map_err(|error| error.to_string())?;
    let assumptions_index_report = model
        .engine
        .calculate_requested_force(&[assumptions_index.clone()])
        .map_err(|error| error.to_string())?;
    let selected_reference = index_report
        .trace
        .formula_read_traces
        .get(&selected_index)
        .and_then(|trace| trace.selected_references.iter().next())
        .map(reference_to_string);
    let selected_target_value = model.engine.get_cell_value(&j9);
    let index_value = model.engine.get_cell_value(&selected_index);
    let j9_report = model
        .engine
        .calculate_requested_force(&[j9.clone()])
        .map_err(|error| error.to_string())?;
    let j9_audit = witness_cell_audit(&model.engine, &j9_report.trace, &j9);
    let assumptions_selected_reference = assumptions_index_report
        .trace
        .formula_read_traces
        .get(&assumptions_index)
        .and_then(|trace| trace.selected_references.iter().next())
        .map(reference_to_string);
    let assumptions_index_value = model.engine.get_cell_value(&assumptions_index);
    let _ = (assumptions_row_report, assumptions_column_report);
    let j9_value_pipeline = vec![
        value_pipeline_stage(
            "J9 MATCH row result",
            &model.engine.get_cell_value(&assumptions_row_match),
        ),
        value_pipeline_stage(
            "J9 MATCH column result",
            &model.engine.get_cell_value(&assumptions_column_match),
        ),
        ValuePipelineStage {
            label: "J9 INDEX selected ReferenceValue".to_string(),
            result_type: assumptions_selected_reference
                .as_ref()
                .map(|_| "ReferenceValue".to_string())
                .unwrap_or_else(|| "Error".to_string()),
            result_value: assumptions_selected_reference
                .clone()
                .unwrap_or_else(|| format!("{assumptions_index_value:?}")),
            error: None,
            reference_identity: assumptions_selected_reference,
        },
        value_pipeline_stage("J9 final result", &model.engine.get_cell_value(&j9)),
    ];
    let j11_report = model
        .engine
        .calculate_requested_force(&[j11.clone()])
        .map_err(|error| error.to_string())?;
    let j11_audit = witness_cell_audit(&model.engine, &j11_report.trace, &j11);
    let i65_report = model
        .engine
        .calculate_requested_force(&[i65.clone()])
        .map_err(|error| error.to_string())?;
    let i65_audit = witness_cell_audit(&model.engine, &i65_report.trace, &i65);
    let k65_report = model
        .engine
        .calculate_requested_force(&[k65.clone()])
        .map_err(|error| error.to_string())?;
    let k65_audit = witness_cell_audit(&model.engine, &k65_report.trace, &k65);
    let j23_report = model
        .engine
        .calculate_requested_force(&[j23.clone()])
        .map_err(|error| error.to_string())?;
    let j23_audit = witness_cell_audit(&model.engine, &j23_report.trace, &j23);
    let k40 = CellId::new("CashFlow Engine", 40, 11);
    let k74 = CellId::new("CashFlow Engine", 74, 11);
    let i74 = CellId::new("CashFlow Engine", 74, 9);
    let k40_report = model
        .engine
        .calculate_requested_force(&[k40.clone()])
        .map_err(|error| error.to_string())?;
    let k40_audit = witness_cell_audit(&model.engine, &k40_report.trace, &k40);
    let k74_report = model
        .engine
        .calculate_requested_force(&[k74.clone()])
        .map_err(|error| error.to_string())?;
    let k74_audit = witness_cell_audit(&model.engine, &k74_report.trace, &k74);
    let i74_report = model
        .engine
        .calculate_requested_force(&[i74.clone()])
        .map_err(|error| error.to_string())?;
    let i74_audit = witness_cell_audit(&model.engine, &i74_report.trace, &i74);

    let mut j11_value_pipeline = vec![
        value_pipeline_stage("MATCH row result", &model.engine.get_cell_value(&row_match)),
        value_pipeline_stage(
            "MATCH column result",
            &model.engine.get_cell_value(&column_match),
        ),
    ];
    j11_value_pipeline.push(ValuePipelineStage {
        label: "INDEX selected ReferenceValue".to_string(),
        result_type: selected_reference
            .as_ref()
            .map(|_| "ReferenceValue".to_string())
            .unwrap_or_else(|| "Error".to_string()),
        result_value: selected_reference
            .clone()
            .unwrap_or_else(|| format!("{index_value:?}")),
        error: None,
        reference_identity: selected_reference.clone(),
    });
    j11_value_pipeline.push(value_pipeline_stage(
        "read selected target CashFlow Inputs!J9",
        &selected_target_value,
    ));
    j11_value_pipeline.push(value_pipeline_stage(
        "value stored at CashFlow Inputs!J9",
        &model.engine.get_cell_value(&j9),
    ));
    j11_value_pipeline.push(value_pipeline_stage(
        "value returned by INDEX",
        &index_value,
    ));
    j11_value_pipeline.push(value_pipeline_stage(
        "final CashFlow Engine!J11",
        &model.engine.get_cell_value(&j11),
    ));

    let mut j23_value_pipeline = Vec::new();
    for (label, cell) in [
        ("J24", j24.clone()),
        ("MIN(K29:K112)", min_k29_k112.clone()),
        ("MIN(K29:K112)-12 / EDATE month input", subtract_12.clone()),
        ("EDATE date input", j24.clone()),
        ("EDATE output", edate_result.clone()),
    ] {
        model
            .engine
            .calculate_requested_force(&[cell.clone()])
            .map_err(|error| error.to_string())?;
        j23_value_pipeline.push(value_pipeline_stage(
            &format!("J23 pipeline {label}"),
            &model.engine.get_cell_value(&cell),
        ));
    }
    j23_value_pipeline.push(value_pipeline_stage(
        "final CashFlow Inputs!J23",
        &model.engine.get_cell_value(&j23),
    ));

    let j9_value = model.engine.get_cell_value(&j9);
    let j11_value = model.engine.get_cell_value(&j11);
    let i65_value = model.engine.get_cell_value(&i65);
    let k65_value = model.engine.get_cell_value(&k65);
    let j23_value = model.engine.get_cell_value(&j23);
    let k65_is_blank = matches!(&k65_value, LiteralValue::Empty)
        || matches!(&k65_value, LiteralValue::Text(value) if value.is_empty());
    let first_value_divergence = if !matches!(&j9_value, LiteralValue::Text(value) if value == "SC")
    {
        format!(
            "CashFlow Inputs!J9 diverges first: V2={} while Excel selected-target value is SC; formula={}",
            j9_value, j9_audit.formula
        )
    } else if !matches!(&j11_value, LiteralValue::Text(value) if value == "SC") {
        format!(
            "CashFlow Engine!J11 diverges first: V2={} while Excel=SC",
            j11_value
        )
    } else if !matches!(&i65_value, LiteralValue::Text(value) if value == "No") {
        format!(
            "CashFlow Engine!I65 diverges first: V2={} while Excel=No",
            i65_value
        )
    } else if !k65_is_blank {
        format!(
            "CashFlow Engine!K65 diverges first: V2={} while Excel is blank",
            k65_value
        )
    } else if !matches!(&j23_value, LiteralValue::Date(_)) {
        if matches!(model.engine.get_cell_value(&i74), LiteralValue::Error(_)) {
            format!(
                "CashFlow Inputs!J23 diverges first through CashFlow Engine!I74: V2={} formula={}",
                model.engine.get_cell_value(&i74),
                i74_audit.formula
            )
        } else if matches!(model.engine.get_cell_value(&k74), LiteralValue::Error(_)) {
            format!(
                "CashFlow Inputs!J23 diverges first through CashFlow Engine!K74: V2={} formula={}",
                model.engine.get_cell_value(&k74),
                k74_audit.formula
            )
        } else {
            format!(
                "CashFlow Inputs!J23 diverges: V2={} while Excel is a date",
                j23_value
            )
        }
    } else {
        "no divergence in the bounded witness values".to_string()
    };

    let _ = row_report;
    let _ = column_report;
    let _ = j23_report;
    Ok(SemanticWitnessResult {
        j11_value_pipeline,
        j9_value_pipeline,
        j9: j9_audit,
        witness_chain: vec![j11_audit, i65_audit, k65_audit, j23_audit],
        j23_upstream_audits: vec![k40_audit, k74_audit, i74_audit],
        j23_value_pipeline,
        first_value_divergence,
    })
}

fn value_pipeline_stage(label: &str, value: &LiteralValue) -> ValuePipelineStage {
    let (result_type, error) = match value {
        LiteralValue::Error(error) => ("Error".to_string(), Some(format!("{error:?}"))),
        LiteralValue::Empty => ("Empty".to_string(), None),
        LiteralValue::Boolean(_) => ("Boolean".to_string(), None),
        LiteralValue::Number(_) => ("Number".to_string(), None),
        LiteralValue::Int(_) => ("Int".to_string(), None),
        LiteralValue::Text(_) => ("Text".to_string(), None),
        LiteralValue::Array(_) => ("Array".to_string(), None),
        LiteralValue::Date(_) => ("Date".to_string(), None),
        LiteralValue::DateTime(_) => ("DateTime".to_string(), None),
        LiteralValue::Time(_) => ("Time".to_string(), None),
        LiteralValue::Duration(_) => ("Duration".to_string(), None),
        LiteralValue::Pending => ("Pending".to_string(), None),
    };
    ValuePipelineStage {
        label: label.to_string(),
        result_type,
        result_value: format!("{value:?}"),
        error,
        reference_identity: None,
    }
}

fn witness_cell_audit(engine: &PocEngine, trace: &TraceReport, cell: &CellId) -> WitnessCellAudit {
    let formula = engine
        .formula(cell)
        .map(|record| record.source.clone())
        .unwrap_or_else(|| "<not a formula>".to_string());
    let read_trace = trace
        .formula_read_traces
        .get(cell)
        .cloned()
        .unwrap_or_default();
    let evaluation_count = trace.evaluation_counts.get(cell).copied().unwrap_or(0);
    let result = engine.get_cell_value(cell);
    let evaluation_status = if evaluation_count == 0 {
        "not_evaluated".to_string()
    } else if matches!(result, LiteralValue::Error(_)) {
        "evaluated_error".to_string()
    } else {
        "evaluated".to_string()
    };
    let name_resolutions = read_trace
        .name_resolutions
        .iter()
        .map(|id| {
            engine
                .names()
                .get(id)
                .map(|record| format!("{} ({:?})", record.display_name, id))
                .unwrap_or_else(|| format!("{:?}", id))
        })
        .collect();
    let emitted_runtime_formula_edges = read_trace
        .runtime_edges
        .iter()
        .map(|(from, to)| format!("{from} -> {to}"))
        .collect();
    WitnessCellAudit {
        address: cell.to_string(),
        formula,
        evaluation_status,
        result_or_error: format!("{result:?}"),
        branch_selected: branch_selection(cell, &result),
        unsupported_functions: read_trace.unsupported_functions.into_iter().collect(),
        exact_cell_reads: read_trace
            .cell_reads
            .iter()
            .map(ToString::to_string)
            .collect(),
        exact_cell_read_values: read_trace
            .cell_read_values
            .iter()
            .map(|(cell, value)| format!("{cell}={value}"))
            .collect(),
        exact_cell_read_formulas: read_trace
            .cell_reads
            .iter()
            .filter_map(|target| {
                engine
                    .formula(target)
                    .map(|record| format!("{target}={}", record.source))
            })
            .collect(),
        exact_formula_reads: read_trace
            .formula_reads
            .iter()
            .map(ToString::to_string)
            .collect(),
        range_reads: read_trace
            .range_reads
            .iter()
            .map(ToString::to_string)
            .collect(),
        range_cells_read: read_trace.range_cells_read,
        name_resolutions,
        selected_references: read_trace
            .selected_references
            .iter()
            .map(reference_to_string)
            .collect(),
        emitted_runtime_formula_edges,
    }
}

fn branch_selection(cell: &CellId, value: &LiteralValue) -> String {
    let error = matches!(value, LiteralValue::Error(_));
    if cell.sheet == "CashFlow Engine" && cell.row == 65 && cell.col == 9 {
        if error {
            "IF branch not selected: AND condition returned an error".to_string()
        } else if matches!(value, LiteralValue::Text(text) if text == "Yes") {
            "IF true branch: Yes".to_string()
        } else {
            "IF false branch: No".to_string()
        }
    } else if cell.sheet == "CashFlow Engine" && cell.row == 65 && cell.col == 11 {
        if error {
            "IF branch not selected: I65 condition returned an error".to_string()
        } else if matches!(value, LiteralValue::Empty)
            || matches!(value, LiteralValue::Text(text) if text.is_empty())
        {
            "IF false branch: blank".to_string()
        } else {
            "IF true branch: K64-1".to_string()
        }
    } else {
        "not a conditional witness cell".to_string()
    }
}

fn witness_edge_audit(trace: &TraceReport, from: &CellId, to: &CellId) -> WitnessEdgeAudit {
    let present = trace
        .runtime_formula_edges
        .contains(&(from.clone(), to.clone()));
    let reason = if present {
        "present in complete exact runtime formula graph".to_string()
    } else if let Some(cell_trace) = trace.formula_read_traces.get(from) {
        if !cell_trace.unsupported_functions.is_empty() {
            format!(
                "source evaluated with unsupported function(s): {}",
                cell_trace
                    .unsupported_functions
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        } else if !trace.evaluation_counts.contains_key(from) {
            "source formula was not evaluated".to_string()
        } else if from.sheet == "CashFlow Engine" && from.row == 11 && from.col == 10 {
            format!(
                "INDEX selected a different target: {}",
                trace
                    .formula_read_traces
                    .get(from)
                    .map(|entry| {
                        entry
                            .selected_references
                            .iter()
                            .map(reference_to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_else(|| "no selected reference recorded".to_string())
            )
        } else {
            format!("target {to} was not an exact formula read by {from}")
        }
    } else if !trace.evaluation_counts.contains_key(from) {
        "source formula was not evaluated".to_string()
    } else {
        format!("target {to} was not an exact formula read by {from}")
    };
    WitnessEdgeAudit {
        from: from.to_string(),
        to: to.to_string(),
        present,
        reason,
    }
}

fn reference_to_string(reference: &ReferenceValue) -> String {
    match reference {
        ReferenceValue::Cell(cell) => cell.to_string(),
        ReferenceValue::Range(range) => range.to_string(),
        ReferenceValue::Spill(spill) => format!("spill:{}", spill.range()),
        ReferenceValue::Table(table) => format!("table:{}", table.name),
    }
}

fn run_sequence_step(label: &str, engine: &mut PocEngine) -> Result<RealSequenceStep, String> {
    let report = engine
        .calculate_all()
        .map_err(|error| format!("calculate {label}: {error}"))?;
    let workspace_member_addresses = report
        .cyclic_workspaces
        .iter()
        .flatten()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let largest_runtime_cyclic_workspace = report
        .cyclic_workspaces
        .iter()
        .map(Vec::len)
        .max()
        .unwrap_or(0);
    Ok(RealSequenceStep {
        label: label.to_string(),
        dirty_candidates: report.dirty_before,
        formulas_evaluated: report.evaluation_count,
        evaluation_events: report.evaluation_count,
        exact_runtime_reads: report.trace.execution_read_count,
        retained_runtime_read_records: report.trace.execution_reads.len(),
        runtime_read_records_truncated: report.trace.execution_reads_truncated,
        runtime_edges: report.trace.runtime_formula_edges.len(),
        runtime_formula_edges_generated: report.trace.runtime_formula_edge_events,
        runtime_formula_edges_processed: report.trace.runtime_formula_edges_processed,
        runtime_formula_edges_retained: report.trace.runtime_formula_edges.len(),
        retained_runtime_edge_records: report.trace.runtime_edges.len(),
        diagnostic_edge_records_dropped: report.trace.diagnostic_edge_records_dropped,
        runtime_edge_records_truncated: report.trace.runtime_edges_truncated,
        call_stack_back_edges: report.trace.call_stack_back_edges,
        runtime_cycle_count: report.runtime_cycle_count,
        largest_runtime_cyclic_workspace,
        workspace_member_addresses,
        solver_passes: report.solver_passes,
        wall_time_ms: report.elapsed_ns as f64 / 1_000_000.0,
        unsupported_formula_count: report.unsupported_formula_count,
    })
}

fn parse_error_ast(message: String) -> ASTNode {
    ASTNode::new(
        ASTNodeType::Literal(LiteralValue::Error(
            ExcelError::new(ExcelErrorKind::NImpl).with_message(message),
        )),
        None,
    )
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArtifactShadowReport {
    pub heavy: ShadowMetrics,
    pub light: ShadowMetrics,
    pub heavy_workbook_found: bool,
    pub light_workbook_found: bool,
    pub artifact_backed: bool,
    pub artifact_paths: Vec<String>,
    pub limitations: Vec<String>,
}

pub fn build_artifact_shadow_report(
    root: impl AsRef<Path>,
) -> Result<ArtifactShadowReport, String> {
    let root = root.as_ref();
    let data_dir = root.join("docs").join("issue-solutions").join("data");
    let baseline_path = data_dir.join("latest-upstream-heavy-baseline.json");
    let comparison_path = data_dir.join("latest-upstream-heavy-comparison.json");
    let graph_path = data_dir.join("heavy-graph-root-cause.json");
    let light_path = data_dir.join("heavy-light-noop-causality.json");
    let static_dump_path = data_dir.join("heavy-static-scc-edge-dump.tsv.zip");
    let runtime_dump_path = data_dir.join("heavy-scc-edge-dump.tsv.zip");

    let baseline = read_json(&baseline_path)?;
    let comparison = read_json(&comparison_path)?;
    let graph = read_json(&graph_path)?;
    let light = read_json(&light_path)?;
    let static_dump = read_zip_text(&static_dump_path)?;
    let runtime_dump = read_zip_text(&runtime_dump_path)?;
    let static_stats = parse_static_dump(&static_dump);
    let runtime_stats = parse_runtime_dump(&runtime_dump);

    let heavy_static = &graph["baseline"]["static_graph"];
    let heavy_runtime = &graph["baseline"]["runtime_observed_graph"];
    let heavy_noop = find_phase(&light["templates"]["heavy"]["phases"], "noop");
    let light_noop = find_phase(&light["templates"]["light"]["phases"], "noop");

    let persistent_relation_count = static_stats
        .direct_pairs
        .saturating_add(static_stats.range_sources)
        .saturating_add(static_stats.name_sources)
        .saturating_add(static_stats.dynamic_sources);
    let mut heavy_notes = vec![
        "Static/range/name descriptor counts are conservative relation proxies derived from the checked-in edge dump; the dump does not encode the original range geometry.".to_string(),
        "Runtime edges are labelled legacy observations. They are not treated as Engine V2 exact reads because the prior collector expanded named/range targets.".to_string(),
    ];
    if static_stats.member_count == 0 || runtime_stats.member_count == 0 {
        heavy_notes
            .push("One or more compressed edge dumps had no readable member records.".to_string());
    }
    let heavy = ShadowMetrics {
        profile: "heavy".to_string(),
        input_source: "checked_in_formualizer_artifacts".to_string(),
        workbook_available: false,
        formula_vertices: (static_stats.cell_member_count > 0)
            .then_some(static_stats.cell_member_count),
        name_definition_count: (static_stats.name_member_count > 0)
            .then_some(static_stats.name_member_count),
        symbolic_range_descriptor_count: Some(static_stats.range_sources),
        selector_descriptor_count: Some(static_stats.dynamic_sources),
        invalidation_dependency_count: Some(
            static_stats
                .direct_pairs
                .saturating_add(static_stats.range_sources)
                .saturating_add(static_stats.name_sources)
                .saturating_add(static_stats.dynamic_sources),
        ),
        persistent_relation_count: Some(persistent_relation_count),
        direct_static_edge_count: Some(static_stats.direct_pairs),
        legacy_static_edge_count: json_usize(heavy_static, "edge_count"),
        legacy_runtime_read_edge_count: json_usize(heavy_runtime, "edge_count"),
        static_cycle_candidate_size: json_usize(heavy_static, "largest_scc"),
        legacy_runtime_cycle_size: json_usize(heavy_runtime, "largest_scc"),
        legacy_runtime_cycle_members: json_usize(heavy_runtime, "original_static_members_retained"),
        dirty_closure: phase_usize(&heavy_noop, &["dirty", "dirty_after_iterative_redirty"]),
        no_op_schedule_ms: comparison["performance_ms"]["latest"]["no_op_median"].as_f64(),
        no_op_evaluations:
            comparison["latest_phase_metrics"]["no_op_median"]["scc_member_evaluations"]
                .as_u64()
                .map(|value| value as usize),
        graph_build_time_ms: None,
        memory_bytes_estimate: None,
        notes: heavy_notes,
    };

    let light = ShadowMetrics {
        profile: "light".to_string(),
        input_source: "checked_in_formualizer_artifacts".to_string(),
        workbook_available: false,
        formula_vertices: None,
        name_definition_count: None,
        symbolic_range_descriptor_count: None,
        selector_descriptor_count: None,
        invalidation_dependency_count: None,
        persistent_relation_count: None,
        direct_static_edge_count: None,
        legacy_static_edge_count: None,
        legacy_runtime_read_edge_count: None,
        static_cycle_candidate_size: None,
        legacy_runtime_cycle_size: None,
        legacy_runtime_cycle_members: None,
        dirty_closure: phase_usize(&light_noop, &["dirty", "dirty_at_request_start"]),
        no_op_schedule_ms: phase_f64(&light_noop, &["wall_ms"]),
        no_op_evaluations: phase_usize(&light_noop, &["recalc", "scc_member_evaluations"]),
        graph_build_time_ms: None,
        memory_bytes_estimate: None,
        notes: vec![
            "The repository contains Light timing/cycle summary JSON but no Light workbook or raw dependency dump.".to_string(),
            "A full Light shadow graph therefore remains an explicit repeat-POC requirement.".to_string(),
        ],
    };

    let artifact_paths = [
        baseline_path,
        comparison_path,
        graph_path,
        light_path,
        static_dump_path,
        runtime_dump_path,
    ]
    .iter()
    .map(|path| path.display().to_string())
    .collect();

    let _ = baseline;
    Ok(ArtifactShadowReport {
        heavy,
        light,
        heavy_workbook_found: find_workbook(root, "Fossil_EstimatingTemplate_2026-08_21_A.xlsx"),
        light_workbook_found: false,
        artifact_backed: true,
        artifact_paths,
        limitations: vec![
            "The Fossil XLSX inputs are not present in this checkout, so the artifact adapter cannot reconstruct all formula ASTs or rerun a full-workbook shadow build.".to_string(),
            "The Heavy runtime edge dump records the prior evaluator's expanded observations and is comparison evidence, not a V2 runtime-read oracle.".to_string(),
        ],
    })
}

#[derive(Clone, Copy, Debug, Default)]
struct DumpStats {
    member_count: usize,
    cell_member_count: usize,
    name_member_count: usize,
    direct_pairs: usize,
    range_sources: usize,
    name_sources: usize,
    dynamic_sources: usize,
}

fn parse_static_dump(text: &str) -> DumpStats {
    let mut stats = DumpStats::default();
    let mut members = BTreeMap::new();
    let mut edges: BTreeMap<(usize, usize), u16> = BTreeMap::new();
    for line in text.lines().skip(1) {
        let fields: Vec<&str> = line.split('\t').collect();
        match fields.first().copied() {
            Some("M") if fields.len() >= 4 => {
                let index = fields[1].parse::<usize>().unwrap_or(usize::MAX);
                members.insert(index, fields[3].contains('!'));
            }
            Some("S") if fields.len() >= 3 => {
                let source = fields[1].parse::<usize>().unwrap_or(usize::MAX);
                let target = fields[2].parse::<usize>().unwrap_or(usize::MAX);
                let mask = fields
                    .get(3)
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(1);
                edges
                    .entry((source, target))
                    .and_modify(|old| *old |= mask)
                    .or_insert(mask);
            }
            _ => {}
        }
    }
    stats.member_count = members.len();
    stats.cell_member_count = members.values().filter(|is_cell| **is_cell).count();
    stats.name_member_count = members.values().filter(|is_cell| !**is_cell).count();
    let mut direct = BTreeSet::new();
    let mut ranges = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut dynamic = BTreeSet::new();
    for ((source, target), mask) in edges {
        if mask & 1 != 0 {
            direct.insert((source, target));
        }
        if mask & 2 != 0 {
            ranges.insert(source);
        }
        if mask & 16 != 0 {
            names.insert(source);
        }
        if mask & 64 != 0 {
            dynamic.insert(source);
        }
    }
    stats.direct_pairs = direct.len();
    stats.range_sources = ranges.len();
    stats.name_sources = names.len();
    stats.dynamic_sources = dynamic.len();
    stats
}

fn parse_runtime_dump(text: &str) -> DumpStats {
    let mut stats = DumpStats::default();
    let mut members = BTreeMap::new();
    for line in text.lines().skip(1) {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.first().copied() == Some("M") && fields.len() >= 4 {
            let index = fields[1].parse::<usize>().unwrap_or(usize::MAX);
            members.insert(index, fields[3].contains('!'));
        }
    }
    stats.member_count = members.len();
    stats.cell_member_count = members.values().filter(|is_cell| **is_cell).count();
    stats.name_member_count = members.values().filter(|is_cell| !**is_cell).count();
    stats
}

fn read_json(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("read {}: {error}", path.display()))?;
    serde_json::from_str(&text).map_err(|error| format!("parse {}: {error}", path.display()))
}

fn read_zip_text(path: &Path) -> Result<String, String> {
    let file = File::open(path).map_err(|error| format!("open {}: {error}", path.display()))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| format!("read {} as zip: {error}", path.display()))?;
    if archive.is_empty() {
        return Err(format!("{} has no entries", path.display()));
    }
    let mut entry = archive
        .by_index(0)
        .map_err(|error| format!("read first entry in {}: {error}", path.display()))?;
    let mut text = String::new();
    entry
        .read_to_string(&mut text)
        .map_err(|error| format!("read {} entry: {error}", path.display()))?;
    Ok(text)
}

fn find_workbook(root: &Path, file_name: &str) -> bool {
    [root.join(file_name), root.join("fixtures").join(file_name)]
        .iter()
        .any(|path| path.exists())
}

fn find_phase<'a>(phases: &'a Value, label: &str) -> Value {
    phases
        .as_array()
        .and_then(|phases| {
            phases
                .iter()
                .find(|phase| phase["label"].as_str() == Some(label))
        })
        .cloned()
        .unwrap_or(Value::Null)
}

fn json_usize(value: &Value, key: &str) -> Option<usize> {
    value[key].as_u64().map(|value| value as usize)
}

fn phase_usize(value: &Value, path: &[&str]) -> Option<usize> {
    let value = path.iter().fold(value, |value, key| &value[*key]);
    value.as_u64().map(|value| value as usize)
}

fn phase_f64(value: &Value, path: &[&str]) -> Option<f64> {
    let value = path.iter().fold(value, |value, key| &value[*key]);
    value.as_f64()
}

#[allow(dead_code)]
fn _normalize_reference(reference: &ReferenceValue) -> String {
    format!("{reference:?}")
}

#[allow(dead_code)]
fn _path_buf(path: &Path) -> PathBuf {
    path.to_path_buf()
}
