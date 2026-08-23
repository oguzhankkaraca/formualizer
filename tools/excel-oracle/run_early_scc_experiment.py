from __future__ import annotations

import argparse
import json
import os
import time
from pathlib import Path
from typing import Any

import formualizer as fz


MAIN_SCC_ID = 1321560910633541638


def new_config() -> Any:
    evaluation = fz.EvaluationConfig()
    evaluation.enable_parallel = True
    evaluation.cycle_detection = "runtime"
    evaluation.cycle_policy = "iterate"
    evaluation.iterate_max_iterations = 100
    evaluation.iterate_max_change = 0.001
    return fz.WorkbookConfig(eval_config=evaluation)


def load_fossil(early: bool) -> Any:
    if early:
        os.environ["FZ_DIAGNOSTIC_EARLY_SCC_TERMINATION"] = "1"
    else:
        os.environ.pop("FZ_DIAGNOSTIC_EARLY_SCC_TERMINATION", None)
    return fz.Workbook.load_path(
        r"C:\Users\OXK0A0A\Downloads\Fossil_EstimatingTemplate_2026-08_21_A.xlsx",
        backend="calamine",
        config=new_config(),
    )


def run_fossil(early: bool) -> list[dict[str, Any]]:
    workbook = load_fossil(early)
    phases = []

    def calculate(label: str) -> None:
        started = time.perf_counter()
        workbook.evaluate_all()
        recalc = workbook.last_recalc_telemetry()
        records = [
            record
            for record in workbook.last_scc_early_termination()
            if record["stable_id"] == MAIN_SCC_ID
        ]
        phases.append(
            {
                "label": label,
                "wall_ms": round((time.perf_counter() - started) * 1000, 3),
                "formula_value_fingerprint": workbook.formula_value_fingerprint(),
                "main_early_termination": records,
                "scc_tasks": recalc.scc_tasks_evaluated,
                "scc_member_evaluations": recalc.scc_member_evaluations,
                "early_attempted": recalc.diagnostic_early_termination_attempted,
                "early_accepted": recalc.diagnostic_early_termination_accepted,
                "early_avoided_member_evaluations": recalc.diagnostic_early_termination_avoided_member_evaluations,
            }
        )

    calculate("initial")
    workbook.set_value("Inputs", 7, 6, 300)
    calculate("f7_300")
    calculate("noop")
    workbook.set_value("Inputs", 7, 6, 300)
    calculate("same_value_write")
    workbook.set_value("Inputs", 7, 6, 301)
    calculate("f7_301")
    workbook.set_value("Inputs", 7, 6, 300)
    calculate("f7_back_300")
    return phases


def run_micro(path: Path, early: bool) -> dict[str, Any]:
    if early:
        os.environ["FZ_DIAGNOSTIC_EARLY_SCC_TERMINATION"] = "1"
    else:
        os.environ.pop("FZ_DIAGNOSTIC_EARLY_SCC_TERMINATION", None)
    workbook = fz.Workbook.load_path(
        str(path), backend="calamine", config=new_config()
    )
    started = time.perf_counter()
    workbook.evaluate_all()
    initial_ms = (time.perf_counter() - started) * 1000
    initial_fp = workbook.formula_value_fingerprint()
    early_initial = workbook.last_scc_early_termination()
    started = time.perf_counter()
    workbook.evaluate_all()
    noop_ms = (time.perf_counter() - started) * 1000
    return {
        "initial_ms": round(initial_ms, 3),
        "noop_ms": round(noop_ms, 3),
        "initial_fingerprint": initial_fp,
        "noop_fingerprint": workbook.formula_value_fingerprint(),
        "early_records": workbook.last_scc_early_termination(),
        "initial_early_records": early_initial,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--micro-directory", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    micro = {
        case: {
            "normal": run_micro(args.micro_directory / f"{case}.xlsx", False),
            "early": run_micro(args.micro_directory / f"{case}.xlsx", True),
        }
        for case in [
            "unused-if-cycle",
            "active-if-cycle",
            "indirect-target-change",
            "offset-target-change",
            "filter-shape-change",
            "two-cell-cycle",
            "same-sheet-cycle",
            "cross-sheet-cycle",
        ]
    }
    fossil = {"normal": run_fossil(False), "early": run_fossil(True)}
    args.output.write_text(
        json.dumps(
            {
                "schema": "formualizer.early-scc-termination-experiment/v1",
                "micro": micro,
                "fossil": fossil,
                "named_range_definition_change": {
                    "control": "crates/formualizer-eval/src/engine/tests/scc_runtime_cycles.rs",
                    "status": "validated by native test with diagnostic env enabled",
                },
            },
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )
    os.environ.pop("FZ_DIAGNOSTIC_EARLY_SCC_TERMINATION", None)
    print(f"Generated {args.output}")


if __name__ == "__main__":
    main()
