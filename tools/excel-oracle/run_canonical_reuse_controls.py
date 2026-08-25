from __future__ import annotations

import datetime
import json
import os
from pathlib import Path
from typing import Any, Callable

os.environ["FZ_DIAGNOSTIC_EXACT_SCC_REUSE"] = "1"

import formualizer as fz


def config(max_iterations: int = 100, max_change: float = 0.001) -> Any:
    evaluation = fz.EvaluationConfig()
    evaluation.cycle_detection = "runtime"
    evaluation.cycle_policy = "iterate"
    evaluation.iterate_max_iterations = max_iterations
    evaluation.iterate_max_change = max_change
    return fz.WorkbookConfig(eval_config=evaluation)


def last_candidate(workbook: Any) -> dict[str, Any] | None:
    records = workbook.last_scc_exact_reuse()
    return records[-1] if records else None


def build_pair(workbook: Any, first: str, second: str) -> None:
    workbook.add_sheet("S")
    workbook.set_formula("S", 1, 1, first)
    workbook.set_formula("S", 1, 2, second)


def volatile_random() -> dict[str, Any]:
    workbook = fz.Workbook(config=config())
    build_pair(workbook, "=B1+RAND()", "=A1/2")
    workbook.evaluate_all()
    workbook.evaluate_all()
    return {"candidate": last_candidate(workbook)}


def volatile_clock() -> dict[str, Any]:
    workbook = fz.Workbook(config=config())
    build_pair(workbook, "=B1+NOW()", "=A1/2")
    workbook.set_deterministic_clock(
        datetime.datetime(2026, 1, 1, tzinfo=datetime.timezone.utc)
    )
    workbook.evaluate_all()
    workbook.set_deterministic_clock(
        datetime.datetime(2026, 1, 2, tzinfo=datetime.timezone.utc)
    )
    workbook.evaluate_all()
    return {"candidate": last_candidate(workbook)}


def dynamic_target_change() -> dict[str, Any]:
    workbook = fz.Workbook(config=config())
    workbook.add_sheet("S")
    workbook.set_value("S", 1, 4, "B1")
    workbook.set_value("S", 1, 3, 10)
    workbook.set_formula("S", 1, 1, "=INDIRECT(D1)+1")
    workbook.set_formula("S", 1, 2, "=A1+1")
    workbook.evaluate_all()
    workbook.evaluate_all()
    before = last_candidate(workbook)
    workbook.set_value("S", 1, 4, "C1")
    workbook.set_value("S", 3, 1, 10)
    workbook.evaluate_all()
    return {"before": before, "after": last_candidate(workbook)}


def shape_change() -> dict[str, Any]:
    workbook = fz.Workbook(config=config())
    workbook.add_sheet("S")
    workbook.set_value("S", 1, 2, 1)
    workbook.set_value("S", 2, 2, 2)
    workbook.set_formula("S", 1, 1, '=FILTER(B1:B2,B1:B2<>"")')
    workbook.set_formula("S", 1, 2, "=A1+1")
    workbook.evaluate_all()
    workbook.evaluate_all()
    return {"candidate": last_candidate(workbook)}


def table_definition_change() -> dict[str, Any]:
    workbook = fz.Workbook(config=config())
    build_pair(workbook, "=B1+1", "=A1/2")
    workbook.evaluate_all()
    workbook.evaluate_all()
    before = last_candidate(workbook)
    workbook.set_value("S", 1, 3, "Value")
    workbook.set_value("S", 2, 3, 1)
    workbook.add_table("T", "S", (1, 3, 2, 3), ["Value"])
    workbook.evaluate_all()
    return {"before": before, "after": last_candidate(workbook)}


def structural_change() -> dict[str, Any]:
    workbook = fz.Workbook(config=config())
    build_pair(workbook, "=B1+1", "=A1/2")
    workbook.evaluate_all()
    workbook.evaluate_all()
    before = last_candidate(workbook)
    workbook.add_sheet("Other")
    workbook.evaluate_all()
    return {"before": before, "after": last_candidate(workbook)}


def udf_change() -> dict[str, Any]:
    counter = {"value": 0}

    def tick() -> float:
        counter["value"] += 1
        return float(counter["value"])

    workbook = fz.Workbook(config=config())
    workbook.register_function("TICK", tick, volatile=True)
    build_pair(workbook, "=B1+TICK()", "=A1/2")
    workbook.evaluate_all()
    workbook.evaluate_all()
    return {"candidate": last_candidate(workbook)}


def tolerance_only() -> dict[str, Any]:
    workbook = fz.Workbook(config=config(max_iterations=100, max_change=0.001))
    build_pair(workbook, "=B1+0.0001", "=A1/2")
    workbook.evaluate_all()
    workbook.evaluate_all()
    return {"candidate": last_candidate(workbook)}


def capped_iteration() -> dict[str, Any]:
    workbook = fz.Workbook(config=config(max_iterations=1, max_change=0.001))
    build_pair(workbook, "=B1+1", "=A1/2")
    workbook.evaluate_all()
    workbook.evaluate_all()
    return {"candidate": last_candidate(workbook)}


def main() -> None:
    controls: dict[str, Callable[[], dict[str, Any]]] = {
        "rand_state_change": volatile_random,
        "now_clock_change": volatile_clock,
        "dynamic_target_change": dynamic_target_change,
        "dynamic_target_shape_change": shape_change,
        "name_table_boundary_change": table_definition_change,
        "structural_boundary_change": structural_change,
        "upstream_fixed_point_change": dynamic_target_change,
        "volatile_external_udf": udf_change,
        "tolerance_only_convergence": tolerance_only,
        "capped_iteration": capped_iteration,
    }
    result = {
        "schema": "formualizer.canonical-retained-workspace-controls/v1",
        "controls": {name: runner() for name, runner in controls.items()},
    }
    output = Path(
        r"C:\rust_engines\formualizer-v0.8.4\docs\issue-solutions\data\canonical-retained-workspace-controls.json"
    )
    output.write_text(json.dumps(result, indent=2, default=str) + "\n", encoding="utf-8")
    print(f"Generated {output}")


if __name__ == "__main__":
    main()
