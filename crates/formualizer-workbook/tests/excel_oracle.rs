use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use formualizer_common::{DateSystem, ExcelErrorKind, LiteralValue, parse_a1_1based};
use formualizer_eval::engine::{CycleDetection, CyclePolicy};
use formualizer_workbook::{
    CalamineAdapter, LoadStrategy, SpreadsheetReader, Workbook, WorkbookConfig,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

const CASE_SCHEMA: &str = "formualizer.excel-oracle.case/v1";
const SNAPSHOT_SCHEMA: &str = "formualizer.excel-oracle.snapshot/v1";

#[derive(Debug, Deserialize)]
struct OracleCase {
    schema: String,
    id: String,
    description: String,
    documentation: Vec<Documentation>,
    #[serde(default)]
    skip: Option<String>,
    workbook: OracleWorkbook,
    targets: Vec<String>,
    comparison: Comparison,
}

#[derive(Debug, Deserialize)]
struct Documentation {
    title: String,
    url: String,
}

#[derive(Debug, Deserialize)]
struct OracleWorkbook {
    file: String,
    date_system: String,
    calculation: Calculation,
}

#[derive(Debug, Deserialize, PartialEq)]
struct Calculation {
    iterate: bool,
    max_iterations: u32,
    max_change: f64,
}

#[derive(Debug, Deserialize)]
struct Comparison {
    absolute_tolerance: f64,
    relative_tolerance: f64,
}

#[derive(Debug, Deserialize)]
struct OracleSnapshot {
    schema: String,
    case_id: String,
    provenance: Provenance,
    results: BTreeMap<String, ExpectedValue>,
}

#[derive(Debug, Deserialize)]
struct Provenance {
    generated_at_utc: String,
    generator: String,
    excel_version: String,
    excel_file_version: String,
    excel_executable: String,
    culture: String,
    case_sha256: String,
    workbook_sha256: String,
    date_system: String,
    calculation: Calculation,
}

#[derive(Debug, Deserialize)]
struct ExpectedValue {
    kind: String,
    value: Option<serde_json::Value>,
    error: Option<String>,
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

fn discover_cases(root: &Path) -> Vec<PathBuf> {
    let mut cases = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let entries = fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("read {}: {error}", directory.display()));
        for entry in entries {
            let path = entry.expect("directory entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().is_some_and(|name| name == "case.json") {
                cases.push(path);
            }
        }
    }
    cases.sort();
    cases
}

fn sha256_file(path: &Path) -> String {
    let bytes = fs::read(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn parse_target(target: &str) -> (String, u32, u32) {
    let separator = target
        .rfind('!')
        .unwrap_or_else(|| panic!("target must be Sheet!A1: {target}"));
    let sheet = target[..separator].trim_matches('\'').replace("''", "'");
    let address = target[separator + 1..].replace('$', "");
    let (row, column, _, _) =
        parse_a1_1based(&address).unwrap_or_else(|error| panic!("parse {target}: {error}"));
    (sheet, row, column)
}

fn workbook_config(case: &OracleCase) -> WorkbookConfig {
    let mut config = WorkbookConfig::ephemeral();
    config.eval.defer_graph_building = false;
    if case.workbook.calculation.iterate {
        config.eval.cycle.detection = CycleDetection::Runtime;
        config.eval.cycle.policy = CyclePolicy::Iterate {
            max_iterations: case.workbook.calculation.max_iterations,
            max_change: case.workbook.calculation.max_change,
        };
    }
    config
}

fn assert_number(case: &OracleCase, target: &str, expected: f64, actual: &LiteralValue) {
    let date_system = match case.workbook.date_system.as_str() {
        "1900" => DateSystem::Excel1900,
        "1904" => DateSystem::Excel1904,
        other => panic!("{} uses unsupported date system {other}", case.id),
    };
    let actual = actual.as_serial_number_for(date_system).unwrap_or_else(|| {
        panic!(
            "{} {target}: expected number {expected}, got {actual:?}",
            case.id
        )
    });
    let difference = (actual - expected).abs();
    let scale = actual.abs().max(expected.abs());
    assert!(
        difference <= case.comparison.absolute_tolerance
            || difference <= case.comparison.relative_tolerance * scale,
        "{} {target}: expected {expected}, got {actual}; absolute difference {difference}",
        case.id
    );
}

fn assert_expected(
    case: &OracleCase,
    target: &str,
    expected: &ExpectedValue,
    actual: LiteralValue,
) {
    match expected.kind.as_str() {
        "number" => {
            let number = expected
                .value
                .as_ref()
                .and_then(serde_json::Value::as_f64)
                .unwrap_or_else(|| panic!("{} {target}: missing expected number", case.id));
            assert_number(case, target, number, &actual);
        }
        "text" => {
            let text = expected
                .value
                .as_ref()
                .and_then(serde_json::Value::as_str)
                .unwrap_or_else(|| panic!("{} {target}: missing expected text", case.id));
            assert_eq!(
                actual,
                LiteralValue::Text(text.to_string()),
                "{} {target}",
                case.id
            );
        }
        "boolean" => {
            let value = expected
                .value
                .as_ref()
                .and_then(serde_json::Value::as_bool)
                .unwrap_or_else(|| panic!("{} {target}: missing expected boolean", case.id));
            assert_eq!(actual, LiteralValue::Boolean(value), "{} {target}", case.id);
        }
        "empty" => assert_eq!(actual, LiteralValue::Empty, "{} {target}", case.id),
        "error" => {
            let label = expected
                .error
                .as_deref()
                .unwrap_or_else(|| panic!("{} {target}: missing expected error", case.id));
            let kind = ExcelErrorKind::try_parse(label)
                .unwrap_or_else(|| panic!("{} {target}: unsupported Excel error {label}", case.id));
            match actual {
                LiteralValue::Error(error) => assert_eq!(error.kind, kind, "{} {target}", case.id),
                other => panic!("{} {target}: expected {label}, got {other:?}", case.id),
            }
        }
        other => panic!("{} {target}: unsupported expected kind {other}", case.id),
    }
}

#[test]
fn excel_oracle_snapshots() {
    let root = repo_root().join("tests").join("excel-oracle");
    let cases = discover_cases(&root);
    assert!(
        !cases.is_empty(),
        "no Excel oracle cases under {}",
        root.display()
    );

    for case_path in cases {
        let case_directory = case_path.parent().expect("case directory");
        let case: OracleCase = serde_json::from_slice(
            &fs::read(&case_path)
                .unwrap_or_else(|error| panic!("read {}: {error}", case_path.display())),
        )
        .unwrap_or_else(|error| panic!("parse {}: {error}", case_path.display()));
        assert_eq!(case.schema, CASE_SCHEMA, "{} schema", case.id);
        assert!(
            !case.description.trim().is_empty(),
            "{} description",
            case.id
        );
        assert!(!case.documentation.is_empty(), "{} documentation", case.id);
        for source in &case.documentation {
            assert!(
                !source.title.trim().is_empty(),
                "{} documentation title",
                case.id
            );
            assert!(
                source.url.starts_with("https://"),
                "{} documentation URL must use HTTPS: {}",
                case.id,
                source.url
            );
        }

        let snapshot_path = case_directory.join("expected.excel.json");
        let snapshot: OracleSnapshot = serde_json::from_slice(
            &fs::read(&snapshot_path)
                .unwrap_or_else(|error| panic!("read {}: {error}", snapshot_path.display())),
        )
        .unwrap_or_else(|error| panic!("parse {}: {error}", snapshot_path.display()));
        assert_eq!(
            snapshot.schema, SNAPSHOT_SCHEMA,
            "{} snapshot schema",
            case.id
        );
        assert_eq!(snapshot.case_id, case.id, "{} snapshot id", case.id);
        assert_eq!(
            snapshot.provenance.case_sha256,
            sha256_file(&case_path),
            "{} case changed without an Excel oracle refresh",
            case.id
        );
        let workbook_path = case_directory.join(&case.workbook.file);
        assert_eq!(
            snapshot.provenance.workbook_sha256,
            sha256_file(&workbook_path),
            "{} workbook changed without an Excel oracle refresh",
            case.id
        );
        assert_eq!(snapshot.provenance.date_system, case.workbook.date_system);
        assert_eq!(
            snapshot.provenance.calculation, case.workbook.calculation,
            "{} calculation settings",
            case.id
        );
        assert!(!snapshot.provenance.generated_at_utc.is_empty());
        assert!(!snapshot.provenance.generator.is_empty());
        assert!(!snapshot.provenance.excel_version.is_empty());
        assert!(!snapshot.provenance.excel_file_version.is_empty());
        assert!(!snapshot.provenance.excel_executable.is_empty());
        assert!(!snapshot.provenance.culture.is_empty());
        assert_eq!(
            snapshot.results.len(),
            case.targets.len(),
            "{} targets",
            case.id
        );

        if let Some(reason) = &case.skip {
            eprintln!("[excel-oracle] skip {}: {reason}", case.id);
            continue;
        }

        let adapter = CalamineAdapter::open_path(&workbook_path)
            .unwrap_or_else(|error| panic!("open {}: {error}", workbook_path.display()));
        let mut workbook =
            Workbook::from_reader(adapter, LoadStrategy::EagerAll, workbook_config(&case))
                .unwrap_or_else(|error| panic!("load {}: {error}", case.id));
        workbook
            .evaluate_all()
            .unwrap_or_else(|error| panic!("evaluate {}: {error}", case.id));

        for target in &case.targets {
            let expected = snapshot
                .results
                .get(target)
                .unwrap_or_else(|| panic!("{} missing Excel result for {target}", case.id));
            let (sheet, row, column) = parse_target(target);
            let actual = workbook
                .get_value(&sheet, row, column)
                .unwrap_or(LiteralValue::Empty);
            assert_expected(&case, target, expected, actual);
        }
    }
}
