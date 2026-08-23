from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any

import formualizer as fz


def config() -> Any:
    evaluation = fz.EvaluationConfig()
    evaluation.cycle_detection = "runtime"
    evaluation.cycle_policy = "iterate"
    evaluation.iterate_max_iterations = 100
    evaluation.iterate_max_change = 0.001
    return fz.WorkbookConfig(eval_config=evaluation)


def parity(workbook: Any, sheet: str, row: int, col: int) -> dict[str, Any]:
    return dict(workbook.compact_dirty_set_parity(sheet, row, col))


def run_fossil(path: str) -> dict[str, Any]:
    workbook = fz.Workbook.load_path(path, backend="calamine", config=config())
    workbook.evaluate_all()
    rows = {"after_initial": parity(workbook, "Inputs", 7, 6)}
    workbook.set_value("Inputs", 7, 6, 300)
    rows["after_f7_300_write"] = parity(workbook, "Inputs", 7, 6)
    workbook.set_value("Inputs", 7, 6, 300)
    rows["after_same_value_write"] = parity(workbook, "Inputs", 7, 6)
    workbook.set_value("Inputs", 7, 6, 301)
    rows["after_f7_301_write"] = parity(workbook, "Inputs", 7, 6)
    rows["unrelated_cell"] = parity(workbook, "Inputs", 58, 15)
    return rows


def run_micro(directory: Path) -> dict[str, Any]:
    controls = {
        "indirect_target": ("indirect-target-change", "Sheet1", 1, 4),
        "offset_target": ("offset-target-change", "Sheet1", 1, 2),
        "filter_shape": ("filter-shape-change", "Sheet1", 2, 2),
        "same_sheet_cycle": ("same-sheet-cycle", "Sheet1", 1, 1),
        "cross_sheet_cycle": ("cross-sheet-cycle", "Sheet1", 1, 1),
    }
    results = {}
    for label, (case, sheet, row, col) in controls.items():
        workbook = fz.Workbook.load_path(
            str(directory / f"{case}.xlsx"), backend="calamine", config=config()
        )
        workbook.evaluate_all()
        results[label] = {
            "before_write": parity(workbook, sheet, row, col),
        }
    return results


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--fossil", required=True)
    parser.add_argument("--micro-directory", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    result = {
        "schema": "formualizer.compact-dirty-parity-experiment/v1",
        "fossil": run_fossil(args.fossil),
        "micro": run_micro(args.micro_directory),
        "named_range_definition_change": {
            "control": "diagnostic_early_termination_rejects_named_definition_change",
            "status": "separate native graph mutation control",
        },
    }
    args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(f"Generated {args.output}")


if __name__ == "__main__":
    main()
