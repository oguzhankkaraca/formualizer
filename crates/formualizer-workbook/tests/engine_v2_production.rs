#![cfg(feature = "calamine")]

use formualizer_common::LiteralValue;
use formualizer_eval::engine::ingest::EngineLoadStream;
use formualizer_eval::engine::{
    CycleConfig, DeterministicMode, Engine, EvalConfig, EvaluationTarget, FormulaParsePolicy,
};
use formualizer_eval::function::FnCaps;
use formualizer_eval::function_contract::FunctionCapabilityClass;
use formualizer_eval::function_registry;
use formualizer_eval::reference::{CellRef, Coord};
use formualizer_eval::test_workbook::TestWorkbook;
use formualizer_eval::timezone::TimeZoneSpec;
use formualizer_parse::parser::{ASTNode, ASTNodeType, ReferenceType};
use formualizer_workbook::{CalamineAdapter, SpreadsheetReader};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::Instant;

const DEFAULT_HEAVY: &str =
    r"C:\Users\OXK0A0A\Downloads\Fossil_EstimatingTemplate_2026-08_21_A.xlsx";
const DEFAULT_LIGHT: &str =
    r"C:\Users\OXK0A0A\Downloads\Fossil_EstimatingTemplate_2026-06-25_X_Fossil.xlsx";

fn config() -> EvalConfig {
    let mut config = EvalConfig::default().with_cycle(CycleConfig::iterate_excel_defaults());
    config.deterministic_mode = DeterministicMode::Enabled {
        timestamp_utc: chrono::DateTime::UNIX_EPOCH,
        timezone: TimeZoneSpec::Utc,
    };
    config.formula_parse_policy = FormulaParsePolicy::CoerceToError;
    config
}

fn load_engine(path: &Path, v2: bool) -> Engine<TestWorkbook> {
    let mut backend = CalamineAdapter::open_path(path).expect("open workbook");
    let mut engine = Engine::new(TestWorkbook::new(), config());
    backend
        .stream_into_engine(&mut engine)
        .expect("stream workbook into production Engine");
    if v2 {
        engine.enable_v2_for_test();
    } else {
        engine.disable_v2_for_test();
    }
    engine
}

fn workbook_path(variable: &str, fallback: &str) -> PathBuf {
    std::env::var_os(variable)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(fallback))
}

fn is_blank(value: &Option<LiteralValue>) -> bool {
    matches!(value, None | Some(LiteralValue::Empty))
        || matches!(value, Some(LiteralValue::Text(text)) if text.is_empty())
}

fn add_reason(reasons: &mut BTreeMap<String, usize>, reason: impl Into<String>) {
    *reasons.entry(reason.into()).or_default() += 1;
}

fn census_positive_selector_literals(args: &[ASTNode]) -> bool {
    let positive = |node: &ASTNode| match &node.node_type {
        ASTNodeType::Literal(LiteralValue::Int(value)) => *value > 0,
        ASTNodeType::Literal(LiteralValue::Number(value)) => *value > 0.0,
        _ => false,
    };
    (2..=3).contains(&args.len()) && positive(&args[1]) && args.get(2).is_none_or(positive)
}

fn census_scalar_syntax_safe(ast: &ASTNode) -> bool {
    match &ast.node_type {
        ASTNodeType::Literal(value) => !matches!(value, LiteralValue::Array(_)),
        ASTNodeType::Reference { reference, .. } => {
            matches!(reference, ReferenceType::Cell { .. })
        }
        ASTNodeType::UnaryOp { expr, .. } => census_scalar_syntax_safe(expr),
        ASTNodeType::BinaryOp { left, right, .. } => {
            census_scalar_syntax_safe(left) && census_scalar_syntax_safe(right)
        }
        ASTNodeType::Function { name, args } => {
            let Some(resolved) = function_registry::resolve_for_arity("", name, args.len()) else {
                return false;
            };
            let Some(contract) = resolved.semantics.contract else {
                return false;
            };
            let scalar_args = args.iter().all(census_scalar_syntax_safe);
            let caps = resolved.function.caps();
            if caps.contains(FnCaps::V2_REFERENCE_SHAPE_OBSERVED) {
                return true;
            }
            if caps.contains(FnCaps::V2_SCALAR_OUTPUT_FROM_SCALAR_ARGS) {
                return scalar_args;
            }
            contract.result
                == formualizer_eval::function_contract::FunctionResultSemantics::ScalarValue
                && matches!(
                    contract.context,
                    formualizer_eval::function_contract::FunctionContextDependence::None
                )
        }
        ASTNodeType::Array(_) | ASTNodeType::Call { .. } | ASTNodeType::Omitted => false,
    }
}

fn census_capability_class(
    resolved: &function_registry::ResolvedFunction,
) -> Option<FunctionCapabilityClass> {
    let contract = resolved.semantics.contract?;
    let caps = resolved.function.caps();
    if !resolved.semantics.trusted_builtin
        || matches!(
            contract.dependency,
            formualizer_eval::function_contract::FunctionDependencySemantics::Unsupported
        )
        || matches!(
            contract.result,
            formualizer_eval::function_contract::FunctionResultSemantics::Unknown
        )
    {
        return Some(FunctionCapabilityClass::Unsupported);
    }
    if caps.contains(FnCaps::DYNAMIC_DEPENDENCY)
        || contract.dependency
            == formualizer_eval::function_contract::FunctionDependencySemantics::Dynamic
    {
        return Some(FunctionCapabilityClass::DynamicReference);
    }
    if contract.context != formualizer_eval::function_contract::FunctionContextDependence::None {
        return Some(FunctionCapabilityClass::ContextDependent);
    }
    if caps.contains(FnCaps::VOLATILE)
        || caps.contains(FnCaps::LOCAL_ENVIRONMENT)
        || contract.environment
            != formualizer_eval::function_contract::FunctionEnvironmentSemantics::None
    {
        return Some(FunctionCapabilityClass::VolatileOrEnvironmentDependent);
    }
    if !caps.contains(FnCaps::PURE) {
        return Some(FunctionCapabilityClass::Unsupported);
    }
    if contract.result.may_return_reference() || contract.result.may_spill() {
        return Some(FunctionCapabilityClass::StructuralReferenceShape);
    }
    (contract.dependency
        == formualizer_eval::function_contract::FunctionDependencySemantics::RecursiveSyntacticArgs)
        .then_some(FunctionCapabilityClass::ArgumentStateSafe)
}

fn census_ast(ast: &ASTNode, reasons: &mut BTreeMap<String, usize>) {
    match &ast.node_type {
        ASTNodeType::Literal(_) | ASTNodeType::Omitted => {}
        ASTNodeType::Reference { reference, .. } => match reference {
            ReferenceType::External(_) => add_reason(reasons, "reference.external"),
            ReferenceType::Table(_) => add_reason(reasons, "reference.table"),
            ReferenceType::Cell3D { .. } => add_reason(reasons, "reference.cell3d"),
            ReferenceType::Range3D { .. } => add_reason(reasons, "reference.range3d"),
            ReferenceType::Cell { .. }
            | ReferenceType::Range { .. }
            | ReferenceType::NamedRange(_) => {}
        },
        ASTNodeType::UnaryOp { expr, .. } => census_ast(expr, reasons),
        ASTNodeType::BinaryOp { left, right, .. } => {
            census_ast(left, reasons);
            census_ast(right, reasons);
        }
        ASTNodeType::Function { name, args } => {
            let Some(resolved) = function_registry::resolve_for_arity("", name, args.len()) else {
                add_reason(
                    reasons,
                    format!("function.identity_missing:{name}/{}", args.len()),
                );
                for arg in args {
                    census_ast(arg, reasons);
                }
                return;
            };
            let Some(class) = census_capability_class(&resolved) else {
                add_reason(reasons, format!("function.contract_missing:{name}"));
                for arg in args {
                    census_ast(arg, reasons);
                }
                return;
            };
            let caps = resolved.function.caps();
            match class {
                FunctionCapabilityClass::ArgumentStateSafe => {}
                FunctionCapabilityClass::ContextDependent => {
                    if !caps.contains(FnCaps::V2_CONTEXT_OBSERVED) {
                        add_reason(
                            reasons,
                            format!("function.context_observation_unproven:{name}"),
                        );
                    }
                }
                FunctionCapabilityClass::DynamicReference => {
                    if !caps.contains(FnCaps::V2_DYNAMIC_TARGET_OBSERVED) {
                        add_reason(
                            reasons,
                            format!("function.dynamic_target_observation_unproven:{name}"),
                        );
                    }
                    if !caps.contains(FnCaps::V2_REFERENCE_SHAPE_OBSERVED) {
                        add_reason(
                            reasons,
                            format!("function.dynamic_shape_observation_unproven:{name}"),
                        );
                    }
                }
                FunctionCapabilityClass::StructuralReferenceShape => {
                    let scalar_args = args.iter().all(census_scalar_syntax_safe);
                    let shape_safe = caps.contains(FnCaps::V2_REFERENCE_SHAPE_OBSERVED)
                        || (caps.contains(FnCaps::V2_SCALAR_OUTPUT_FROM_SCALAR_ARGS)
                            && scalar_args)
                        || (caps.contains(FnCaps::V2_POSITIVE_SELECTORS_SCALAR_REFERENCE)
                            && census_positive_selector_literals(args));
                    if !shape_safe {
                        add_reason(
                            reasons,
                            format!("function.reference_shape_observation_unproven:{name}"),
                        );
                    }
                }
                FunctionCapabilityClass::VolatileOrEnvironmentDependent => {
                    add_reason(
                        reasons,
                        format!("function.volatile_or_environment_dependent:{name}"),
                    );
                }
                FunctionCapabilityClass::Unsupported => {
                    add_reason(
                        reasons,
                        format!("function.semantic_contract_unsupported:{name}"),
                    );
                }
            }
            for arg in args {
                census_ast(arg, reasons);
            }
        }
        ASTNodeType::Array(rows) => {
            add_reason(reasons, "ast.array");
            for arg in rows.iter().flatten() {
                census_ast(arg, reasons);
            }
        }
        ASTNodeType::Call { callee, args } => {
            add_reason(reasons, "ast.call");
            census_ast(callee, reasons);
            for arg in args {
                census_ast(arg, reasons);
            }
        }
    }
}

fn workbook_admission_census(path: &Path) -> BTreeMap<String, usize> {
    formualizer_eval::builtins::load_builtins();
    let mut backend = CalamineAdapter::open_path(path).expect("open workbook for census");
    let mut reasons = BTreeMap::new();
    let defined_names = backend.defined_names().expect("read defined names");
    for name in defined_names {
        if matches!(
            name.definition,
            formualizer_workbook::DefinedNameDefinition::Formula { .. }
        ) {
            add_reason(&mut reasons, "name.formula_definition");
        }
    }
    for sheet in backend.sheet_names().expect("read sheet names") {
        let data = backend.read_sheet(&sheet).expect("read sheet for census");
        for cell in data.cells.values() {
            if let Some(formula) = &cell.formula {
                if let Ok(ast) = formualizer_parse::parser::parse(formula) {
                    census_ast(&ast, &mut reasons);
                } else {
                    add_reason(&mut reasons, "formula.parse_error");
                }
            }
        }
    }
    reasons
}

fn load_and_print_production_stats(path: &Path) {
    let backend_started = Instant::now();
    let mut backend = CalamineAdapter::open_path(path).expect("open workbook");
    let backend_open_elapsed = backend_started.elapsed();
    let engine_started = Instant::now();
    let mut engine = Engine::new(TestWorkbook::new(), config());
    let engine_new_elapsed = engine_started.elapsed();
    let stream_started = Instant::now();
    backend
        .stream_into_engine(&mut engine)
        .expect("stream workbook into production Engine");
    let stream_elapsed = stream_started.elapsed();
    let total_elapsed = backend_started.elapsed();
    let stats = backend.load_stats().expect("Calamine load stats");
    let baseline = engine.baseline_stats();
    println!(
        "production_backend_open_elapsed_ns={}",
        backend_open_elapsed.as_nanos()
    );
    println!(
        "production_engine_new_elapsed_ns={}",
        engine_new_elapsed.as_nanos()
    );
    println!("production_stream_elapsed_ns={}", stream_elapsed.as_nanos());
    println!("production_load_elapsed_ns={}", total_elapsed.as_nanos());
    println!("production_load_stats={stats:?}");
    println!("production_baseline_stats={baseline:?}");
}

#[test]
#[ignore = "requires the local real Light workbook"]
fn light_production_load_performance_instrumented() {
    let path = workbook_path("FORMUALIZER_LIGHT_WORKBOOK", DEFAULT_LIGHT);
    assert!(path.is_file(), "Light workbook missing: {}", path.display());
    load_and_print_production_stats(&path);
}

#[test]
#[ignore = "requires the local real Heavy workbook"]
fn heavy_production_load_performance_instrumented() {
    let path = workbook_path("FORMUALIZER_HEAVY_WORKBOOK", DEFAULT_HEAVY);
    assert!(path.is_file(), "Heavy workbook missing: {}", path.display());
    load_and_print_production_stats(&path);
}

#[test]
#[ignore = "requires the local real Heavy workbook"]
fn heavy_admission_reason_census() {
    let path = workbook_path("FORMUALIZER_HEAVY_WORKBOOK", DEFAULT_HEAVY);
    assert!(path.is_file(), "Heavy workbook missing: {}", path.display());
    let reasons = workbook_admission_census(&path);
    println!("Heavy admission reasons: {reasons:?}");
    assert!(
        !reasons.is_empty(),
        "Heavy unexpectedly has no admission reasons"
    );
}

#[test]
#[ignore = "requires the local real Light workbook"]
fn light_admission_reason_census() {
    let path = workbook_path("FORMUALIZER_LIGHT_WORKBOOK", DEFAULT_LIGHT);
    assert!(path.is_file(), "Light workbook missing: {}", path.display());
    let reasons = workbook_admission_census(&path);
    println!("Light admission reasons: {reasons:?}");
    assert!(
        !reasons.is_empty(),
        "Light unexpectedly has no admission reasons"
    );
}

#[test]
#[ignore = "requires the local real Heavy workbook"]
fn heavy_f7_outputs_d53_use_demand_scoped_v2_without_k55_map() {
    let path = workbook_path("FORMUALIZER_HEAVY_WORKBOOK", DEFAULT_HEAVY);
    assert!(path.is_file(), "Heavy workbook missing: {}", path.display());
    let mut v2 = load_engine(&path, true);
    let outputs = v2.sheet_id("Outputs").expect("Outputs sheet");
    let k55 = v2
        .vertex_for_cell(&CellRef::new(
            v2.sheet_id("CashFlow Inputs")
                .expect("CashFlow Inputs sheet"),
            Coord::from_excel(55, 11, true, true),
        ))
        .expect("CashFlow Inputs!K55 vertex");
    let d53 = v2
        .vertex_for_cell(&CellRef::new(outputs, Coord::from_excel(53, 4, true, true)))
        .expect("Outputs!D53 vertex");
    let demand = v2.v2_demand_vertices_for_test(&[d53]);
    assert!(
        !demand.contains(&k55),
        "Outputs!D53 demand unexpectedly includes CashFlow Inputs!K55"
    );

    v2.set_cell_value("Inputs", 7, 6, LiteralValue::Number(300.0))
        .expect("set Inputs!F7");
    let result = v2
        .evaluate_targets(&[EvaluationTarget::Cell {
            sheet: "Outputs".to_string(),
            row: 53,
            col: 4,
        }])
        .expect("targeted Heavy F7/Outputs!D53 evaluation");

    assert!(result.computed_vertices > 0);
    let diagnostics = v2.v2_diagnostics_for_test();
    assert_eq!(diagnostics.fallback_activations, 0);
    assert!(diagnostics.formula_evaluations > 0);
    assert!(diagnostics.current_read_sets > 0);
    assert!(v2.vertex_value(k55).is_none());

    v2.evaluate_targets(&[EvaluationTarget::Cell {
        sheet: "CashFlow Inputs".to_string(),
        row: 55,
        col: 11,
    }])
    .expect("unsupported MAP target should fail closed through V1");
    assert!(matches!(
        v2.get_cell_value("CashFlow Inputs", 55, 11),
        Some(LiteralValue::Error(error)) if error.kind == formualizer_common::ExcelErrorKind::Name
    ));
    assert!(v2.v2_diagnostics_for_test().fallback_activations >= 1);
}

#[test]
#[ignore = "requires the local real Heavy workbook"]
fn heavy_targeted_v2_calculation_only_validation() {
    let path = workbook_path("FORMUALIZER_HEAVY_WORKBOOK", DEFAULT_HEAVY);
    assert!(path.is_file(), "Heavy workbook missing: {}", path.display());
    let targets = [EvaluationTarget::Cell {
        sheet: "Outputs".to_string(),
        row: 53,
        col: 4,
    }];
    let mut v2 = load_engine(&path, true);
    let outputs = v2.sheet_id("Outputs").expect("Outputs sheet");
    let cashflow_inputs = v2
        .sheet_id("CashFlow Inputs")
        .expect("CashFlow Inputs sheet");
    let d53 = v2
        .vertex_for_cell(&CellRef::new(outputs, Coord::from_excel(53, 4, true, true)))
        .expect("Outputs!D53 vertex");
    let k55 = v2
        .vertex_for_cell(&CellRef::new(
            cashflow_inputs,
            Coord::from_excel(55, 11, true, true),
        ))
        .expect("CashFlow Inputs!K55 vertex");
    assert!(!v2.v2_demand_vertices_for_test(&[d53]).contains(&k55));
    println!(
        "heavy_loaded_v2_baseline_working_set_bytes={:?}",
        process_working_set_bytes()
    );

    v2.set_cell_value("Inputs", 7, 6, LiteralValue::Number(300.0))
        .expect("set Inputs!F7=300");
    let f7_300 = run_v2_profile_request(&mut v2, "heavy_f7_300_v2", "outputs_d53", &targets, 53, 4);

    v2.set_cell_value("Inputs", 7, 6, LiteralValue::Number(500.0))
        .expect("set Inputs!F7=500");
    let f7_500 = run_v2_profile_request(&mut v2, "heavy_f7_500_v2", "outputs_d53", &targets, 53, 4);
    if let Some(LiteralValue::Number(value)) = f7_500.as_ref() {
        println!("heavy_f7_500_v2_outputs_d53_actual={value:.15}");
        println!("heavy_f7_500_v2_outputs_d53_rounded={}", value.round());
        assert!((value.round() - 5767.0).abs() <= 1.0);
    } else {
        panic!("Heavy V2 Outputs!D53 did not produce a numeric value: {f7_500:?}");
    }

    let unchanged = run_v2_profile_request(
        &mut v2,
        "heavy_f7_500_unchanged_v2",
        "outputs_d53",
        &targets,
        53,
        4,
    );
    assert_eq!(f7_500, unchanged);
    assert!(f7_300.is_some());
}

#[test]
#[ignore = "requires the local real Light workbook"]
fn light_targeted_v2_calculation_only_validation() {
    let path = workbook_path("FORMUALIZER_LIGHT_WORKBOOK", DEFAULT_LIGHT);
    assert!(path.is_file(), "Light workbook missing: {}", path.display());
    let targets = [EvaluationTarget::Cell {
        sheet: "Outputs".to_string(),
        row: 41,
        col: 4,
    }];
    let mut v2 = load_engine(&path, true);
    println!(
        "light_loaded_v2_baseline_working_set_bytes={:?}",
        process_working_set_bytes()
    );

    v2.set_cell_value("Inputs", 6, 6, LiteralValue::Number(300.0))
        .expect("set Inputs!F6=300");
    let f6_300 = run_v2_profile_request(&mut v2, "light_f6_300_v2", "outputs_d41", &targets, 41, 4);

    v2.set_cell_value("Inputs", 6, 6, LiteralValue::Number(500.0))
        .expect("set Inputs!F6=500");
    let f6_500 = run_v2_profile_request(&mut v2, "light_f6_500_v2", "outputs_d41", &targets, 41, 4);
    let unchanged = run_v2_profile_request(
        &mut v2,
        "light_f6_500_unchanged_v2",
        "outputs_d41",
        &targets,
        41,
        4,
    );

    assert!(f6_300.is_some());
    assert!(f6_500.is_some());
    assert_eq!(f6_500, unchanged);
}

fn run_v2_profile_request(
    engine: &mut Engine<TestWorkbook>,
    label: &str,
    output_label: &str,
    targets: &[EvaluationTarget],
    output_row: u32,
    output_col: u32,
) -> Option<LiteralValue> {
    let started = Instant::now();
    let result = engine
        .evaluate_targets(targets)
        .expect("V2 target evaluation");
    let wall = started.elapsed();
    let output = engine.get_cell_value("Outputs", output_row, output_col);
    let diagnostics = engine.v2_diagnostics_for_test();

    println!("{label}_{output_label}={output:?}");
    println!("{label}_wall_ns={}", wall.as_nanos());
    println!("{label}_kernel_elapsed_ns={}", result.elapsed.as_nanos());
    print_v2_profile_diagnostics(label, &diagnostics);
    print_v2_exclusive_attribution(label, &diagnostics.exclusive_attribution);
    print_v2_workspace_profiles(label, engine, &diagnostics);
    if std::env::var_os("FZ_BENCH_V2_OWNER_RESOLVERS").is_some()
        && matches!(label, "light_f6_500_v2" | "heavy_f7_500_v2")
    {
        println!(
            "{label}_owner_resolver_benchmark={:?}",
            engine.v2_owner_resolver_benchmark_for_test()
        );
    }
    if label == "light_f6_500_v2" && std::env::var_os("FZ_PROFILE_WORKSPACE_STRUCTURE").is_some() {
        print_v2_workspace_structure(engine);
    }
    println!(
        "{label}_working_set_bytes={:?}",
        process_working_set_bytes()
    );
    output
}

fn print_v2_profile_diagnostics(
    label: &str,
    diagnostics: &formualizer_eval::engine::EngineV2Diagnostics,
) {
    println!(
        "{label}_formulas_evaluated={}",
        diagnostics.formula_evaluations
    );
    println!(
        "{label}_formulas_evaluated_inside_workspaces={}",
        diagnostics.formulas_evaluated_inside_workspaces
    );
    println!(
        "{label}_formulas_evaluated_outside_workspaces={}",
        diagnostics.formulas_evaluated_outside_workspaces
    );
    println!("{label}_schedule_units={}", diagnostics.schedule_units);
    println!("{label}_workspace_units={}", diagnostics.workspace_units);
    println!(
        "{label}_active_cyclic_workspace_members={}",
        diagnostics.active_cyclic_workspace_members
    );
    println!(
        "{label}_workspace_members={}",
        diagnostics.workspace_members
    );
    println!("{label}_solver_passes={}", diagnostics.solver_passes);
    println!(
        "{label}_exact_formula_edges_retained={}",
        diagnostics.exact_formula_edges_retained
    );
    println!(
        "{label}_logical_range_positions={}",
        diagnostics.logical_range_positions
    );
    println!(
        "{label}_physical_cells_fetched={}",
        diagnostics.physical_cells_read
    );
    println!(
        "{label}_fallback_activations={}",
        diagnostics.fallback_activations
    );
    println!(
        "{label}_conservative_dirty_formula_count={}",
        diagnostics.conservative_dirty_formula_count
    );
    println!(
        "{label}_effective_dirty_formula_count={}",
        diagnostics.effective_dirty_formula_count
    );
    println!(
        "{label}_pruned_dirty_formula_count={}",
        diagnostics.pruned_dirty_formula_count
    );
    println!(
        "{label}_conservative_workspace_candidate_count={}",
        diagnostics.conservative_workspace_candidate_count
    );
    println!(
        "{label}_effective_workspace_count={}",
        diagnostics.effective_workspace_count
    );
    println!(
        "{label}_pruned_workspace_count={}",
        diagnostics.pruned_workspace_count
    );
    println!(
        "{label}_exact_pruning_accepted_count={}",
        diagnostics.exact_pruning_accepted_count
    );
    println!(
        "{label}_exact_pruning_rejected_count={}",
        diagnostics.exact_pruning_rejected_count
    );
    println!(
        "{label}_exact_reverse_propagation_vertices_visited={}",
        diagnostics.exact_reverse_propagation_vertices_visited
    );
    println!(
        "{label}_exact_reverse_read_formulas_reached={}",
        diagnostics.exact_reverse_read_formulas_reached
    );
    println!(
        "{label}_exact_formula_edge_formulas_reached={}",
        diagnostics.exact_formula_edge_formulas_reached
    );
    println!(
        "{label}_runtime_expansion_reopen_count={}",
        diagnostics.runtime_expansion_reopen_count
    );
    println!(
        "{label}_runtime_contract_candidates_hits_misses={}/{}/{}",
        diagnostics.runtime_contract_validation_candidates,
        diagnostics.runtime_contract_validation_cache_hits,
        diagnostics.runtime_contract_validation_cache_misses
    );
    println!(
        "{label}_runtime_contract_edges_skipped_examined={}/{}",
        diagnostics.runtime_contract_edges_skipped, diagnostics.runtime_contract_edges_examined
    );
    println!(
        "{label}_runtime_contract_certificates_invalidated={}",
        diagnostics.runtime_contract_certificates_invalidated
    );
    println!(
        "{label}_runtime_contract_certificate_invalidation_reasons={:?}",
        diagnostics.runtime_contract_certificate_invalidation_reasons
    );
    println!(
        "{label}_pruning_rejection_reasons={:?}",
        diagnostics.pruning_rejection_reasons
    );
    println!(
        "{label}_conservative_workspace_member_count={}",
        diagnostics.conservative_workspace_member_count
    );
    println!(
        "{label}_exact_scc_member_count={}",
        diagnostics.exact_scc_member_count
    );
    println!(
        "{label}_non_feedback_workspace_member_count={}",
        diagnostics.non_feedback_workspace_member_count
    );
    println!(
        "{label}_workspace_discovery_formula_evaluations={}",
        diagnostics.workspace_discovery_formula_evaluations
    );
    println!(
        "{label}_workspace_exact_scc_formula_evaluations={}",
        diagnostics.workspace_exact_scc_formula_evaluations
    );
    println!(
        "{label}_workspace_upstream_formula_evaluations={}",
        diagnostics.workspace_upstream_formula_evaluations
    );
    println!(
        "{label}_workspace_downstream_formula_evaluations={}",
        diagnostics.workspace_downstream_formula_evaluations
    );
    println!(
        "{label}_repeated_non_feedback_evaluations={}",
        diagnostics.repeated_non_feedback_evaluations
    );
    println!(
        "{label}_repeated_non_feedback_evaluations_avoided={}",
        diagnostics.repeated_non_feedback_evaluations_avoided
    );
    println!(
        "{label}_workspaces_using_exact_scc_kernel={}",
        diagnostics.workspaces_using_exact_scc_kernel
    );
    println!(
        "{label}_workspaces_using_full_conservative_solver={}",
        diagnostics.workspaces_using_full_conservative_solver
    );
    println!(
        "{label}_workspace_kernel_fallback_reasons={:?}",
        diagnostics.workspace_kernel_fallback_reasons
    );
    println!(
        "{label}_exact_scc_rebuild_count={}",
        diagnostics.exact_scc_rebuild_count
    );
    println!(
        "{label}_exact_scc_expansion_count={}",
        diagnostics.exact_scc_expansion_count
    );
    println!(
        "{label}_workspace_reopen_count={}",
        diagnostics.workspace_reopen_count
    );
    println!(
        "{label}_retained_current_read_sets={}",
        diagnostics.current_read_sets
    );
    println!(
        "{label}_retained_reverse_buckets={}",
        diagnostics.reverse_buckets
    );
    println!(
        "{label}_retained_exact_formula_edges={}",
        diagnostics.exact_formula_edges_retained
    );
    println!(
        "{label}_retained_state_scan_read_sets={}",
        diagnostics.retained_state_scan_read_sets
    );
    println!(
        "{label}_retained_state_scan_edges={}",
        diagnostics.retained_state_scan_edges
    );
    println!(
        "{label}_retained_state_scan_ns={}",
        diagnostics.retained_state_scan_ns
    );
    println!(
        "{label}_demand_nodes_visited={}",
        diagnostics.demand_nodes_visited
    );
    println!(
        "{label}_demand_explicit_edges_visited={}",
        diagnostics.demand_explicit_edges_visited
    );
    println!(
        "{label}_demand_virtual_edges_visited={}",
        diagnostics.demand_virtual_edges_visited
    );
    println!(
        "{label}_demand_dedup_entries={}",
        diagnostics.demand_dedup_entries
    );
    println!(
        "{label}_demand_allocation_ns={}",
        diagnostics.demand_allocation_ns
    );
    println!(
        "{label}_demand_dependency_traversal_ns={}",
        diagnostics.demand_dependency_traversal_ns
    );
    println!(
        "{label}_demand_virtual_traversal_ns={}",
        diagnostics.demand_virtual_traversal_ns
    );
    println!(
        "{label}_validation_read_sets_examined={}",
        diagnostics.validation_read_sets_examined
    );
    println!(
        "{label}_validation_runtime_formula_edges={}/{}/{}",
        diagnostics.validation_runtime_formula_edges_examined,
        diagnostics.validation_runtime_formula_edges_unchanged,
        diagnostics.validation_runtime_formula_edges_invalidated
    );
    println!(
        "{label}_validation_reference_observations={}/{}/{}",
        diagnostics.validation_reference_observations_examined,
        diagnostics.validation_reference_observations_unchanged,
        diagnostics.validation_reference_observations_invalidated
    );
    println!(
        "{label}_validation_topology_checks={}/{}/{}",
        diagnostics.validation_topology_checks,
        diagnostics
            .validation_topology_checks
            .saturating_sub(diagnostics.validation_topology_invalidated),
        diagnostics.validation_topology_invalidated
    );
    println!(
        "{label}_validation_symbol_name={}/{}/{}",
        diagnostics.validation_symbol_name_entries,
        diagnostics.validation_symbol_name_unchanged,
        diagnostics.validation_symbol_name_invalidated
    );
    println!(
        "{label}_validation_table_shape={}/{}/{}",
        diagnostics.validation_table_shape_entries,
        diagnostics.validation_table_shape_unchanged,
        diagnostics.validation_table_shape_invalidated
    );
    println!(
        "{label}_validation_spill_shape={}/{}/{}",
        diagnostics.validation_spill_shape_entries,
        diagnostics.validation_spill_shape_unchanged,
        diagnostics.validation_spill_shape_invalidated
    );
    println!(
        "{label}_validation_provider_effect={}/{}/{}",
        diagnostics.validation_provider_effect_entries,
        diagnostics.validation_provider_effect_unchanged,
        diagnostics.validation_provider_effect_invalidated
    );
    println!(
        "{label}_validation_selected_reference={}/{}/{}",
        diagnostics.validation_selected_reference_entries,
        diagnostics.validation_selected_reference_unchanged,
        diagnostics.validation_selected_reference_invalidated
    );
    println!(
        "{label}_validation_range_reference={}/{}/{}",
        diagnostics.validation_range_reference_entries,
        diagnostics.validation_range_reference_unchanged,
        diagnostics.validation_range_reference_invalidated
    );
    println!(
        "{label}_validation_times_ns=runtime:{} reference:{} topology:{} metadata:{}",
        diagnostics.validation_runtime_formula_ns,
        diagnostics.validation_reference_ns,
        diagnostics.validation_topology_ns,
        diagnostics.validation_metadata_ns
    );
    println!(
        "{label}_exact_read_sets_finalized_changed_unchanged={}/{}/{}",
        diagnostics.exact_read_sets_finalized,
        diagnostics.exact_read_sets_changed,
        diagnostics.exact_read_sets_unchanged
    );
    println!(
        "{label}_diagnostic_read_set_compare_ns={}",
        diagnostics.diagnostic_read_set_compare_ns
    );
    println!(
        "{label}_exact_edges_examined_removed_inserted_unchanged={}/{}/{}/{}",
        diagnostics.exact_edges_examined,
        diagnostics.exact_edges_removed,
        diagnostics.exact_edges_inserted,
        diagnostics.exact_edges_unchanged
    );
    println!(
        "{label}_reverse_buckets_touched={}",
        diagnostics.reverse_buckets_touched
    );
    println!(
        "{label}_edge_times_ns=compare:{} remove:{} insert:{} canonicalize:{}",
        diagnostics.exact_edge_compare_ns,
        diagnostics.exact_edge_remove_ns,
        diagnostics.exact_edge_insert_ns,
        diagnostics.exact_edge_canonicalize_ns
    );
    println!(
        "{label}_kernel_named_phase_ns={}",
        diagnostics.kernel_named_phase_ns
    );
    println!(
        "{label}_kernel_unattributed_ns={}",
        diagnostics.kernel_unattributed_ns
    );
    println!(
        "{label}_outside_acyclic_formula_evaluations={}",
        diagnostics.outside_acyclic_formula_evaluations
    );
    println!(
        "{label}_outside_acyclic_formula_evaluation_ns={}",
        diagnostics.outside_acyclic_formula_evaluation_ns
    );
    println!(
        "{label}_workspace_discovery_formula_evaluation_ns={}",
        diagnostics.workspace_discovery_formula_evaluation_ns
    );
    println!(
        "{label}_workspace_upstream_formula_evaluation_ns={}",
        diagnostics.workspace_upstream_formula_evaluation_ns
    );
    println!(
        "{label}_workspace_downstream_formula_evaluation_ns={}",
        diagnostics.workspace_downstream_formula_evaluation_ns
    );
    println!(
        "{label}_scc_preparation_formula_evaluations_ns={}/{}",
        diagnostics.scc_preparation_formula_evaluations, diagnostics.scc_preparation_ns
    );
    println!(
        "{label}_retained_state_scan_read_sets_edges_ns={}/{}/{}",
        diagnostics.retained_state_scan_read_sets,
        diagnostics.retained_state_scan_edges,
        diagnostics.retained_state_scan_ns
    );
    println!(
        "{label}_demand_nodes_explicit_virtual_dedup={}/{}/{}/{}",
        diagnostics.demand_nodes_visited,
        diagnostics.demand_explicit_edges_visited,
        diagnostics.demand_virtual_edges_visited,
        diagnostics.demand_dedup_entries
    );
    println!(
        "{label}_demand_cost_ns=allocation:{} dependencies:{} virtual:{}",
        diagnostics.demand_allocation_ns,
        diagnostics.demand_dependency_traversal_ns,
        diagnostics.demand_virtual_traversal_ns
    );
    let virtual_demand = &diagnostics.virtual_demand;
    println!(
        "{label}_virtual_demand_counts=expansion_requests:{} expansion_calls:{} sources_with_edges:{} unique_sources:{} unique_targets:{} range_source_lookups:{} range_sources_with_deps:{} range_dependency_records:{} range_expansions:{} dynamic_source_checks:{} dynamic_expansions:{} sheet_resolutions:{} coordinates_examined:{} grid_lookups:{} formula_owner_lookups:{} raw_edges:{} unique_pairs:{} duplicate_pairs:{} closure_probes:{} closure_new_targets:{} stack_pushes:{} temporary_vecs:{} temporary_maps:{}",
        virtual_demand.expansion_requests,
        virtual_demand.expansion_calls,
        virtual_demand.sources_with_edges,
        virtual_demand.unique_sources,
        virtual_demand.unique_targets,
        virtual_demand.range_source_lookups,
        virtual_demand.range_sources_with_dependencies,
        virtual_demand.range_dependency_records,
        virtual_demand.range_expansions,
        virtual_demand.dynamic_source_checks,
        virtual_demand.dynamic_expansion_calls,
        virtual_demand.sheet_identity_resolutions,
        virtual_demand.coordinates_examined,
        virtual_demand.vertex_grid_lookups,
        virtual_demand.formula_owner_graph_lookups,
        virtual_demand.raw_edges_emitted,
        virtual_demand.unique_source_target_pairs,
        virtual_demand.duplicate_source_target_pairs,
        virtual_demand.closure_membership_probes,
        virtual_demand.closure_new_targets,
        virtual_demand.stack_pushes,
        virtual_demand.temporary_vec_allocations,
        virtual_demand.temporary_map_allocations
    );
    println!(
        "{label}_virtual_demand_times_ns=source_lookup:{} range_resolution:{} expansion_materialization:{} identity_conversion:{} target_lookup_filter:{} dynamic_evaluation:{} builder_dedup:{} builder_map:{} closure_source_lookup:{} closure_publish:{} closure_membership:{}",
        virtual_demand.source_lookup_ns,
        virtual_demand.range_resolution_ns,
        virtual_demand.expansion_materialization_ns,
        virtual_demand.identity_conversion_ns,
        virtual_demand.target_lookup_filter_ns,
        virtual_demand.dynamic_evaluation_ns,
        virtual_demand.builder_dedup_ns,
        virtual_demand.builder_map_ns,
        virtual_demand.closure_source_lookup_ns,
        virtual_demand.closure_publish_ns,
        virtual_demand.closure_membership_ns
    );
    println!(
        "{label}_admission_demand_nodes_edges_virtual={}/{}/{}",
        diagnostics.admission_demand_nodes_visited,
        diagnostics.admission_demand_explicit_edges_visited,
        diagnostics.admission_demand_virtual_edges_visited
    );
    println!(
        "{label}_admission_demand_cost_ns=allocation:{} dependencies:{} virtual:{}",
        diagnostics.admission_demand_allocation_ns,
        diagnostics.admission_demand_dependency_traversal_ns,
        diagnostics.admission_demand_virtual_traversal_ns
    );
    println!(
        "{label}_schedule_demand_nodes_edges_virtual={}/{}/{}",
        diagnostics.schedule_demand_nodes_visited,
        diagnostics.schedule_demand_explicit_edges_visited,
        diagnostics.schedule_demand_virtual_edges_visited
    );
    println!(
        "{label}_schedule_demand_cost_ns=allocation:{} dependencies:{} virtual:{}",
        diagnostics.schedule_demand_allocation_ns,
        diagnostics.schedule_demand_dependency_traversal_ns,
        diagnostics.schedule_demand_virtual_traversal_ns
    );
    println!(
        "{label}_demand_closures_built_reuse_hits_rejections={}/{}/{}",
        diagnostics.demand_closures_built,
        diagnostics.demand_closure_reuse_hits,
        diagnostics.demand_closure_reuse_rejections
    );
    println!(
        "{label}_demand_closure_reuse_rejection_reasons={:?}",
        diagnostics.demand_closure_reuse_rejection_reasons
    );
    println!(
        "{label}_demand_reuse_consumption_ns={}",
        diagnostics.demand_reuse_consumption_ns
    );
    println!(
        "{label}_retained_plan_candidates_hits_rejections={}/{}/{}",
        diagnostics.workspace_retained_plan_candidates,
        diagnostics.workspace_retained_plan_hits,
        diagnostics.workspace_retained_plan_rejections
    );
    println!(
        "{label}_retained_plan_rejection_reasons={:?}",
        diagnostics.workspace_retained_plan_rejection_reasons
    );
    println!(
        "{label}_discovery_avoided_dirty_upstream_clean_reuses={}/{}/{}",
        diagnostics.discovery_evaluations_avoided,
        diagnostics.dirty_upstream_evaluations,
        diagnostics.clean_upstream_cache_reuses
    );
    println!(
        "{label}_scc_discovery_avoided_downstream_discovery_avoided={}/{}",
        diagnostics.scc_discovery_evaluations_avoided,
        diagnostics.downstream_discovery_evaluations_avoided
    );
    println!(
        "{label}_retained_plan_runtime_invalidations_reopens={}/{}",
        diagnostics.retained_plan_runtime_invalidations, diagnostics.retained_plan_reopens
    );
    println!(
        "{label}_retained_plan_runtime_invalidation_reasons={:?}",
        diagnostics.retained_plan_runtime_invalidation_reasons
    );
    println!(
        "{label}_classification_effective_clean_exact_upstream_downstream_unrelated={}/{}/{}/{}/{}/{}",
        diagnostics.retained_classification_effective_dirty_members,
        diagnostics.retained_classification_clean_members,
        diagnostics.retained_classification_exact_scc_members,
        diagnostics.retained_classification_upstream_members,
        diagnostics.retained_classification_downstream_members,
        diagnostics.retained_classification_unrelated_members
    );
    println!(
        "{label}_classification_missing_invalid_reusable={}/{}/{}",
        diagnostics.retained_classification_missing_reads,
        diagnostics.retained_classification_invalid_members,
        diagnostics.retained_classification_reusable_values
    );
    println!(
        "{label}_validation_read_sets={}",
        diagnostics.validation_read_sets_examined
    );
    println!(
        "{label}_validation_runtime_edges_examined_unchanged_invalidated={}/{}/{}",
        diagnostics.validation_runtime_formula_edges_examined,
        diagnostics.validation_runtime_formula_edges_unchanged,
        diagnostics.validation_runtime_formula_edges_invalidated
    );
    println!(
        "{label}_validation_reference_observations_examined_unchanged_invalidated={}/{}/{}",
        diagnostics.validation_reference_observations_examined,
        diagnostics.validation_reference_observations_unchanged,
        diagnostics.validation_reference_observations_invalidated
    );
    println!(
        "{label}_validation_topology_checks_unchanged_invalidated={}/{}/{}",
        diagnostics.validation_topology_checks,
        diagnostics
            .validation_topology_checks
            .saturating_sub(diagnostics.validation_topology_invalidated),
        diagnostics.validation_topology_invalidated
    );
    println!(
        "{label}_validation_metadata_entries=names:{} tables:{} spills:{} providers:{} selected:{} ranges:{}",
        diagnostics.validation_symbol_name_entries,
        diagnostics.validation_table_shape_entries,
        diagnostics.validation_spill_shape_entries,
        diagnostics.validation_provider_effect_entries,
        diagnostics.validation_selected_reference_entries,
        diagnostics.validation_range_reference_entries
    );
    println!(
        "{label}_validation_metadata_unchanged=names:{} tables:{} spills:{} providers:{} selected:{} ranges:{}",
        diagnostics.validation_symbol_name_unchanged,
        diagnostics.validation_table_shape_unchanged,
        diagnostics.validation_spill_shape_unchanged,
        diagnostics.validation_provider_effect_unchanged,
        diagnostics.validation_selected_reference_unchanged,
        diagnostics.validation_range_reference_unchanged
    );
    println!(
        "{label}_validation_metadata_invalidated=names:{} tables:{} spills:{} providers:{} selected:{} ranges:{}",
        diagnostics.validation_symbol_name_invalidated,
        diagnostics.validation_table_shape_invalidated,
        diagnostics.validation_spill_shape_invalidated,
        diagnostics.validation_provider_effect_invalidated,
        diagnostics.validation_selected_reference_invalidated,
        diagnostics.validation_range_reference_invalidated
    );
    println!(
        "{label}_validation_times_ns=runtime:{} reference:{} topology:{} metadata:{}",
        diagnostics.validation_runtime_formula_ns,
        diagnostics.validation_reference_ns,
        diagnostics.validation_topology_ns,
        diagnostics.validation_metadata_ns
    );
    println!(
        "{label}_exact_read_sets_finalized_changed_unchanged={}/{}/{}",
        diagnostics.exact_read_sets_finalized,
        diagnostics.exact_read_sets_changed,
        diagnostics.exact_read_sets_unchanged
    );
    println!(
        "{label}_diagnostic_read_set_compare_ns={}",
        diagnostics.diagnostic_read_set_compare_ns
    );
    println!(
        "{label}_exact_edges_examined_removed_inserted_unchanged={}/{}/{}/{}",
        diagnostics.exact_edges_examined,
        diagnostics.exact_edges_removed,
        diagnostics.exact_edges_inserted,
        diagnostics.exact_edges_unchanged
    );
    println!(
        "{label}_reverse_buckets_touched={}",
        diagnostics.reverse_buckets_touched
    );
    println!(
        "{label}_edge_sets_compared_identical_changed={}/{}/{}",
        diagnostics.exact_edge_sets_compared,
        diagnostics.exact_identical_edge_sets,
        diagnostics.exact_changed_edge_sets
    );
    println!(
        "{label}_reverse_buckets_untouched_mutated={}/{}",
        diagnostics.exact_reverse_buckets_untouched, diagnostics.exact_reverse_buckets_mutated
    );
    println!(
        "{label}_full_replacement_fallback_count={}",
        diagnostics.exact_full_replacement_fallback_count
    );
    println!(
        "{label}_full_replacement_fallback_reasons={:?}",
        diagnostics.exact_full_replacement_fallback_reasons
    );
    println!(
        "{label}_edge_cost_ns=compare:{} remove:{} insert:{} canonicalize:{}",
        diagnostics.exact_edge_compare_ns,
        diagnostics.exact_edge_remove_ns,
        diagnostics.exact_edge_insert_ns,
        diagnostics.exact_edge_canonicalize_ns
    );
    println!(
        "{label}_kernel_named_phase_ns={}",
        diagnostics.kernel_named_phase_ns
    );
    println!(
        "{label}_kernel_unattributed_ns={}",
        diagnostics.kernel_unattributed_ns
    );
    println!(
        "{label}_kernel_top_level_named_phase_ns={}",
        diagnostics.kernel_top_level_named_phase_ns
    );
    println!(
        "{label}_kernel_top_level_unattributed_ns={}",
        diagnostics.kernel_top_level_unattributed_ns
    );
    println!("{label}_kernel_elapsed_ns={}", diagnostics.elapsed_ns);
    println!("{label}_formula_wrapper_ns={}", diagnostics.formula_ns);
    println!("{label}_workspace_wrapper_ns={}", diagnostics.workspace_ns);
    println!("{label}_cleanup_ns={}", diagnostics.cleanup_ns);
    println!(
        "{label}_phase_demand_subgraph_ns={}",
        diagnostics.demand_subgraph_ns
    );
    println!(
        "{label}_phase_schedule_demand_subgraph_ns={}",
        diagnostics.schedule_demand_subgraph_ns
    );
    println!(
        "{label}_phase_scoped_admission_ns={}",
        diagnostics.scoped_admission_ns
    );
    println!(
        "{label}_phase_dirty_seed_selection_ns={}",
        diagnostics.dirty_seed_selection_ns
    );
    println!(
        "{label}_phase_schedule_construction_ns={}",
        diagnostics.schedule_construction_ns
    );
    println!(
        "{label}_phase_acyclic_formula_evaluation_ns={}",
        diagnostics.acyclic_formula_evaluation_ns
    );
    println!(
        "{label}_phase_workspace_construction_ns={}",
        diagnostics.workspace_construction_ns
    );
    println!(
        "{label}_phase_iterative_solver_execution_ns={}",
        diagnostics.iterative_solver_execution_ns
    );
    println!(
        "{label}_phase_exact_read_finalization_ns={}",
        diagnostics.exact_read_finalization_ns
    );
    println!(
        "{label}_phase_exact_edge_replacement_ns={}",
        diagnostics.exact_edge_replacement_ns
    );
    println!(
        "{label}_phase_generation_reference_validation_ns={}",
        diagnostics.generation_reference_validation_ns
    );
    println!(
        "{label}_phase_spill_effect_commit_ns={}",
        diagnostics.spill_effect_commit_ns
    );
}

fn print_v2_exclusive_attribution(
    label: &str,
    attribution: &formualizer_eval::engine::EngineV2ExclusiveAttribution,
) {
    let print_formula =
        |name: &str, formula: &formualizer_eval::engine::EngineV2FormulaAttribution| {
            println!(
                "{label}_{name}=invocations:{} read_sets:{} logical:{} physical:{} formula_ns:{} interpreter_ns:{} observation_ns:{} range_materialization_ns:{} exact_read_canonicalization_ns:{} read_entries_p50:{} read_entries_p95:{} read_entries_max:{} finalization_p50_ns:{} finalization_p95_ns:{} finalization_max_ns:{}",
                formula.invocations,
                formula.exact_read_sets_produced,
                formula.logical_range_positions,
                formula.physical_cells_fetched,
                formula.formula_execution_ns,
                formula.interpreter_function_execution_ns,
                formula.observation_recording_ns,
                formula.range_read_materialization_ns,
                formula.exact_read_canonicalization_ns,
                formula.finalization_read_entries_p50,
                formula.finalization_read_entries_p95,
                formula.finalization_read_entries_max,
                formula.finalization_time_p50_ns,
                formula.finalization_time_p95_ns,
                formula.finalization_time_max_ns
            );
        };
    let formulas = [
        (
            "attribution_outside_workspace",
            &attribution.outside_workspace,
        ),
        (
            "attribution_retained_dirty_upstream",
            &attribution.retained_dirty_upstream,
        ),
        ("attribution_exact_scc", &attribution.exact_scc),
        ("attribution_downstream", &attribution.downstream),
    ];
    for (name, formula) in formulas {
        print_formula(name, formula);
        println!(
            "{label}_{name}_edge_distribution=scalar_events_p50:{} scalar_events_p95:{} scalar_events_max:{} unique_coordinates_p50:{} unique_coordinates_p95:{} unique_coordinates_max:{} duplicate_events_p50:{} duplicate_events_p95:{} duplicate_events_max:{} extraction_p50_ns:{} extraction_p95_ns:{} extraction_max_ns:{} unique_edges_p50:{} unique_edges_p95:{} unique_edges_max:{}",
            formula.edge_scalar_events_p50,
            formula.edge_scalar_events_p95,
            formula.edge_scalar_events_max,
            formula.edge_owner_lookups_p50,
            formula.edge_owner_lookups_p95,
            formula.edge_owner_lookups_max,
            formula.edge_duplicate_events_p50,
            formula.edge_duplicate_events_p95,
            formula.edge_duplicate_events_max,
            formula.edge_extraction_time_p50_ns,
            formula.edge_extraction_time_p95_ns,
            formula.edge_extraction_time_max_ns,
            formula.edge_unique_edges_p50,
            formula.edge_unique_edges_p95,
            formula.edge_unique_edges_max
        );
        let operation_names = [
            "scalar_cell",
            "range",
            "selected_reference",
            "reference_generation",
            "name_symbol",
            "table",
            "provider",
            "semantic_effect",
        ];
        for (index, operation_name) in operation_names.into_iter().enumerate() {
            println!(
                "{label}_{name}_{operation_name}=raw_events:{} unique_entries:{} duplicate_lower_bound:{} sampled_elapsed_ns:{}",
                formula.recorder.raw_events[index],
                formula.recorder.unique_entries[index],
                formula.recorder.raw_events[index]
                    .saturating_sub(formula.recorder.unique_entries[index]),
                formula.recorder.sampled_elapsed_ns[index]
            );
        }
        println!(
            "{label}_{name}_formula_edge=raw_events:{} unique_entries:{} duplicate_lower_bound:{}",
            formula.recorder.formula_edge_raw_events,
            formula.recorder.formula_edge_unique_entries,
            formula
                .recorder
                .formula_edge_raw_events
                .saturating_sub(formula.recorder.formula_edge_unique_entries)
        );
        let effect_names = [
            "recalc_epoch",
            "clock",
            "random",
            "dynamic_selector",
            "dynamic_target",
            "spill_shape",
            "table_shape",
            "external_provider",
            "structural_generation",
            "date_system",
            "placement_context",
        ];
        for (index, effect_name) in effect_names.into_iter().enumerate() {
            println!(
                "{label}_{name}_effect_{effect_name}=raw_events:{} unique_entries:{} duplicate_lower_bound:{}",
                formula.recorder.effect_raw_events[index],
                formula.recorder.effect_unique_entries[index],
                formula.recorder.effect_raw_events[index]
                    .saturating_sub(formula.recorder.effect_unique_entries[index])
            );
        }
        println!(
            "{label}_{name}_finalization_ns=recorder_extraction:{} cloning_copying:{} sorting:{} deduplication:{} range_canonicalization:{} formula_edge_extraction:{} reference_generation_canonicalization:{} selected_reference_handling:{} spill_shape_metadata:{} semantic_effect_metadata:{} summary_construction:{} other:{}",
            formula.finalization.recorder_extraction_ns,
            formula.finalization.cloning_copying_ns,
            formula.finalization.sorting_ns,
            formula.finalization.deduplication_ns,
            formula.finalization.range_canonicalization_ns,
            formula.finalization.formula_edge_extraction_ns,
            formula
                .finalization
                .reference_generation_canonicalization_ns,
            formula.finalization.selected_reference_handling_ns,
            formula.finalization.spill_shape_metadata_ns,
            formula.finalization.semantic_effect_metadata_ns,
            formula.finalization.summary_construction_ns,
            formula.finalization.other_ns
        );
        println!(
            "{label}_{name}_finalization_counts=raw_before:{} unique_after:{} duplicates_removed:{} elements_copied:{}",
            formula.finalization.raw_entries_before,
            formula.finalization.unique_entries_after,
            formula.finalization.duplicate_entries_removed,
            formula.finalization.elements_copied
        );
        let edge = &formula.finalization.formula_edge;
        println!(
            "{label}_{name}_edge_counts=scalar_events:{} event_coordinates:{} unique_coordinates:{} range_observations:{} range_expanded_coordinates:{} sheet_lookups:{}/{} owner_lookups:{} owner_hits:{} owner_misses:{} formula_membership_lookups:{} formula_resolutions:{} non_formula_resolutions:{} raw_edge_candidates:{} edge_insert_attempts:{} duplicate_edge_candidates:{} unique_edges:{} exact_cell_insert_attempts:{} exact_cells:{} name_resolutions:{}/{} table_resolutions:{}/{} generation_revision_lookups:{} owner_index_builds:{} owner_index_entries:{}",
            edge.scalar_events,
            edge.scalar_event_coordinates_inspected,
            edge.scalar_unique_coordinates_inspected,
            edge.range_observations_inspected,
            edge.range_coordinates_expanded,
            edge.sheet_lookups_succeeded,
            edge.sheet_lookups_attempted,
            edge.dependency_owner_lookups_attempted,
            edge.dependency_owner_lookup_hits,
            edge.dependency_owner_lookup_misses,
            edge.formula_membership_lookups,
            edge.formula_vertex_resolutions,
            edge.non_formula_vertex_resolutions,
            edge.raw_formula_edge_candidates,
            edge.formula_edge_insert_attempts,
            edge.duplicate_formula_edge_candidates,
            edge.unique_formula_edges,
            edge.exact_cell_insert_attempts,
            edge.exact_cells_produced,
            edge.name_resolution_hits,
            edge.name_resolution_attempts,
            edge.table_resolution_hits,
            edge.table_resolution_attempts,
            edge.generation_revision_lookups,
            edge.formula_owner_index_builds,
            edge.formula_owner_index_entries
        );
        println!(
            "{label}_{name}_edge_times_ns=owner_index_build:{} scalar_event_scan:{} scalar_exact_cell_edge:{} name_resolution:{} table_resolution:{} other:{} total:{}",
            edge.formula_owner_index_build_ns,
            edge.scalar_event_scan_ns,
            edge.scalar_exact_cell_edge_ns,
            edge.name_resolution_ns,
            edge.table_resolution_ns,
            edge.other_ns,
            formula.finalization.formula_edge_extraction_ns
        );
        println!(
            "{label}_{name}_finalization_top_read_sets={:?}",
            formula.finalization_top_read_sets
        );
    }
    let owner = &attribution.owner_reuse;
    let mut sizes = owner
        .read_sets
        .iter()
        .map(|read_set| read_set.size)
        .collect::<Vec<_>>();
    sizes.sort_unstable();
    let percentile = |percent: usize| {
        sizes
            .get(sizes.len().saturating_sub(1).saturating_mul(percent) / 100)
            .copied()
            .unwrap_or_default()
    };
    let mut top_read_sets = owner.read_sets.clone();
    top_read_sets.sort_unstable_by(|left, right| {
        right
            .size
            .cmp(&left.size)
            .then(right.misses.cmp(&left.misses))
            .then(left.vertex.cmp(&right.vertex))
    });
    top_read_sets.truncate(20);
    println!(
        "{label}_owner_global=probes:{} unique_coordinates:{} repeated_coordinates:{} repeated_positive_probes:{} repeated_negative_probes:{} unique_positive_coordinates:{} unique_negative_coordinates:{} read_sets:{} size_p50:{} size_p95:{} size_max:{}",
        owner.probes,
        owner.unique_coordinates,
        owner.repeated_coordinates,
        owner.repeated_positive_probes,
        owner.repeated_negative_probes,
        owner.unique_positive_coordinates,
        owner.unique_negative_coordinates,
        owner.read_sets.len(),
        percentile(50),
        percentile(95),
        sizes.last().copied().unwrap_or_default()
    );
    println!("{label}_owner_per_sheet={:?}", owner.per_sheet);
    println!("{label}_owner_top_read_sets={top_read_sets:?}");
    println!(
        "{label}_attribution_phases_ns=retained_state_scan:{} demand_scheduling:{} retained_plan_validation:{} contract_validation:{} adjacency_replacement:{} cleanup:{} exclusive_children:{} explicit_residual:{} kernel_elapsed:{}",
        attribution.retained_state_scan_ns,
        attribution.demand_scheduling_ns,
        attribution.retained_plan_validation_ns,
        attribution.contract_validation_ns,
        attribution.adjacency_replacement_ns,
        attribution.cleanup_ns,
        attribution.exclusive_children_ns,
        attribution.explicit_residual_ns,
        attribution.kernel_elapsed_ns
    );
}

fn print_v2_workspace_profiles(
    label: &str,
    engine: &Engine<TestWorkbook>,
    diagnostics: &formualizer_eval::engine::EngineV2Diagnostics,
) {
    let mut profiles = BTreeMap::<u64, (u128, usize, usize)>::new();
    for profile in engine.last_scc_pass_profile() {
        let entry = profiles.entry(profile.stable_id).or_default();
        entry.0 = entry.0.saturating_add(profile.elapsed_ns);
        entry.1 = entry.1.max(profile.evaluated_members);
        entry.2 = entry.2.max(profile.iteration);
    }
    let mut top = profiles
        .into_iter()
        .map(|(stable_id, (elapsed_ns, member_count, pass_count))| {
            (stable_id, elapsed_ns, member_count, pass_count)
        })
        .collect::<Vec<_>>();
    top.sort_unstable_by(|left, right| {
        right
            .1
            .cmp(&left.1)
            .then(right.2.cmp(&left.2))
            .then(left.0.cmp(&right.0))
    });
    top.truncate(10);
    println!(
        "{label}_workspace_profile_records={}",
        engine.last_scc_pass_profile().len()
    );
    println!("{label}_top_workspaces_by_elapsed_ns={top:?}");
    let cycle = engine.last_cycle_telemetry();
    println!("{label}_iterated_workspaces={}", cycle.iterated_sccs);
    println!(
        "{label}_workspace_passes_total={}",
        cycle.settle_passes_total
    );
    println!(
        "{label}_workspace_max_passes={}",
        cycle.max_passes_single_scc
    );
    println!("{label}_workspace_elapsed_ms={}", cycle.elapsed_ms);
    println!(
        "{label}_workspace_formulas_evaluated={}",
        diagnostics.formulas_evaluated_inside_workspaces
    );
    println!(
        "{label}_outside_workspace_formulas_evaluated={}",
        diagnostics.formulas_evaluated_outside_workspaces
    );
    let workspace_plan_summary = engine
        .v2_workspace_diagnostics_for_test()
        .into_iter()
        .map(|workspace| {
            format!(
                "{}:candidate={}/valid={}/reason={:?}/dirty={}/clean={}/dirty_upstream={}/clean_upstream={}/scc={}/upstream={}/downstream={}/unrelated={}/missing_reads={}/missing_cert={}/topology_sensitive={}/generation_invalid={}/cached_reuse={}",
                workspace.stable_id,
                workspace.retained_plan_candidate,
                workspace.retained_plan_valid,
                workspace.retained_plan_rejection_reason,
                workspace.stage1_effective_dirty_members,
                workspace.stage1_clean_members,
                workspace.dirty_upstream_members,
                workspace.clean_upstream_members,
                workspace.exact_scc_members,
                workspace.upstream_members,
                workspace.downstream_members,
                workspace.unrelated_conservative_members,
                workspace.exact_read_state_missing,
                workspace.contract_certificate_missing,
                workspace.topology_sensitive_members,
                workspace.generation_revision_invalid_members,
                workspace.cached_value_reusable_members,
            )
        })
        .collect::<Vec<_>>();
    println!("{label}_workspace_retained_plan_classification={workspace_plan_summary:?}");
}

fn build_dependency_maps(
    engine: &Engine<TestWorkbook>,
) -> (
    BTreeMap<String, BTreeSet<String>>,
    BTreeMap<String, BTreeSet<String>>,
    BTreeMap<String, BTreeSet<String>>,
    BTreeMap<String, BTreeSet<String>>,
) {
    let mut exact_forward = BTreeMap::<String, BTreeSet<String>>::new();
    let mut exact_reverse = BTreeMap::<String, BTreeSet<String>>::new();
    for (reader, cells, formulas) in engine.v2_exact_read_records_for_test() {
        for dependency in cells.into_iter().chain(formulas) {
            exact_forward
                .entry(reader.clone())
                .or_default()
                .insert(dependency.clone());
            exact_reverse
                .entry(dependency)
                .or_default()
                .insert(reader.clone());
        }
    }

    let mut static_forward = BTreeMap::<String, BTreeSet<String>>::new();
    let mut static_reverse = BTreeMap::<String, BTreeSet<String>>::new();
    for (reader, dependencies) in engine.v2_static_dependency_records_for_test() {
        for dependency in dependencies {
            static_forward
                .entry(reader.clone())
                .or_default()
                .insert(dependency.clone());
            static_reverse
                .entry(dependency)
                .or_default()
                .insert(reader.clone());
        }
    }
    (exact_forward, exact_reverse, static_forward, static_reverse)
}

fn find_dependency_path(
    reverse: &BTreeMap<String, BTreeSet<String>>,
    source: &str,
    target: &str,
) -> Option<Vec<String>> {
    let mut previous = BTreeMap::<String, Option<String>>::new();
    let mut queue = VecDeque::new();
    previous.insert(source.to_string(), None);
    queue.push_back(source.to_string());
    while let Some(current) = queue.pop_front() {
        if current == target {
            let mut path = Vec::new();
            let mut cursor = current;
            path.push(cursor.clone());
            while let Some(Some(parent)) = previous.get(&cursor) {
                cursor = parent.clone();
                path.push(cursor.clone());
            }
            path.reverse();
            return Some(path);
        }
        for reader in reverse.get(&current).into_iter().flatten() {
            if !previous.contains_key(reader) {
                previous.insert(reader.clone(), Some(current.clone()));
                queue.push_back(reader.clone());
            }
        }
    }
    None
}

fn dependency_closure(
    forward: &BTreeMap<String, BTreeSet<String>>,
    seeds: &BTreeSet<String>,
    members: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut closure = BTreeSet::new();
    let mut queue = VecDeque::new();
    for seed in seeds {
        if members.contains(seed) {
            closure.insert(seed.clone());
            queue.push_back(seed.clone());
        }
    }
    while let Some(reader) = queue.pop_front() {
        for dependency in forward.get(&reader).into_iter().flatten() {
            if members.contains(dependency) && closure.insert(dependency.clone()) {
                queue.push_back(dependency.clone());
            }
        }
    }
    closure
}

fn merge_dependency_maps(
    left: &BTreeMap<String, BTreeSet<String>>,
    right: &BTreeMap<String, BTreeSet<String>>,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut merged = left.clone();
    for (reader, dependencies) in right {
        merged
            .entry(reader.clone())
            .or_default()
            .extend(dependencies.iter().cloned());
    }
    merged
}

fn print_v2_workspace_structure(engine: &Engine<TestWorkbook>) {
    let workspaces = engine.v2_workspace_diagnostics_for_test();
    let (exact_forward, exact_reverse, static_forward, static_reverse) =
        build_dependency_maps(engine);
    let combined_forward = merge_dependency_maps(&exact_forward, &static_forward);
    let combined_reverse = merge_dependency_maps(&exact_reverse, &static_reverse);
    let source = "Inputs!F6";
    let mut member_counts = BTreeMap::<usize, usize>::new();
    for workspace in &workspaces {
        *member_counts.entry(workspace.members.len()).or_default() += 1;
    }
    let dirty_telemetry = engine.last_scc_dirty_telemetry();
    println!("light_f6_500_v2_workspace_count={}", workspaces.len());
    println!("light_f6_500_v2_workspace_member_count_distribution={member_counts:?}");
    println!(
        "light_f6_500_v2_dirty_root_sources={:?}",
        dirty_telemetry.dirty_root_sources
    );
    println!(
        "light_f6_500_v2_dirty_root_samples={:?}",
        dirty_telemetry.dirty_root_samples
    );
    println!(
        "light_f6_500_v2_dirty_provenance_counts={:?}",
        dirty_telemetry.dirty_provenance_counts
    );
    println!(
        "light_f6_500_v2_dirty_provenance_samples={:?}",
        dirty_telemetry.dirty_provenance_samples
    );
    println!(
        "light_f6_500_v2_user_edit_root_samples={:?}",
        dirty_telemetry.user_edit_root_samples
    );
    println!(
        "light_f6_500_v2_exact_f6_readers={:?}",
        exact_forward
            .iter()
            .filter(|(_, dependencies)| dependencies.contains(source))
            .map(|(reader, _)| reader)
            .collect::<Vec<_>>()
    );
    println!(
        "light_f6_500_v2_static_f6_readers={:?}",
        static_forward
            .iter()
            .filter(|(_, dependencies)| dependencies.contains(source))
            .map(|(reader, _)| reader)
            .collect::<Vec<_>>()
    );
    let mut reports = workspaces;
    reports.sort_unstable_by(|left, right| {
        right
            .elapsed_ns
            .cmp(&left.elapsed_ns)
            .then(left.stable_id.cmp(&right.stable_id))
    });
    for (index, workspace) in reports.iter().enumerate() {
        let members = workspace.members.iter().cloned().collect::<BTreeSet<_>>();
        let cyclic_members = workspace
            .actual_cyclic_components
            .iter()
            .flatten()
            .cloned()
            .collect::<BTreeSet<_>>();
        let non_cyclic_members = members
            .difference(&cyclic_members)
            .cloned()
            .collect::<Vec<_>>();
        let upstream_members = dependency_closure(&combined_forward, &cyclic_members, &members)
            .difference(&cyclic_members)
            .cloned()
            .collect::<BTreeSet<_>>();
        let downstream_members = dependency_closure(&combined_reverse, &cyclic_members, &members)
            .difference(&cyclic_members)
            .cloned()
            .collect::<BTreeSet<_>>();
        let static_path = members
            .iter()
            .find_map(|member| find_dependency_path(&static_reverse, source, member));
        let exact_path = members
            .iter()
            .find_map(|member| find_dependency_path(&exact_reverse, source, member));
        let inclusion_reasons = non_cyclic_members
            .iter()
            .map(|member| {
                let reason = match (
                    upstream_members.contains(member),
                    downstream_members.contains(member),
                ) {
                    (true, true) => "conservative-static-SCC-closure;upstream-and-downstream",
                    (true, false) => "conservative-static-SCC-closure;upstream-prerequisite",
                    (false, true) => "conservative-static-SCC-closure;downstream-dependent",
                    (false, false) => "conservative-static-SCC-closure;outside-live-feedback",
                };
                (member.clone(), reason)
            })
            .collect::<Vec<_>>();
        println!(
            "light_f6_500_v2_workspace_{index}_stable_id={}",
            workspace.stable_id
        );
        println!(
            "light_f6_500_v2_workspace_{index}_total_members={}",
            workspace.members.len()
        );
        println!(
            "light_f6_500_v2_workspace_{index}_members={:?}",
            workspace.members
        );
        println!(
            "light_f6_500_v2_workspace_{index}_actual_scc_sizes={:?}",
            workspace
                .actual_cyclic_components
                .iter()
                .map(Vec::len)
                .collect::<Vec<_>>()
        );
        println!(
            "light_f6_500_v2_workspace_{index}_actual_cyclic_scc_members={:?}",
            workspace.actual_cyclic_components
        );
        println!(
            "light_f6_500_v2_workspace_{index}_non_cyclic_members={:?}",
            non_cyclic_members
        );
        println!(
            "light_f6_500_v2_workspace_{index}_non_cyclic_member_count={}",
            non_cyclic_members.len()
        );
        println!(
            "light_f6_500_v2_workspace_{index}_dirty_members_at_entry={:?}",
            workspace.dirty_members
        );
        println!(
            "light_f6_500_v2_workspace_{index}_dirty_member_count={}",
            workspace.dirty_members.len()
        );
        println!(
            "light_f6_500_v2_workspace_{index}_formulas_evaluated_per_pass={:?}",
            workspace.pass_formula_evaluations
        );
        println!(
            "light_f6_500_v2_workspace_{index}_passes={}",
            workspace.pass_formula_evaluations.len()
        );
        println!(
            "light_f6_500_v2_workspace_{index}_elapsed_ns={}",
            workspace.elapsed_ns
        );
        println!(
            "light_f6_500_v2_workspace_{index}_non_cyclic_inclusion_reasons={:?}",
            inclusion_reasons
        );
        println!(
            "light_f6_500_v2_workspace_{index}_upstream_prerequisite_count={}",
            upstream_members.len()
        );
        println!(
            "light_f6_500_v2_workspace_{index}_downstream_dependent_count={}",
            downstream_members.len()
        );
        println!("light_f6_500_v2_workspace_{index}_source_f6_static_path={static_path:?}");
        println!("light_f6_500_v2_workspace_{index}_source_f6_exact_path={exact_path:?}");
        println!(
            "light_f6_500_v2_workspace_{index}_graph_basis=combination(conservative-static-or-virtual-schedule,exact-runtime-feedback-verdict)"
        );
        if let Some(dirty) = dirty_telemetry
            .per_scc
            .iter()
            .find(|record| record.stable_id == workspace.stable_id)
        {
            println!(
                "light_f6_500_v2_workspace_{index}_dirty_reason={} natural_dirty={} volatile_redirty={} iterative_redirty={} static_members={} static_cycles={} live_cycles={}",
                dirty.reason,
                dirty.naturally_dirty_member_count,
                dirty.volatile_redirty_member_count,
                dirty.iterative_redirty_member_count,
                dirty.static_member_count,
                dirty.static_cycle_count,
                dirty.live_cycle_count
            );
        }
    }

    for workspace in reports.iter().take(3) {
        let members = workspace.members.iter().cloned().collect::<BTreeSet<_>>();
        let cyclic_members = workspace
            .actual_cyclic_components
            .iter()
            .flatten()
            .cloned()
            .collect::<BTreeSet<_>>();
        let upstream = dependency_closure(&combined_forward, &cyclic_members, &members)
            .difference(&cyclic_members)
            .cloned()
            .collect::<Vec<_>>();
        let downstream = dependency_closure(&combined_reverse, &cyclic_members, &members)
            .difference(&cyclic_members)
            .cloned()
            .collect::<Vec<_>>();
        let pass_two_changes = engine
            .last_scc_member_profile()
            .iter()
            .filter(|profile| {
                profile.stable_id == workspace.stable_id
                    && profile.iteration == 2
                    && profile.changed
            })
            .map(|profile| {
                (
                    profile.address.clone(),
                    format!("{:?}", profile.before_value),
                    format!("{:?}", profile.after_value),
                )
            })
            .collect::<Vec<_>>();
        let reevaluated_stable = engine
            .last_scc_member_profile()
            .iter()
            .filter(|profile| {
                profile.stable_id == workspace.stable_id
                    && profile.iteration >= 2
                    && !profile.changed
            })
            .map(|profile| (profile.address.clone(), profile.iteration))
            .collect::<BTreeSet<_>>();
        println!(
            "light_f6_500_v2_largest_workspace_total_members={} actual_cyclic_members={} non_feedback_members={} dirty_members_at_entry={}",
            workspace.members.len(),
            cyclic_members.len(),
            members.difference(&cyclic_members).count(),
            workspace.dirty_members.len()
        );
        println!(
            "light_f6_500_v2_largest_workspace_changed_pass1_to_pass2_count={}",
            pass_two_changes.len()
        );
        println!(
            "light_f6_500_v2_largest_workspace_reevaluated_stable_count={}",
            reevaluated_stable.len()
        );
        println!(
            "light_f6_500_v2_largest_workspace_stable_id={}",
            workspace.stable_id
        );
        println!(
            "light_f6_500_v2_largest_workspace_scc_members={:?}",
            workspace.actual_cyclic_components
        );
        println!("light_f6_500_v2_largest_workspace_upstream_prerequisites={upstream:?}");
        println!("light_f6_500_v2_largest_workspace_downstream_dependents={downstream:?}");
        println!(
            "light_f6_500_v2_largest_workspace_non_feedback_members={:?}",
            members.difference(&cyclic_members).collect::<Vec<_>>()
        );
        println!("light_f6_500_v2_largest_workspace_pass1_to_pass2_changes={pass_two_changes:?}");
        println!(
            "light_f6_500_v2_largest_workspace_reevaluated_stable_members={reevaluated_stable:?}"
        );
    }
}

#[cfg(windows)]
fn process_working_set_bytes() -> Option<u64> {
    use std::ffi::c_void;
    use std::mem::size_of;

    #[repr(C)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
    }

    #[link(name = "psapi")]
    unsafe extern "system" {
        fn GetProcessMemoryInfo(
            process: *mut c_void,
            counters: *mut ProcessMemoryCounters,
            size: u32,
        ) -> i32;
    }

    let mut counters = ProcessMemoryCounters {
        cb: size_of::<ProcessMemoryCounters>() as u32,
        page_fault_count: 0,
        peak_working_set_size: 0,
        working_set_size: 0,
        quota_peak_paged_pool_usage: 0,
        quota_paged_pool_usage: 0,
        quota_peak_non_paged_pool_usage: 0,
        quota_non_paged_pool_usage: 0,
        pagefile_usage: 0,
        peak_pagefile_usage: 0,
    };
    let success = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut counters,
            size_of::<ProcessMemoryCounters>() as u32,
        )
    };
    (success != 0).then_some(counters.working_set_size as u64)
}

#[cfg(not(windows))]
fn process_working_set_bytes() -> Option<u64> {
    None
}

#[test]
#[ignore = "requires the local real Heavy workbook"]
fn heavy_production_v1_v2_witness_and_durability() {
    let path = workbook_path("FORMUALIZER_HEAVY_WORKBOOK", DEFAULT_HEAVY);
    assert!(path.is_file(), "Heavy workbook missing: {}", path.display());
    let mut v2 = load_engine(&path, true);
    let basic_admission = v2.v2_basic_contract_diagnostics_for_test();
    assert!(
        basic_admission.eligible,
        "Heavy basic V2 admission rejected: {:?}",
        basic_admission.rejection_counts
    );
    let before_diagnostics = v2.v2_diagnostics_for_test();
    let admission = v2.v2_contract_diagnostics_for_test();
    let after_diagnostics = v2.v2_diagnostics_for_test();
    assert_eq!(before_diagnostics, after_diagnostics);
    assert!(
        admission.eligible,
        "Heavy V2 admission rejected: {:?}",
        admission.rejection_counts
    );
    let mut v1 = load_engine(&path, false);
    for engine in [&mut v1, &mut v2] {
        engine
            .set_cell_value("Inputs", 7, 6, LiteralValue::Number(300.0))
            .expect("set Inputs!F7");
        engine.evaluate_all().expect("production full evaluation");
    }

    for (sheet, row, col) in [
        ("CashFlow Engine", 11, 10),
        ("CashFlow Engine", 65, 9),
        ("CashFlow Engine", 65, 11),
        ("CashFlow Inputs", 23, 10),
        ("Outputs", 53, 4),
    ] {
        assert_eq!(
            v2.get_cell_value(sheet, row, col),
            v1.get_cell_value(sheet, row, col),
            "V1/V2 mismatch at {sheet}!R{row}C{col}"
        );
    }
    assert_eq!(
        v2.get_cell_value("CashFlow Engine", 11, 10),
        Some(LiteralValue::Text("SC".to_string()))
    );
    assert_eq!(
        v2.get_cell_value("CashFlow Engine", 65, 9),
        Some(LiteralValue::Text("No".to_string()))
    );
    assert!(is_blank(&v2.get_cell_value("CashFlow Engine", 65, 11)));
    let j23 = v2
        .get_cell_value("CashFlow Inputs", 23, 10)
        .expect("CashFlow Inputs!J23");
    let j23 = match j23 {
        LiteralValue::Number(serial) => LiteralValue::try_from_serial_number_for(
            formualizer_common::DateSystem::Excel1900,
            serial,
        )
        .expect("J23 serial date"),
        value => value,
    };
    assert_eq!(
        j23,
        LiteralValue::Date(chrono::NaiveDate::from_ymd_opt(2025, 12, 1).unwrap())
    );

    let diagnostics = v2.v2_diagnostics_for_test();
    assert!(diagnostics.enabled);
    assert_eq!(
        diagnostics.fallback_activations, 0,
        "Heavy used V1 fallback"
    );
    assert!(diagnostics.formula_evaluations > 0, "Heavy did not use V2");
    assert!(diagnostics.schedule_ns <= diagnostics.elapsed_ns);
    assert!(diagnostics.formula_ns <= diagnostics.elapsed_ns);
    assert!(diagnostics.workspace_ns <= diagnostics.elapsed_ns);
    assert!(diagnostics.cleanup_ns <= diagnostics.elapsed_ns);

    let selected = v2.v2_selected_targets_for_test("CashFlow Engine", 11, 10);
    assert!(selected.contains(&"CashFlow Inputs!J9".to_string()));
    let edges = v2.v2_formula_edges_for_test();
    assert!(!edges.contains(&(
        "CashFlow Engine!J11".to_string(),
        "CashFlow Inputs!J23".to_string(),
    )));

    let state_size = (diagnostics.current_read_sets, diagnostics.reverse_buckets);
    let contract_scans = diagnostics.contract_scans;
    for _ in 0..10 {
        v2.evaluate_all().expect("Heavy clean no-op");
        let no_op = v2.v2_diagnostics_for_test();
        assert_eq!(no_op.formula_evaluations, 0);
        assert_eq!(no_op.queue_steps, 0);
        assert_eq!(no_op.contract_scans, contract_scans);
        assert_eq!((no_op.current_read_sets, no_op.reverse_buckets), state_size);
    }
}

#[test]
#[ignore = "requires the local real Light workbook"]
fn light_production_v1_v2_comparison() {
    let path = workbook_path("FORMUALIZER_LIGHT_WORKBOOK", DEFAULT_LIGHT);
    assert!(path.is_file(), "Light workbook missing: {}", path.display());
    let mut v2 = load_engine(&path, true);
    let basic_admission = v2.v2_basic_contract_diagnostics_for_test();
    assert!(
        basic_admission.eligible,
        "Light basic V2 admission rejected: {:?}",
        basic_admission.rejection_counts
    );
    let before_diagnostics = v2.v2_diagnostics_for_test();
    let admission = v2.v2_contract_diagnostics_for_test();
    let after_diagnostics = v2.v2_diagnostics_for_test();
    assert_eq!(before_diagnostics, after_diagnostics);
    assert!(
        admission.eligible,
        "Light V2 admission rejected: {:?}",
        admission.rejection_counts
    );
    let mut v1 = load_engine(&path, false);
    for engine in [&mut v1, &mut v2] {
        engine
            .set_cell_value("Inputs", 6, 6, LiteralValue::Number(300.0))
            .expect("set Inputs!F6");
        engine.evaluate_all().expect("production Light evaluation");
    }
    assert_eq!(
        v2.get_cell_value("Outputs", 53, 4),
        v1.get_cell_value("Outputs", 53, 4)
    );
    let diagnostics = v2.v2_diagnostics_for_test();
    assert_eq!(
        diagnostics.fallback_activations, 0,
        "Light used V1 fallback"
    );
    assert!(diagnostics.formula_evaluations > 0, "Light did not use V2");
}
