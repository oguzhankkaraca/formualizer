use std::path::PathBuf;

use formualizer_common::LiteralValue;
use formualizer_workbook::{
    CalamineAdapter, LoadStrategy, SpreadsheetReader, Workbook, WorkbookConfig,
};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("repository root")
        .join("tests")
        .join("excel-oracle")
        .join("calamine-native-tables")
        .join("fixture.xlsx")
}

fn assert_reader_metadata(mut adapter: CalamineAdapter) {
    assert!(adapter.capabilities().tables);
    let data = adapter.read_sheet("Data").expect("read Data");
    assert_eq!(data.tables.len(), 1);
    let table = &data.tables[0];
    assert_eq!(table.name, "SalesTable");
    assert_eq!(table.range, (1, 1, 4, 2));
    assert!(table.header_row);
    assert_eq!(table.headers, ["Key", "Amount"]);
    assert!(!table.totals_row);
}

fn assert_defined_name_metadata(mut adapter: CalamineAdapter) {
    let names = adapter.defined_names().expect("read defined names");
    let derived_name = names
        .iter()
        .find(|name| name.name == "DerivedAmounts")
        .expect("DerivedAmounts");
    assert!(matches!(
        &derived_name.definition,
        formualizer_workbook::DefinedNameDefinition::Formula { formula }
            if formula == "=SUM(SalesAmounts)"
    ));

    let workbook_name = names
        .iter()
        .find(|name| name.name == "SalesAmounts")
        .expect("SalesAmounts");
    assert_eq!(
        workbook_name.scope,
        formualizer_workbook::DefinedNameScope::Workbook
    );
    assert!(workbook_name.scope_sheet.is_none());
    assert!(matches!(
        &workbook_name.definition,
        formualizer_workbook::DefinedNameDefinition::Formula { formula }
            if formula == "=SalesTable[Amount]"
    ));

    let local_name = names
        .iter()
        .find(|name| name.name == "LocalAmounts")
        .expect("LocalAmounts");
    assert_eq!(
        local_name.scope,
        formualizer_workbook::DefinedNameScope::Sheet
    );
    assert_eq!(local_name.scope_sheet.as_deref(), Some("Data"));
    assert!(matches!(
        &local_name.definition,
        formualizer_workbook::DefinedNameDefinition::Formula { formula }
            if formula == "=SalesTable[Amount]"
    ));

    let all_name = names
        .iter()
        .find(|name| name.name == "SalesAllAmounts")
        .expect("SalesAllAmounts");
    assert!(matches!(
        &all_name.definition,
        formualizer_workbook::DefinedNameDefinition::Formula { formula }
            if formula == "=SalesTable[[#All],[Amount]]"
    ));

    let coerced_name = names
        .iter()
        .find(|name| name.name == "CoercedAmounts")
        .expect("CoercedAmounts");
    assert!(matches!(
        &coerced_name.definition,
        formualizer_workbook::DefinedNameDefinition::Formula { formula }
            if formula == "=VALUE(SalesTable[Amount])"
    ));

    let scope_names: Vec<_> = names
        .iter()
        .filter(|name| name.name == "ScopeProbe")
        .collect();
    assert_eq!(scope_names.len(), 2);
    assert!(scope_names.iter().any(|name| {
        name.scope == formualizer_workbook::DefinedNameScope::Workbook && name.scope_sheet.is_none()
    }));
    assert!(scope_names.iter().any(|name| {
        name.scope == formualizer_workbook::DefinedNameScope::Sheet
            && name.scope_sheet.as_deref() == Some("Data")
    }));
}

fn assert_workbook_evaluation(adapter: CalamineAdapter) {
    let mut workbook =
        Workbook::from_reader(adapter, LoadStrategy::EagerAll, WorkbookConfig::ephemeral())
            .expect("load workbook");
    workbook.evaluate_all().expect("evaluate workbook");
    for row in 2..=6 {
        assert_eq!(
            workbook.get_value("Data", row, 4),
            Some(LiteralValue::Number(60.0)),
            "structured name result at row {row}"
        );
    }
    assert_eq!(
        workbook.get_value("Data", 7, 4),
        Some(LiteralValue::Error(formualizer_common::ExcelError::new(
            formualizer_common::ExcelErrorKind::Value,
        )))
    );
    for row in 8..=9 {
        assert_eq!(
            workbook.get_value("Data", row, 4),
            Some(LiteralValue::Number(3.0)),
            "VALUE/MATCH result at row {row}"
        );
    }
    assert_eq!(
        workbook.get_value("Data", 10, 4),
        Some(LiteralValue::Number(20.0))
    );
    assert_eq!(
        workbook.get_value("Data", 11, 4),
        Some(LiteralValue::Number(60.0))
    );
    assert_eq!(
        workbook.get_value("Other", 2, 4),
        Some(LiteralValue::Number(10.0))
    );
    let tables = workbook.tables();
    assert_eq!(tables.len(), 1);
    let table = &tables[0];
    assert_eq!(table.name, "SalesTable");
    assert_eq!(table.sheet, "Data");
    assert_eq!(
        (
            table.start_row,
            table.start_col,
            table.end_row,
            table.end_col,
        ),
        (1, 1, 4, 2)
    );
    assert_eq!(table.headers, ["Key", "Amount"]);
}

#[test]
fn calamine_native_tables_are_available_from_path_and_bytes() {
    let path = fixture();
    assert_reader_metadata(CalamineAdapter::open_path(&path).expect("open path"));
    let bytes = std::fs::read(&path).expect("read fixture");
    assert_reader_metadata(CalamineAdapter::open_bytes(bytes).expect("open bytes"));

    assert_defined_name_metadata(CalamineAdapter::open_path(&path).expect("open path"));
    let bytes = std::fs::read(path).expect("read fixture");
    assert_defined_name_metadata(CalamineAdapter::open_bytes(bytes).expect("open bytes"));
}

#[test]
fn calamine_registers_tables_before_formula_ingest_from_path_and_bytes() {
    let path = fixture();
    assert_workbook_evaluation(CalamineAdapter::open_path(&path).expect("open path"));
    let bytes = std::fs::read(path).expect("read fixture");
    assert_workbook_evaluation(CalamineAdapter::open_bytes(bytes).expect("open bytes"));
}

#[test]
fn table_updates_invalidate_formula_backed_names() {
    let path = fixture();
    let adapter = CalamineAdapter::open_path(&path).expect("open path");
    let mut workbook =
        Workbook::from_reader(adapter, LoadStrategy::EagerAll, WorkbookConfig::ephemeral())
            .expect("load workbook");
    workbook.evaluate_all().expect("initial evaluation");
    workbook
        .set_value("Data", 5, 1, LiteralValue::Text("D".to_string()))
        .expect("set new key");
    workbook
        .set_value("Data", 5, 2, LiteralValue::Number(40.0))
        .expect("set new amount");

    let sheet_id = workbook.engine().sheet_id("Data").expect("Data sheet");
    let range = formualizer_eval::reference::RangeRef::new(
        formualizer_eval::reference::CellRef::new(
            sheet_id,
            formualizer_eval::reference::Coord::new(0, 0, true, true),
        ),
        formualizer_eval::reference::CellRef::new(
            sheet_id,
            formualizer_eval::reference::Coord::new(4, 1, true, true),
        ),
    );
    workbook
        .engine_mut()
        .update_table(
            "SalesTable",
            range,
            true,
            vec!["Key".into(), "Amount".into()],
            false,
        )
        .expect("update table");
    workbook.evaluate_all().expect("updated evaluation");
    for row in 2..=6 {
        assert_eq!(
            workbook.get_value("Data", row, 4),
            Some(LiteralValue::Number(100.0)),
            "updated named formula result at row {row}"
        );
    }
    assert_eq!(
        workbook.get_value("Data", 11, 4),
        Some(LiteralValue::Number(100.0))
    );
}
