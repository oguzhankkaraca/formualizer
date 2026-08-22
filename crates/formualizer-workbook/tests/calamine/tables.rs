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

fn assert_workbook_evaluation(adapter: CalamineAdapter) {
    let mut workbook =
        Workbook::from_reader(adapter, LoadStrategy::EagerAll, WorkbookConfig::ephemeral())
            .expect("load workbook");
    workbook.evaluate_all().expect("evaluate workbook");
    assert_eq!(
        workbook.get_value("Data", 2, 4),
        Some(LiteralValue::Number(60.0))
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
    let bytes = std::fs::read(path).expect("read fixture");
    assert_reader_metadata(CalamineAdapter::open_bytes(bytes).expect("open bytes"));
}

#[test]
fn calamine_registers_tables_before_formula_ingest_from_path_and_bytes() {
    let path = fixture();
    assert_workbook_evaluation(CalamineAdapter::open_path(&path).expect("open path"));
    let bytes = std::fs::read(path).expect("read fixture");
    assert_workbook_evaluation(CalamineAdapter::open_bytes(bytes).expect("open bytes"));
}
