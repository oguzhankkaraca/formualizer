#[cfg(feature = "formualizer_runner")]
use std::collections::BTreeMap;
#[cfg(feature = "formualizer_runner")]
use std::fs;
#[cfg(feature = "formualizer_runner")]
use std::path::{Path, PathBuf};
#[cfg(feature = "formualizer_runner")]
use std::time::Instant;

#[cfg(feature = "formualizer_runner")]
use anyhow::{Context, Result, bail};
#[cfg(feature = "formualizer_runner")]
use clap::{Parser, ValueEnum};
#[cfg(feature = "formualizer_runner")]
use formualizer_common::{DateSystem, LiteralValue};
#[cfg(feature = "formualizer_runner")]
use formualizer_eval::engine::{CycleDetection, CyclePolicy};
#[cfg(feature = "formualizer_runner")]
use formualizer_workbook::{
    CalamineAdapter, LoadStrategy, SpreadsheetReader, Workbook, WorkbookConfig,
};
#[cfg(feature = "formualizer_runner")]
use serde::Serialize;
#[cfg(feature = "formualizer_runner")]
use sha2::{Digest, Sha256};

#[cfg(not(feature = "formualizer_runner"))]
fn main() {
    eprintln!(
        "This binary requires feature `formualizer_runner`: cargo run -p formualizer-bench-core --features formualizer_runner --bin compare-excel-workbook -- --workbook <path>"
    );
    std::process::exit(2);
}

#[cfg(feature = "formualizer_runner")]
fn main() -> Result<()> {
    let cli = Cli::parse();
    let report = compare_workbook(&cli)?;
    let json = serde_json::to_string_pretty(&report)? + "\n";
    if let Some(path) = &cli.output {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create output directory {}", parent.display()))?;
        }
        fs::write(path, &json).with_context(|| format!("write report {}", path.display()))?;
    }
    print!("{json}");
    Ok(())
}

#[cfg(feature = "formualizer_runner")]
#[derive(Debug, Parser)]
#[command(about = "Compare Excel cached formula values with Formualizer evaluation")]
struct Cli {
    #[arg(long)]
    workbook: PathBuf,
    #[arg(long, value_enum, default_value_t = EvaluationMode::Iterate)]
    mode: EvaluationMode,
    #[arg(long)]
    output: Option<PathBuf>,
    #[arg(long, default_value_t = 25)]
    max_examples_per_class: usize,
    #[arg(long, default_value_t = 1e-9)]
    absolute_tolerance: f64,
    #[arg(long, default_value_t = 1e-9)]
    relative_tolerance: f64,
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    parallel: bool,
}

#[cfg(feature = "formualizer_runner")]
#[derive(Debug, Clone, Copy, ValueEnum, Serialize)]
#[serde(rename_all = "snake_case")]
enum EvaluationMode {
    Static,
    Runtime,
    Iterate,
}

#[cfg(feature = "formualizer_runner")]
impl EvaluationMode {
    fn workbook_config(self, parallel: bool) -> WorkbookConfig {
        let mut config = WorkbookConfig::ephemeral();
        config.eval.defer_graph_building = false;
        config.eval.enable_parallel = parallel;
        match self {
            Self::Static => {
                config.eval.cycle.detection = CycleDetection::Static;
                config.eval.cycle.policy = CyclePolicy::Error;
            }
            Self::Runtime => {
                config.eval.cycle.detection = CycleDetection::Runtime;
                config.eval.cycle.policy = CyclePolicy::Error;
            }
            Self::Iterate => {
                config.eval.cycle.detection = CycleDetection::Runtime;
                config.eval.cycle.policy = CyclePolicy::Iterate {
                    max_iterations: CyclePolicy::EXCEL_DEFAULT_MAX_ITERATIONS,
                    max_change: CyclePolicy::EXCEL_DEFAULT_MAX_CHANGE,
                };
            }
        }
        config
    }
}

#[cfg(feature = "formualizer_runner")]
#[derive(Debug, Clone)]
struct FormulaCell {
    sheet: String,
    row: u32,
    column: u32,
    formula: String,
    source: LiteralValue,
}

#[cfg(feature = "formualizer_runner")]
#[derive(Debug, Clone, Serialize, PartialEq)]
struct NormalizedValue {
    kind: String,
    value: Option<serde_json::Value>,
    error: Option<String>,
}

#[cfg(feature = "formualizer_runner")]
impl NormalizedValue {
    fn transition_label(&self) -> String {
        self.error.clone().unwrap_or_else(|| self.kind.clone())
    }
}

#[cfg(feature = "formualizer_runner")]
#[derive(Debug, Serialize)]
struct CompatibilityExample {
    cell: String,
    formula: String,
    excel_cached: NormalizedValue,
    formualizer: NormalizedValue,
}

#[cfg(feature = "formualizer_runner")]
#[derive(Debug, Serialize)]
struct CompatibilityReport {
    schema: &'static str,
    metadata: ReportMetadata,
    evaluation: EvaluationReport,
    classes: BTreeMap<String, u64>,
    transitions: BTreeMap<String, u64>,
    by_sheet: BTreeMap<String, BTreeMap<String, u64>>,
    formualizer_errors: BTreeMap<String, u64>,
    examples: BTreeMap<String, Vec<CompatibilityExample>>,
}

#[cfg(feature = "formualizer_runner")]
#[derive(Debug, Serialize)]
struct ReportMetadata {
    workbook: String,
    workbook_sha256: String,
    runner_version: &'static str,
    build_profile: &'static str,
    architecture: &'static str,
    os: &'static str,
    mode: EvaluationMode,
    date_system: &'static str,
    parallel: bool,
    absolute_tolerance: f64,
    relative_tolerance: f64,
    formula_count: usize,
    source_load_ms: f64,
    evaluated_load_ms: f64,
}

#[cfg(feature = "formualizer_runner")]
#[derive(Debug, Serialize)]
struct EvaluationReport {
    elapsed_ms: f64,
    error: Option<String>,
    cycle: CycleReport,
    recalc: RecalcReport,
}

#[cfg(feature = "formualizer_runner")]
#[derive(Debug, Serialize)]
struct CycleReport {
    static_sccs: usize,
    live_cycles_witnessed: usize,
    iterated_sccs: usize,
    converged_sccs: usize,
    exactly_stable_sccs: usize,
    capped_sccs: usize,
    settle_passes_total: usize,
    max_passes_single_scc: usize,
    max_abs_delta_at_stop: f64,
}

#[cfg(feature = "formualizer_runner")]
#[derive(Debug, Serialize)]
struct RecalcReport {
    total_ms: f64,
    graph_build_ms: f64,
    dirty_detection_ms: f64,
    plan_build_ms: f64,
    acyclic_evaluation_ms: f64,
    iterative_scc_evaluation_ms: f64,
    virtual_dependency_change_detection_ms: f64,
    cleanup_ms: f64,
    dirty_roots: usize,
    planned_vertices: usize,
    planned_layers: usize,
    planned_sccs: usize,
    evaluated_vertices: usize,
    acyclic_vertices_evaluated: usize,
    scc_tasks_evaluated: usize,
    scc_member_count: usize,
    scc_member_evaluations: usize,
    scc_units_reused: usize,
    scc_units_invalidated: usize,
}

#[cfg(feature = "formualizer_runner")]
fn compare_workbook(cli: &Cli) -> Result<CompatibilityReport> {
    if cli.max_examples_per_class == 0 {
        bail!("--max-examples-per-class must be greater than zero");
    }
    if !cli.absolute_tolerance.is_finite() || cli.absolute_tolerance < 0.0 {
        bail!("--absolute-tolerance must be finite and non-negative");
    }
    if !cli.relative_tolerance.is_finite() || cli.relative_tolerance < 0.0 {
        bail!("--relative-tolerance must be finite and non-negative");
    }

    let workbook_path = cli
        .workbook
        .canonicalize()
        .with_context(|| format!("resolve workbook {}", cli.workbook.display()))?;
    let source_started = Instant::now();
    let mut source = CalamineAdapter::open_path(&workbook_path).map_err(|error| {
        anyhow::anyhow!("open source evidence {}: {error}", workbook_path.display())
    })?;
    let formula_cells = collect_formula_cells(&mut source)?;
    let source_load_ms = source_started.elapsed().as_secs_f64() * 1000.0;

    let evaluated_started = Instant::now();
    let mut evaluated = load_workbook(&workbook_path, cli.mode, cli.parallel)?;
    let evaluated_load_ms = evaluated_started.elapsed().as_secs_f64() * 1000.0;
    let evaluation_started = Instant::now();
    let evaluation_error = evaluated
        .evaluate_all()
        .err()
        .map(|error| error.to_string());
    let evaluation_elapsed_ms = evaluation_started.elapsed().as_secs_f64() * 1000.0;
    let date_system = evaluated.engine().config.date_system;

    let mut classes = BTreeMap::new();
    let mut transitions = BTreeMap::new();
    let mut by_sheet: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    let mut formualizer_errors = BTreeMap::new();
    let mut examples: BTreeMap<String, Vec<CompatibilityExample>> = BTreeMap::new();

    if evaluation_error.is_none() {
        for cell in &formula_cells {
            let computed = evaluated
                .get_value(&cell.sheet, cell.row, cell.column)
                .unwrap_or(LiteralValue::Empty);
            let source = normalize_value(&cell.source, date_system);
            let computed = normalize_value(&computed, date_system);
            let class = classify_values(
                &source,
                &computed,
                cli.absolute_tolerance,
                cli.relative_tolerance,
            );
            increment(&mut classes, class);
            increment(
                &mut transitions,
                &format!(
                    "{} -> {}",
                    source.transition_label(),
                    computed.transition_label()
                ),
            );
            increment(by_sheet.entry(cell.sheet.clone()).or_default(), class);
            if let Some(error) = &computed.error {
                increment(&mut formualizer_errors, error);
            }
            if class != "match" {
                let class_examples = examples.entry(class.to_string()).or_default();
                if class_examples.len() < cli.max_examples_per_class {
                    class_examples.push(CompatibilityExample {
                        cell: format!("{}!{}{}", cell.sheet, column_letters(cell.column), cell.row),
                        formula: cell.formula.clone(),
                        excel_cached: source,
                        formualizer: computed,
                    });
                }
            }
        }
    }

    let cycle = evaluated.engine().last_cycle_telemetry();
    let recalc = evaluated.engine().last_recalc_telemetry();
    Ok(CompatibilityReport {
        schema: "formualizer.compatibility-report/v1",
        metadata: ReportMetadata {
            workbook: workbook_path.display().to_string(),
            workbook_sha256: sha256_file(&workbook_path)?,
            runner_version: env!("CARGO_PKG_VERSION"),
            build_profile: if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
            architecture: std::env::consts::ARCH,
            os: std::env::consts::OS,
            mode: cli.mode,
            date_system: match date_system {
                DateSystem::Excel1900 => "1900",
                DateSystem::Excel1904 => "1904",
            },
            parallel: cli.parallel,
            absolute_tolerance: cli.absolute_tolerance,
            relative_tolerance: cli.relative_tolerance,
            formula_count: formula_cells.len(),
            source_load_ms,
            evaluated_load_ms,
        },
        evaluation: EvaluationReport {
            elapsed_ms: evaluation_elapsed_ms,
            error: evaluation_error,
            cycle: CycleReport {
                static_sccs: cycle.static_sccs,
                live_cycles_witnessed: cycle.live_cycles_witnessed,
                iterated_sccs: cycle.iterated_sccs,
                converged_sccs: cycle.converged_sccs,
                exactly_stable_sccs: cycle.exactly_stable_sccs,
                capped_sccs: cycle.capped_sccs,
                settle_passes_total: cycle.settle_passes_total,
                max_passes_single_scc: cycle.max_passes_single_scc,
                max_abs_delta_at_stop: cycle.max_abs_delta_at_stop,
            },
            recalc: RecalcReport {
                total_ms: ns_to_ms(recalc.total_ns),
                graph_build_ms: ns_to_ms(recalc.graph_build_ns),
                dirty_detection_ms: ns_to_ms(recalc.dirty_detection_ns),
                plan_build_ms: ns_to_ms(recalc.plan_build_ns),
                acyclic_evaluation_ms: ns_to_ms(recalc.acyclic_evaluation_ns),
                iterative_scc_evaluation_ms: ns_to_ms(recalc.iterative_scc_evaluation_ns),
                virtual_dependency_change_detection_ms: ns_to_ms(
                    recalc.virtual_dependency_change_detection_ns,
                ),
                cleanup_ms: ns_to_ms(recalc.cleanup_ns),
                dirty_roots: recalc.dirty_roots,
                planned_vertices: recalc.planned_vertices,
                planned_layers: recalc.planned_layers,
                planned_sccs: recalc.planned_sccs,
                evaluated_vertices: recalc.evaluated_vertices,
                acyclic_vertices_evaluated: recalc.acyclic_vertices_evaluated,
                scc_tasks_evaluated: recalc.scc_tasks_evaluated,
                scc_member_count: recalc.scc_member_count,
                scc_member_evaluations: recalc.scc_member_evaluations,
                scc_units_reused: recalc.scc_units_reused,
                scc_units_invalidated: recalc.scc_units_invalidated,
            },
        },
        classes,
        transitions,
        by_sheet,
        formualizer_errors,
        examples,
    })
}

#[cfg(feature = "formualizer_runner")]
fn load_workbook(path: &Path, mode: EvaluationMode, parallel: bool) -> Result<Workbook> {
    let adapter = CalamineAdapter::open_path(path)
        .map_err(|error| anyhow::anyhow!("open {}: {error}", path.display()))?;
    Workbook::from_reader(
        adapter,
        LoadStrategy::EagerAll,
        mode.workbook_config(parallel),
    )
    .map_err(|error| anyhow::anyhow!("load {}: {error}", path.display()))
}

#[cfg(feature = "formualizer_runner")]
fn collect_formula_cells(adapter: &mut CalamineAdapter) -> Result<Vec<FormulaCell>> {
    let mut cells = Vec::new();
    let sheets = adapter
        .sheet_names()
        .map_err(|error| anyhow::anyhow!("read source sheet names: {error}"))?;
    for sheet in sheets {
        let data = adapter
            .read_sheet(&sheet)
            .map_err(|error| anyhow::anyhow!("read source sheet {sheet}: {error}"))?;
        for ((row, column), cell) in data.cells {
            let Some(formula) = cell.formula else {
                continue;
            };
            cells.push(FormulaCell {
                sheet: sheet.clone(),
                row,
                column,
                formula,
                source: cell.value.unwrap_or(LiteralValue::Empty),
            });
        }
    }
    Ok(cells)
}

#[cfg(feature = "formualizer_runner")]
fn normalize_value(value: &LiteralValue, date_system: DateSystem) -> NormalizedValue {
    match value {
        LiteralValue::Empty => NormalizedValue {
            kind: "empty".to_string(),
            value: None,
            error: None,
        },
        LiteralValue::Number(number) => normalized_number(*number),
        LiteralValue::Int(integer) => normalized_number(*integer as f64),
        LiteralValue::Date(_)
        | LiteralValue::DateTime(_)
        | LiteralValue::Time(_)
        | LiteralValue::Duration(_) => normalized_number(
            value
                .as_serial_number_for(date_system)
                .expect("temporal value has serial representation"),
        ),
        LiteralValue::Text(text) => NormalizedValue {
            kind: "text".to_string(),
            value: Some(serde_json::Value::String(text.clone())),
            error: None,
        },
        LiteralValue::Boolean(boolean) => NormalizedValue {
            kind: "boolean".to_string(),
            value: Some(serde_json::Value::Bool(*boolean)),
            error: None,
        },
        LiteralValue::Error(error) => NormalizedValue {
            kind: "error".to_string(),
            value: None,
            error: Some(error.kind.to_string()),
        },
        LiteralValue::Array(rows) => NormalizedValue {
            kind: "array".to_string(),
            value: Some(serde_json::Value::String(format!("{rows:?}"))),
            error: None,
        },
        LiteralValue::Pending => NormalizedValue {
            kind: "error".to_string(),
            value: None,
            error: Some("#CALC!".to_string()),
        },
    }
}

#[cfg(feature = "formualizer_runner")]
fn normalized_number(number: f64) -> NormalizedValue {
    NormalizedValue {
        kind: "number".to_string(),
        value: serde_json::Number::from_f64(number).map(serde_json::Value::Number),
        error: None,
    }
}

#[cfg(feature = "formualizer_runner")]
fn classify_values(
    source: &NormalizedValue,
    computed: &NormalizedValue,
    absolute_tolerance: f64,
    relative_tolerance: f64,
) -> &'static str {
    match (&source.error, &computed.error) {
        (Some(source), Some(computed)) if source == computed => "same_error",
        (Some(_), Some(_)) => "different_error_kind",
        (None, Some(_)) => "formualizer_only_error",
        (Some(_), None) => "excel_error_formualizer_value",
        (None, None) if values_equal(source, computed, absolute_tolerance, relative_tolerance) => {
            "match"
        }
        (None, None) => "value_mismatch",
    }
}

#[cfg(feature = "formualizer_runner")]
fn values_equal(
    source: &NormalizedValue,
    computed: &NormalizedValue,
    absolute_tolerance: f64,
    relative_tolerance: f64,
) -> bool {
    if source.kind != computed.kind {
        return false;
    }
    if source.kind != "number" {
        return source.value == computed.value;
    }
    let Some(source) = source.value.as_ref().and_then(serde_json::Value::as_f64) else {
        return false;
    };
    let Some(computed) = computed.value.as_ref().and_then(serde_json::Value::as_f64) else {
        return false;
    };
    let difference = (source - computed).abs();
    let scale = source.abs().max(computed.abs());
    difference <= absolute_tolerance || difference <= relative_tolerance * scale
}

#[cfg(feature = "formualizer_runner")]
fn increment(counts: &mut BTreeMap<String, u64>, key: &str) {
    *counts.entry(key.to_string()).or_default() += 1;
}

#[cfg(feature = "formualizer_runner")]
fn sha256_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let digest = Sha256::digest(bytes);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(feature = "formualizer_runner")]
fn column_letters(mut column: u32) -> String {
    let mut letters = Vec::new();
    while column > 0 {
        column -= 1;
        letters.push((b'A' + (column % 26) as u8) as char);
        column /= 26;
    }
    letters.iter().rev().collect()
}

#[cfg(feature = "formualizer_runner")]
fn ns_to_ms(ns: u128) -> f64 {
    ns as f64 / 1_000_000.0
}

#[cfg(all(test, feature = "formualizer_runner"))]
mod tests {
    use super::*;

    #[test]
    fn error_classes_preserve_direction_and_kind() {
        let value = normalized_number(1.0);
        let reference_error = NormalizedValue {
            kind: "error".to_string(),
            value: None,
            error: Some("#REF!".to_string()),
        };
        let name_error = NormalizedValue {
            kind: "error".to_string(),
            value: None,
            error: Some("#NAME?".to_string()),
        };

        assert_eq!(
            classify_values(&reference_error, &reference_error, 1e-9, 1e-9),
            "same_error"
        );
        assert_eq!(
            classify_values(&reference_error, &name_error, 1e-9, 1e-9),
            "different_error_kind"
        );
        assert_eq!(
            classify_values(&value, &name_error, 1e-9, 1e-9),
            "formualizer_only_error"
        );
        assert_eq!(
            classify_values(&reference_error, &value, 1e-9, 1e-9),
            "excel_error_formualizer_value"
        );
    }

    #[test]
    fn numeric_comparison_uses_absolute_and_relative_tolerance() {
        assert!(values_equal(
            &normalized_number(1.0),
            &normalized_number(1.0 + 1e-10),
            1e-9,
            1e-9
        ));
        assert!(values_equal(
            &normalized_number(1_000_000.0),
            &normalized_number(1_000_000.0005),
            0.0,
            1e-9
        ));
        assert!(!values_equal(
            &normalized_number(1.0),
            &normalized_number(1.1),
            1e-9,
            1e-9
        ));
    }
}
