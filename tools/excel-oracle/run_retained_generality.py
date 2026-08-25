from __future__ import annotations

import hashlib
import json
import os
import time
from pathlib import Path
from typing import Any, Callable

os.environ["FZ_DIAGNOSTIC_EXACT_SCC_REUSE"] = "1"
os.environ.pop("FZ_TRACE_SCC_PASS_PROFILE", None)
os.environ.pop("FZ_TRACE_SCC_ITERATIONS", None)

import formualizer as fz

LIGHT_PATH = r"C:\Users\OXK0A0A\Downloads\Fossil_EstimatingTemplate_2026-06-25_X_Fossil.xlsx"


def config() -> Any:
    evaluation = fz.EvaluationConfig()
    evaluation.enable_parallel = True
    evaluation.cycle_detection = "runtime"
    evaluation.cycle_policy = "iterate"
    evaluation.iterate_max_iterations = 100
    evaluation.iterate_max_change = 0.001
    return fz.WorkbookConfig(eval_config=evaluation)


def digest(workbook: Any) -> str:
    values = dict(workbook.formula_output_snapshot())
    payload = json.dumps(values, sort_keys=True, default=str, separators=(",", ":"))
    return hashlib.sha256(payload.encode()).hexdigest()


def phase(workbook: Any, label: str) -> dict[str, Any]:
    started = time.perf_counter()
    workbook.evaluate_all()
    recalc = workbook.last_recalc_telemetry()
    return {
        "label": label,
        "wall_ms": round((time.perf_counter() - started) * 1000, 3),
        "output_sha256": digest(workbook),
        "scc_tasks": recalc.scc_tasks_evaluated,
        "scc_member_evaluations": recalc.scc_member_evaluations,
        "exact_candidates": workbook.last_scc_exact_reuse(),
    }


def pair(workbook: Any, sheet: str = "S", offset: int = 0) -> None:
    workbook.add_sheet(sheet)
    workbook.set_formula(sheet, 1, 1 + offset, f"={chr(66 + offset)}1")
    workbook.set_formula(sheet, 1, 2 + offset, f"={chr(65 + offset)}1")


def small_exact(workbook: Any) -> None:
    pair(workbook)


def independent(workbook: Any) -> None:
    pair(workbook)
    pair(workbook, offset=3)


def cross_scc(workbook: Any) -> None:
    workbook.add_sheet("S")
    workbook.set_formula("S", 1, 1, "=B1")
    workbook.set_formula("S", 1, 2, "=A1")
    workbook.set_formula("S", 1, 3, "=D1+A1")
    workbook.set_formula("S", 1, 4, "=C1")


def dynamic_reference(workbook: Any) -> None:
    workbook.add_sheet("S")
    workbook.set_value("S", 1, 5, "C1")
    workbook.set_value("S", 1, 3, 0)
    workbook.set_formula("S", 1, 1, "=B1+INDIRECT(E1)")
    workbook.set_formula("S", 1, 2, "=A1")


def volatile_random(workbook: Any) -> None:
    workbook.add_sheet("S")
    workbook.set_formula("S", 1, 1, "=B1+RAND()")
    workbook.set_formula("S", 1, 2, "=A1/2")


def loaded_light(workbook: Any) -> None:
    pass


def run_scenario(name: str, builder: Callable[[Any], None]) -> dict[str, Any]:
    if name == "light_fossil":
        workbook = fz.Workbook.load_path(LIGHT_PATH, backend="calamine", config=config())
    else:
        workbook = fz.Workbook(config=config())
        builder(workbook)
    phases = [phase(workbook, "initial"), phase(workbook, "no_op_1"), phase(workbook, "no_op_2")]
    return {"phases": phases}


def main() -> None:
    scenarios: dict[str, Callable[[Any], None]] = {
        "small_exact_iterative": small_exact,
        "two_independent_sccs": independent,
        "cross_scc_dependency": cross_scc,
        "dynamic_reference_scc": dynamic_reference,
        "rand_volatile_scc": volatile_random,
        "light_fossil": loaded_light,
    }
    result = {
        "schema": "formualizer.retained-generality/v1",
        "diagnostic_exact_reuse": True,
        "scenarios": {name: run_scenario(name, builder) for name, builder in scenarios.items()},
    }
    output = Path(
        r"C:\rust_engines\formualizer-v0.8.4\docs\issue-solutions\data\retained-generality.json"
    )
    output.write_text(json.dumps(result, indent=2, default=str) + "\n", encoding="utf-8")
    print(f"Generated {output}")


if __name__ == "__main__":
    main()
