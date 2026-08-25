from __future__ import annotations

import hashlib
import json
import os
import time
from pathlib import Path
from typing import Any

os.environ["FZ_TRACE_SCC_PASS_PROFILE"] = "1"
os.environ["FZ_TRACE_SCC_ITERATIONS"] = "1"

import formualizer as fz


MAIN_SCC_FALLBACK = 1321560910633541638
WORKBOOK_PATH = Path(r"C:\Users\OXK0A0A\Downloads\Fossil_EstimatingTemplate_2026-08_21_A.xlsx")
OUTPUT_PATH = Path(
    r"C:\rust_engines\formualizer-v0.8.4\docs\issue-solutions\data\latest-upstream-heavy-baseline.json"
)


def config() -> Any:
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
    if isinstance(value, list):
        return [json_value(item) for item in value]
    if isinstance(value, dict):
        return {str(key): json_value(item) for key, item in value.items()}
    return str(value)


def snapshot_hash(snapshot: dict[str, Any]) -> str:
    payload = json.dumps(snapshot, sort_keys=True, default=str, separators=(",", ":"))
    return hashlib.sha256(payload.encode()).hexdigest()


def numeric_values(value: Any) -> list[float]:
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        return [float(value)]
    if isinstance(value, list):
        output: list[float] = []
        for item in value:
            output.extend(numeric_values(item))
        return output
    return []


def snapshot_diff(before: dict[str, Any], after: dict[str, Any]) -> dict[str, Any]:
    changed: dict[str, Any] = {}
    for address in sorted(set(before) | set(after)):
        old = before.get(address, "<missing>")
        new = after.get(address, "<missing>")
        if old == new:
            continue
        old_numbers = numeric_values(old)
        new_numbers = numeric_values(new)
        deltas = [abs(new_value - old_value) for old_value, new_value in zip(old_numbers, new_numbers)]
        changed[address] = {
            "before": old,
            "after": new,
            "max_abs_numeric_delta": max(deltas) if deltas else None,
        }
    return changed


def scalar_fields(obj: Any, fields: tuple[str, ...]) -> dict[str, Any]:
    return {field: json_value(getattr(obj, field)) for field in fields}


def choose_main_scc(workbook: Any, profile: list[dict[str, Any]]) -> tuple[int, dict[str, Any] | None]:
    runtime_rows = workbook.last_scc_dirty_telemetry()["per_scc"]
    if runtime_rows:
        row = max(runtime_rows, key=lambda item: item["member_count"])
        return row["stable_id"], json_value(row)
    if profile:
        row = max(profile, key=lambda item: item["evaluated_members"])
        return row["stable_id"], None
    return MAIN_SCC_FALLBACK, None


def summarize_pass(row: dict[str, Any]) -> dict[str, Any]:
    return {
        "iteration": row["iteration"],
        "evaluated_members": row["evaluated_members"],
        "elapsed_ms": round(row["elapsed_ns"] / 1_000_000, 3),
        "formula_eval_ms": round(row["formula_eval_ns"] / 1_000_000, 3),
        "post_eval_bookkeeping_ms": round(row["post_eval_bookkeeping_ns"] / 1_000_000, 3),
        "live_edge_analysis_ms": round(row["live_edge_analysis_ns"] / 1_000_000, 3),
        "convergence_check_ms": round(row["convergence_check_ns"] / 1_000_000, 3),
        "scalar_reads": row["scalar_reads"],
        "range_reads": row["range_reads"],
        "range_cells": row["range_cells"],
        "range_membership_checks": row["range_membership_checks"],
        "collection_ms": row["collection_ns"] / 1_000_000,
        "named_reads": row["named_reads"],
        "internal_target_events": row["internal_target_events"],
        "live_edge_events": row["live_edge_events"],
        "lookup_builds": row["lookup_builds"],
        "lookup_hits": row["lookup_hits"],
        "lookup_misses": row["lookup_misses"],
        "dynamic_source_member_count": row["dynamic_source_member_count"],
        "dynamic_source_read_events": row["dynamic_source_read_events"],
        "changed_member_count": len(row["changed_member_addresses"]),
        "changed_member_addresses": row["changed_member_addresses"],
        "static_changed_member_addresses": row["static_changed_member_addresses"],
        "dirty_propagation_visits": row["dirty_propagation_visits"],
    }


def phase_record(
    workbook: Any,
    label: str,
    wall_ms: float,
    before: dict[str, Any] | None,
    include_changed_values: bool = True,
) -> tuple[dict[str, Any], dict[str, Any]]:
    current = {key: json_value(value) for key, value in workbook.formula_output_snapshot().items()}
    changed = {} if before is None else snapshot_diff(before, current)
    all_profile = list(workbook.last_scc_pass_profile())
    main_id, runtime = choose_main_scc(workbook, all_profile)
    main_profile = [row for row in all_profile if row["stable_id"] == main_id]
    trace = [
        row
        for row in workbook.last_scc_iteration_trace()
        if row["stable_id"] == main_id
    ]
    recalc_fields = (
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
    recalc = scalar_fields(workbook.last_recalc_telemetry(), recalc_fields)
    dirty = json_value(workbook.last_scc_dirty_telemetry())
    cycle = scalar_fields(
        workbook.last_cycle_telemetry(),
        (
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
        ),
    )
    numeric_deltas = [
        item["max_abs_numeric_delta"]
        for item in changed.values()
        if item["max_abs_numeric_delta"] is not None
    ]
    record = {
        "label": label,
        "wall_ms": round(wall_ms, 3),
        "formula_count": len(current),
        "snapshot_sha256": snapshot_hash(current),
        "formula_value_fingerprint": workbook.formula_value_fingerprint(),
        "changed_formula_count": len(changed),
        "max_abs_numeric_delta": max(numeric_deltas) if numeric_deltas else 0.0,
        "changed_formulas": changed if include_changed_values else {},
        "main_scc_id": main_id,
        "main_runtime": runtime,
        "main_passes": [summarize_pass(row) for row in main_profile],
        "main_iteration_trace": json_value(trace),
        "recalc": recalc,
        "cycle": cycle,
        "dirty": dirty,
        "coordinate_index_build_ns": workbook.last_scc_coordinate_index_build_ns(),
    }
    return record, current


def main() -> None:
    load_started = time.perf_counter()
    workbook = fz.Workbook.load_path(
        str(WORKBOOK_PATH), backend="calamine", config=config()
    )
    load_ms = (time.perf_counter() - load_started) * 1000

    initial_started = time.perf_counter()
    workbook.evaluate_all()
    initial_record, initial_snapshot = phase_record(
        workbook, "initial", (time.perf_counter() - initial_started) * 1000, None
    )

    edit_started = time.perf_counter()
    workbook.set_value("Inputs", 7, 6, 300)
    edit_set_ms = (time.perf_counter() - edit_started) * 1000
    f7_started = time.perf_counter()
    workbook.evaluate_all()
    f7_record, f7_snapshot = phase_record(
        workbook, "f7_edit", (time.perf_counter() - f7_started) * 1000, initial_snapshot
    )

    steps = [initial_record, f7_record]
    previous = f7_snapshot
    for calculate in range(1, 6):
        started = time.perf_counter()
        workbook.evaluate_all()
        record, previous = phase_record(
            workbook,
            f"noop_{calculate}",
            (time.perf_counter() - started) * 1000,
            previous,
        )
        steps.append(record)

    same_set_started = time.perf_counter()
    workbook.set_value("Inputs", 7, 6, 300)
    same_set_ms = (time.perf_counter() - same_set_started) * 1000
    same_started = time.perf_counter()
    workbook.evaluate_all()
    same_record, same_snapshot = phase_record(
        workbook,
        "same_value_edit",
        (time.perf_counter() - same_started) * 1000,
        previous,
    )
    steps.append(same_record)

    static_probe = json_value(workbook.static_scc_probe())
    result = {
        "schema": "formualizer.latest-upstream-heavy-baseline/v1",
        "workbook": WORKBOOK_PATH.name,
        "branch_expected": "investigation/fossil-upstream-integration",
        "input": {"sheet": "Inputs", "address": "F7", "value": 300},
        "settings": {
            "cycle_detection": "runtime",
            "cycle_policy": "iterate",
            "max_iterations": 100,
            "max_change": 0.001,
            "parallel_enabled": True,
        },
        "load_ms": round(load_ms, 3),
        "edit_set_ms": round(edit_set_ms, 3),
        "same_value_set_ms": round(same_set_ms, 3),
        "static_scc_probe": static_probe,
        "seed_formula_output_values": f7_snapshot,
        "steps": steps,
    }
    OUTPUT_PATH.write_text(json.dumps(result, indent=2, default=str) + "\n", encoding="utf-8")
    print(f"Generated {OUTPUT_PATH}")


if __name__ == "__main__":
    main()
