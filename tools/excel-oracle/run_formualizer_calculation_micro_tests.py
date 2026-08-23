from __future__ import annotations

import argparse
import json
import time
from pathlib import Path
from typing import Any

import formualizer as fz


def a1_coordinates(address: str) -> tuple[int, int]:
    column_text = "".join(character for character in address if character.isalpha())
    row_text = "".join(character for character in address if character.isdigit())
    column = 0
    for character in column_text.upper():
        column = column * 26 + ord(character) - ord("A") + 1
    return int(row_text), column


def json_value(value: Any) -> Any:
    if isinstance(value, (str, int, float, bool)) or value is None:
        return value
    if isinstance(value, list):
        return [json_value(item) for item in value]
    if isinstance(value, dict):
        return {str(key): json_value(item) for key, item in value.items()}
    return str(value)


def telemetry_snapshot(telemetry: Any, fields: tuple[str, ...]) -> dict[str, Any]:
    return {field: json_value(getattr(telemetry, field)) for field in fields}


def snapshot(workbook: Any, targets: list[str]) -> dict[str, Any]:
    result = {}
    for target in targets:
        separator = target.rfind("!")
        sheet = target[:separator]
        address = target[separator + 1:]
        row, column = a1_coordinates(address)
        result[target] = json_value(workbook.get_value(sheet, row, column))
    return result


def make_workbook(case: dict[str, Any], max_iterations: int) -> Any:
    evaluation = fz.EvaluationConfig()
    evaluation.cycle_detection = "runtime"
    evaluation.cycle_policy = "iterate"
    evaluation.iterate_max_iterations = max_iterations
    evaluation.iterate_max_change = 0.001
    workbook = fz.Workbook(config=fz.WorkbookConfig(eval_config=evaluation))
    sheets = list(case["sheets"])
    if sheets and sheets[0] != "Sheet1":
        workbook.add_sheet(sheets[0])
    for sheet in sheets:
        if sheet != "Sheet1" and sheet not in workbook.sheet_names:
            workbook.add_sheet(sheet)
    for sheet, cells in case["cells"].items():
        for address, value in cells.items():
            row, column = a1_coordinates(address)
            if isinstance(value, str) and value.startswith("="):
                workbook.set_formula(sheet, row, column, value)
            else:
                workbook.set_value(sheet, row, column, value)
    return workbook


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--runs", type=int, default=7)
    parser.add_argument("--max-iterations", type=int, nargs="+", default=[1, 2, 3, 5, 10, 20, 50, 100])
    args = parser.parse_args()
    manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
    results = []
    for case in manifest["cases"]:
        for max_iterations in args.max_iterations:
            for run in range(1, args.runs + 1):
                workbook = make_workbook(case, max_iterations)
                started = time.perf_counter()
                workbook.evaluate_all()
                initial_ms = (time.perf_counter() - started) * 1000
                initial_targets = snapshot(workbook, case["targets"])
                mutation_ms = None
                mutation_targets = None
                if case["mutations"]:
                    mutation = case["mutations"][0]
                    row, column = a1_coordinates(mutation["address"])
                    started = time.perf_counter()
                    workbook.set_value(mutation["sheet"], row, column, mutation["value"])
                    workbook.evaluate_all()
                    mutation_ms = (time.perf_counter() - started) * 1000
                    mutation_targets = snapshot(workbook, case["targets"])
                started = time.perf_counter()
                workbook.evaluate_all()
                noop_ms = (time.perf_counter() - started) * 1000
                results.append(
                    {
                        "case_id": case["id"],
                        "max_iterations": max_iterations,
                        "run": run,
                        "initial_calculate_ms": round(initial_ms, 3),
                        "mutation_calculate_ms": None if mutation_ms is None else round(mutation_ms, 3),
                        "noop_calculate_ms": round(noop_ms, 3),
                        "initial_targets": initial_targets,
                        "mutation_targets": mutation_targets,
                        "noop_targets": snapshot(workbook, case["targets"]),
                        "cycle_telemetry": telemetry_snapshot(
                            workbook.last_cycle_telemetry(),
                            (
                                "iterated_sccs",
                                "converged_sccs",
                                "capped_sccs",
                                "settle_passes_total",
                                "max_passes_single_scc",
                                "circ_cells_stamped",
                                "elapsed_ms",
                            ),
                        ),
                        "recalc_telemetry": telemetry_snapshot(
                            workbook.last_recalc_telemetry(),
                            (
                                "scc_tasks_evaluated",
                                "scc_member_count",
                                "scc_member_evaluations",
                                "scc_units_reused",
                                "scc_units_invalidated",
                                "volatile_vertices_redirtied",
                                "iterative_vertices_redirtied",
                            ),
                        ),
                    }
                )
    report = {
        "schema": "formualizer.formualizer-calculation-micro-results/v1",
        "manifest": str(args.manifest.resolve()),
        "runs": args.runs,
        "max_iterations": args.max_iterations,
        "configuration": {
            "cycle_detection": "runtime",
            "cycle_policy": "iterate",
            "max_change": 0.001,
        },
        "results": results,
    }
    args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(f"Generated {args.output}")


if __name__ == "__main__":
    main()
