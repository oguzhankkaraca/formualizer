use super::*;
use formualizer_common::{ExcelErrorKind, LiteralValue};
use formualizer_parse::parser::parse;
use std::collections::BTreeSet;

fn number(engine: &PocEngine, cell: &CellId) -> f64 {
    match engine.get_cell_value(cell) {
        LiteralValue::Number(value) => value,
        LiteralValue::Int(value) => value as f64,
        other => panic!("expected number at {cell}, got {other:?}"),
    }
}

fn cell(engine: &mut PocEngine, sheet: &str, row: u32, col: u32, value: f64) -> CellId {
    engine.set_cell_value(sheet, row, col, LiteralValue::Number(value))
}

fn reference_to_string_for_test(reference: &ReferenceValue) -> String {
    match reference {
        ReferenceValue::Cell(cell) => cell.to_string(),
        ReferenceValue::Range(range) => range.to_string(),
        ReferenceValue::Spill(spill) => spill.range().to_string(),
        ReferenceValue::Table(table) => table.range.to_string(),
    }
}

#[test]
fn named_range_is_grid_backed_and_whole_range_consumers_read_cells() {
    let mut engine = PocEngine::new();
    cell(&mut engine, "Inputs", 1, 1, 1.0);
    cell(&mut engine, "Inputs", 2, 1, 2.0);
    cell(&mut engine, "Inputs", 3, 1, 3.0);
    engine.define_name(
        "Data",
        NameScope::Workbook,
        None,
        NameDefinition::Range(RangeDescriptor::new("Inputs", 1, 1, 3, 1)),
    );
    let result = engine
        .set_formula_text("Output", 1, 1, "=SUM(Data)")
        .unwrap();

    let report = engine.calculate_all().unwrap();
    assert_eq!(number(&engine, &result), 6.0);
    assert!(
        engine
            .static_dependencies(&result)
            .contains(&DependencyDescriptor::Name(NameId(0)))
    );
    assert!(engine.static_dependencies(&result).iter().any(
        |dependency| matches!(dependency, DependencyDescriptor::Range(range) if range.area() == 3)
    ));
    assert!(
        report
            .trace
            .execution_reads
            .contains(&ExecutionRead::Cell(CellId::new("Inputs", 1, 1)))
    );
    assert_eq!(report.trace.range_cells_read, 3);
}

#[test]
fn index_keeps_source_invalidation_but_records_only_selected_target() {
    let mut engine = PocEngine::new();
    for row in 1..=3 {
        cell(&mut engine, "Inputs", row, 1, row as f64 * 10.0);
    }
    let selector = cell(&mut engine, "Selectors", 1, 1, 2.0);
    engine.define_name(
        "Data",
        NameScope::Workbook,
        None,
        NameDefinition::Range(RangeDescriptor::new("Inputs", 1, 1, 3, 1)),
    );
    let result = engine
        .set_formula_text("Output", 1, 1, "=INDEX(Data,Selectors!A1,1)")
        .unwrap();

    let first = engine.calculate_all().unwrap();
    assert_eq!(number(&engine, &result), 20.0);
    assert!(
        first
            .trace
            .execution_reads
            .contains(&ExecutionRead::Cell(CellId::new("Inputs", 2, 1)))
    );
    assert!(
        !first
            .trace
            .execution_reads
            .contains(&ExecutionRead::Cell(CellId::new("Inputs", 1, 1)))
    );
    assert!(
        !first
            .trace
            .execution_reads
            .contains(&ExecutionRead::Cell(CellId::new("Inputs", 3, 1)))
    );
    assert!(
        engine
            .static_dependencies(&result)
            .contains(&DependencyDescriptor::Selector(selector.clone()))
    );
    assert!(
        engine
            .static_dependencies(&result)
            .iter()
            .any(|dependency| matches!(dependency, DependencyDescriptor::Name(_)))
    );

    engine.set_cell_value("Selectors", 1, 1, LiteralValue::Number(3.0));
    let second = engine.calculate_all().unwrap();
    assert_eq!(number(&engine, &result), 30.0);
    assert_eq!(second.evaluated_cells, 1);
    assert!(
        second
            .trace
            .execution_reads
            .contains(&ExecutionRead::Cell(CellId::new("Inputs", 3, 1)))
    );

    cell(&mut engine, "Inputs", 1, 1, 999.0);
    let third = engine.calculate_all().unwrap();
    assert_eq!(number(&engine, &result), 30.0);
    assert_eq!(third.evaluated_cells, 1);
    assert!(
        third
            .trace
            .execution_reads
            .contains(&ExecutionRead::Cell(CellId::new("Inputs", 3, 1)))
    );
}

#[test]
fn selected_index_relations_scale_with_consumers_not_source_area() {
    let mut model = ShadowModel::new();
    for row in 1..=200 {
        model
            .add_formula_text("Output", row, 1, "=INDEX(Data,1,1)")
            .unwrap();
    }
    model.define_name(
        "Data",
        NameScope::Workbook,
        None,
        NameDefinition::Range(RangeDescriptor::new("Inputs", 1, 1, 10_000, 1)),
    );
    let metrics = model.build_metrics("index-scale");
    assert_eq!(metrics.formula_vertices, Some(200));
    assert_eq!(metrics.symbolic_range_descriptor_count, Some(200));
    assert_eq!(metrics.selector_descriptor_count, Some(0));
    assert!(metrics.persistent_relation_count.unwrap() < 1_000);
}

#[test]
fn inactive_if_branch_is_static_but_not_runtime_cycle() {
    let mut engine = PocEngine::new();
    engine.set_cell_value("Sheet1", 1, 1, LiteralValue::Boolean(true));
    let a2 = engine
        .set_formula_text("Sheet1", 2, 1, "=IF(A1,555,A3)")
        .unwrap();
    let a3 = engine
        .set_formula_text("Sheet1", 3, 1, "=IF(A1,A2,999)")
        .unwrap();

    let report = engine.calculate_all().unwrap();
    assert_eq!(number(&engine, &a2), 555.0);
    assert_eq!(number(&engine, &a3), 555.0);
    assert_eq!(report.static_cycle_count, 1);
    assert_eq!(report.static_cycle_members.len(), 2);
    assert_eq!(report.runtime_cycle_count, 0);
    assert!(report.trace.runtime_cycle_members.is_empty());
    assert!(
        !report
            .trace
            .execution_reads
            .contains(&ExecutionRead::Cell(CellId::new("Sheet1", 3, 1)))
    );
}

#[test]
fn reference_returning_if_and_choose_preserve_identity_for_sum() {
    let mut engine = PocEngine::new();
    engine.set_cell_value("Sheet1", 1, 1, LiteralValue::Boolean(true));
    cell(&mut engine, "Sheet1", 1, 2, 4.0);
    cell(&mut engine, "Sheet1", 1, 4, 7.0);
    let if_result = engine
        .set_formula_text("Sheet1", 1, 5, "=SUM(IF(A1,B1,D1))")
        .unwrap();
    let choose_result = engine
        .set_formula_text("Sheet1", 2, 5, "=SUM(CHOOSE(1,B1,D1))")
        .unwrap();

    let report = engine.calculate_all().unwrap();
    assert_eq!(number(&engine, &if_result), 4.0);
    assert_eq!(number(&engine, &choose_result), 4.0);
    assert!(
        report
            .trace
            .execution_reads
            .contains(&ExecutionRead::Cell(CellId::new("Sheet1", 1, 2)))
    );
    assert!(
        !report
            .trace
            .execution_reads
            .contains(&ExecutionRead::Cell(CellId::new("Sheet1", 1, 4)))
    );
}

#[test]
fn dynamic_offset_and_indirect_targets_are_replanned() {
    let mut engine = PocEngine::new();
    cell(&mut engine, "Sheet1", 1, 1, 10.0);
    cell(&mut engine, "Sheet1", 1, 2, 20.0);
    cell(&mut engine, "Sheet1", 1, 3, 1.0);
    let offset = engine
        .set_formula_text("Sheet1", 2, 1, "=SUM(OFFSET(A1,0,C1))")
        .unwrap();
    engine.set_cell_value("Sheet1", 3, 1, LiteralValue::Text("B1".to_string()));
    engine.set_cell_value("Sheet1", 5, 1, LiteralValue::Text("B1".to_string()));
    let indirect = engine
        .set_formula_text("Sheet1", 4, 2, "=INDIRECT(A5)")
        .unwrap();

    let first = engine.calculate_all().unwrap();
    assert_eq!(number(&engine, &offset), 20.0);
    assert_eq!(number(&engine, &indirect), 20.0);
    assert!(first.trace.dynamic_reads >= 2);
    assert!(
        first
            .trace
            .execution_reads
            .contains(&ExecutionRead::Cell(CellId::new("Sheet1", 1, 2)))
    );

    engine.set_cell_value("Sheet1", 1, 3, LiteralValue::Number(0.0));
    engine.set_cell_value("Sheet1", 5, 1, LiteralValue::Text("A1".to_string()));
    let second = engine.calculate_all().unwrap();
    assert_eq!(number(&engine, &offset), 10.0);
    assert_eq!(number(&engine, &indirect), 10.0);
    assert!(
        second
            .trace
            .execution_reads
            .contains(&ExecutionRead::Cell(CellId::new("Sheet1", 1, 1)))
    );
}

#[test]
fn name_scope_constant_formula_and_structural_generation_are_explicit() {
    let mut engine = PocEngine::new();
    cell(&mut engine, "Sheet1", 1, 1, 10.0);
    cell(&mut engine, "Sheet1", 1, 2, 20.0);
    engine.define_name(
        "Value",
        NameScope::Workbook,
        None,
        NameDefinition::Constant(LiteralValue::Number(3.0)),
    );
    let formula_name = engine.define_name(
        "Derived",
        NameScope::Workbook,
        None,
        NameDefinition::Formula {
            ast: parse("=A1+1").unwrap(),
        },
    );
    engine.define_name(
        "Choice",
        NameScope::Workbook,
        None,
        NameDefinition::Cell(CellId::new("Sheet1", 1, 1)),
    );
    engine.define_name(
        "Choice",
        NameScope::Sheet,
        Some("Sheet2".to_string()),
        NameDefinition::Cell(CellId::new("Sheet1", 1, 2)),
    );
    let value = engine
        .set_formula_text("Sheet1", 2, 1, "=Value+Derived")
        .unwrap();
    let local = engine.set_formula_text("Sheet2", 1, 1, "=Choice").unwrap();
    let global = engine.set_formula_text("Sheet1", 3, 1, "=Choice").unwrap();
    engine.define_name(
        "Window",
        NameScope::Workbook,
        None,
        NameDefinition::Range(RangeDescriptor::new("Sheet1", 1, 1, 2, 1)),
    );
    let sum = engine
        .set_formula_text("Sheet1", 4, 1, "=SUM(Window)")
        .unwrap();
    engine.calculate_all().unwrap();
    assert_eq!(number(&engine, &value), 14.0);
    assert_eq!(number(&engine, &local), 20.0);
    assert_eq!(number(&engine, &global), 10.0);
    assert_eq!(
        engine.names().get(&formula_name).unwrap().resolved_kind,
        ResolvedKind::Formula
    );

    engine.insert_rows("Sheet1", 2, 1);
    let record = engine.names().resolve("Window", "Sheet1").unwrap();
    assert!(record.structural_generation > 0);
    engine.calculate_all().unwrap();
    assert_eq!(number(&engine, &CellId::new("Sheet1", 5, 1)), 24.0);
    assert_eq!(sum, CellId::new("Sheet1", 4, 1));
}

#[test]
fn spill_shape_is_an_invalidation_surface_not_a_copied_table() {
    let mut engine = PocEngine::new();
    cell(&mut engine, "Sheet1", 1, 1, 2.0);
    cell(&mut engine, "Sheet1", 2, 1, 3.0);
    let spill = SpillRef {
        anchor: CellId::new("Sheet1", 1, 1),
        rows: 2,
        cols: 1,
    };
    engine.set_spill(spill.clone());
    engine.define_name(
        "Spilled",
        NameScope::Workbook,
        None,
        NameDefinition::Spill(spill.clone()),
    );
    let result = engine
        .set_formula_text("Sheet1", 1, 3, "=SUM(Spilled)")
        .unwrap();
    engine.calculate_all().unwrap();
    assert_eq!(number(&engine, &result), 5.0);

    cell(&mut engine, "Sheet1", 3, 1, 4.0);
    engine.set_spill(SpillRef { rows: 3, ..spill });
    let report = engine.calculate_all().unwrap();
    assert_eq!(number(&engine, &result), 9.0);
    assert!(
        report
            .trace
            .execution_reads
            .contains(&ExecutionRead::Spill(SpillRef {
                anchor: CellId::new("Sheet1", 1, 1),
                rows: 3,
                cols: 1,
            }))
    );
}

#[test]
fn runtime_cycle_workspace_is_created_from_actual_feedback_and_iterated() {
    let mut engine = PocEngine::new().with_iteration(64);
    let a = engine
        .set_formula_text("Inputs", 1, 1, "='Engine'!A1+1")
        .unwrap();
    let b = engine
        .set_formula_text("Engine", 1, 1, "='Inputs'!A1/2")
        .unwrap();

    let report = engine.calculate_all().unwrap();
    assert_eq!(report.static_cycle_count, 1);
    assert_eq!(report.runtime_cycle_count, 1);
    assert_eq!(report.runtime_cycle_members.len(), 2);
    assert_eq!(report.cyclic_workspaces[0].len(), 2);
    assert!(report.solver_passes > 1);
    assert!((number(&engine, &a) - 2.0).abs() < 0.001);
    assert!((number(&engine, &b) - 1.0).abs() < 0.001);
}

#[test]
fn multiple_independent_cycles_remain_separate_workspaces() {
    let mut engine = PocEngine::new().with_iteration(8);
    engine.set_formula_text("Sheet1", 1, 1, "=B1+1").unwrap();
    engine.set_formula_text("Sheet1", 1, 2, "=A1/2").unwrap();
    engine.set_formula_text("Sheet2", 1, 1, "=B1+2").unwrap();
    engine.set_formula_text("Sheet2", 1, 2, "=A1/2").unwrap();

    let report = engine.calculate_all().unwrap();
    assert_eq!(report.runtime_cycle_count, 2);
    assert_eq!(
        report.cyclic_workspaces.iter().map(Vec::len).sum::<usize>(),
        4
    );
}

#[test]
fn cycle_error_mode_is_explicit_and_does_not_change_static_routing() {
    let mut engine = PocEngine::new().with_cycle_error();
    let a = engine.set_formula_text("Sheet1", 1, 1, "=B1+1").unwrap();
    engine.set_formula_text("Sheet1", 1, 2, "=A1/2").unwrap();
    let report = engine.calculate_all().unwrap();
    assert_eq!(report.runtime_cycle_count, 1);
    assert!(
        matches!(engine.get_cell_value(&a), LiteralValue::Error(error) if error.kind == ExcelErrorKind::Circ)
    );
}

#[test]
fn artifact_shadow_is_honest_about_missing_workbooks_and_legacy_runtime_edges() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .unwrap();
    let report = build_artifact_shadow_report(root).unwrap();
    assert!(report.artifact_backed);
    assert!(!report.heavy_workbook_found);
    assert!(!report.light_workbook_found);
    assert_eq!(report.heavy.static_cycle_candidate_size, Some(4829));
    assert_eq!(report.heavy.legacy_runtime_cycle_size, Some(4142));
    assert!(
        report
            .heavy
            .notes
            .iter()
            .any(|note| note.contains("legacy observations"))
    );
    assert!(
        report
            .limitations
            .iter()
            .any(|limitation| limitation.contains("not present"))
    );
}

#[test]
fn unsupported_function_fails_explicitly() {
    let mut engine = PocEngine::new();
    let result = engine.set_formula_text("Sheet1", 1, 1, "=RAND()").unwrap();
    let report = engine.calculate_all().unwrap();
    assert!(matches!(
        engine.get_cell_value(&result),
        LiteralValue::Error(error) if error.kind == ExcelErrorKind::NImpl
    ));
    assert_eq!(report.unsupported_formula_count, 1);
    assert_eq!(engine.formula_count(), 1);
    assert_eq!(result, CellId::new("Sheet1", 1, 1));
}

#[test]
fn iferror_catches_evaluator_errors_as_fallback_values() {
    let mut engine = PocEngine::new();
    let output = engine
        .set_formula_text("Sheet1", 1, 1, "=IFERROR(1/0,\"\")")
        .unwrap();
    engine.calculate_all().unwrap();
    assert_eq!(
        engine.get_cell_value(&output),
        LiteralValue::Text(String::new())
    );
}

#[test]
fn edate_accepts_date_values_and_returns_calendar_months() {
    let mut engine = PocEngine::new();
    engine.set_cell_value(
        "Sheet1",
        1,
        1,
        LiteralValue::Date(chrono::NaiveDate::from_ymd_opt(2032, 6, 1).unwrap()),
    );
    let output = engine
        .set_formula_text("Sheet1", 1, 2, "=EDATE(A1,-78)")
        .unwrap();
    engine.calculate_all().unwrap();
    assert_eq!(
        engine.get_cell_value(&output),
        LiteralValue::Date(chrono::NaiveDate::from_ymd_opt(2025, 12, 1).unwrap())
    );
}

#[test]
fn vlookup_supports_exact_table_lookup() {
    let mut engine = PocEngine::new();
    engine.set_cell_value("Sheet1", 1, 1, LiteralValue::Text("a".to_string()));
    engine.set_cell_value("Sheet1", 1, 2, LiteralValue::Number(10.0));
    engine.set_cell_value("Sheet1", 2, 1, LiteralValue::Text("b".to_string()));
    engine.set_cell_value("Sheet1", 2, 2, LiteralValue::Number(20.0));
    engine.set_cell_value("Sheet1", 3, 1, LiteralValue::Text("b".to_string()));
    let output = engine
        .set_formula_text("Sheet1", 3, 2, "=VLOOKUP(A3,A1:B2,2,FALSE)")
        .unwrap();
    engine.calculate_all().unwrap();
    assert_eq!(number(&engine, &output), 20.0);
}

#[test]
fn substitute_supports_all_and_instance_replacement() {
    let mut engine = PocEngine::new();
    engine.set_cell_value("Sheet1", 1, 1, LiteralValue::Text("A-B-A".to_string()));
    let all = engine
        .set_formula_text("Sheet1", 1, 2, "=SUBSTITUTE(A1,\"-\",\"_\")")
        .unwrap();
    let instance = engine
        .set_formula_text("Sheet1", 1, 3, "=SUBSTITUTE(A1,\"A\",\"X\",2)")
        .unwrap();
    let column_label = engine
        .set_formula_text(
            "Sheet1",
            1,
            11,
            "=SUBSTITUTE(ADDRESS(1,COLUMN(),4),\"1\",\"\")",
        )
        .unwrap();
    engine.calculate_all().unwrap();
    assert_eq!(
        engine.get_cell_value(&all),
        LiteralValue::Text("A_B_A".to_string())
    );
    assert_eq!(
        engine.get_cell_value(&instance),
        LiteralValue::Text("A-B-X".to_string())
    );
    assert_eq!(
        engine.get_cell_value(&column_label),
        LiteralValue::Text("K".to_string())
    );
}

#[test]
fn substitute_fix_preserves_index_selected_reference_identity() {
    let mut engine = PocEngine::new();
    engine.set_cell_value("Inputs", 1, 1, LiteralValue::Text("r1".to_string()));
    engine.set_cell_value("Inputs", 2, 1, LiteralValue::Text("r2".to_string()));
    engine.set_cell_value("Inputs", 1, 2, LiteralValue::Text("c1".to_string()));
    engine.set_cell_value("Inputs", 1, 3, LiteralValue::Text("c2".to_string()));
    engine.set_cell_value("Inputs", 2, 2, LiteralValue::Number(11.0));
    engine.set_cell_value("Inputs", 2, 3, LiteralValue::Number(22.0));
    engine.set_cell_value("Inputs", 3, 2, LiteralValue::Number(33.0));
    engine.set_cell_value("Inputs", 3, 3, LiteralValue::Number(44.0));
    engine.define_name(
        "Matrix",
        NameScope::Workbook,
        None,
        NameDefinition::Range(RangeDescriptor::new("Inputs", 2, 2, 3, 3)),
    );
    engine.define_name(
        "Rows",
        NameScope::Workbook,
        None,
        NameDefinition::Range(RangeDescriptor::new("Inputs", 1, 1, 2, 1)),
    );
    engine.define_name(
        "Cols",
        NameScope::Workbook,
        None,
        NameDefinition::Range(RangeDescriptor::new("Inputs", 1, 2, 1, 3)),
    );
    let output = engine
        .set_formula_text(
            "Output",
            1,
            1,
            "=INDEX(Matrix,MATCH(SUBSTITUTE(\"r2-x\",\"-x\",\"\"),Rows,0),MATCH(SUBSTITUTE(\"c2-x\",\"-x\",\"\"),Cols,0))",
        )
        .unwrap();
    let report = engine.calculate_all().unwrap();
    assert_eq!(number(&engine, &output), 44.0);
    assert!(
        report
            .trace
            .execution_reads
            .contains(&ExecutionRead::Cell(CellId::new("Inputs", 3, 3)))
    );
    assert_eq!(
        report
            .trace
            .execution_reads
            .iter()
            .filter(|read| matches!(read, ExecutionRead::Cell(CellId { row: 3, col: 3, .. })))
            .count(),
        1
    );
}

#[test]
fn heavy_witness_j11_keeps_j9_reference_after_semantic_fixes() {
    let mut engine = PocEngine::new().with_formula_read_tracking(true);
    engine.set_cell_value(
        "CashFlow Inputs",
        9,
        2,
        LiteralValue::Text("F_CF_6".to_string()),
    );
    engine.set_cell_value(
        "CashFlow Inputs",
        6,
        10,
        LiteralValue::Text("J".to_string()),
    );
    engine.set_cell_value(
        "CashFlow Inputs",
        9,
        10,
        LiteralValue::Text("SC".to_string()),
    );
    engine.define_name(
        "Cash_Flow_Inputs",
        NameScope::Workbook,
        None,
        NameDefinition::Range(RangeDescriptor::new("CashFlow Inputs", 1, 1, 995, 702)),
    );
    engine.define_name(
        "Cash_Flow_Inputs_R",
        NameScope::Workbook,
        None,
        NameDefinition::Range(RangeDescriptor::new("CashFlow Inputs", 1, 2, 1_048_576, 2)),
    );
    engine.define_name(
        "Cash_Flow_Inputs_C",
        NameScope::Workbook,
        None,
        NameDefinition::Range(RangeDescriptor::new("CashFlow Inputs", 6, 1, 6, 20)),
    );
    engine.set_cell_value(
        "CashFlow Engine",
        11,
        3,
        LiteralValue::Text("F_CF_6".to_string()),
    );
    engine.set_cell_value(
        "CashFlow Engine",
        6,
        10,
        LiteralValue::Text("J".to_string()),
    );
    let j11 = engine
        .set_formula_text(
            "CashFlow Engine",
            11,
            10,
            "=INDEX(Cash_Flow_Inputs,MATCH($C11,Cash_Flow_Inputs_R,0),MATCH($J$6,Cash_Flow_Inputs_C,0))",
        )
        .unwrap();
    let report = engine.calculate_all().unwrap();
    assert_eq!(
        engine.get_cell_value(&j11),
        LiteralValue::Text("SC".to_string())
    );
    assert_eq!(
        report
            .trace
            .formula_read_traces
            .get(&j11)
            .and_then(|trace| trace.selected_references.iter().next())
            .map(reference_to_string_for_test),
        Some("CashFlow Inputs!J9".to_string())
    );
}

#[test]
fn runtime_trace_edges_are_exact_for_selected_targets() {
    let mut engine = PocEngine::new();
    cell(&mut engine, "Sheet1", 1, 1, 1.0);
    cell(&mut engine, "Sheet1", 1, 2, 2.0);
    engine.define_name(
        "Data",
        NameScope::Workbook,
        None,
        NameDefinition::Range(RangeDescriptor::new("Sheet1", 1, 1, 1, 2)),
    );
    let output = engine
        .set_formula_text("Sheet1", 2, 1, "=INDEX(Data,1,2)")
        .unwrap();
    let report = engine.calculate_all().unwrap();
    assert_eq!(number(&engine, &output), 2.0);
    let expected = BTreeSet::from([(output.clone(), CellId::new("Sheet1", 1, 2))]);
    assert_eq!(report.trace.runtime_edges, expected);
    assert!(report.trace.runtime_cycle_edges.is_empty());
}

#[test]
fn name_definition_change_invalidates_users_without_copying_values() {
    let mut engine = PocEngine::new();
    cell(&mut engine, "Sheet1", 1, 1, 1.0);
    cell(&mut engine, "Sheet1", 1, 2, 2.0);
    let name = engine.define_name(
        "Data",
        NameScope::Workbook,
        None,
        NameDefinition::Range(RangeDescriptor::new("Sheet1", 1, 1, 1, 1)),
    );
    let output = engine
        .set_formula_text("Sheet1", 2, 1, "=SUM(Data)")
        .unwrap();
    engine.calculate_all().unwrap();
    assert_eq!(number(&engine, &output), 1.0);

    engine.define_name(
        "Data",
        NameScope::Workbook,
        None,
        NameDefinition::Range(RangeDescriptor::new("Sheet1", 1, 2, 1, 2)),
    );
    let report = engine.calculate_all().unwrap();
    assert_eq!(number(&engine, &output), 2.0);
    assert_eq!(report.evaluated_cells, 1);
    assert_eq!(engine.names().get(&name).unwrap().definition_generation, 2);
}

#[test]
fn index_zero_dimension_returns_reference_ranges() {
    let mut engine = PocEngine::new();
    for (row, left, right) in [(1, 1.0, 2.0), (2, 3.0, 4.0)] {
        engine.set_cell_value("Sheet1", row, 1, LiteralValue::Number(left));
        engine.set_cell_value("Sheet1", row, 2, LiteralValue::Number(right));
    }
    let column = engine
        .set_formula_text("Sheet1", 3, 1, "=SUM(INDEX(A1:B2,0,2))")
        .unwrap();
    let row = engine
        .set_formula_text("Sheet1", 3, 2, "=SUM(INDEX(A1:B2,1,0))")
        .unwrap();
    engine.calculate_all().unwrap();
    assert_eq!(number(&engine, &column), 6.0);
    assert_eq!(number(&engine, &row), 3.0);
}

#[test]
fn index_match_preserves_selector_and_selected_target_layers() {
    let mut engine = PocEngine::new();
    engine.set_cell_value("Selectors", 1, 1, LiteralValue::Text("r2".to_string()));
    engine.set_cell_value("Selectors", 1, 2, LiteralValue::Text("c2".to_string()));
    for (row, label) in [(1, "r1"), (2, "r2")] {
        engine.set_cell_value("Inputs", row, 1, LiteralValue::Text(label.to_string()));
    }
    for (col, label) in [(2, "c1"), (3, "c2")] {
        engine.set_cell_value("Inputs", 1, col, LiteralValue::Text(label.to_string()));
    }
    engine.set_cell_value("Inputs", 2, 2, LiteralValue::Number(11.0));
    engine.set_cell_value("Inputs", 2, 3, LiteralValue::Number(22.0));
    engine.define_name(
        "Matrix",
        NameScope::Workbook,
        None,
        NameDefinition::Range(RangeDescriptor::new("Inputs", 1, 2, 2, 3)),
    );
    engine.define_name(
        "Rows",
        NameScope::Workbook,
        None,
        NameDefinition::Range(RangeDescriptor::new("Inputs", 1, 1, 2, 1)),
    );
    engine.define_name(
        "Cols",
        NameScope::Workbook,
        None,
        NameDefinition::Range(RangeDescriptor::new("Inputs", 1, 2, 1, 3)),
    );
    let output = engine
        .set_formula_text(
            "Output",
            1,
            1,
            "=INDEX(Matrix,MATCH(Selectors!A1,Rows,0),MATCH(Selectors!B1,Cols,0))",
        )
        .unwrap();
    let report = engine.calculate_all().unwrap();
    assert_eq!(number(&engine, &output), 22.0);
    assert!(
        report
            .trace
            .execution_reads
            .contains(&ExecutionRead::Cell(CellId::new("Inputs", 2, 3)))
    );
    assert!(
        !report
            .trace
            .execution_reads
            .contains(&ExecutionRead::Cell(CellId::new("Inputs", 2, 2)))
    );
    assert!(
        engine
            .static_dependencies(&output)
            .iter()
            .filter(|dependency| matches!(dependency, DependencyDescriptor::Range(_)))
            .count()
            >= 3
    );
}

#[test]
fn demand_scheduler_skips_clean_noop_and_unrelated_edits() {
    let mut engine = PocEngine::new();
    cell(&mut engine, "Inputs", 1, 1, 4.0);
    let output = engine
        .set_formula_text("Output", 1, 1, "=Inputs!A1+2")
        .unwrap();
    let initial = engine.calculate_all().unwrap();
    assert_eq!(initial.evaluated_cells, 1);
    let noop = engine.calculate_all().unwrap();
    assert_eq!(noop.evaluated_cells, 0);
    engine.set_cell_value("Inputs", 1, 2, LiteralValue::Number(99.0));
    let unrelated = engine.calculate_all().unwrap();
    assert_eq!(unrelated.evaluated_cells, 0);
    assert_eq!(number(&engine, &output), 6.0);
}

#[test]
fn heavy_representative_slice_has_broad_static_surface_and_four_cell_runtime_cycle() {
    let mut engine = PocEngine::new().with_iteration(16);
    let source_range_end = 4_822;
    engine.define_name(
        "Cash_Flow_Inputs",
        NameScope::Workbook,
        None,
        NameDefinition::Range(RangeDescriptor::new(
            "CashFlow Inputs",
            23,
            10,
            source_range_end,
            10,
        )),
    );
    engine
        .set_formula_text("CashFlow Inputs", 23, 10, "=SUM('CashFlow Engine'!K65)")
        .unwrap();
    for row in 24..=source_range_end {
        engine
            .set_formula_text("CashFlow Inputs", row, 10, "=IF(FALSE,J23,0)")
            .unwrap();
    }
    engine
        .set_formula_text("CashFlow Engine", 65, 11, "=I65")
        .unwrap();
    engine
        .set_formula_text("CashFlow Engine", 65, 9, "=J11")
        .unwrap();
    let j11 = engine
        .set_formula_text("CashFlow Engine", 11, 10, "=INDEX(Cash_Flow_Inputs,1,1)")
        .unwrap();

    let report = engine.calculate_all().unwrap();
    let largest_static = report.static_cycle_members.len();
    assert!(largest_static >= 4_800);
    assert_eq!(report.runtime_cycle_count, 1);
    assert_eq!(report.runtime_cycle_members.len(), 4);
    assert_eq!(report.cyclic_workspaces[0].len(), 4);
    assert!(
        report
            .trace
            .runtime_edges
            .contains(&(j11, CellId::new("CashFlow Inputs", 23, 10)))
    );
    assert!(
        !report
            .trace
            .execution_reads
            .contains(&ExecutionRead::Cell(CellId::new("CashFlow Inputs", 24, 10)))
    );
}

#[test]
fn diagnostic_trace_limit_does_not_change_complete_runtime_cycle_graph() {
    fn run(limit: usize) -> ScheduleReport {
        let mut engine = PocEngine::new()
            .with_iteration(4)
            .with_diagnostic_trace_limit(limit);
        engine.set_formula_text("Sheet1", 1, 1, "=B1+1").unwrap();
        engine.set_formula_text("Sheet1", 1, 2, "=A1/2").unwrap();
        engine.calculate_all().unwrap()
    }

    let full = run(100_000);
    let reduced = run(1);
    assert_eq!(full.runtime_cycle_count, reduced.runtime_cycle_count);
    assert_eq!(full.runtime_cycle_members, reduced.runtime_cycle_members);
    assert_eq!(
        full.trace.runtime_formula_edges,
        reduced.trace.runtime_formula_edges
    );
    assert!(reduced.trace.runtime_edges_truncated);
    assert!(reduced.trace.diagnostic_edge_records_dropped > 0);
}

#[test]
fn xlsx_shadow_adapter_reads_a_repository_fixture_without_v1_graph_building() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .unwrap()
        .join("crates")
        .join("formualizer-workbook")
        .join("tests")
        .join("fixtures")
        .join("issue162-failure.xlsx");
    let metrics = build_xlsx_shadow_metrics(path).unwrap();
    assert!(metrics.workbook_available);
    assert_eq!(metrics.input_source, "xlsx_calamine_plus_v2_shadow");
    assert!(metrics.formula_vertices.unwrap_or(0) > 0);
    assert!(metrics.graph_build_time_ms.unwrap_or(0.0) >= 0.0);
    assert!(metrics.memory_bytes_estimate.unwrap_or(0) > 0);
}
