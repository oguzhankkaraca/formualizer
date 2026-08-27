use formualizer_engine_v2_poc::{
    ArtifactShadowReport, HeavyWitnessAudit, RealHeavyPocReport, ShadowMetrics,
    build_artifact_shadow_report, load_xlsx_poc_model, run_real_heavy_poc,
    run_real_heavy_witness_audit, run_real_light_poc,
};
use std::path::{Path, PathBuf};

const DEFAULT_HEAVY_WORKBOOK: &str =
    r"C:\Users\OXK0A0A\Downloads\Fossil_EstimatingTemplate_2026-08_21_A.xlsx";
const DEFAULT_LIGHT_WORKBOOK: &str =
    r"C:\Users\OXK0A0A\Downloads\Fossil_EstimatingTemplate_2026-06-25_X_Fossil.xlsx";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<String> = std::env::args().collect();
    match arguments.as_slice() {
        [_] => run_and_print_real_with_light(
            Path::new(DEFAULT_HEAVY_WORKBOOK),
            Path::new(DEFAULT_LIGHT_WORKBOOK),
        ),
        [_, flag, path] if flag == "--heavy" => run_and_print_real_with_light(
            Path::new(path),
            Path::new(DEFAULT_LIGHT_WORKBOOK),
        ),
        [_, flag, path] if flag == "--witness" => run_and_print_witness_only(Path::new(path)),
        [_, flag, path] if flag == "--light" => run_and_print_light_only(Path::new(path)),
        [_, flag, path] if flag == "--load-only" => run_and_print_load_only(Path::new(path)),
        [_, flag] if flag == "--artifacts" => run_and_print_artifacts(),
        [_, flag, heavy, light] if flag == "--workbooks" => {
            run_and_print_real_with_light(Path::new(heavy), Path::new(light))
        }
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "usage: engine-v2-poc [--heavy heavy.xlsx | --witness heavy.xlsx | --light light.xlsx | --workbooks heavy.xlsx light.xlsx | --load-only workbook.xlsx | --artifacts]",
        )
        .into()),
    }
}

fn run_and_print_witness_only(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let report = match run_real_heavy_witness_audit(path) {
        Ok(report) => report,
        Err(error) => {
            eprintln!(
                "Heavy source=real_workbook workbook_available=false path={} error={error}",
                path.display()
            );
            return Err(std::io::Error::other(format!(
                "real Heavy witness gate failed for {}: {error}",
                path.display()
            ))
            .into());
        }
    };
    print_witness_report(&report);
    Ok(())
}

fn run_and_print_light_only(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let report = match run_real_light_poc(path) {
        Ok(report) => report,
        Err(error) => {
            eprintln!(
                "Light source=real_workbook workbook_available=false path={} error={error}",
                path.display()
            );
            return Err(std::io::Error::other(format!(
                "real Light control gate failed for {}: {error}",
                path.display()
            ))
            .into());
        }
    };
    print_template_report("Light", &report);
    Ok(())
}

fn run_and_print_load_only(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let model = match load_xlsx_poc_model(path) {
        Ok(model) => model,
        Err(error) => {
            eprintln!(
                "Heavy source=real_workbook workbook_available=false path={} error={error}",
                path.display()
            );
            return Err(std::io::Error::other(format!(
                "real Heavy workbook load failed for {}: {error}",
                path.display()
            ))
            .into());
        }
    };
    println!(
        "Heavy source=real_workbook workbook_available=true path={} worksheets={} load_only=true",
        model.path,
        model.worksheets.len(),
    );
    print_model_stats("Heavy", &model.model_stats, model.model_build_time_ms);
    Ok(())
}

fn run_and_print_real_with_light(
    heavy_path: &Path,
    light_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let witness = match run_real_heavy_witness_audit(heavy_path) {
        Ok(report) => report,
        Err(error) => {
            eprintln!(
                "Heavy source=real_workbook workbook_available=false path={} error={error}",
                heavy_path.display()
            );
            return Err(std::io::Error::other(format!(
                "real Heavy witness gate failed for {}: {error}",
                heavy_path.display()
            ))
            .into());
        }
    };
    print_witness_report(&witness);

    let heavy = match run_real_heavy_poc(heavy_path) {
        Ok(report) => report,
        Err(error) => {
            eprintln!(
                "Heavy source=real_workbook workbook_available=false path={} error={error}",
                heavy_path.display()
            );
            return Err(std::io::Error::other(format!(
                "real Heavy sequence gate failed for {}: {error}",
                heavy_path.display()
            ))
            .into());
        }
    };
    print_template_report("Heavy", &heavy);

    let light = match run_real_light_poc(light_path) {
        Ok(report) => report,
        Err(error) => {
            eprintln!(
                "Light source=real_workbook workbook_available=false path={} error={error}",
                light_path.display()
            );
            return Err(std::io::Error::other(format!(
                "real Light control gate failed for {}: {error}",
                light_path.display()
            ))
            .into());
        }
    };
    print_template_report("Light", &light);
    Ok(())
}

fn print_model_stats(label: &str, stats: &formualizer_engine_v2_poc::PocModelStats, build_ms: f64) {
    println!(
        "{} model formula_count={} full_defined_name_count={} symbolic_dependency_descriptor_count={} persistent_relation_count={} invalidation_index_count={} model_build_time_ms={:.3} memory_state_bytes={} opaque_formula_count={}",
        label,
        stats.formula_count,
        stats.defined_name_count,
        stats.symbolic_dependency_descriptor_count,
        stats.persistent_relation_count,
        stats.invalidation_index_count,
        build_ms,
        stats.memory_state_bytes,
        stats.opaque_formula_count,
    );
}

fn print_template_report(label: &str, report: &RealHeavyPocReport) {
    println!(
        "{} source={} workbook_available={} path={} worksheets={}",
        label,
        report.source,
        report.workbook_available,
        report.path,
        report.worksheets.len(),
    );
    print_model_stats(label, &report.model_stats, report.model_build_time_ms);
    for step in &report.steps {
        println!(
            "{} step={} dirty_candidates={} formulas_evaluated={} exact_runtime_reads={} runtime_edges={} runtime_formula_edges_generated={} runtime_formula_edges_processed={} runtime_formula_edges_retained={} diagnostic_edge_records_stored={} diagnostic_edge_records_dropped={} call_stack_back_edges={} runtime_cycle_count={} largest_runtime_cyclic_workspace={} workspace_member_addresses={:?} solver_passes={} wall_time_ms={:.3} unsupported_formula_count={}",
            label,
            step.label,
            step.dirty_candidates,
            step.formulas_evaluated,
            step.exact_runtime_reads,
            step.runtime_edges,
            step.runtime_formula_edges_generated,
            step.runtime_formula_edges_processed,
            step.runtime_formula_edges_retained,
            step.retained_runtime_edge_records,
            step.diagnostic_edge_records_dropped,
            step.call_stack_back_edges,
            step.runtime_cycle_count,
            step.largest_runtime_cyclic_workspace,
            step.workspace_member_addresses,
            step.solver_passes,
            step.wall_time_ms,
            step.unsupported_formula_count,
        );
    }
}

fn print_witness_report(report: &HeavyWitnessAudit) {
    println!(
        "Heavy source={} workbook_available={} path={} witness_state={}",
        report.source, report.workbook_available, report.path, report.f7_state,
    );
    for cell in &report.cells {
        print_witness_cell("WITNESS", cell);
    }
    println!(
        "Heavy first_value_divergence={}",
        report.first_value_divergence,
    );
    for stage in &report.j11_value_pipeline {
        println!(
            "J11_PIPELINE stage={} result_type={} result_value={} error={:?} reference_identity={:?}",
            stage.label,
            stage.result_type,
            stage.result_value,
            stage.error,
            stage.reference_identity,
        );
    }
    for stage in &report.j9_value_pipeline {
        println!(
            "J9_PIPELINE stage={} result_type={} result_value={} error={:?} reference_identity={:?}",
            stage.label,
            stage.result_type,
            stage.result_value,
            stage.error,
            stage.reference_identity,
        );
    }
    print_witness_cell("J9", &report.j9);
    for cell in &report.witness_chain {
        print_witness_cell("CHAIN", cell);
    }
    for cell in &report.j23_upstream_audits {
        print_witness_cell("J23_UPSTREAM", cell);
    }
    for stage in &report.j23_value_pipeline {
        println!(
            "J23_PIPELINE stage={} result_type={} result_value={} error={:?} reference_identity={:?}",
            stage.label,
            stage.result_type,
            stage.result_value,
            stage.error,
            stage.reference_identity,
        );
    }
    for edge in &report.edges {
        println!(
            "EDGE {} -> {}: {} + {}",
            edge.from,
            edge.to,
            if edge.present { "PRESENT" } else { "ABSENT" },
            edge.reason,
        );
    }
    println!(
        "Heavy complete_runtime_graph runtime_formula_edges_generated={} runtime_formula_edges_processed={} runtime_formula_edges_retained={} diagnostic_edge_records_stored={} diagnostic_edge_records_dropped={} call_stack_back_edges={} runtime_graph_cyclic_scc_count={} largest_runtime_graph_cyclic_scc={} largest_runtime_graph_cyclic_scc_members={:?}",
        report.runtime_formula_edges_generated,
        report.runtime_formula_edges_processed,
        report.runtime_formula_edges_retained,
        report.diagnostic_edge_records_stored,
        report.diagnostic_edge_records_dropped,
        report.call_stack_back_edges,
        report.runtime_graph_cyclic_scc_count,
        report.largest_runtime_graph_cyclic_scc,
        report.largest_runtime_graph_cyclic_scc_members,
    );
    println!(
        "Heavy J23 required_range={} consumed_cells={} edate_min_supported={}",
        report.j23_required_range,
        report.j23_required_range_consumed_cells,
        report.j23_edate_min_supported,
    );
    println!("Heavy J11 selected_target={:?}", report.j11_selected_target,);
    println!(
        "Heavy diagnostic_trace_limit_control passed={} default_cycle_count={} reduced_cycle_count={} default_complete_edge_count={} reduced_complete_edge_count={} reduced_records_stored={} reduced_records_dropped={}",
        report.diagnostic_limit_control_passed,
        report.diagnostic_limit_control_default_cycle_count,
        report.diagnostic_limit_control_reduced_cycle_count,
        report.diagnostic_limit_control_default_edge_count,
        report.diagnostic_limit_control_reduced_edge_count,
        report.diagnostic_limit_control_reduced_records_stored,
        report.diagnostic_limit_control_reduced_records_dropped,
    );
}

fn print_witness_cell(prefix: &str, cell: &formualizer_engine_v2_poc::WitnessCellAudit) {
    println!(
        "{} cell={} formula={} evaluation_status={} result_or_error={} branch_selected={}",
        prefix,
        cell.address,
        cell.formula,
        cell.evaluation_status,
        cell.result_or_error,
        cell.branch_selected,
    );
    println!(
        "{} cell={} unsupported_functions={:?} range_cells_read={}",
        prefix, cell.address, cell.unsupported_functions, cell.range_cells_read,
    );
    for read in &cell.exact_cell_reads {
        println!("{} cell={} exact_cell_read={}", prefix, cell.address, read);
    }
    for read in &cell.exact_cell_read_values {
        println!(
            "{} cell={} exact_cell_read_value={}",
            prefix, cell.address, read
        );
    }
    for read in &cell.exact_cell_read_formulas {
        println!(
            "{} cell={} exact_cell_read_formula={}",
            prefix, cell.address, read
        );
    }
    for read in &cell.exact_formula_reads {
        println!(
            "{} cell={} exact_formula_read={}",
            prefix, cell.address, read
        );
    }
    for read in &cell.range_reads {
        println!("{} cell={} range_read={}", prefix, cell.address, read);
    }
    for read in &cell.name_resolutions {
        println!("{} cell={} name_resolution={}", prefix, cell.address, read);
    }
    for read in &cell.selected_references {
        println!(
            "{} cell={} selected_reference={}",
            prefix, cell.address, read
        );
    }
    for edge in &cell.emitted_runtime_formula_edges {
        println!(
            "{} cell={} emitted_runtime_formula_edge={}",
            prefix, cell.address, edge
        );
    }
}

fn run_and_print_artifacts() -> Result<(), Box<dyn std::error::Error>> {
    let root = repository_root()?;
    let report = build_artifact_shadow_report(root).map_err(std::io::Error::other)?;
    print_artifact_report(&report);
    Ok(())
}

fn print_artifacts_metrics(label: &str, metrics: &ShadowMetrics) {
    println!(
        "{} source={} workbook_available={} formulas={:?} names={:?} ranges={:?} invalidation={:?} persistent={:?} direct_static_edges={:?} legacy_static_edges={:?} legacy_runtime_edges={:?} v1_static_cycle={:?} v1_runtime_cycle={:?} noop_ms={:?} noop_evaluations={:?}",
        label,
        metrics.input_source,
        metrics.workbook_available,
        metrics.formula_vertices,
        metrics.name_definition_count,
        metrics.symbolic_range_descriptor_count,
        metrics.invalidation_dependency_count,
        metrics.persistent_relation_count,
        metrics.direct_static_edge_count,
        metrics.legacy_static_edge_count,
        metrics.legacy_runtime_read_edge_count,
        metrics.static_cycle_candidate_size,
        metrics.legacy_runtime_cycle_size,
        metrics.no_op_schedule_ms,
        metrics.no_op_evaluations,
    );
}

fn print_artifact_report(report: &ArtifactShadowReport) {
    print_artifacts_metrics("Heavy", &report.heavy);
    print_artifacts_metrics("Light", &report.light);
    println!(
        "POC_A artifact_mode=true heavy_workbook_found={} light_workbook_found={} limitations={}",
        report.heavy_workbook_found,
        report.light_workbook_found,
        report.limitations.len(),
    );
}

fn repository_root() -> Result<PathBuf, std::io::Error> {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .map(Path::to_path_buf)
        .ok_or_else(|| std::io::Error::other("repository root not found"))
}
