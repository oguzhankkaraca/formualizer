from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import sys
import time
from collections import defaultdict
from pathlib import Path
from typing import Any


MAIN_SCC_FALLBACK = 1321560910633541638
WORKBOOK_PATH = r"C:\Users\OXK0A0A\Downloads\Fossil_EstimatingTemplate_2026-08_21_A.xlsx"


RECALC_FIELDS = (
    "dirty_roots",
    "planned_vertices",
    "planned_layers",
    "planned_sccs",
    "evaluated_vertices",
    "acyclic_vertices_evaluated",
    "scc_tasks_evaluated",
    "scc_units_considered",
    "scc_units_reused",
    "scc_units_invalidated",
    "scc_units_reusable_after_recalc",
    "scc_member_count",
    "scc_member_evaluations",
    "volatile_vertices_redirtied",
    "iterative_vertices_redirtied",
)

CYCLE_FIELDS = (
    "static_sccs",
    "phantom_sccs",
    "live_cycles_witnessed",
    "circ_cells_stamped",
    "settle_passes_total",
    "max_passes_single_scc",
    "iterated_sccs",
    "converged_sccs",
    "capped_sccs",
    "max_abs_delta_at_stop",
    "nan_converged",
    "elapsed_ms",
)


def config() -> Any:
    import formualizer as fz

    evaluation = fz.EvaluationConfig()
    evaluation.enable_parallel = True
    evaluation.cycle_detection = "runtime"
    evaluation.cycle_policy = "iterate"
    evaluation.iterate_max_iterations = 100
    evaluation.iterate_max_change = 0.001
    return fz.WorkbookConfig(eval_config=evaluation)


def json_value(value: Any) -> Any:
    if value is None or isinstance(value, (str, int, float, bool)):
        return value
    if isinstance(value, (list, tuple)):
        return [json_value(item) for item in value]
    if isinstance(value, dict):
        return {str(key): json_value(item) for key, item in value.items()}
    return str(value)


def summarize_pass(row: dict[str, Any]) -> dict[str, Any]:
    return {
        "iteration": row["iteration"],
        "evaluated_members": row["evaluated_members"],
        "elapsed_ms": round(row["elapsed_ns"] / 1_000_000, 3),
        "formula_eval_ms": round(row["formula_eval_ns"] / 1_000_000, 3),
        "live_edge_analysis_ms": round(row["live_edge_analysis_ns"] / 1_000_000, 3),
        "scalar_reads": row["scalar_reads"],
        "range_reads": row["range_reads"],
        "range_cells": row["range_cells"],
        "range_membership_checks": row["range_membership_checks"],
        "collection_ms": row["collection_ns"] / 1_000_000,
        "named_reads": row["named_reads"],
        "internal_target_events": row["internal_target_events"],
        "live_edge_events": row["live_edge_events"],
        "dynamic_source_member_count": row["dynamic_source_member_count"],
        "dynamic_source_read_events": row["dynamic_source_read_events"],
        "changed_member_count": len(row["changed_member_addresses"]),
        "changed_member_addresses": row["changed_member_addresses"],
        "static_changed_member_addresses": row["static_changed_member_addresses"],
    }


def profile_groups(profile: list[dict[str, Any]]) -> list[dict[str, Any]]:
    groups: dict[int, list[dict[str, Any]]] = defaultdict(list)
    for row in profile:
        groups[row["stable_id"]].append(row)
    output = []
    for stable_id, rows in groups.items():
        output.append(
            {
                "stable_id": stable_id,
                "passes": len(rows),
                "evaluated_members": sum(row["evaluated_members"] for row in rows),
                "wall_ms": sum(row["elapsed_ns"] for row in rows) / 1_000_000,
                "formula_eval_ms": sum(row["formula_eval_ns"] for row in rows) / 1_000_000,
                "live_edge_analysis_ms": sum(row["live_edge_analysis_ns"] for row in rows) / 1_000_000,
                "collector_ms": sum(row["collection_ns"] for row in rows) / 1_000_000,
                "range_membership_checks": sum(row["range_membership_checks"] for row in rows),
                "scalar_reads": sum(row["scalar_reads"] for row in rows),
                "range_reads": sum(row["range_reads"] for row in rows),
                "range_cells": sum(row["range_cells"] for row in rows),
                "named_reads": sum(row["named_reads"] for row in rows),
                "internal_target_events": sum(row["internal_target_events"] for row in rows),
                "passes_detail": [summarize_pass(row) for row in rows],
            }
        )
    output.sort(key=lambda row: row["wall_ms"], reverse=True)
    return output


def choose_main_id(workbook: Any, groups: list[dict[str, Any]]) -> tuple[int, dict[str, Any] | None]:
    runtime_rows = workbook.last_scc_dirty_telemetry()["per_scc"]
    if runtime_rows:
        row = max(runtime_rows, key=lambda item: item["member_count"])
        return row["stable_id"], json_value(row)
    if groups:
        return groups[0]["stable_id"], None
    return MAIN_SCC_FALLBACK, None


def schedule_reason(record: dict[str, Any] | None) -> str:
    if record is None:
        return "scheduled_without_redirty_record"
    if record["iterative_redirty_member_count"] > 0:
        return "iterative_redirty"
    if record["volatile_redirty_member_count"] > 0:
        return "volatile_redirty"
    if record["naturally_dirty_member_count"] > 0:
        return "natural_dirty"
    return record["reason"]


def formula_output_digest(workbook: Any) -> tuple[int, str]:
    snapshot = {
        key: json_value(value)
        for key, value in workbook.formula_output_snapshot().items()
    }
    payload = json.dumps(snapshot, sort_keys=True, default=str, separators=(",", ":"))
    return len(snapshot), hashlib.sha256(payload.encode()).hexdigest()


def phase_snapshot(workbook: Any, label: str, wall_ms: float) -> dict[str, Any]:
    dirty_raw = workbook.last_scc_dirty_telemetry()
    profile_raw = list(workbook.last_scc_pass_profile())
    groups = profile_groups(profile_raw)
    main_id, main_runtime = choose_main_id(workbook, groups)
    dirty_records = {row["stable_id"]: row for row in dirty_raw["per_scc"]}
    for group in groups:
        record = dirty_records.get(group["stable_id"])
        group["schedule_reason"] = schedule_reason(record)
        group["redirty_record"] = None if record is None else {
            "member_count": record["member_count"],
            "naturally_dirty_member_count": record["naturally_dirty_member_count"],
            "volatile_redirty_member_count": record["volatile_redirty_member_count"],
            "iterative_redirty_member_count": record["iterative_redirty_member_count"],
            "reason": record["reason"],
        }
    main_group = next((group for group in groups if group["stable_id"] == main_id), None)
    formula_output_count, formula_output_sha256 = formula_output_digest(workbook)
    return {
        "label": label,
        "wall_ms": round(wall_ms, 3),
        "formula_value_fingerprint": workbook.formula_value_fingerprint(),
        "formula_output_count": formula_output_count,
        "formula_output_sha256": formula_output_sha256,
        "recalc": {field: json_value(getattr(workbook.last_recalc_telemetry(), field)) for field in RECALC_FIELDS},
        "cycle": {field: json_value(getattr(workbook.last_cycle_telemetry(), field)) for field in CYCLE_FIELDS},
        "dirty": json_value(dirty_raw),
        "main_scc_id": main_id,
        "main_runtime": main_runtime,
        "main_profile": main_group,
        "scheduled_scc_count": len(groups),
        "scheduled_sccs_by_wall": groups,
    }


def run_worker(mode: str) -> dict[str, Any]:
    os.environ["FZ_TRACE_SCC_PASS_PROFILE"] = "1"
    os.environ["FZ_TRACE_SCC_ITERATIONS"] = "1"
    if mode in ("no_iterative", "no_both"):
        os.environ["FZ_DIAGNOSTIC_DISABLE_ITERATIVE_REDIRTY"] = "1"
    else:
        os.environ.pop("FZ_DIAGNOSTIC_DISABLE_ITERATIVE_REDIRTY", None)
    if mode in ("no_volatile", "no_both"):
        os.environ["FZ_DIAGNOSTIC_DISABLE_VOLATILE_REDIRTY"] = "1"
    else:
        os.environ.pop("FZ_DIAGNOSTIC_DISABLE_VOLATILE_REDIRTY", None)

    workbook = fz.Workbook.load_path(WORKBOOK_PATH, backend="calamine", config=config())
    phases = []
    started = time.perf_counter()
    workbook.evaluate_all()
    phases.append(phase_snapshot(workbook, "initial", (time.perf_counter() - started) * 1000))
    workbook.set_value("Inputs", 7, 6, 300)
    started = time.perf_counter()
    workbook.evaluate_all()
    phases.append(phase_snapshot(workbook, "f7_edit", (time.perf_counter() - started) * 1000))
    started = time.perf_counter()
    workbook.evaluate_all()
    phases.append(phase_snapshot(workbook, "noop", (time.perf_counter() - started) * 1000))
    return {
        "mode": mode,
        "disabled_iterative_redirty": mode in ("no_iterative", "no_both"),
        "disabled_volatile_redirty": mode in ("no_volatile", "no_both"),
        "phases": phases,
    }


def run_child(mode: str) -> dict[str, Any]:
    command = [sys.executable, str(Path(__file__).resolve()), "--worker", "--mode", mode]
    completed = subprocess.run(
        command,
        cwd=str(Path(__file__).resolve().parents[2]),
        env=os.environ.copy(),
        capture_output=True,
        text=True,
        check=True,
        timeout=300,
    )
    return json.loads(completed.stdout)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--worker", action="store_true")
    parser.add_argument("--mode", choices=["normal", "no_iterative", "no_volatile", "no_both"])
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.worker:
        global fz
        import formualizer as fz

        print(json.dumps(run_worker(args.mode), separators=(",", ":")))
        return
    if args.output is None:
        parser.error("--output is required outside worker mode")
    result = {
        "schema": "formualizer.heavy-redirty-isolation/v1",
        "workbook": Path(WORKBOOK_PATH).name,
        "input": "Inputs!F7=300",
        "modes": {mode: run_child(mode) for mode in ["normal", "no_iterative", "no_volatile", "no_both"]},
    }
    args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(f"Generated {args.output}")


if __name__ == "__main__":
    main()
