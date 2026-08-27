use crate::args::ArgSchema;
use crate::engine::named_range::{NameScope, NamedDefinition};
use crate::engine::{
    CycleConfig, CycleDetection, CyclePolicy, DeterministicMode, Engine, EvalConfig,
    EvaluationTarget,
};
use crate::function::{FnCaps, Function};
use crate::function_contract::FunctionSemanticContract;
use crate::reference::{CellRef, Coord, RangeRef};
use crate::test_workbook::TestWorkbook;
use crate::timezone::TimeZoneSpec;
use crate::traits::{ArgumentHandle, FunctionContext};
use formualizer_common::{ExcelError, ExcelErrorKind, LiteralValue};
use formualizer_parse::parser::parse;
use std::sync::Arc;

fn make_engine() -> Engine<TestWorkbook> {
    let mut config = EvalConfig::default().with_cycle(CycleConfig::iterate_excel_defaults());
    config.deterministic_mode = DeterministicMode::Enabled {
        timestamp_utc: chrono::DateTime::UNIX_EPOCH,
        timezone: TimeZoneSpec::Utc,
    };
    let mut engine = Engine::new(TestWorkbook::new(), config);
    engine.enable_v2_for_test();
    engine
}

fn runtime_config() -> EvalConfig {
    let mut config = EvalConfig::default().with_cycle(CycleConfig::iterate_excel_defaults());
    config.deterministic_mode = DeterministicMode::Enabled {
        timestamp_utc: chrono::DateTime::UNIX_EPOCH,
        timezone: TimeZoneSpec::Utc,
    };
    config
}

fn engine_with_cycle(cycle: CycleConfig, v2: bool, dynamic_topo: bool) -> Engine<TestWorkbook> {
    let mut config = EvalConfig::default().with_cycle(cycle);
    config.deterministic_mode = DeterministicMode::Enabled {
        timestamp_utc: chrono::DateTime::UNIX_EPOCH,
        timezone: TimeZoneSpec::Utc,
    };
    config.use_dynamic_topo = dynamic_topo;
    let mut engine = Engine::new(TestWorkbook::new(), config);
    if v2 {
        engine.enable_v2_for_test();
    }
    engine
}

#[derive(Debug)]
struct SelfContractedFn;

impl Function for SelfContractedFn {
    fn caps(&self) -> FnCaps {
        FnCaps::PURE | FnCaps::V2_READS_OBSERVED
    }

    fn name(&self) -> &'static str {
        "SELF_CONTRACTED"
    }

    fn semantic_contract(&self, _arity: usize) -> Option<FunctionSemanticContract> {
        Some(FunctionSemanticContract::trusted_builtin_default(None))
    }

    fn arg_schema(&self) -> &'static [ArgSchema] {
        &[]
    }

    fn eval<'a, 'b, 'c>(
        &self,
        _args: &'c [ArgumentHandle<'a, 'b>],
        _ctx: &dyn FunctionContext<'b>,
    ) -> Result<crate::traits::CalcValue<'b>, ExcelError> {
        Ok(crate::traits::CalcValue::Scalar(LiteralValue::Int(9)))
    }
}

fn set_formula(engine: &mut Engine<TestWorkbook>, row: u32, col: u32, source: &str) {
    engine
        .set_cell_formula("Sheet1", row, col, parse(source).expect("parse formula"))
        .expect("set formula");
}

fn set_value(engine: &mut Engine<TestWorkbook>, row: u32, col: u32, value: LiteralValue) {
    engine
        .set_cell_value("Sheet1", row, col, value)
        .expect("set value");
}

fn cell(engine: &Engine<TestWorkbook>, row: u32, col: u32) -> CellRef {
    engine.graph.make_cell_ref("Sheet1", row, col)
}

fn has_edge(engine: &Engine<TestWorkbook>, reader: CellRef, dependency: CellRef) -> bool {
    engine
        .v2_current_formula_edges_for_test()
        .contains(&(reader, dependency))
}

fn target(row: u32, col: u32) -> EvaluationTarget {
    EvaluationTarget::Cell {
        sheet: "Sheet1".to_string(),
        row,
        col,
    }
}

#[test]
fn warm_exact_pruning_skips_unrelated_scalar_edit() {
    let mut engine = make_engine();
    set_value(&mut engine, 1, 1, LiteralValue::Number(1.0));
    set_formula(&mut engine, 1, 2, "=A1+1");
    set_value(&mut engine, 1, 3, LiteralValue::Number(10.0));
    engine
        .evaluate_targets(&[target(1, 2)])
        .expect("initial scalar target");

    set_value(&mut engine, 1, 3, LiteralValue::Number(11.0));
    engine
        .evaluate_targets(&[target(1, 2)])
        .expect("unrelated scalar edit");

    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 2),
        Some(LiteralValue::Number(2.0))
    );
    assert_eq!(engine.last_v2_metrics().formulas_evaluated, 0);
    assert_eq!(engine.last_v2_metrics().effective_workspace_count, 0);
    assert_eq!(engine.last_v2_metrics().pruned_workspace_count, 0);
    assert_eq!(engine.last_v2_metrics().exact_pruning_accepted_count, 1);
}

#[test]
fn warm_exact_pruning_skips_unaffected_workspace_candidate() {
    let mut engine = make_engine();
    set_formula(&mut engine, 1, 1, "=B1+1");
    set_formula(&mut engine, 1, 2, "=A1+1");
    engine
        .evaluate_targets(&[target(1, 1)])
        .expect("initial workspace target");

    set_value(&mut engine, 1, 3, LiteralValue::Number(9.0));
    engine
        .evaluate_targets(&[target(1, 1)])
        .expect("unaffected workspace target");

    assert_eq!(engine.last_v2_metrics().formulas_evaluated, 0);
    assert_eq!(engine.last_v2_metrics().effective_workspace_count, 0);
    assert_eq!(engine.last_v2_metrics().pruned_workspace_count, 1);
    assert_eq!(engine.last_v2_metrics().pruned_dirty_formula_count, 2);
}

#[test]
fn warm_exact_pruning_propagates_multi_hop_formula_edges() {
    let mut engine = make_engine();
    set_value(&mut engine, 1, 1, LiteralValue::Number(1.0));
    set_formula(&mut engine, 1, 2, "=A1+1");
    set_formula(&mut engine, 1, 3, "=B1+1");
    engine
        .evaluate_targets(&[target(1, 3)])
        .expect("initial multi-hop target");

    set_value(&mut engine, 1, 1, LiteralValue::Number(5.0));
    engine
        .evaluate_targets(&[target(1, 3)])
        .expect("changed multi-hop target");

    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 3),
        Some(LiteralValue::Number(7.0))
    );
    assert_eq!(engine.last_v2_metrics().formulas_evaluated, 2);
    assert_eq!(engine.last_v2_metrics().effective_dirty_formula_count, 2);
    assert!(engine.last_v2_metrics().exact_reverse_read_formulas_reached >= 1);
    assert!(engine.last_v2_metrics().exact_formula_edge_formulas_reached >= 1);
}

#[test]
fn duplicate_scalar_reads_preserve_exact_cell_and_edge_event_evidence() {
    let mut engine = make_engine();
    set_formula(&mut engine, 1, 1, "=1");
    set_formula(&mut engine, 1, 2, "=A1+A1");

    engine
        .evaluate_targets(&[target(1, 2)])
        .expect("duplicate scalar reads");

    let dependency = cell(&engine, 1, 1);
    let reads = engine
        .v2_read_set_for_test(cell(&engine, 1, 2))
        .expect("exact read set");
    assert_eq!(reads.cells.len(), 1);
    assert!(reads.contains_cell(&dependency));
    assert_eq!(reads.formula_edges.len(), 1);
    assert_eq!(reads.formula_edge_events, 2);
    assert!(has_edge(&engine, cell(&engine, 1, 2), dependency));
}

#[test]
fn warm_exact_pruning_skips_unused_if_branch_but_tracks_selector_edit() {
    let mut engine = make_engine();
    set_value(&mut engine, 1, 1, LiteralValue::Boolean(true));
    set_formula(&mut engine, 1, 2, "=10");
    set_value(&mut engine, 2, 3, LiteralValue::Number(20.0));
    set_formula(&mut engine, 1, 3, "=C2");
    set_formula(&mut engine, 1, 4, "=IF(A1,B1,C1)");
    engine
        .evaluate_targets(&[target(1, 4)])
        .expect("initial IF target");

    set_value(&mut engine, 2, 3, LiteralValue::Number(30.0));
    engine
        .evaluate_targets(&[target(1, 4)])
        .expect("unused IF branch edit");
    println!(
        "IF pruning metrics after branch source edit: {:?}",
        engine.last_v2_metrics()
    );
    println!(
        "IF formula read set after branch source edit: {:?}",
        engine.v2_read_set_for_test(cell(&engine, 1, 4))
    );
    assert_eq!(engine.last_v2_metrics().formulas_evaluated, 1);
    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 4),
        Some(LiteralValue::Number(10.0))
    );

    set_value(&mut engine, 1, 1, LiteralValue::Boolean(false));
    engine
        .evaluate_targets(&[target(1, 4)])
        .expect("IF selector edit");
    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 4),
        Some(LiteralValue::Number(30.0))
    );
    assert_eq!(engine.last_v2_metrics().formulas_evaluated, 1);
    assert!(has_edge(&engine, cell(&engine, 1, 4), cell(&engine, 1, 3)));
}

#[test]
fn warm_exact_pruning_matches_changed_cell_inside_consumed_range() {
    let mut engine = make_engine();
    set_value(&mut engine, 1, 1, LiteralValue::Number(1.0));
    set_value(&mut engine, 1, 2, LiteralValue::Number(2.0));
    set_formula(&mut engine, 1, 3, "=SUM(A1:B1)");
    engine
        .evaluate_targets(&[target(1, 3)])
        .expect("initial range target");

    set_value(&mut engine, 1, 4, LiteralValue::Number(9.0));
    engine
        .evaluate_targets(&[target(1, 3)])
        .expect("unrelated range edit");
    assert_eq!(engine.last_v2_metrics().formulas_evaluated, 0);

    set_value(&mut engine, 1, 2, LiteralValue::Number(5.0));
    engine
        .evaluate_targets(&[target(1, 3)])
        .expect("consumed range edit");
    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 3),
        Some(LiteralValue::Number(6.0))
    );
    assert_eq!(engine.last_v2_metrics().formulas_evaluated, 1);
}

#[test]
fn cold_exact_pruning_rejects_missing_retained_state() {
    let mut engine = make_engine();
    set_value(&mut engine, 1, 1, LiteralValue::Number(1.0));
    set_formula(&mut engine, 1, 2, "=A1+1");

    engine
        .evaluate_targets(&[target(1, 2)])
        .expect("cold target");

    assert_eq!(engine.last_v2_metrics().exact_pruning_rejected_count, 1);
    assert_eq!(
        engine
            .last_v2_metrics()
            .pruning_rejection_reasons
            .get("no_retained_exact_state"),
        Some(&1)
    );
}

#[test]
fn generation_change_rejects_warm_exact_pruning_fail_closed() {
    let mut engine = make_engine();
    set_value(&mut engine, 1, 1, LiteralValue::Number(1.0));
    set_formula(&mut engine, 1, 2, "=A1+1");
    engine
        .evaluate_targets(&[target(1, 2)])
        .expect("initial generation target");

    engine.graph.bump_symbol_revision();
    engine
        .evaluate_targets(&[target(1, 2)])
        .expect("symbol revision target");

    assert_eq!(engine.last_v2_metrics().exact_pruning_rejected_count, 1);
    assert_eq!(
        engine
            .last_v2_metrics()
            .pruning_rejection_reasons
            .get("no_retained_exact_state"),
        Some(&1)
    );
    assert_eq!(engine.last_v2_metrics().fallback_mode_activations, 0);
}

#[test]
fn inactive_if_branch_is_not_an_exact_runtime_edge() {
    let mut engine = make_engine();
    set_value(&mut engine, 1, 1, LiteralValue::Boolean(true));
    set_formula(&mut engine, 1, 2, "=5");
    set_formula(&mut engine, 2, 4, "=7");
    set_formula(&mut engine, 1, 3, "=IF(A1,B1,D2)");

    engine.evaluate_all().expect("evaluate");

    let reader = cell(&engine, 1, 3);
    assert!(has_edge(&engine, reader, cell(&engine, 1, 2)));
    assert!(!has_edge(&engine, reader, cell(&engine, 2, 4)));
}

#[test]
fn false_static_cycle_is_not_admitted_to_the_runtime_workspace() {
    let mut engine = make_engine();
    set_value(&mut engine, 1, 1, LiteralValue::Boolean(true));
    set_formula(&mut engine, 2, 1, "=IF(A1,555,A3)");
    set_formula(&mut engine, 3, 1, "=IF(A1,A2,999)");

    let result = engine.evaluate_all().expect("evaluate");
    assert_eq!(result.cycle_errors, 0);
    assert_eq!(
        engine.get_cell_value("Sheet1", 2, 1),
        Some(LiteralValue::Number(555.0))
    );
    assert_eq!(
        engine.get_cell_value("Sheet1", 3, 1),
        Some(LiteralValue::Number(555.0))
    );
    assert_eq!(engine.last_v2_metrics().active_cyclic_workspace_members, 0);
}

#[test]
fn index_records_only_the_selected_formula_target() {
    let mut engine = make_engine();
    set_formula(&mut engine, 1, 1, "=10");
    set_formula(&mut engine, 2, 1, "=20");
    set_formula(&mut engine, 3, 1, "=30");
    set_formula(&mut engine, 1, 3, "=INDEX(A1:A3,2)");

    engine.evaluate_all().expect("evaluate");
    let reader = cell(&engine, 1, 3);
    let selected = cell(&engine, 2, 1);
    assert!(has_edge(&engine, reader, selected));
    assert!(!has_edge(&engine, reader, cell(&engine, 1, 1)));
    assert!(!has_edge(&engine, reader, cell(&engine, 3, 1)));
    let reads = engine.v2_read_set_for_test(reader).expect("read set");
    assert_eq!(
        reads.selected_targets.into_iter().collect::<Vec<_>>(),
        vec![selected]
    );
}

#[test]
fn index_dynamic_and_zero_shapes_use_the_generic_reference_shape_host_contract() {
    let mut dynamic = make_engine();
    set_formula(&mut dynamic, 1, 1, "=10");
    set_formula(&mut dynamic, 2, 1, "=20");
    set_value(&mut dynamic, 1, 2, LiteralValue::Int(2));
    set_formula(&mut dynamic, 1, 3, "=INDEX(A1:A2,B1)");
    dynamic.evaluate_all().expect("dynamic selector V2");
    assert_eq!(
        dynamic.get_cell_value("Sheet1", 1, 3),
        Some(LiteralValue::Number(20.0))
    );
    assert!(dynamic.last_v2_metrics().formulas_evaluated > 0);
    assert_eq!(dynamic.last_v2_metrics().fallback_mode_activations, 0);
    assert!(has_edge(
        &dynamic,
        cell(&dynamic, 1, 3),
        cell(&dynamic, 2, 1)
    ));

    let mut zero = make_engine();
    set_formula(&mut zero, 1, 1, "=10");
    set_formula(&mut zero, 2, 1, "=20");
    set_formula(&mut zero, 1, 3, "=INDEX(A1:A2,0)");
    zero.evaluate_all().expect("zero selector V2");
    assert_eq!(
        zero.get_cell_value("Sheet1", 1, 3),
        Some(LiteralValue::Number(10.0))
    );
    assert!(zero.last_v2_metrics().formulas_evaluated > 0);
    assert_eq!(zero.last_v2_metrics().fallback_mode_activations, 0);
}

#[test]
fn stage3a_reuses_runtime_contract_certificate_after_unrelated_value_edit() {
    let mut engine = make_engine();
    set_formula(&mut engine, 1, 1, "=1");
    set_formula(&mut engine, 1, 2, "=A1+1");
    set_value(&mut engine, 1, 3, LiteralValue::Number(10.0));
    engine.evaluate_all().expect("initial contract certificate");

    set_value(&mut engine, 1, 3, LiteralValue::Number(11.0));
    engine
        .evaluate_vertex(
            engine
                .graph
                .get_vertex_for_cell(&cell(&engine, 1, 2))
                .unwrap(),
        )
        .expect("unrelated value edit");

    let metrics = engine.last_v2_metrics();
    assert!(metrics.runtime_contract_validation_candidates > 0);
    assert!(metrics.runtime_contract_validation_cache_hits > 0);
    assert_eq!(metrics.runtime_contract_edges_examined, 0);
    assert_eq!(metrics.runtime_contract_certificates_invalidated, 0);
}

#[test]
fn stage3a_reuse_does_not_suppress_direct_dependency_recalculation() {
    let mut engine = make_engine();
    set_formula(&mut engine, 1, 1, "=C1");
    set_formula(&mut engine, 1, 2, "=A1+1");
    set_value(&mut engine, 1, 3, LiteralValue::Number(1.0));
    engine.evaluate_all().expect("initial direct dependency");

    set_value(&mut engine, 1, 3, LiteralValue::Number(5.0));
    engine
        .evaluate_targets(&[target(1, 2)])
        .expect("direct dependency edit");

    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 2),
        Some(LiteralValue::Number(6.0))
    );
    assert_eq!(engine.last_v2_metrics().formulas_evaluated, 2);
    assert!(
        engine
            .last_v2_metrics()
            .runtime_contract_validation_cache_hits
            > 0
    );
    assert_eq!(
        engine
            .last_v2_metrics()
            .runtime_contract_certificates_invalidated,
        0
    );
}

#[test]
fn stage3a_formula_revision_invalidates_contract_certificate() {
    let mut engine = make_engine();
    set_formula(&mut engine, 1, 4, "=1");
    set_formula(&mut engine, 1, 3, "=D1");
    set_formula(&mut engine, 1, 2, "=C1");
    set_formula(&mut engine, 1, 1, "=B1+1");
    engine.evaluate_all().expect("initial contract certificate");

    set_formula(&mut engine, 1, 3, "=D1+1");
    engine
        .evaluate_targets(&[target(1, 1)])
        .expect("formula revision");

    let metrics = engine.last_v2_metrics();
    assert!(metrics.runtime_contract_validation_candidates > 0);
    assert!(metrics.runtime_contract_validation_cache_misses > 0);
    assert!(metrics.runtime_contract_certificates_invalidated > 0);
    assert!(
        metrics
            .runtime_contract_certificate_invalidation_reasons
            .contains_key("topology_revision")
    );
}

#[test]
fn stage3a_selector_retarget_validates_new_formula_dependency() {
    let mut engine = make_engine();
    set_formula(&mut engine, 1, 5, "=1");
    set_formula(&mut engine, 1, 6, "=2");
    set_formula(&mut engine, 1, 2, "=E1+1");
    set_formula(&mut engine, 1, 3, "=F1+1");
    set_value(&mut engine, 1, 1, LiteralValue::Boolean(true));
    set_formula(&mut engine, 1, 4, "=IF(A1,B1,C1)");
    engine.evaluate_all().expect("initial selector");

    set_value(&mut engine, 1, 1, LiteralValue::Boolean(false));
    engine.evaluate_all().expect("selector retarget");

    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 4),
        Some(LiteralValue::Number(3.0))
    );
    assert!(
        engine
            .last_v2_metrics()
            .runtime_contract_validation_candidates
            > 0
    );
    assert!(
        engine
            .last_v2_metrics()
            .runtime_contract_validation_cache_misses
            > 0
    );
}

#[test]
fn stage3c_admission_and_schedule_share_one_demand_closure() {
    let mut engine = make_engine();
    set_formula(&mut engine, 1, 1, "=1");
    set_formula(&mut engine, 1, 2, "=A1+1");

    engine
        .evaluate_targets(&[target(1, 2)])
        .expect("shared demand closure target");

    assert_eq!(engine.last_v2_metrics().demand_closures_built, 1);
    assert_eq!(engine.last_v2_metrics().demand_closure_reuse_hits, 1);
    assert_eq!(engine.last_v2_metrics().demand_closure_reuse_rejections, 0);
    assert!(engine.last_v2_metrics().demand_reuse_consumption_ns > 0);
    assert!(engine.last_v2_metrics().admission_demand_nodes_visited > 0);
    assert_eq!(engine.last_v2_metrics().schedule_demand_nodes_visited, 0);
    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 2),
        Some(LiteralValue::Number(2.0))
    );
}

#[test]
fn stage3c_revision_change_rejects_request_local_closure() {
    let mut engine = make_engine();
    set_formula(&mut engine, 1, 1, "=1");
    let root = cell(&engine, 1, 1);
    let root_vertex = engine.graph.get_vertex_for_cell(&root).unwrap();

    assert!(engine.v2_contract_safe_for_vertices(&[root_vertex]));
    engine.graph.bump_symbol_revision();
    let roots = [root_vertex];
    crate::engine::v2::V2Host::v2_schedule(&mut engine, Some(&roots), false)
        .expect("safe demand closure rebuild");

    let closure_stats = engine.v2_demand_closure_stats_for_test();
    assert_eq!(closure_stats.reuse_hits, 0);
    assert_eq!(closure_stats.reuse_rejections, 1);
    assert_eq!(
        closure_stats
            .rejection_reasons
            .get("symbol_revision_changed"),
        Some(&1)
    );
    assert_eq!(closure_stats.closures_built, 2);
}

#[test]
fn stage3d_warm_retained_plan_avoids_workspace_discovery() {
    let mut engine = engine_with_cycle(CycleConfig::iterate(20, 0.001), true, false);
    set_value(&mut engine, 1, 4, LiteralValue::Number(1.0));
    set_formula(&mut engine, 1, 3, "=IF(FALSE,A1,D1*10)");
    set_formula(&mut engine, 1, 1, "=(B1+C1)/2");
    set_formula(&mut engine, 1, 2, "=(A1+C1)/2");
    engine
        .evaluate_targets(&[target(1, 1)])
        .expect("initial retained workspace plan");

    set_value(&mut engine, 1, 4, LiteralValue::Number(2.0));
    engine
        .evaluate_targets(&[target(1, 1)])
        .expect("warm retained workspace plan");

    let metrics = engine.last_v2_metrics();
    assert_eq!(metrics.workspace_retained_plan_candidates, 1);
    assert_eq!(metrics.workspace_retained_plan_hits, 1);
    assert_eq!(metrics.workspace_retained_plan_rejections, 0);
    assert_eq!(metrics.retained_plan_runtime_invalidations, 0);
    assert_eq!(metrics.retained_plan_reopens, 0);
    assert!(metrics.discovery_evaluations_avoided > 0);
    assert!(metrics.workspace_upstream_formula_evaluations > 0);
    assert_eq!(metrics.workspaces_using_exact_scc_kernel, 1);
}

#[test]
fn stage3d_runtime_edge_change_reopens_workspace_fail_closed() {
    let mut engine = engine_with_cycle(CycleConfig::iterate(20, 0.001), true, false);
    set_value(&mut engine, 1, 4, LiteralValue::Boolean(false));
    set_formula(&mut engine, 1, 3, "=IF(D1,A1,10)");
    set_formula(&mut engine, 1, 1, "=(B1+C1)/2");
    set_formula(&mut engine, 1, 2, "=(A1+C1)/2");
    engine
        .evaluate_targets(&[target(1, 1)])
        .expect("initial branch workspace plan");

    set_value(&mut engine, 1, 4, LiteralValue::Boolean(true));
    engine
        .evaluate_targets(&[target(1, 1)])
        .expect("runtime edge change must fall back safely");

    let metrics = engine.last_v2_metrics();
    assert_eq!(metrics.workspace_retained_plan_candidates, 1);
    assert_eq!(metrics.workspace_retained_plan_hits, 1);
    assert_eq!(metrics.retained_plan_runtime_invalidations, 1);
    assert_eq!(metrics.retained_plan_reopens, 1);
    assert!(
        metrics
            .retained_plan_runtime_invalidation_reasons
            .contains_key("scc_membership_changed")
    );
    assert_eq!(metrics.workspaces_using_full_conservative_solver, 0);
}

#[test]
fn stage3b_value_change_with_identical_dependencies_skips_graph_mutation() {
    let mut engine = make_engine();
    set_formula(&mut engine, 1, 1, "=C1");
    set_formula(&mut engine, 1, 2, "=A1+1");
    set_value(&mut engine, 1, 3, LiteralValue::Number(1.0));
    engine.evaluate_all().expect("initial dependency graph");

    set_value(&mut engine, 1, 3, LiteralValue::Number(5.0));
    engine
        .evaluate_targets(&[target(1, 2)])
        .expect("value-only dependency change");

    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 2),
        Some(LiteralValue::Number(6.0))
    );
    assert!(engine.last_v2_metrics().exact_edge_sets_compared > 0);
    assert!(engine.last_v2_metrics().exact_identical_edge_sets > 0);
    assert_eq!(engine.last_v2_metrics().exact_edges_removed, 0);
    assert_eq!(engine.last_v2_metrics().exact_edges_inserted, 0);
    assert_eq!(engine.last_v2_metrics().exact_reverse_buckets_mutated, 0);
}

#[test]
fn stage3b_selector_retarget_updates_only_adjacency_delta() {
    let mut engine = make_engine();
    set_value(&mut engine, 1, 1, LiteralValue::Boolean(true));
    set_formula(&mut engine, 1, 2, "=10");
    set_formula(&mut engine, 1, 3, "=20");
    set_formula(&mut engine, 1, 4, "=IF(A1,B1,C1)");
    engine.evaluate_all().expect("initial selector graph");

    set_value(&mut engine, 1, 1, LiteralValue::Boolean(false));
    engine.evaluate_all().expect("selector retarget graph");

    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 4),
        Some(LiteralValue::Number(20.0))
    );
    assert!(engine.last_v2_metrics().exact_changed_edge_sets > 0);
    assert!(engine.last_v2_metrics().exact_edges_removed > 0);
    assert!(engine.last_v2_metrics().exact_edges_inserted > 0);
    assert!(engine.last_v2_metrics().exact_reverse_buckets_mutated > 0);
    assert!(has_edge(&engine, cell(&engine, 1, 4), cell(&engine, 1, 3)));
    assert!(!has_edge(&engine, cell(&engine, 1, 4), cell(&engine, 1, 2)));
}

#[test]
fn stage2_exact_scc_kernel_keeps_static_prerequisite_out_of_iterations() {
    let mut engine = engine_with_cycle(CycleConfig::iterate(20, 0.001), true, false);
    set_formula(&mut engine, 1, 1, "=(B1+C1)/2");
    set_formula(&mut engine, 1, 2, "=(A1+C1)/2");
    set_formula(&mut engine, 1, 3, "=IF(FALSE,A1,10)");

    engine.evaluate_all().expect("stage2 prerequisite cycle");

    assert_eq!(
        engine.last_v2_metrics().workspaces_using_exact_scc_kernel,
        1
    );
    assert_eq!(
        engine
            .last_v2_metrics()
            .workspaces_using_full_conservative_solver,
        0
    );
    assert_eq!(engine.last_v2_metrics().exact_scc_member_count, 2);
    assert_eq!(
        engine.last_v2_metrics().non_feedback_workspace_member_count,
        1
    );
    assert_eq!(
        engine
            .last_v2_metrics()
            .workspace_discovery_formula_evaluations,
        3
    );
    assert_eq!(
        engine
            .last_v2_metrics()
            .workspace_upstream_formula_evaluations,
        1
    );
    assert!(
        engine
            .last_v2_metrics()
            .workspace_exact_scc_formula_evaluations
            > 0
    );
    assert!(
        engine
            .last_v2_metrics()
            .repeated_non_feedback_evaluations_avoided
            > 0
    );
}

#[test]
fn stage2_downstream_member_runs_once_after_scc_convergence() {
    let mut engine = engine_with_cycle(CycleConfig::iterate(20, 0.001), true, false);
    set_value(&mut engine, 1, 5, LiteralValue::Boolean(false));
    set_formula(&mut engine, 1, 1, "=IF(E1,D1,B1)");
    set_formula(&mut engine, 1, 2, "=(A1+10)/2");
    set_formula(&mut engine, 1, 4, "=A1*2");

    engine.evaluate_all().expect("stage2 downstream cycle");

    assert_eq!(
        engine.last_v2_metrics().workspaces_using_exact_scc_kernel,
        1
    );
    assert_eq!(engine.last_v2_metrics().exact_scc_member_count, 2);
    assert_eq!(
        engine.last_v2_metrics().non_feedback_workspace_member_count,
        1
    );
    assert_eq!(
        engine
            .last_v2_metrics()
            .workspace_downstream_formula_evaluations,
        1
    );
    assert_eq!(
        engine.last_v2_metrics().repeated_non_feedback_evaluations,
        0
    );
    assert!(
        engine
            .last_v2_metrics()
            .repeated_non_feedback_evaluations_avoided
            > 0
    );
}

#[test]
fn stage2_exact_scc_kernel_honors_total_iteration_cap() {
    let mut engine = engine_with_cycle(CycleConfig::iterate(3, 0.0), true, false);
    set_formula(&mut engine, 1, 1, "=(B1+1)/2");
    set_formula(&mut engine, 1, 2, "=(A1+1)/2");

    engine.evaluate_all().expect("stage2 capped cycle");

    assert_eq!(
        engine.last_v2_metrics().workspaces_using_exact_scc_kernel,
        1
    );
    assert_eq!(engine.last_v2_metrics().solver_passes, 3);
    assert_eq!(
        engine
            .last_v2_metrics()
            .workspace_discovery_formula_evaluations,
        2
    );
    assert_eq!(
        engine
            .last_v2_metrics()
            .workspace_exact_scc_formula_evaluations,
        4
    );
}

#[test]
fn stage2_does_not_introduce_iterative_kernel_for_acyclic_region() {
    let mut engine = make_engine();
    set_formula(&mut engine, 1, 1, "=1+2");

    engine.evaluate_all().expect("stage2 acyclic region");

    assert_eq!(
        engine.last_v2_metrics().workspaces_using_exact_scc_kernel,
        0
    );
    assert_eq!(
        engine
            .last_v2_metrics()
            .workspace_exact_scc_formula_evaluations,
        0
    );
    assert_eq!(
        engine
            .last_v2_metrics()
            .workspace_downstream_formula_evaluations,
        0
    );
    assert_eq!(engine.last_v2_metrics().solver_passes, 0);
}

#[test]
fn stage2_branch_change_rederives_exact_scc_before_iteration() {
    let mut engine = make_engine();
    set_value(&mut engine, 1, 3, LiteralValue::Boolean(false));
    set_formula(&mut engine, 1, 1, "=IF(C1,B1,1)");
    set_formula(&mut engine, 1, 2, "=A1+1");

    engine.evaluate_all().expect("initial acyclic branch");
    assert_eq!(
        engine.last_v2_metrics().workspaces_using_exact_scc_kernel,
        0
    );

    set_value(&mut engine, 1, 3, LiteralValue::Boolean(true));
    engine.evaluate_all().expect("branch-created cycle");

    assert_eq!(
        engine.last_v2_metrics().workspaces_using_exact_scc_kernel,
        1
    );
    assert_eq!(engine.last_v2_metrics().active_cyclic_workspace_members, 2);
    assert!(
        engine
            .last_v2_metrics()
            .workspace_exact_scc_formula_evaluations
            > 0
    );
}

#[test]
fn targeted_admission_allows_runtime_dynamic_target_expansion() {
    let mut engine = make_engine();
    set_value(&mut engine, 1, 1, LiteralValue::Text("C1".to_string()));
    set_value(&mut engine, 1, 3, LiteralValue::Number(10.0));
    set_value(&mut engine, 1, 4, LiteralValue::Number(20.0));
    set_formula(&mut engine, 1, 2, "=INDIRECT(A1)");

    engine
        .evaluate_targets(&[target(1, 2)])
        .expect("initial target");
    set_value(&mut engine, 1, 5, LiteralValue::Boolean(true));
    set_formula(&mut engine, 1, 1, "=IF(E1,\"D1\",\"C1\")");

    let reader = cell(&engine, 1, 2);
    let expanded_target = cell(&engine, 1, 4);
    let initial_demand =
        engine.v2_demand_vertices_for_test(&[engine.graph.get_vertex_for_cell(&reader).unwrap()]);
    assert!(!initial_demand.contains(&engine.graph.get_vertex_for_cell(&expanded_target).unwrap()));

    engine
        .evaluate_targets(&[target(1, 2)])
        .expect("expanded dynamic target");

    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 2),
        Some(LiteralValue::Number(20.0))
    );
    assert_eq!(engine.last_v2_metrics().fallback_mode_activations, 0);
    let reads = engine
        .v2_read_set_for_test(reader)
        .expect("dynamic read set");
    assert!(reads.selected_targets.contains(&expanded_target));
}

#[test]
fn targeted_admission_expands_into_a_supported_formula_vertex() {
    let mut engine = make_engine();
    set_value(&mut engine, 1, 1, LiteralValue::Text("C1".to_string()));
    set_value(&mut engine, 1, 3, LiteralValue::Number(10.0));
    set_formula(&mut engine, 1, 4, "=C1+1");
    set_formula(&mut engine, 1, 2, "=INDIRECT(A1)");

    engine
        .evaluate_targets(&[target(1, 4)])
        .expect("cache target");
    engine
        .evaluate_targets(&[target(1, 2)])
        .expect("initial dynamic target");
    set_value(&mut engine, 1, 5, LiteralValue::Boolean(true));
    set_formula(&mut engine, 1, 1, "=IF(E1,\"D1\",\"C1\")");

    let reader = cell(&engine, 1, 2);
    let expanded_target = cell(&engine, 1, 4);
    let expanded_vertex = engine.graph.get_vertex_for_cell(&expanded_target).unwrap();
    let initial_demand =
        engine.v2_demand_vertices_for_test(&[engine.graph.get_vertex_for_cell(&reader).unwrap()]);
    assert!(!initial_demand.contains(&expanded_vertex));

    engine
        .evaluate_targets(&[target(1, 2)])
        .expect("supported formula target expansion");

    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 2),
        Some(LiteralValue::Number(11.0))
    );
    assert_eq!(engine.last_v2_metrics().fallback_mode_activations, 0);
    let reads = engine
        .v2_read_set_for_test(reader)
        .expect("formula read set");
    assert!(reads.formula_edges.contains(&expanded_vertex));
}

#[test]
fn targeted_runtime_unsupported_expansion_aborts_without_v1_fallback() {
    let mut engine = make_engine();
    set_value(&mut engine, 1, 1, LiteralValue::Text("C1".to_string()));
    set_value(&mut engine, 1, 3, LiteralValue::Number(10.0));
    set_formula(&mut engine, 1, 2, "=INDIRECT(A1)");

    engine
        .evaluate_targets(&[target(1, 2)])
        .expect("initial target");
    set_value(&mut engine, 1, 5, LiteralValue::Boolean(true));
    set_formula(&mut engine, 1, 1, "=IF(E1,\"D1\",\"C1\")");
    set_formula(&mut engine, 1, 4, "=UNKNOWN_FUNCTION(A1)");

    let error = engine
        .evaluate_targets(&[target(1, 2)])
        .expect_err("unsupported runtime expansion must abort V2");
    assert!(error.message.as_deref().is_some_and(|message| {
        message.contains("runtime demand") && message.contains("unsupported")
    }));
    assert_eq!(engine.v2_state_sizes_for_test(), (0, 0, 0));
    assert_eq!(engine.v2_raw_recorder_entries_for_test(), 0);
    assert_eq!(engine.last_v2_metrics().fallback_mode_activations, 0);
    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 2),
        Some(LiteralValue::Number(10.0))
    );
    assert!(
        engine.graph.is_dirty(
            engine
                .graph
                .get_vertex_for_cell(&cell(&engine, 1, 2))
                .unwrap()
        )
    );
}

#[test]
fn consecutive_targeted_requests_replace_and_retain_exact_regions() {
    let mut engine = make_engine();
    set_value(&mut engine, 1, 1, LiteralValue::Text("C1".to_string()));
    set_value(&mut engine, 2, 1, LiteralValue::Text("D1".to_string()));
    set_formula(&mut engine, 1, 3, "=10");
    set_formula(&mut engine, 1, 4, "=20");
    set_formula(&mut engine, 1, 2, "=INDIRECT(A1)");
    set_formula(&mut engine, 2, 2, "=INDIRECT(A2)");

    engine
        .evaluate_targets(&[target(1, 2)])
        .expect("first targeted request");
    engine
        .evaluate_targets(&[target(2, 2)])
        .expect("expanding targeted request");
    let first = cell(&engine, 1, 2);
    let second = cell(&engine, 2, 2);
    let c1 = cell(&engine, 1, 3);
    let d1 = cell(&engine, 1, 4);
    assert_eq!(engine.v2_state_sizes_for_test().0, 4);
    assert_eq!(engine.v2_runtime_readers_for_test(c1), vec![first]);
    assert_eq!(engine.v2_runtime_readers_for_test(d1), vec![second]);

    set_value(&mut engine, 1, 1, LiteralValue::Text("D1".to_string()));
    engine
        .evaluate_targets(&[target(1, 2)])
        .expect("first target retarget");
    assert!(engine.v2_runtime_readers_for_test(c1).is_empty());
    assert_eq!(engine.v2_runtime_readers_for_test(d1), vec![first, second]);

    set_value(&mut engine, 2, 1, LiteralValue::Text("C1".to_string()));
    engine
        .evaluate_targets(&[target(2, 2)])
        .expect("second target retarget");
    assert_eq!(engine.v2_runtime_readers_for_test(c1), vec![second]);
    assert_eq!(engine.v2_runtime_readers_for_test(d1), vec![first]);
    assert_eq!(engine.v2_state_sizes_for_test().0, 4);
}

#[test]
fn targeted_generation_changes_rebuild_name_table_and_spill_regions() {
    let mut engine = make_engine();
    set_value(&mut engine, 1, 3, LiteralValue::Number(10.0));
    set_value(&mut engine, 1, 4, LiteralValue::Number(20.0));
    let sheet_id = engine.graph.sheet_id("Sheet1").expect("sheet");
    engine
        .define_name(
            "TARGET_NAME",
            NamedDefinition::Range(RangeRef::new(
                CellRef::new(sheet_id, Coord::from_excel(1, 3, true, true)),
                CellRef::new(sheet_id, Coord::from_excel(1, 3, true, true)),
            )),
            NameScope::Workbook,
        )
        .expect("define target name");
    set_formula(&mut engine, 1, 2, "=SUM(TARGET_NAME)");
    engine
        .evaluate_targets(&[target(1, 2)])
        .expect("initial name target");
    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 2),
        Some(LiteralValue::Number(10.0))
    );
    engine
        .update_name(
            "TARGET_NAME",
            NamedDefinition::Range(RangeRef::new(
                CellRef::new(sheet_id, Coord::from_excel(1, 4, true, true)),
                CellRef::new(sheet_id, Coord::from_excel(1, 4, true, true)),
            )),
            NameScope::Workbook,
        )
        .expect("update target name");
    engine
        .evaluate_targets(&[target(1, 2)])
        .expect("updated name target");
    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 2),
        Some(LiteralValue::Number(20.0))
    );

    set_value(&mut engine, 5, 1, LiteralValue::Text("Region".to_string()));
    set_value(&mut engine, 5, 2, LiteralValue::Text("Amount".to_string()));
    set_value(&mut engine, 6, 2, LiteralValue::Number(10.0));
    set_value(&mut engine, 7, 2, LiteralValue::Number(20.0));
    engine
        .define_table(
            "TARGET_TABLE",
            RangeRef::new(
                CellRef::new(sheet_id, Coord::from_excel(5, 1, true, true)),
                CellRef::new(sheet_id, Coord::from_excel(7, 2, true, true)),
            ),
            true,
            vec!["Region".into(), "Amount".into()],
            false,
        )
        .expect("define target table");
    set_formula(&mut engine, 5, 3, "=SUM(TARGET_TABLE[Amount])");
    engine
        .evaluate_targets(&[target(5, 3)])
        .expect("initial table target");
    assert_eq!(
        engine.get_cell_value("Sheet1", 5, 3),
        Some(LiteralValue::Number(30.0))
    );
    set_value(&mut engine, 8, 1, LiteralValue::Text("West".to_string()));
    set_value(&mut engine, 8, 2, LiteralValue::Number(30.0));
    engine
        .update_table(
            "TARGET_TABLE",
            RangeRef::new(
                CellRef::new(sheet_id, Coord::from_excel(5, 1, true, true)),
                CellRef::new(sheet_id, Coord::from_excel(8, 2, true, true)),
            ),
            true,
            vec!["Region".into(), "Amount".into()],
            false,
        )
        .expect("expand target table");
    engine
        .evaluate_targets(&[target(5, 3)])
        .expect("expanded table target");
    assert_eq!(
        engine.get_cell_value("Sheet1", 5, 3),
        Some(LiteralValue::Number(60.0))
    );

    set_formula(&mut engine, 1, 5, "=VALUE({1,2})");
    engine
        .evaluate_targets(&[target(1, 5)])
        .expect("initial spill target");
    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 6),
        Some(LiteralValue::Number(2.0))
    );
    set_formula(&mut engine, 1, 5, "=VALUE({1,2,3})");
    engine
        .evaluate_targets(&[target(1, 5)])
        .expect("expanded spill target");
    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 7),
        Some(LiteralValue::Number(3.0))
    );
    assert_eq!(engine.last_v2_metrics().fallback_mode_activations, 0);
}

#[test]
fn range_consumers_use_the_production_kernel_and_record_formula_targets() {
    let mut engine = make_engine();
    set_formula(&mut engine, 1, 1, "=10");
    set_formula(&mut engine, 2, 1, "=20");
    set_formula(&mut engine, 1, 3, "=SUM(A1:A2)");

    engine.evaluate_all().expect("evaluate");
    let reader = cell(&engine, 1, 3);
    assert!(has_edge(&engine, reader, cell(&engine, 1, 1)));
    assert!(has_edge(&engine, reader, cell(&engine, 2, 1)));
    assert!(engine.last_v2_metrics().logical_range_positions > 0);
}

#[test]
fn early_stop_lookup_records_only_inspected_formula_targets() {
    let mut engine = make_engine();
    set_formula(&mut engine, 1, 1, "=\"first\"");
    set_formula(&mut engine, 2, 1, "=\"second\"");
    set_formula(&mut engine, 3, 1, "=\"third\"");
    set_formula(&mut engine, 1, 3, "=MATCH(\"*\",A1:A3,0)");

    engine.evaluate_all().expect("evaluate");
    let reader = cell(&engine, 1, 3);
    assert!(has_edge(&engine, reader, cell(&engine, 1, 1)));
    assert!(!has_edge(&engine, reader, cell(&engine, 2, 1)));
    assert!(!has_edge(&engine, reader, cell(&engine, 3, 1)));

    set_formula(&mut engine, 3, 1, "=\"changed\"");
    engine
        .evaluate_all()
        .expect("conservative range invalidation");
    assert!(engine.last_v2_metrics().formulas_evaluated >= 2);
    assert!(has_edge(&engine, reader, cell(&engine, 1, 1)));
    assert!(!has_edge(&engine, reader, cell(&engine, 2, 1)));
    assert!(!has_edge(&engine, reader, cell(&engine, 3, 1)));
}

#[test]
fn lookup_index_cache_hit_records_formula_backed_axis() {
    let mut engine = make_engine();
    for row in 1..=64 {
        set_formula(&mut engine, row, 1, &format!("={row}"));
    }
    set_formula(&mut engine, 1, 3, "=MATCH(1,A1:A64,0)");
    engine.evaluate_all().expect("initial evaluate");

    let reader = cell(&engine, 1, 3);
    let reader_vertex = engine
        .graph
        .get_vertex_for_cell(&reader)
        .expect("reader vertex");
    for _ in 0..5 {
        engine
            .evaluate_vertex(reader_vertex)
            .expect("cache warm evaluate");
    }
    assert!(engine.last_lookup_index_cache_report().hits > 0);
    assert!(has_edge(&engine, reader, cell(&engine, 1, 1)));
    assert!(has_edge(&engine, reader, cell(&engine, 64, 1)));
}

#[test]
fn sum_error_early_stop_records_only_inspected_formula_targets() {
    let mut engine = make_engine();
    set_formula(&mut engine, 1, 1, "=1/0");
    set_formula(&mut engine, 2, 1, "=20");
    set_formula(&mut engine, 3, 1, "=30");
    set_formula(&mut engine, 1, 3, "=SUM(A1:A3)");

    engine.evaluate_all().expect("evaluate");
    let reader = cell(&engine, 1, 3);
    assert!(has_edge(&engine, reader, cell(&engine, 1, 1)));
    assert!(!has_edge(&engine, reader, cell(&engine, 2, 1)));
    assert!(!has_edge(&engine, reader, cell(&engine, 3, 1)));
}

#[test]
fn representative_lookup_and_error_functions_reuse_production_implementations() {
    let mut engine = make_engine();
    set_value(&mut engine, 1, 1, LiteralValue::Int(1));
    set_value(&mut engine, 2, 1, LiteralValue::Int(2));
    set_value(&mut engine, 1, 2, LiteralValue::Text("one".to_string()));
    set_value(&mut engine, 2, 2, LiteralValue::Text("two".to_string()));
    set_formula(&mut engine, 1, 4, "=MATCH(2,A1:A2,0)");
    set_formula(&mut engine, 2, 4, "=VLOOKUP(2,A1:B2,2,FALSE)");
    set_formula(&mut engine, 3, 4, "=MIN(A1:A2)");
    set_formula(&mut engine, 4, 4, "=IFERROR(1/0,\"fallback\")");

    engine.evaluate_all().expect("evaluate");
    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 4),
        Some(LiteralValue::Number(2.0))
    );
    assert_eq!(
        engine.get_cell_value("Sheet1", 2, 4),
        Some(LiteralValue::Text("two".to_string()))
    );
    assert_eq!(
        engine.get_cell_value("Sheet1", 3, 4),
        Some(LiteralValue::Number(1.0))
    );
    assert_eq!(
        engine.get_cell_value("Sheet1", 4, 4),
        Some(LiteralValue::Text("fallback".to_string()))
    );
    let lookup_reads = engine
        .v2_read_set_for_test(cell(&engine, 2, 4))
        .expect("lookup read set");
    assert_eq!(
        lookup_reads
            .selected_targets
            .into_iter()
            .collect::<Vec<_>>(),
        vec![cell(&engine, 2, 2)]
    );
}

#[test]
fn names_use_scope_resolution_and_keep_range_invalidation_separate_from_edges() {
    let mut engine = make_engine();
    set_formula(&mut engine, 1, 1, "=10");
    set_formula(&mut engine, 2, 1, "=20");
    let sheet_id = engine.graph.sheet_id("Sheet1").expect("sheet");
    let named_range = RangeRef::new(
        CellRef::new(sheet_id, Coord::from_excel(1, 1, true, true)),
        CellRef::new(sheet_id, Coord::from_excel(2, 1, true, true)),
    );
    engine
        .define_name(
            "DATA",
            NamedDefinition::Range(named_range),
            NameScope::Workbook,
        )
        .expect("define workbook name");
    set_formula(&mut engine, 1, 3, "=SUM(DATA)");

    engine.add_sheet("Sheet2").expect("add sheet");
    let sheet2_id = engine.graph.sheet_id("Sheet2").expect("sheet");
    engine
        .define_name(
            "SCOPE_VALUE",
            NamedDefinition::Literal(LiteralValue::Int(11)),
            NameScope::Workbook,
        )
        .expect("define workbook scoped value");
    engine
        .define_name(
            "SCOPE_VALUE",
            NamedDefinition::Literal(LiteralValue::Int(22)),
            NameScope::Sheet(sheet2_id),
        )
        .expect("define local scoped value");
    engine
        .set_cell_formula(
            "Sheet2",
            1,
            1,
            parse("=SCOPE_VALUE").expect("parse formula"),
        )
        .expect("set scoped formula");

    engine.evaluate_all().expect("evaluate");
    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 3),
        Some(LiteralValue::Number(30.0))
    );
    assert!(has_edge(&engine, cell(&engine, 1, 3), cell(&engine, 1, 1)));
    assert!(has_edge(&engine, cell(&engine, 1, 3), cell(&engine, 2, 1)));
    assert_eq!(
        engine.get_cell_value("Sheet2", 1, 1),
        Some(LiteralValue::Number(22.0))
    );
}

#[test]
fn index_named_range_records_name_and_selected_target() {
    let mut engine = make_engine();
    set_formula(&mut engine, 1, 1, "=10");
    set_formula(&mut engine, 2, 1, "=20");
    let sheet_id = engine.graph.sheet_id("Sheet1").expect("sheet");
    engine
        .define_name(
            "INDEX_DATA",
            NamedDefinition::Range(RangeRef::new(
                CellRef::new(sheet_id, Coord::from_excel(1, 1, true, true)),
                CellRef::new(sheet_id, Coord::from_excel(2, 1, true, true)),
            )),
            NameScope::Workbook,
        )
        .expect("define name");
    set_formula(&mut engine, 1, 3, "=INDEX(INDEX_DATA,2)");

    engine.evaluate_all().expect("evaluate");
    let reader = cell(&engine, 1, 3);
    let selected = cell(&engine, 2, 1);
    let reads = engine.v2_read_set_for_test(reader).expect("read set");
    assert!(reads.names.contains("INDEX_DATA"));
    assert_eq!(
        reads.selected_targets.into_iter().collect::<Vec<_>>(),
        vec![selected]
    );
    assert!(has_edge(&engine, reader, selected));
    assert!(!has_edge(&engine, reader, cell(&engine, 1, 1)));
}

#[test]
fn placement_context_function_uses_the_generic_context_host_contract() {
    let mut engine = make_engine();
    set_formula(&mut engine, 1, 1, "=COLUMN()");

    engine.evaluate_all().expect("evaluate");
    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 1),
        Some(LiteralValue::Number(1.0))
    );
    assert_eq!(engine.last_v2_metrics().formulas_evaluated, 1);
    assert_eq!(engine.last_v2_metrics().fallback_mode_activations, 0);
    let reads = engine
        .v2_read_set_for_test(cell(&engine, 1, 1))
        .expect("ROW read set");
    assert!(
        reads
            .effects
            .contains(&crate::engine::v2::EffectKind::PlacementContext)
    );
}

#[test]
fn generic_argument_state_safe_builtin_enters_v2_without_a_whitelist_bit() {
    let mut engine = make_engine();
    set_formula(&mut engine, 1, 1, "=ABS(A1)");
    set_value(&mut engine, 1, 1, LiteralValue::Number(3.0));
    set_formula(&mut engine, 1, 2, "=ABS(A1)");
    engine.evaluate_all().expect("generic V2 scalar evaluation");
    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 2),
        Some(LiteralValue::Number(3.0))
    );
    assert_eq!(engine.last_v2_metrics().formulas_evaluated, 1);
    assert_eq!(engine.last_v2_metrics().fallback_mode_activations, 0);
    let reads = engine
        .v2_read_set_for_test(cell(&engine, 1, 2))
        .expect("ABS read set");
    assert_eq!(reads.cells.len(), 1);
    assert!(reads.contains_cell(&cell(&engine, 1, 1)));

    let mut range = make_engine();
    set_value(&mut range, 1, 1, LiteralValue::Number(2.0));
    set_value(&mut range, 2, 1, LiteralValue::Number(4.0));
    set_formula(&mut range, 1, 2, "=AVERAGE(A1:A2)");
    range.evaluate_all().expect("generic V2 range evaluation");
    assert_eq!(
        range.get_cell_value("Sheet1", 1, 2),
        Some(LiteralValue::Number(3.0))
    );
    assert_eq!(range.last_v2_metrics().fallback_mode_activations, 0);
    let range_reads = range
        .v2_read_set_for_test(cell(&range, 1, 2))
        .expect("AVERAGE read set");
    assert!(range_reads.logical_range_positions > 0);
    assert!(range_reads.contains_cell(&cell(&range, 1, 1)));
    assert!(range_reads.contains_cell(&cell(&range, 2, 1)));
}

#[test]
fn custom_and_nested_unsupported_functions_fail_closed() {
    let workbook = TestWorkbook::new().with_function(Arc::new(SelfContractedFn));
    let mut custom = Engine::new(workbook, runtime_config());
    custom.enable_v2_for_test();
    set_formula(&mut custom, 1, 1, "=SELF_CONTRACTED()");
    custom.evaluate_all().expect("custom fallback");
    assert_eq!(
        custom.get_cell_value("Sheet1", 1, 1),
        Some(LiteralValue::Number(9.0))
    );
    assert_eq!(custom.last_v2_metrics().formulas_evaluated, 0);
    assert_eq!(custom.last_v2_metrics().fallback_mode_activations, 1);

    let mut nested = make_engine();
    set_formula(&mut nested, 1, 1, "=IF(TRUE,1,INDIRECT(\"A1\"))");
    nested.evaluate_all().expect("nested V2 evaluation");
    assert_eq!(
        nested.get_cell_value("Sheet1", 1, 1),
        Some(LiteralValue::Number(1.0))
    );
    assert_eq!(nested.last_v2_metrics().formulas_evaluated, 1);
    assert_eq!(nested.last_v2_metrics().fallback_mode_activations, 0);
    assert!(!has_edge(&nested, cell(&nested, 1, 1), cell(&nested, 1, 1)));
}

#[test]
fn spill_result_shape_contract_stays_on_v2_path() {
    let mut engine = make_engine();
    set_formula(&mut engine, 1, 1, "=VALUE({1,2})");

    engine.evaluate_all().expect("evaluate spill result");

    assert_eq!(engine.last_v2_metrics().fallback_mode_activations, 0);
    assert!(engine.last_v2_metrics().formulas_evaluated > 0);
    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 1),
        Some(LiteralValue::Number(1.0))
    );
    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 2),
        Some(LiteralValue::Number(2.0))
    );
}

#[test]
fn lexical_bindings_enter_v2() {
    let mut engine = make_engine();
    set_value(&mut engine, 1, 1, LiteralValue::Number(4.0));
    set_formula(&mut engine, 1, 2, "=LET(x,A1+1,x*2)");

    engine.evaluate_all().expect("evaluate lexical formula");

    assert_eq!(engine.last_v2_metrics().fallback_mode_activations, 0);
    assert!(engine.last_v2_metrics().formulas_evaluated > 0);
    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 2),
        Some(LiteralValue::Number(10.0))
    );
}

#[test]
fn table_reference_enters_v2_and_tracks_table_dependency() {
    let mut engine = make_engine();
    set_value(&mut engine, 2, 1, LiteralValue::Text("North".to_string()));
    set_value(&mut engine, 2, 2, LiteralValue::Number(10.0));
    set_value(&mut engine, 3, 1, LiteralValue::Text("South".to_string()));
    set_value(&mut engine, 3, 2, LiteralValue::Number(20.0));
    let sheet_id = engine.graph.sheet_id("Sheet1").expect("sheet");
    engine
        .define_table(
            "Sales",
            RangeRef::new(
                CellRef::new(sheet_id, Coord::from_excel(1, 1, true, true)),
                CellRef::new(sheet_id, Coord::from_excel(3, 2, true, true)),
            ),
            true,
            vec!["Region".into(), "Amount".into()],
            false,
        )
        .expect("define table");
    set_formula(&mut engine, 1, 4, "=SUM(Sales[Amount])");

    engine.evaluate_all().expect("evaluate table formula");

    assert_eq!(engine.last_v2_metrics().fallback_mode_activations, 0);
    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 4),
        Some(LiteralValue::Number(30.0))
    );
    let table_vertex = engine
        .graph
        .resolve_table_entry("Sales")
        .expect("table vertex")
        .vertex;
    let reads = engine
        .v2_read_set_for_test(cell(&engine, 1, 4))
        .expect("table formula read set");
    assert!(reads.tables.contains("Sales"));
    assert!(reads.formula_edges.contains(&table_vertex));

    set_value(&mut engine, 2, 2, LiteralValue::Number(100.0));
    engine.evaluate_all().expect("re-evaluate table formula");
    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 4),
        Some(LiteralValue::Number(120.0))
    );
}

#[test]
fn formula_backed_names_enter_v2_with_exact_name_edges() {
    let mut engine = make_engine();
    set_formula(&mut engine, 1, 1, "=41");
    engine
        .define_name(
            "FORMULA_NAME",
            NamedDefinition::Formula {
                ast: parse("=A1+1").expect("parse formula name"),
                dependencies: Vec::new(),
                range_deps: Vec::new(),
            },
            NameScope::Workbook,
        )
        .expect("define workbook formula name");
    engine
        .define_name(
            "REFERENCE_NAME",
            NamedDefinition::Formula {
                ast: parse("=A1").expect("parse reference formula name"),
                dependencies: Vec::new(),
                range_deps: Vec::new(),
            },
            NameScope::Workbook,
        )
        .expect("define reference formula name");
    set_formula(&mut engine, 1, 2, "=FORMULA_NAME");
    set_formula(&mut engine, 1, 3, "=SUM(REFERENCE_NAME)");

    engine.add_sheet("Sheet2").expect("add sheet");
    let sheet2_id = engine.graph.sheet_id("Sheet2").expect("sheet");
    engine
        .define_name(
            "LOCAL_FORMULA_NAME",
            NamedDefinition::Formula {
                ast: parse("=A1+2").expect("parse local formula name"),
                dependencies: Vec::new(),
                range_deps: Vec::new(),
            },
            NameScope::Sheet(sheet2_id),
        )
        .expect("define local formula name");
    engine
        .set_cell_value("Sheet2", 1, 1, LiteralValue::Int(10))
        .expect("set local input");
    engine
        .set_cell_formula(
            "Sheet2",
            1,
            2,
            parse("=LOCAL_FORMULA_NAME").expect("parse local formula consumer"),
        )
        .expect("set local formula consumer");

    engine.evaluate_all().expect("initial evaluate");
    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 2),
        Some(LiteralValue::Number(42.0))
    );
    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 3),
        Some(LiteralValue::Number(41.0))
    );
    assert_eq!(
        engine.get_cell_value("Sheet2", 1, 2),
        Some(LiteralValue::Number(12.0))
    );
    assert!(engine.last_v2_metrics().formulas_evaluated > 0);
    assert_eq!(engine.last_v2_metrics().fallback_mode_activations, 0);
    let formula_name_vertex = engine
        .resolve_name_entry("FORMULA_NAME", engine.graph.default_sheet_id())
        .expect("formula name vertex")
        .vertex;
    let formula_name_reads = engine
        .v2_read_set_for_test(cell(&engine, 1, 2))
        .expect("formula-name consumer read set");
    assert!(
        formula_name_reads
            .formula_edges
            .contains(&formula_name_vertex)
    );

    engine
        .update_name(
            "FORMULA_NAME",
            NamedDefinition::Formula {
                ast: parse("=A1+2").expect("parse changed formula name"),
                dependencies: Vec::new(),
                range_deps: Vec::new(),
            },
            NameScope::Workbook,
        )
        .expect("update workbook formula name");
    engine.evaluate_all().expect("changed name evaluate");
    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 2),
        Some(LiteralValue::Number(43.0))
    );
    assert!(engine.last_v2_metrics().formulas_evaluated > 0);
    assert_eq!(engine.last_v2_metrics().fallback_mode_activations, 0);
}

#[test]
fn offset_target_uses_the_generic_dynamic_reference_host_contract() {
    let mut engine = make_engine();
    set_value(&mut engine, 1, 4, LiteralValue::Int(0));
    set_formula(&mut engine, 1, 1, "=10");
    set_formula(&mut engine, 2, 1, "=20");
    set_formula(&mut engine, 1, 3, "=OFFSET(A1,D1,0)");

    engine.evaluate_all().expect("initial evaluate");
    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 3),
        Some(LiteralValue::Number(10.0))
    );
    assert!(engine.last_v2_metrics().formulas_evaluated > 0);
    assert_eq!(engine.last_v2_metrics().fallback_mode_activations, 0);
    let reads = engine
        .v2_read_set_for_test(cell(&engine, 1, 3))
        .expect("OFFSET read set");
    assert!(
        reads
            .effects
            .contains(&crate::engine::v2::EffectKind::DynamicTarget)
    );
    assert!(!reads.reference_observations.is_empty());
    assert!(has_edge(&engine, cell(&engine, 1, 3), cell(&engine, 1, 1)));

    set_formula(&mut engine, 1, 1, "=11");
    engine.evaluate_all().expect("dynamic target value change");
    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 3),
        Some(LiteralValue::Number(11.0))
    );
    assert!(has_edge(&engine, cell(&engine, 1, 3), cell(&engine, 1, 1)));

    set_value(&mut engine, 1, 4, LiteralValue::Int(1));
    engine.evaluate_all().expect("target evaluate");
    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 3),
        Some(LiteralValue::Number(20.0))
    );
    assert!(engine.last_v2_metrics().formulas_evaluated > 0);
    assert_eq!(engine.last_v2_metrics().fallback_mode_activations, 0);
    assert!(!has_edge(&engine, cell(&engine, 1, 3), cell(&engine, 1, 1)));
    assert!(has_edge(&engine, cell(&engine, 1, 3), cell(&engine, 2, 1)));
}

#[test]
fn dynamic_target_uses_the_generic_dynamic_reference_host_contract() {
    let mut engine = make_engine();
    set_value(&mut engine, 1, 4, LiteralValue::Text("A1".to_string()));
    set_formula(&mut engine, 1, 1, "=10");
    set_formula(&mut engine, 2, 1, "=20");
    set_formula(&mut engine, 1, 3, "=INDIRECT(D1)");

    engine.evaluate_all().expect("initial evaluate");
    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 3),
        Some(LiteralValue::Number(10.0))
    );
    assert!(engine.last_v2_metrics().formulas_evaluated > 0);
    assert_eq!(engine.last_v2_metrics().fallback_mode_activations, 0);
    let reads = engine
        .v2_read_set_for_test(cell(&engine, 1, 3))
        .expect("INDIRECT read set");
    assert!(
        reads
            .effects
            .contains(&crate::engine::v2::EffectKind::DynamicTarget)
    );
    assert!(!reads.reference_observations.is_empty());
    assert!(has_edge(&engine, cell(&engine, 1, 3), cell(&engine, 1, 1)));

    set_value(&mut engine, 1, 4, LiteralValue::Text("A2".to_string()));
    engine.evaluate_all().expect("target evaluate");
    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 3),
        Some(LiteralValue::Number(20.0))
    );
    assert!(engine.last_v2_metrics().formulas_evaluated > 0);
    assert_eq!(engine.last_v2_metrics().fallback_mode_activations, 0);
    assert!(!has_edge(&engine, cell(&engine, 1, 3), cell(&engine, 1, 1)));
    assert!(has_edge(&engine, cell(&engine, 1, 3), cell(&engine, 2, 1)));

    set_formula(&mut engine, 2, 1, "=10");
    engine.evaluate_all().expect("equal-valued target identity");
    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 3),
        Some(LiteralValue::Number(10.0))
    );
    assert!(has_edge(&engine, cell(&engine, 1, 3), cell(&engine, 2, 1)));
}

#[test]
fn forwarded_dynamic_reference_retains_target_observation() {
    let mut engine = make_engine();
    set_formula(&mut engine, 1, 1, "=10");
    set_value(&mut engine, 1, 4, LiteralValue::Text("A1".to_string()));
    set_formula(&mut engine, 1, 5, "=COLUMN(INDIRECT(D1))");

    engine.evaluate_all().expect("forwarded dynamic reference");
    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 5),
        Some(LiteralValue::Number(1.0))
    );
    assert_eq!(engine.last_v2_metrics().fallback_mode_activations, 0);
    assert!(!has_edge(&engine, cell(&engine, 1, 5), cell(&engine, 1, 1)));
    let reads = engine
        .v2_read_set_for_test(cell(&engine, 1, 5))
        .expect("forwarded reference read set");
    assert!(!reads.reference_observations.is_empty());
    assert!(reads.contains_cell(&cell(&engine, 1, 4)));
    assert!(reads.selected_targets.contains(&cell(&engine, 1, 1)));
}

#[test]
fn genuine_feedback_uses_a_runtime_workspace_and_clean_noop_does_no_work() {
    let mut engine = make_engine();
    set_formula(&mut engine, 1, 1, "=B1+1");
    set_formula(&mut engine, 1, 2, "=A1+1");

    let first = engine.evaluate_all().expect("cycle evaluate");
    assert_eq!(first.cycle_errors, 0);
    assert!(engine.last_v2_metrics().active_cyclic_workspace_members >= 2);
    assert!(has_edge(&engine, cell(&engine, 1, 1), cell(&engine, 1, 2)));
    assert!(has_edge(&engine, cell(&engine, 1, 2), cell(&engine, 1, 1)));

    let before = engine.last_v2_metrics().formulas_evaluated;
    engine.evaluate_all().expect("iterative recalc");
    assert!(engine.last_v2_metrics().formulas_evaluated >= 2);
    assert!(engine.last_v2_metrics().queue_steps >= 2);
    assert!(before >= 2);

    let mut clean = make_engine();
    set_formula(&mut clean, 1, 1, "=1+2");
    clean.evaluate_all().expect("clean initial evaluate");
    clean.evaluate_all().expect("clean no-op evaluate");
    assert_eq!(clean.last_v2_metrics().formulas_evaluated, 0);
    assert_eq!(clean.last_v2_metrics().queue_steps, 0);
}

#[test]
fn gate2_replaces_forward_and_reverse_edges_without_accumulation() {
    let mut engine = make_engine();
    set_value(&mut engine, 1, 1, LiteralValue::Boolean(true));
    set_formula(&mut engine, 1, 2, "=10");
    set_formula(&mut engine, 1, 4, "=20");
    set_formula(&mut engine, 1, 3, "=IF(A1,B1,D1)");
    engine.evaluate_all().expect("initial evaluate");

    let reader = cell(&engine, 1, 3);
    let left = cell(&engine, 1, 2);
    let right = cell(&engine, 1, 4);
    assert_eq!(engine.v2_runtime_readers_for_test(left), vec![reader]);
    assert!(engine.v2_runtime_readers_for_test(right).is_empty());
    let baseline = engine.v2_state_sizes_for_test();

    for iteration in 0..100 {
        let take_left = iteration % 2 != 0;
        set_value(&mut engine, 1, 1, LiteralValue::Boolean(take_left));
        engine.evaluate_all().expect("branch evaluate");
        let (selected, stale) = if take_left {
            (left, right)
        } else {
            (right, left)
        };
        assert_eq!(engine.v2_runtime_readers_for_test(selected), vec![reader]);
        assert!(engine.v2_runtime_readers_for_test(stale).is_empty());
        assert!(has_edge(&engine, reader, selected));
        assert!(!has_edge(&engine, reader, stale));
        assert_eq!(engine.v2_state_sizes_for_test(), baseline);
    }
}

#[test]
fn gate2_v1_route_invalidates_and_full_v2_rebuilds_exact_state() {
    let mut engine = make_engine();
    set_value(&mut engine, 1, 1, LiteralValue::Boolean(true));
    set_formula(&mut engine, 1, 2, "=10");
    set_formula(&mut engine, 1, 4, "=20");
    set_formula(&mut engine, 1, 3, "=IF(A1,B1,D1)");
    engine.evaluate_all().expect("initial V2 evaluate");
    let reader = cell(&engine, 1, 3);
    assert!(has_edge(&engine, reader, cell(&engine, 1, 2)));

    set_value(&mut engine, 1, 1, LiteralValue::Boolean(false));
    let targets = [crate::engine::EvaluationTarget::Cell {
        sheet: "Sheet1".to_string(),
        row: 1,
        col: 3,
    }];
    engine
        .evaluate_targets_with_delta(&targets)
        .expect("V1-only target delta route");
    assert!(engine.v2_current_formula_edges_for_test().is_empty());
    assert!(engine.v2_read_set_for_test(reader).is_none());
    assert_eq!(engine.v2_raw_recorder_entries_for_test(), 0);

    engine.evaluate_all().expect("full V2 rebuild");
    assert!(engine.last_v2_metrics().formulas_evaluated >= 3);
    assert!(has_edge(&engine, reader, cell(&engine, 1, 4)));
    assert!(!has_edge(&engine, reader, cell(&engine, 1, 2)));
}

#[test]
fn gate2_runtime_abort_discards_partial_exact_state_and_does_not_fallback() {
    let mut engine = make_engine();
    set_formula(&mut engine, 1, 1, "=1");
    set_formula(&mut engine, 1, 2, "=A1+1");
    engine.set_v2_fail_after_formula_commits_for_test(Some(1));

    let error = engine.evaluate_all().expect_err("injected V2 failure");
    assert!(
        error
            .message
            .as_deref()
            .is_some_and(|message| message.contains("Injected"))
    );
    assert_eq!(engine.evaluation_request_begin_count_for_test(), 1);
    assert_eq!(engine.v2_state_sizes_for_test(), (0, 0, 0));
    assert_eq!(engine.v2_raw_recorder_entries_for_test(), 0);
    assert!(
        engine
            .graph
            .vertices_with_formulas()
            .all(|vertex| engine.graph.is_dirty(vertex))
    );

    engine.set_v2_fail_after_formula_commits_for_test(None);
    engine.evaluate_all().expect("retry after terminal abort");
    assert_eq!(engine.evaluation_request_begin_count_for_test(), 2);
    assert!(engine.v2_read_set_for_test(cell(&engine, 1, 2)).is_some());
}

#[test]
fn gate2_revision_change_rejects_commit_and_date_change_forces_rebuild() {
    let mut engine = make_engine();
    set_value(&mut engine, 1, 1, LiteralValue::Number(1.0));
    set_formula(&mut engine, 1, 2, "=A1+1");
    set_formula(&mut engine, 1, 3, "=EDATE(45322,1)");
    engine.evaluate_all().expect("initial evaluate");
    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 2),
        Some(LiteralValue::Number(2.0))
    );

    set_value(&mut engine, 1, 1, LiteralValue::Number(2.0));
    engine.bump_v2_symbol_after_evaluation_for_test();
    engine
        .evaluate_all()
        .expect_err("symbol revision changed before commit");
    assert_eq!(engine.v2_state_sizes_for_test(), (0, 0, 0));
    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 2),
        Some(LiteralValue::Number(2.0))
    );

    engine.evaluate_all().expect("rebuild after revision abort");
    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 2),
        Some(LiteralValue::Number(3.0))
    );
    engine.config.date_system = crate::engine::DateSystem::Excel1904;
    engine.evaluate_all().expect("date-system rebuild");
    assert!(engine.last_v2_metrics().formulas_evaluated >= 2);
    assert!(engine.v2_read_set_for_test(cell(&engine, 1, 2)).is_some());
    assert!(engine.v2_read_set_for_test(cell(&engine, 1, 3)).is_some());
}

#[test]
fn gate2_name_revision_replaces_edges_and_v2_cycles_retain_exact_reads() {
    let mut engine = make_engine();
    set_formula(&mut engine, 1, 1, "=10");
    set_formula(&mut engine, 1, 2, "=20");
    let sheet_id = engine.graph.sheet_id("Sheet1").expect("sheet");
    let range = |col| {
        NamedDefinition::Range(RangeRef::new(
            CellRef::new(sheet_id, Coord::from_excel(1, col, true, true)),
            CellRef::new(sheet_id, Coord::from_excel(1, col, true, true)),
        ))
    };
    engine
        .define_name("GATE2_DATA", range(1), NameScope::Workbook)
        .expect("define name");
    set_formula(&mut engine, 1, 3, "=SUM(GATE2_DATA)");
    engine.evaluate_all().expect("initial name evaluate");
    let reader = cell(&engine, 1, 3);
    assert!(has_edge(&engine, reader, cell(&engine, 1, 1)));

    engine
        .update_name("GATE2_DATA", range(2), NameScope::Workbook)
        .expect("update name");
    engine.evaluate_all().expect("changed name evaluate");
    assert!(!has_edge(&engine, reader, cell(&engine, 1, 1)));
    assert!(has_edge(&engine, reader, cell(&engine, 1, 2)));
    assert!(
        engine
            .v2_runtime_readers_for_test(cell(&engine, 1, 1))
            .is_empty()
    );

    let mut fallback = make_engine();
    set_formula(&mut fallback, 1, 1, "=B1+SEQUENCE(1)");
    set_formula(&mut fallback, 1, 2, "=A1+1");
    for _ in 0..10 {
        fallback.evaluate_all().expect("V1 cycle evaluate");
        assert_eq!(fallback.v2_raw_recorder_entries_for_test(), 0);
        assert!(!fallback.v2_current_formula_edges_for_test().is_empty());
        assert_eq!(fallback.last_v2_metrics().fallback_mode_activations, 0);
    }
}

#[test]
fn gate3_solver_seed_cap_and_error_policy_match_v1() {
    for (max_iterations, max_change) in [(1, 0.001), (3, 0.0), (100, 0.01)] {
        let cycle = CycleConfig::iterate(max_iterations, max_change);
        let mut v1 = engine_with_cycle(cycle, false, false);
        let mut v2 = engine_with_cycle(cycle, true, false);
        set_formula(&mut v1, 1, 1, "=A1+1");
        set_formula(&mut v2, 1, 1, "=A1+1");
        v1.evaluate_all().expect("V1 cycle evaluate");
        v2.evaluate_all().expect("V2 cycle evaluate");
        assert_eq!(
            v2.get_cell_value("Sheet1", 1, 1),
            v1.get_cell_value("Sheet1", 1, 1)
        );
        assert_eq!(v2.last_v2_metrics().solver_passes, max_iterations as usize);
        if max_iterations == 1 {
            assert_eq!(
                v2.get_cell_value("Sheet1", 1, 1),
                Some(LiteralValue::Number(1.0))
            );
        }
    }

    let error_cycle = CycleConfig {
        detection: CycleDetection::Runtime,
        policy: CyclePolicy::Error,
    };
    let mut v1 = engine_with_cycle(error_cycle, false, false);
    let mut v2 = engine_with_cycle(error_cycle, true, false);
    for engine in [&mut v1, &mut v2] {
        set_formula(engine, 1, 1, "=B1+1");
        set_formula(engine, 1, 2, "=A1+1");
    }
    let v1_result = v1.evaluate_all().expect("V1 error-policy cycle");
    let v2_result = v2.evaluate_all().expect("V2 error-policy cycle");
    assert_eq!(v2_result.cycle_errors, v1_result.cycle_errors);
    assert_eq!(
        v2.get_cell_value("Sheet1", 1, 1),
        v1.get_cell_value("Sheet1", 1, 1)
    );
    assert_eq!(
        v2.get_cell_value("Sheet1", 1, 2),
        v1.get_cell_value("Sheet1", 1, 2)
    );

    let tolerance = CycleConfig::iterate(100, 0.1);
    let mut v1 = engine_with_cycle(tolerance, false, false);
    let mut v2 = engine_with_cycle(tolerance, true, false);
    set_formula(&mut v1, 1, 1, "=(A1+1)/2");
    set_formula(&mut v2, 1, 1, "=(A1+1)/2");
    v1.evaluate_all().expect("V1 tolerance cycle");
    v2.evaluate_all().expect("V2 tolerance cycle");
    assert_eq!(
        v2.get_cell_value("Sheet1", 1, 1),
        v1.get_cell_value("Sheet1", 1, 1)
    );
}

#[test]
fn gate3_external_predecessor_cycle_and_downstream_are_dependency_stable() {
    fn setup(engine: &mut Engine<TestWorkbook>, predecessor_first: bool) {
        set_value(engine, 1, 5, LiteralValue::Number(1.0));
        if predecessor_first {
            set_formula(engine, 1, 3, "=E1");
            set_formula(engine, 1, 1, "=IF(C1>0,(B1+1)/2,0)");
            set_formula(engine, 1, 2, "=IF(C1>0,(A1+1)/2,0)");
        } else {
            set_formula(engine, 1, 1, "=(B1+1)/2");
            set_formula(engine, 1, 2, "=(A1+1)/2");
            set_formula(engine, 1, 3, "=E1");
            set_formula(engine, 1, 1, "=IF(C1>0,(B1+1)/2,0)");
            set_formula(engine, 1, 2, "=IF(C1>0,(A1+1)/2,0)");
        }
        set_formula(engine, 1, 4, "=A1+10");
    }

    let cycle = CycleConfig::iterate(100, 0.001);
    let mut v1 = engine_with_cycle(cycle, false, true);
    let mut v2 = engine_with_cycle(cycle, true, true);
    let mut reversed = engine_with_cycle(cycle, true, true);
    setup(&mut v1, false);
    setup(&mut v2, false);
    setup(&mut reversed, true);

    let a = cell(&v2, 1, 1);
    let c = cell(&v2, 1, 3);
    let a_vertex = v2.graph.get_vertex_for_cell(&a).expect("A1 vertex");
    let c_vertex = v2.graph.get_vertex_for_cell(&c).expect("C1 vertex");
    assert!(
        c_vertex > a_vertex,
        "predecessor must have the larger VertexId witness"
    );

    v1.evaluate_all().expect("V1 initial");
    v2.evaluate_all().expect("V2 initial");
    reversed.evaluate_all().expect("V2 reversed initial");
    for col in 1..=4 {
        assert_eq!(
            v2.get_cell_value("Sheet1", 1, col),
            v1.get_cell_value("Sheet1", 1, col)
        );
        assert_eq!(
            reversed.get_cell_value("Sheet1", 1, col),
            v1.get_cell_value("Sheet1", 1, col)
        );
    }
    assert!(v2.last_v2_metrics().active_cyclic_workspace_members >= 2);

    for engine in [&mut v1, &mut v2, &mut reversed] {
        set_value(engine, 1, 5, LiteralValue::Number(0.0));
        engine.evaluate_all().expect("branch change evaluate");
    }
    assert_eq!(
        v2.get_cell_value("Sheet1", 1, 4),
        v1.get_cell_value("Sheet1", 1, 4)
    );
    assert_eq!(
        reversed.get_cell_value("Sheet1", 1, 4),
        v1.get_cell_value("Sheet1", 1, 4)
    );
    assert_eq!(v2.last_v2_metrics().active_cyclic_workspace_members, 0);
    assert!(!has_edge(&v2, cell(&v2, 1, 1), cell(&v2, 1, 2)));
    assert!(!has_edge(&v2, cell(&v2, 1, 2), cell(&v2, 1, 1)));
}

#[test]
fn gate3_targeted_schedule_limits_workspace_to_requested_cycle() {
    let mut engine = make_engine();
    set_formula(&mut engine, 1, 1, "=(B1+1)/2");
    set_formula(&mut engine, 1, 2, "=(A1+1)/2");
    set_formula(&mut engine, 1, 3, "=(D1+1)/2");
    set_formula(&mut engine, 1, 4, "=(C1+1)/2");
    engine.evaluate_all().expect("initial cycles");
    assert!(engine.last_v2_metrics().active_cyclic_workspace_members >= 4);

    let first_cycle = cell(&engine, 1, 1);
    let first_vertex = engine
        .graph
        .get_vertex_for_cell(&first_cycle)
        .expect("first cycle vertex");
    engine
        .evaluate_vertex(first_vertex)
        .expect("targeted cycle");
    assert_eq!(engine.last_v2_metrics().active_cyclic_workspace_members, 2);
}

#[test]
fn gate3_targeted_requests_preserve_recalc_epoch() {
    let mut engine = make_engine();
    set_value(&mut engine, 1, 1, LiteralValue::Number(1.0));
    set_formula(&mut engine, 1, 2, "=A1+1");
    engine.evaluate_all().expect("full evaluate");
    let epoch = engine.recalc_epoch;

    set_value(&mut engine, 1, 1, LiteralValue::Number(2.0));
    let target = cell(&engine, 1, 2);
    let target_vertex = engine
        .graph
        .get_vertex_for_cell(&target)
        .expect("target vertex");
    engine
        .evaluate_vertex(target_vertex)
        .expect("direct target");
    assert_eq!(engine.recalc_epoch, epoch);
    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 2),
        Some(LiteralValue::Number(3.0))
    );

    set_value(&mut engine, 1, 1, LiteralValue::Number(3.0));
    engine
        .evaluate_until(&[("Sheet1", 1, 2)])
        .expect("evaluate until");
    assert_eq!(engine.recalc_epoch, epoch);
    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 2),
        Some(LiteralValue::Number(4.0))
    );

    set_value(&mut engine, 1, 1, LiteralValue::Number(4.0));
    engine
        .evaluate_targets(&[crate::engine::EvaluationTarget::Cell {
            sheet: "Sheet1".to_string(),
            row: 1,
            col: 2,
        }])
        .expect("typed target");
    assert_eq!(engine.recalc_epoch, epoch);
    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 2),
        Some(LiteralValue::Number(5.0))
    );
}

#[test]
fn gate3_cancellation_reaches_queue_and_workspace_solver() {
    let mut ordinary = make_engine();
    set_formula(&mut ordinary, 1, 1, "=1+2");
    let cancelled = crate::engine::CancelToken::new();
    cancelled.cancel();
    let error = ordinary
        .evaluate_all_cancellable(cancelled)
        .expect_err("pre-cancelled V2 request");
    assert_eq!(error.kind, ExcelErrorKind::Cancelled);
    assert_eq!(ordinary.v2_state_sizes_for_test(), (0, 0, 0));

    let mut cycle = make_engine();
    set_formula(&mut cycle, 1, 1, "=B1+1");
    set_formula(&mut cycle, 1, 2, "=A1+1");
    cycle.cancel_v2_before_workspace_for_test();
    let token = crate::engine::CancelToken::new();
    let error = cycle
        .evaluate_all_cancellable(token)
        .expect_err("workspace cancellation");
    assert_eq!(error.kind, ExcelErrorKind::Cancelled);
    assert_eq!(cycle.v2_state_sizes_for_test(), (0, 0, 0));
    assert!(
        cycle
            .graph
            .vertices_with_formulas()
            .all(|vertex| cycle.graph.is_dirty(vertex))
    );
}

#[test]
fn gate4_contract_probe_is_observation_only() {
    let mut engine = make_engine();
    set_formula(&mut engine, 1, 1, "=SUM(1,2)");
    engine.evaluate_all().expect("evaluate");
    let before = engine.v2_diagnostics_for_test();
    let admission = engine.v2_contract_diagnostics_for_test();
    let after = engine.v2_diagnostics_for_test();
    assert!(admission.eligible);
    assert_eq!(before, after);
    assert!(engine.v2_read_set_for_test(cell(&engine, 1, 1)).is_some());
}

#[test]
fn gate4_metrics_noop_contract_cache_and_state_are_truthful() {
    let mut engine = make_engine();
    set_formula(&mut engine, 1, 1, "=1");
    set_formula(&mut engine, 1, 2, "=2");
    set_formula(&mut engine, 1, 3, "=SUM(A1:B1,A1:B1)");
    engine.evaluate_all().expect("initial evaluate");

    let initial = engine.v2_diagnostics_for_test();
    assert!(initial.enabled);
    assert_eq!(initial.fallback_activations, 0);
    assert!(initial.formula_evaluations >= 3);
    assert!(initial.runtime_formula_edge_events > initial.unique_runtime_formula_edges);
    assert_eq!(initial.unique_runtime_formula_edges, 2);
    assert!(initial.logical_range_positions >= 4);
    assert!(initial.physical_cells_read >= 4);
    assert_eq!(initial.workspace_members, 0);
    assert_eq!(initial.solver_passes, 0);
    assert!(initial.schedule_ns <= initial.elapsed_ns);
    assert!(initial.formula_ns <= initial.elapsed_ns);
    assert!(initial.cleanup_ns <= initial.elapsed_ns);
    assert_eq!(initial.contract_scans, 1);
    let state_size = (initial.current_read_sets, initial.reverse_buckets);

    for _ in 0..100 {
        engine.evaluate_all().expect("clean no-op");
        let no_op = engine.v2_diagnostics_for_test();
        assert_eq!(no_op.formula_evaluations, 0);
        assert_eq!(no_op.queue_steps, 0);
        assert_eq!(no_op.contract_scans, 1);
        assert_eq!((no_op.current_read_sets, no_op.reverse_buckets), state_size);
    }

    let mut fallback = make_engine();
    set_formula(&mut fallback, 1, 1, "=CELL(\"address\",A1)");
    for _ in 0..10 {
        fallback.evaluate_all().expect("cached V1 fallback");
    }
    let fallback_metrics = fallback.v2_diagnostics_for_test();
    assert_eq!(fallback_metrics.contract_scans, 1);
    assert_eq!(fallback_metrics.formula_evaluations, 0);
    assert!(fallback_metrics.fallback_activations >= 1);
}

#[test]
fn v2_is_opt_in_and_missing_provider_revision_falls_back_to_v1() {
    let mut default_engine = Engine::new(TestWorkbook::new(), runtime_config());
    default_engine.disable_v2_for_test();
    set_formula(&mut default_engine, 1, 1, "=1+2");
    default_engine.evaluate_all().expect("default evaluate");
    assert_eq!(default_engine.last_v2_metrics().formulas_evaluated, 0);

    let mut fallback = Engine::new(
        TestWorkbook::new().without_planning_revision(),
        runtime_config(),
    );
    fallback.enable_v2_for_test();
    set_formula(&mut fallback, 1, 1, "=1+2");
    fallback.evaluate_all().expect("fallback evaluate");
    assert_eq!(
        fallback.get_cell_value("Sheet1", 1, 1),
        Some(LiteralValue::Number(3.0))
    );
    assert_eq!(fallback.last_v2_metrics().formulas_evaluated, 0);
    assert_eq!(fallback.last_v2_metrics().fallback_mode_activations, 1);
}

#[test]
fn edate_reuses_the_production_date_semantics_on_the_v2_path() {
    let mut engine = make_engine();
    set_formula(&mut engine, 1, 1, "=EDATE(45322,1)");
    engine.evaluate_all().expect("evaluate");
    assert_eq!(
        engine.get_cell_value("Sheet1", 1, 1),
        Some(LiteralValue::Number(45351.0))
    );
    let reads = engine
        .v2_read_set_for_test(cell(&engine, 1, 1))
        .expect("read set");
    assert!(
        reads
            .effects
            .contains(&crate::engine::v2::EffectKind::DateSystem)
    );
}
