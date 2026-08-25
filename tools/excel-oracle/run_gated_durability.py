from __future__ import annotations

import hashlib
import json
import os
import statistics
import time
from pathlib import Path
from typing import Any

os.environ["FZ_DIAGNOSTIC_EXACT_SCC_REUSE"] = "1"

import formualizer as fz


def config() -> Any:
    evaluation = fz.EvaluationConfig()
    evaluation.cycle_detection = "runtime"
    evaluation.cycle_policy = "iterate"
    evaluation.iterate_max_iterations = 20
    evaluation.iterate_max_change = 0.001
    return fz.WorkbookConfig(eval_config=evaluation)


def digest(workbook: Any) -> str:
    payload = json.dumps(
        dict(workbook.formula_output_snapshot()),
        sort_keys=True,
        default=str,
        separators=(",", ":"),
    )
    return hashlib.sha256(payload.encode()).hexdigest()


def phase(workbook: Any, label: str) -> dict[str, Any]:
    started = time.perf_counter()
    workbook.evaluate_all()
    exact = workbook.last_scc_exact_reuse()
    recalc = workbook.last_recalc_telemetry()
    return {
        "label": label,
        "wall_ms": round((time.perf_counter() - started) * 1000, 3),
        "output_sha256": digest(workbook),
        "scc_tasks": recalc.scc_tasks_evaluated,
        "scc_member_evaluations": recalc.scc_member_evaluations,
        "candidates": [(row["accepted"], row["reason"]) for row in exact],
        "candidate_count": len(exact),
    }


def static_exact() -> Any:
    workbook = fz.Workbook(config=config())
    workbook.add_sheet("S")
    workbook.set_formula("S", 1, 1, "=B1")
    workbook.set_formula("S", 1, 2, "=A1")
    return workbook


def dynamic_exact() -> Any:
    workbook = fz.Workbook(config=config())
    workbook.add_sheet("S")
    workbook.set_value("S", 1, 3, 0)
    workbook.set_value("S", 1, 5, "C1")
    workbook.set_formula("S", 1, 1, "=B1*0+INDIRECT(E1)")
    workbook.set_formula("S", 1, 2, "=A1")
    return workbook


def volatile_equal() -> Any:
    workbook = fz.Workbook(config=config())
    workbook.register_function("CONST_TICK", lambda: 0.0, volatile=True)
    workbook.add_sheet("S")
    workbook.set_formula("S", 1, 1, "=B1*0+CONST_TICK()")
    workbook.set_formula("S", 1, 2, "=A1")
    return workbook


def run_repeated(builder: Any, count: int = 100) -> dict[str, Any]:
    workbook = builder()
    first = phase(workbook, "initial")
    rows = [phase(workbook, f"noop_{index}") for index in range(1, count + 1)]
    reasons = {}
    for row in rows:
        for accepted, reason in row["candidates"]:
            reasons[reason] = reasons.get(reason, 0) + 1
    return {
        "request_count": count,
        "output_hash_count": len({row["output_sha256"] for row in rows}),
        "accepted_count": sum(
            accepted for row in rows for accepted, _ in row["candidates"]
        ),
        "candidate_reason_counts": reasons,
        "wall_ms_mean": statistics.mean(row["wall_ms"] for row in rows),
        "wall_ms_max": max(row["wall_ms"] for row in rows),
        "scc_tasks_set": sorted({row["scc_tasks"] for row in rows}),
        "scc_evaluations_set": sorted({row["scc_member_evaluations"] for row in rows}),
        "first": first,
        "last": rows[-1],
    }


def mixed_sequence() -> dict[str, Any]:
    workbook = dynamic_exact()
    rows = [phase(workbook, "initial"), phase(workbook, "no_op_1")]
    workbook.set_value("S", 1, 5, "D1")
    workbook.set_value("S", 1, 4, 0)
    rows.append(phase(workbook, "dynamic_identity_equal_value"))
    rows.append(phase(workbook, "no_op_after_identity_change"))
    workbook.set_value("S", 1, 3, 1)
    rows.append(phase(workbook, "target_value_change_same_identity"))
    rows.append(phase(workbook, "no_op_after_value_change"))
    return {"phases": rows}


def main() -> None:
    result = {
        "schema": "formualizer.gated-durability/v1",
        "diagnostic_exact_reuse": True,
        "static_exact_100": run_repeated(static_exact),
        "dynamic_exact_100": run_repeated(dynamic_exact),
        "volatile_equal_100": run_repeated(volatile_equal),
        "mixed_sequence": mixed_sequence(),
    }
    output = Path(
        r"C:\rust_engines\formualizer-v0.8.4\docs\issue-solutions\data\gated-durability.json"
    )
    output.write_text(json.dumps(result, indent=2, default=str) + "\n", encoding="utf-8")
    print(f"Generated {output}")


if __name__ == "__main__":
    main()
