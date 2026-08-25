from __future__ import annotations

import hashlib
import json
import os
import time
from pathlib import Path
from typing import Any

os.environ["FZ_DIAGNOSTIC_EXACT_SCC_REUSE"] = "1"

import formualizer as fz

WORKBOOK_PATH = r"C:\Users\OXK0A0A\Downloads\Fossil_EstimatingTemplate_2026-08_21_A.xlsx"
MAIN_SCC_ID = 1321560910633541638


def config() -> Any:
    evaluation = fz.EvaluationConfig()
    evaluation.enable_parallel = True
    evaluation.cycle_detection = "runtime"
    evaluation.cycle_policy = "iterate"
    evaluation.iterate_max_iterations = 100
    evaluation.iterate_max_change = 0.001
    return fz.WorkbookConfig(eval_config=evaluation)


def output_digest(workbook: Any) -> str:
    values = dict(workbook.formula_output_snapshot())
    payload = json.dumps(values, sort_keys=True, default=str, separators=(",", ":"))
    return hashlib.sha256(payload.encode()).hexdigest()


def phase(workbook: Any, label: str) -> dict[str, Any]:
    started = time.perf_counter()
    workbook.evaluate_all()
    exact = [
        row
        for row in workbook.last_scc_exact_reuse()
        if row["stable_id"] == MAIN_SCC_ID
    ]
    recalc = workbook.last_recalc_telemetry()
    dirty = workbook.last_scc_dirty_telemetry()
    return {
        "label": label,
        "wall_ms": round((time.perf_counter() - started) * 1000, 3),
        "output_sha256": output_digest(workbook),
        "scc_tasks": recalc.scc_tasks_evaluated,
        "scc_member_evaluations": recalc.scc_member_evaluations,
        "main_exact": exact,
        "dirty_start": dirty["dirty_at_request_start"],
        "dirty_roots": dirty["dirty_root_sources"],
        "iterative_state_values": dirty["iterative_state_value_count"],
    }


def new_workbook() -> Any:
    return fz.Workbook.load_path(WORKBOOK_PATH, backend="calamine", config=config())


def run_100_noops() -> dict[str, Any]:
    workbook = new_workbook()
    phase(workbook, "initial")
    workbook.set_value("Inputs", 7, 6, 300)
    phase(workbook, "f7_edit")
    phases = [phase(workbook, f"noop_{index}") for index in range(1, 101)]
    output_hashes = {row["output_sha256"] for row in phases}
    accepted = [
        row["main_exact"][-1]
        for row in phases
        if row["main_exact"] and row["main_exact"][-1]["accepted"]
    ]
    return {
        "request_count": len(phases),
        "output_hash_count": len(output_hashes),
        "output_hashes": sorted(output_hashes),
        "accepted_main_count": len(accepted),
        "no_op_wall_ms": [row["wall_ms"] for row in phases],
        "no_op_scc_tasks": [row["scc_tasks"] for row in phases],
        "no_op_scc_member_evaluations": [row["scc_member_evaluations"] for row in phases],
        "no_op_iterative_state_values": [row["iterative_state_values"] for row in phases],
        "last": phases[-1],
    }


def run_mixed_sequence() -> dict[str, Any]:
    workbook = new_workbook()
    phases = [phase(workbook, "initial")]
    workbook.set_value("Inputs", 7, 6, 300)
    phases.append(phase(workbook, "f7_300"))
    for index in range(1, 6):
        phases.append(phase(workbook, f"no_op_after_f7_{index}"))

    workbook.set_value("Inputs", 7, 6, 301)
    phases.append(phase(workbook, "relevant_edit_f7_301"))
    phases.append(phase(workbook, "no_op_after_relevant_edit"))

    workbook.set_value("Inputs", 2, 6, "retained-workspace-unrelated-probe")
    phases.append(phase(workbook, "unrelated_project_name_edit"))
    phases.append(phase(workbook, "no_op_after_unrelated_edit"))

    workbook.set_value("CashFlow Inputs", 19, 10, "CashFlow Engine!$A$1")
    phases.append(phase(workbook, "dynamic_selector_edit"))
    phases.append(phase(workbook, "no_op_after_dynamic_selector_edit"))
    return {"phases": phases}


def main() -> None:
    output = Path(
        r"C:\rust_engines\formualizer-v0.8.4\docs\issue-solutions\data\retained-durability.json"
    )
    result = {
        "schema": "formualizer.retained-durability/v1",
        "workbook": Path(WORKBOOK_PATH).name,
        "settings": {
            "cycle_detection": "runtime",
            "cycle_policy": "iterate",
            "max_iterations": 100,
            "max_change": 0.001,
            "diagnostic_exact_reuse": True,
        },
        "hundred_noops": run_100_noops(),
        "mixed_sequence": run_mixed_sequence(),
    }
    output.write_text(json.dumps(result, indent=2, default=str) + "\n", encoding="utf-8")
    print(f"Generated {output}")


if __name__ == "__main__":
    main()
