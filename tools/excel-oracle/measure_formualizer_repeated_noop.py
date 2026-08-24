from __future__ import annotations

import hashlib
import json
import os
import time
from pathlib import Path
from typing import Any

os.environ["FZ_TRACE_SCC_PASS_PROFILE"] = "1"
os.environ["FZ_TRACE_SCC_ITERATIONS"] = "1"


MAIN_SCC_ID = 1321560910633541638


def config() -> Any:
    evaluation = fz.EvaluationConfig()
    evaluation.enable_parallel = True
    evaluation.cycle_detection = "runtime"
    evaluation.cycle_policy = "iterate"
    evaluation.iterate_max_iterations = 100
    evaluation.iterate_max_change = 0.001
    return fz.WorkbookConfig(eval_config=evaluation)


def snapshot_hash(snapshot: dict[str, Any]) -> str:
    payload = json.dumps(snapshot, sort_keys=True, default=str, separators=(",", ":"))
    return hashlib.sha256(payload.encode()).hexdigest()


def numeric_values(value: Any) -> list[float]:
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        return [float(value)]
    if isinstance(value, list):
        values: list[float] = []
        for item in value:
            values.extend(numeric_values(item))
        return values
    return []


def snapshot_diff(before: dict[str, Any], after: dict[str, Any]) -> dict[str, Any]:
    changed: dict[str, Any] = {}
    for address in sorted(set(before) | set(after)):
        old = before.get(address, "<missing>")
        new = after.get(address, "<missing>")
        if old != new:
            old_numbers = numeric_values(old)
            new_numbers = numeric_values(new)
            deltas = [abs(new_value - old_value) for old_value, new_value in zip(old_numbers, new_numbers)]
            changed[address] = {
                "before": old,
                "after": new,
                "max_abs_numeric_delta": max(deltas) if deltas else None,
            }
    return changed


def main() -> None:
    global fz
    import formualizer as fz

    path = r"C:\Users\OXK0A0A\Downloads\Fossil_EstimatingTemplate_2026-08_21_A.xlsx"
    workbook = fz.Workbook.load_path(path, backend="calamine", config=config())
    initial_started = time.perf_counter()
    workbook.evaluate_all()
    initial_wall_ms = (time.perf_counter() - initial_started) * 1000
    workbook.set_value("Inputs", 7, 6, 300)
    seed_started = time.perf_counter()
    workbook.evaluate_all()
    seed_wall_ms = (time.perf_counter() - seed_started) * 1000
    previous = dict(workbook.formula_output_snapshot())
    seed_hash = snapshot_hash(previous)
    steps: list[dict[str, Any]] = []
    progression_members: set[str] = set()

    for calculate in range(1, 6):
        started = time.perf_counter()
        workbook.evaluate_all()
        current = dict(workbook.formula_output_snapshot())
        changed = snapshot_diff(previous, current)
        trace = [
            row
            for row in workbook.last_scc_iteration_trace()
            if row["stable_id"] == MAIN_SCC_ID
        ]
        profile = [
            row
            for row in workbook.last_scc_pass_profile()
            if row["stable_id"] == MAIN_SCC_ID
        ]
        first_pass = next((row for row in profile if row["iteration"] == 1), None)
        static_changed = [] if first_pass is None else first_pass["static_changed_member_addresses"]
        progression_members.update(static_changed)
        numeric_deltas = [
            item["max_abs_numeric_delta"]
            for item in changed.values()
            if item["max_abs_numeric_delta"] is not None
        ]
        steps.append(
            {
                "calculate": calculate,
                "wall_ms": round((time.perf_counter() - started) * 1000, 3),
                "formula_count": len(current),
                "snapshot_sha256": snapshot_hash(current),
                "formula_value_fingerprint": workbook.formula_value_fingerprint(),
                "changed_formula_count": len(changed),
                "changed_formulas": changed,
                "max_abs_numeric_delta": max(numeric_deltas) if numeric_deltas else 0.0,
                "static_remainder_changed_member_values": {
                    address: current.get(address.replace("$", "")) for address in static_changed
                },
                "main_passes": [
                    {
                        "iteration": row["iteration"],
                        "evaluated_members": row["evaluated_members"],
                        "elapsed_ms": round(row["elapsed_ns"] / 1_000_000, 3),
                        "formula_eval_ms": round(row["formula_eval_ns"] / 1_000_000, 3),
                        "changed_member_addresses": row["changed_member_addresses"],
                        "static_changed_member_addresses": row["static_changed_member_addresses"],
                    }
                    for row in profile
                ],
                "main_iteration_trace": trace,
            }
        )
        previous = current

    progression_values = {
        step["calculate"]: step["static_remainder_changed_member_values"]
        for step in steps
    }
    result = {
        "schema": "formualizer.formualizer-repeated-noop/v1",
        "workbook": Path(path).name,
        "input": {"sheet": "Inputs", "address": "F7", "value": 300},
        "settings": {
            "cycle_detection": "runtime",
            "cycle_policy": "iterate",
            "max_iterations": 100,
            "max_change": 0.001,
            "parallel_enabled": True,
        },
        "initial_calculation_wall_ms": round(initial_wall_ms, 3),
        "seed": {
            "wall_ms": round(seed_wall_ms, 3),
            "formula_count": len(workbook.formula_output_snapshot()),
            "snapshot_sha256": seed_hash,
            "formula_output_values": previous,
            "formula_value_fingerprint": workbook.formula_value_fingerprint(),
        },
        "steps": steps,
        "static_remainder_progression_members": sorted(progression_members),
        "static_remainder_progression_member_count": len(progression_members),
        "static_remainder_values": progression_values,
    }
    output = Path(r"C:\rust_engines\formualizer-v0.8.4\docs\issue-solutions\data\formualizer-heavy-repeated-noop.json")
    output.write_text(json.dumps(result, indent=2, default=str) + "\n", encoding="utf-8")
    print(f"Generated {output}")


if __name__ == "__main__":
    main()
