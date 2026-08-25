from __future__ import annotations

import hashlib
import json
import os
import time
from pathlib import Path
from typing import Any

os.environ["FZ_TRACE_SCC_PASS_PROFILE"] = "1"
os.environ["FZ_TRACE_SCC_ITERATIONS"] = "1"
os.environ["FZ_DIAGNOSTIC_EXACT_SCC_REUSE"] = "1"
os.environ["FZ_DIAGNOSTIC_SAME_REQUEST_EXTRA_PASS"] = "1"
os.environ["FZ_DIAGNOSTIC_SAME_REQUEST_EXTRA_PASS_TWICE"] = "1"

import formualizer as fz


MAIN_SCC_ID = 1321560910633541638
WORKBOOK_PATH = r"C:\Users\OXK0A0A\Downloads\Fossil_EstimatingTemplate_2026-08_21_A.xlsx"
OUTPUT_PATH = Path(
    r"C:\rust_engines\formualizer-v0.8.4\docs\issue-solutions\data\scc-state-transition.json"
)


def json_value(value: Any) -> Any:
    if value is None or isinstance(value, (str, int, float, bool)):
        return value
    if isinstance(value, (list, tuple)):
        return [json_value(item) for item in value]
    if isinstance(value, dict):
        return {str(key): json_value(item) for key, item in value.items()}
    return str(value)


def output_digest(workbook: Any) -> tuple[int, str]:
    snapshot = {
        key: json_value(value)
        for key, value in workbook.formula_output_snapshot().items()
    }
    payload = json.dumps(snapshot, sort_keys=True, separators=(",", ":"))
    return len(snapshot), hashlib.sha256(payload.encode()).hexdigest()


def main_runtime(workbook: Any) -> dict[str, Any] | None:
    rows = [
        row
        for row in workbook.last_scc_dirty_telemetry()["per_scc"]
        if row["stable_id"] == MAIN_SCC_ID
    ]
    if not rows:
        return None
    row = rows[0]
    return {
        key: json_value(row[key])
        for key in [
            "stable_id",
            "member_count",
            "naturally_dirty_member_count",
            "volatile_member_count",
            "dynamic_member_count",
            "volatile_redirty_member_count",
            "iterative_redirty_member_count",
            "static_member_count",
            "static_cycle_count",
            "static_cycle_member_count",
            "live_cycle_count",
            "live_cycle_member_count",
            "live_edge_fingerprint",
            "converged",
            "exactly_stable",
            "capped",
            "reason",
        ]
    }


def compact_recalc(workbook: Any) -> dict[str, Any]:
    telemetry = workbook.last_recalc_telemetry()
    return {
        key: json_value(getattr(telemetry, key))
        for key in [
            "scc_tasks_evaluated",
            "scc_member_count",
            "scc_member_evaluations",
            "scc_units_considered",
            "scc_units_reused",
            "scc_units_invalidated",
            "volatile_vertices_redirtied",
            "iterative_vertices_redirtied",
        ]
    }


def compact_dirty(workbook: Any) -> dict[str, Any]:
    telemetry = workbook.last_scc_dirty_telemetry()
    return {
        key: json_value(telemetry[key])
        for key in [
            "dirty_at_request_start",
            "naturally_dirty_before_redirty",
            "dirty_after_volatile_redirty",
            "dirty_after_iterative_redirty",
            "dirty_root_sources",
            "dirty_provenance_counts",
            "user_edit_root_count",
            "iterative_state_value_count",
        ]
    }


def changed_member_rows(workbook: Any) -> list[dict[str, Any]]:
    rows = [
        row
        for row in workbook.last_scc_slowest_members(100000)
        if row["stable_id"] == MAIN_SCC_ID and row["changed"]
    ]
    return [
        {
            "iteration": row["iteration"],
            "address": row["address"],
            "dynamic_source": row["dynamic_source"],
            "before_value": json_value(row["before_value"]),
            "after_value": json_value(row["after_value"]),
            "read_trace": row["read_trace"],
        }
        for row in rows
    ]


def pass_summary(workbook: Any) -> list[dict[str, Any]]:
    return [
        {
            key: json_value(row[key])
            for key in [
                "stable_id",
                "iteration",
                "operator",
                "evaluated_members",
                "elapsed_ns",
                "formula_eval_ns",
                "live_edge_analysis_ns",
                "scalar_reads",
                "range_reads",
                "range_cells",
                "named_reads",
                "internal_target_events",
                "changed_member_addresses",
                "static_changed_member_addresses",
            ]
        }
        for row in workbook.last_scc_pass_profile()
        if row["stable_id"] == MAIN_SCC_ID
    ]


def exact_summary(workbook: Any) -> list[dict[str, Any]]:
    return [
        json_value(row)
        for row in workbook.last_scc_exact_reuse()
        if row["stable_id"] == MAIN_SCC_ID
    ]


def extra_summary(workbook: Any) -> list[dict[str, Any]]:
    return [
        json_value(row)
        for row in workbook.last_scc_same_request_extra_pass()
        if row["stable_id"] == MAIN_SCC_ID
    ]


def main() -> None:
    evaluation = fz.EvaluationConfig()
    evaluation.enable_parallel = True
    evaluation.cycle_detection = "runtime"
    evaluation.cycle_policy = "iterate"
    evaluation.iterate_max_iterations = 100
    evaluation.iterate_max_change = 0.001
    load_started = time.perf_counter()
    workbook = fz.Workbook.load_path(
        WORKBOOK_PATH,
        backend="calamine",
        config=fz.WorkbookConfig(eval_config=evaluation),
    )
    load_ms = (time.perf_counter() - load_started) * 1000
    phases = []
    for label, edit in [
        ("initial", None),
        ("f7_edit", ("Inputs", 7, 6, 300)),
        ("noop", None),
    ]:
        if edit is not None:
            workbook.set_value(*edit)
        started = time.perf_counter()
        workbook.evaluate_all()
        output_count, output_sha = output_digest(workbook)
        phases.append(
            {
                "label": label,
                "wall_ms": round((time.perf_counter() - started) * 1000, 3),
                "formula_output_count": output_count,
                "formula_output_sha256": output_sha,
                "recalc": compact_recalc(workbook),
                "dirty": compact_dirty(workbook),
                "main_runtime": main_runtime(workbook),
                "main_passes": pass_summary(workbook),
                "main_changed_members": changed_member_rows(workbook),
                "exact_reuse_candidate": exact_summary(workbook),
                "same_request_extra_pass": extra_summary(workbook),
            }
        )
    result = {
        "schema": "formualizer.scc-state-transition/v1",
        "workbook": Path(WORKBOOK_PATH).name,
        "input": "Inputs!F7=300",
        "load_ms": round(load_ms, 3),
        "settings": {
            "cycle_detection": "runtime",
            "cycle_policy": "iterate",
            "max_iterations": 100,
            "max_change": 0.001,
            "profile": True,
            "same_request_extra_pass": True,
            "exact_reuse_state_probe": True,
        },
        "phases": phases,
    }
    OUTPUT_PATH.write_text(json.dumps(result, indent=2, default=str) + "\n", encoding="utf-8")
    print(f"Generated {OUTPUT_PATH}")


if __name__ == "__main__":
    main()
