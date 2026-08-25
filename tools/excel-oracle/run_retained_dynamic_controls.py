from __future__ import annotations

import datetime
import json
import os
from pathlib import Path
from typing import Any

os.environ["FZ_DIAGNOSTIC_EXACT_SCC_REUSE"] = "1"

import formualizer as fz


def config() -> Any:
    evaluation = fz.EvaluationConfig()
    evaluation.cycle_detection = "runtime"
    evaluation.cycle_policy = "iterate"
    evaluation.iterate_max_iterations = 100
    evaluation.iterate_max_change = 0.001
    return fz.WorkbookConfig(eval_config=evaluation)


def candidate(workbook: Any) -> dict[str, Any] | None:
    rows = workbook.last_scc_exact_reuse()
    return rows[-1] if rows else None


def exact_dynamic_workbook() -> Any:
    workbook = fz.Workbook(config=config())
    workbook.add_sheet("S")
    workbook.set_value("S", 1, 3, 0)
    workbook.set_value("S", 1, 4, 0)
    workbook.set_value("S", 1, 5, "C1")
    workbook.set_formula("S", 1, 1, "=B1*0+INDIRECT(E1)")
    workbook.set_formula("S", 1, 2, "=A1")
    return workbook


def identity_changes_equal_value() -> dict[str, Any]:
    workbook = exact_dynamic_workbook()
    workbook.evaluate_all()
    workbook.evaluate_all()
    before = candidate(workbook)
    workbook.set_value("S", 1, 5, "D1")
    after = None
    workbook.evaluate_all()
    after = candidate(workbook)
    return {"before": before, "after": after}


def target_value_changes_same_identity() -> dict[str, Any]:
    workbook = exact_dynamic_workbook()
    workbook.evaluate_all()
    workbook.evaluate_all()
    before = candidate(workbook)
    workbook.set_value("S", 1, 3, 1)
    workbook.evaluate_all()
    return {"before": before, "after": candidate(workbook)}


def offset_identity_change_equal_value() -> dict[str, Any]:
    workbook = fz.Workbook(config=config())
    workbook.add_sheet("S")
    workbook.set_value("S", 1, 3, 0)
    workbook.set_value("S", 1, 4, 0)
    workbook.set_formula("S", 1, 1, "=B1*0+OFFSET(C1,0,0)")
    workbook.set_formula("S", 1, 2, "=A1")
    workbook.evaluate_all()
    workbook.evaluate_all()
    before = candidate(workbook)
    workbook.set_formula("S", 1, 1, "=B1*0+OFFSET(D1,0,0)")
    workbook.evaluate_all()
    return {"before": before, "after": candidate(workbook)}


def volatile_udf_equal_value() -> dict[str, Any]:
    workbook = fz.Workbook(config=config())
    workbook.register_function("CONST_TICK", lambda: 0.0, volatile=True)
    workbook.add_sheet("S")
    workbook.set_formula("S", 1, 1, "=B1*0+CONST_TICK()")
    workbook.set_formula("S", 1, 2, "=A1")
    workbook.evaluate_all()
    workbook.evaluate_all()
    return {"candidate": candidate(workbook)}


def clock_generation_equal_value() -> dict[str, Any]:
    workbook = fz.Workbook(config=config())
    workbook.add_sheet("S")
    workbook.set_formula("S", 1, 1, "=B1*0+NOW()")
    workbook.set_formula("S", 1, 2, "=A1")
    clock = datetime.datetime(2026, 1, 1, tzinfo=datetime.timezone.utc)
    workbook.set_deterministic_clock(clock)
    workbook.evaluate_all()
    workbook.set_deterministic_clock(clock)
    workbook.evaluate_all()
    return {"candidate": candidate(workbook)}


def main() -> None:
    controls = {
        "dynamic_identity_equal_value": identity_changes_equal_value,
        "dynamic_value_same_identity": target_value_changes_same_identity,
        "offset_identity_equal_value": offset_identity_change_equal_value,
        "volatile_udf_equal_value": volatile_udf_equal_value,
        "clock_generation_equal_value": clock_generation_equal_value,
    }
    result = {
        "schema": "formualizer.retained-dynamic-controls/v1",
        "controls": {name: fn() for name, fn in controls.items()},
    }
    output = Path(
        r"C:\rust_engines\formualizer-v0.8.4\docs\issue-solutions\data\retained-dynamic-controls.json"
    )
    output.write_text(json.dumps(result, indent=2, default=str) + "\n", encoding="utf-8")
    print(f"Generated {output}")


if __name__ == "__main__":
    main()
