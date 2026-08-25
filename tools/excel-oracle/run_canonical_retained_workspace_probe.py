from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

WORKBOOK_PATH = r"C:\Users\OXK0A0A\Downloads\Fossil_EstimatingTemplate_2026-08_21_A.xlsx"
MAIN_SCC_ID = 1321560910633541638


def config() -> Any:
    import formualizer as fz

    evaluation = fz.EvaluationConfig()
    evaluation.enable_parallel = True
    evaluation.cycle_detection = "runtime"
    evaluation.cycle_policy = "iterate"
    evaluation.iterate_max_iterations = 100
    evaluation.iterate_max_change = 0.001
    return fz.WorkbookConfig(eval_config=evaluation)


def output_digest(workbook: Any) -> tuple[int, str]:
    snapshot = {
        key: value
        for key, value in workbook.formula_output_snapshot().items()
    }
    payload = json.dumps(snapshot, sort_keys=True, default=str, separators=(",", ":"))
    return len(snapshot), hashlib.sha256(payload.encode()).hexdigest()


def run(mode: str) -> dict[str, Any]:
    if mode == "normal":
        os.environ.pop("FZ_DIAGNOSTIC_EXACT_SCC_REUSE", None)
    else:
        os.environ["FZ_DIAGNOSTIC_EXACT_SCC_REUSE"] = "1"
    os.environ["FZ_TRACE_SCC_PASS_PROFILE"] = "1"
    os.environ["FZ_TRACE_SCC_ITERATIONS"] = "1"
    os.environ.pop("FZ_DIAGNOSTIC_SAME_REQUEST_EXTRA_PASS", None)
    os.environ.pop("FZ_DIAGNOSTIC_SAME_REQUEST_EXTRA_PASS_TWICE", None)

    workbook = fz.Workbook.load_path(WORKBOOK_PATH, backend="calamine", config=config())
    phases = []
    for label, edit in [("initial", None), ("f7_edit", ("Inputs", 7, 6, 300)), ("noop", None)]:
        if edit is not None:
            workbook.set_value(*edit)
        started = time.perf_counter()
        workbook.evaluate_all()
        output_count, output_sha = output_digest(workbook)
        exact = [
            row
            for row in workbook.last_scc_exact_reuse()
            if row["stable_id"] == MAIN_SCC_ID
        ]
        recalc = workbook.last_recalc_telemetry()
        phases.append(
            {
                "label": label,
                "wall_ms": round((time.perf_counter() - started) * 1000, 3),
                "formula_output_count": output_count,
                "formula_output_sha256": output_sha,
                "scc_tasks_evaluated": recalc.scc_tasks_evaluated,
                "scc_member_evaluations": recalc.scc_member_evaluations,
                "exact_reuse": exact,
            }
        )
    return {"mode": mode, "phases": phases}


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
    parser.add_argument("--mode", choices=["normal", "canonical"])
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.worker:
        global fz
        import formualizer as fz

        print(json.dumps(run(args.mode), separators=(",", ":")))
        return
    if args.output is None:
        parser.error("--output is required outside worker mode")
    result = {
        "schema": "formualizer.canonical-retained-workspace-probe/v1",
        "workbook": Path(WORKBOOK_PATH).name,
        "input": "Inputs!F7=300",
        "modes": {mode: run_child(mode) for mode in ["normal", "canonical"]},
    }
    args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(f"Generated {args.output}")


if __name__ == "__main__":
    main()
