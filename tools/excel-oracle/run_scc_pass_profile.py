from __future__ import annotations

import argparse
import json
import os
import time
from pathlib import Path
from typing import Any

import formualizer as fz


MAIN_SCC_ID = 1321560910633541638


def config() -> Any:
    evaluation = fz.EvaluationConfig()
    evaluation.enable_parallel = True
    evaluation.cycle_detection = "runtime"
    evaluation.cycle_policy = "iterate"
    evaluation.iterate_max_iterations = 100
    evaluation.iterate_max_change = 0.001
    return fz.WorkbookConfig(eval_config=evaluation)


def select_main(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    main = [row for row in rows if row["stable_id"] == MAIN_SCC_ID]
    if main:
        return main
    if not rows:
        return []
    selected = max(rows, key=lambda row: row["evaluated_members"])
    return [row for row in rows if row["stable_id"] == selected["stable_id"]]


def summarize_phase(workbook: Any, label: str, main_id: int | None) -> dict[str, Any]:
    started = time.perf_counter()
    workbook.evaluate_all()
    wall_ms = (time.perf_counter() - started) * 1000
    pass_rows = list(workbook.last_scc_pass_profile())
    if main_id is None and pass_rows:
        main_id = max(pass_rows, key=lambda row: row["evaluated_members"])["stable_id"]
    main_passes = [row for row in pass_rows if row["stable_id"] == main_id]
    member_rows = list(workbook.last_scc_slowest_members(10000))
    main_members = [row for row in member_rows if row["stable_id"] == main_id]
    by_pass = {}
    for iteration in sorted({row["iteration"] for row in main_members}):
        rows = [row for row in main_members if row["iteration"] == iteration]
        rows.sort(key=lambda row: row["elapsed_ns"], reverse=True)
        by_pass[str(iteration)] = rows[:20]
    recalc = workbook.last_recalc_telemetry()
    return {
        "label": label,
        "wall_ms": round(wall_ms, 3),
        "formula_value_fingerprint": workbook.formula_value_fingerprint(),
        "main_scc_id": main_id,
        "main_passes": main_passes,
        "top_slow_members_by_pass": by_pass,
        "all_profile_pass_count": len(pass_rows),
        "all_profile_member_count": len(member_rows),
        "recalc": {
            "total_ns": recalc.total_ns,
            "graph_build_ns": recalc.graph_build_ns,
            "dirty_detection_ns": recalc.dirty_detection_ns,
            "plan_build_ns": recalc.plan_build_ns,
            "acyclic_evaluation_ns": recalc.acyclic_evaluation_ns,
            "iterative_scc_evaluation_ns": recalc.iterative_scc_evaluation_ns,
            "virtual_dependency_change_detection_ns": recalc.virtual_dependency_change_detection_ns,
            "cleanup_ns": recalc.cleanup_ns,
            "evaluation_passes": recalc.evaluation_passes,
            "scc_tasks": recalc.scc_tasks_evaluated,
            "scc_member_evaluations": recalc.scc_member_evaluations,
        },
    }, main_id


def run_fossil(path: str, enabled: bool) -> list[dict[str, Any]]:
    if enabled:
        os.environ["FZ_TRACE_SCC_PASS_PROFILE"] = "1"
    else:
        os.environ.pop("FZ_TRACE_SCC_PASS_PROFILE", None)
    workbook = fz.Workbook.load_path(path, backend="calamine", config=config())
    phases = []
    main_id = None
    for label, edit in [
        ("initial", None),
        ("f7_300", ("Inputs", 7, 6, 300)),
        ("noop", None),
    ]:
        if edit is not None:
            workbook.set_value(*edit)
        phase, main_id = summarize_phase(workbook, label, main_id)
        phases.append(phase)
    return phases


def run_synthetic(enabled: bool) -> list[dict[str, Any]]:
    if enabled:
        os.environ["FZ_TRACE_SCC_PASS_PROFILE"] = "1"
    else:
        os.environ.pop("FZ_TRACE_SCC_PASS_PROFILE", None)
    workbook = fz.Workbook(config=config())
    workbook.add_sheet("S")
    workbook.set_formula("S", 1, 1, "=B1+1")
    workbook.set_formula("S", 1, 2, "=A1/2")
    phases = []
    main_id = None
    for label in ["initial", "noop"]:
        phase, main_id = summarize_phase(workbook, label, main_id)
        phases.append(phase)
    return phases


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--fossil", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    result = {
        "schema": "formualizer.scc-pass-profile/v1",
        "settings": {
            "cycle_detection": "runtime",
            "cycle_policy": "iterate",
            "max_iterations": 100,
            "max_change": 0.001,
            "profile_env": "FZ_TRACE_SCC_PASS_PROFILE=1",
        },
        "fossil_profile_enabled": run_fossil(args.fossil, True),
        "fossil_profile_disabled": run_fossil(args.fossil, False),
        "synthetic_profile_enabled": run_synthetic(True),
        "synthetic_profile_disabled": run_synthetic(False),
        "measurement_limits": {
            "allocation_bytes": "not directly measured; no global allocator changed",
            "arrow_materialization": "represented by resolved range read volume; exact per-cell Arrow kernel instrumentation is not enabled",
            "parallel_scc_execution": "SCC member passes are serial; parallel_enabled records workbook configuration",
        },
    }
    args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    os.environ.pop("FZ_TRACE_SCC_PASS_PROFILE", None)
    print(f"Generated {args.output}")


if __name__ == "__main__":
    main()
