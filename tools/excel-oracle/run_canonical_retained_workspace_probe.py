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


def ranked_scc_profiles(workbook: Any) -> list[dict[str, Any]]:
    groups: dict[int, list[dict[str, Any]]] = {}
    for row in workbook.last_scc_pass_profile():
        groups.setdefault(row["stable_id"], []).append(row)
    ranked = []
    for stable_id, rows in groups.items():
        ranked.append(
            {
                "stable_id": stable_id,
                "passes": len(rows),
                "evaluated_members": sum(row["evaluated_members"] for row in rows),
                "wall_ms": sum(row["elapsed_ns"] for row in rows) / 1_000_000,
                "formula_eval_ms": sum(row["formula_eval_ns"] for row in rows) / 1_000_000,
                "live_edge_analysis_ms": sum(row["live_edge_analysis_ns"] for row in rows) / 1_000_000,
                "collection_ms": sum(row["collection_ns"] for row in rows) / 1_000_000,
            }
        )
    ranked.sort(key=lambda row: row["wall_ms"], reverse=True)
    return ranked


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
        evaluation_wall_ms = (time.perf_counter() - started) * 1000
        digest_started = time.perf_counter()
        output_count, output_sha = output_digest(workbook)
        output_digest_ms = (time.perf_counter() - digest_started) * 1000
        exact = [
            row
            for row in workbook.last_scc_exact_reuse()
            if row["stable_id"] == MAIN_SCC_ID
        ]
        recalc = workbook.last_recalc_telemetry()
        recalc_summary = {
            field: getattr(recalc, field)
            for field in [
                "total_ns",
                "graph_build_ns",
                "dirty_detection_ns",
                "plan_build_ns",
                "acyclic_evaluation_ns",
                "iterative_scc_evaluation_ns",
                "virtual_dependency_change_detection_ns",
                "cleanup_ns",
                "evaluation_passes",
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
                "scc_member_count",
                "scc_member_evaluations",
            ]
        }
        profile_started = time.perf_counter()
        profile_rank = ranked_scc_profiles(workbook)
        profile_rank_ms = (time.perf_counter() - profile_started) * 1000
        phases.append(
            {
                "label": label,
                "wall_ms": round(evaluation_wall_ms, 3),
                "formula_output_count": output_count,
                "output_digest_ms": round(output_digest_ms, 3),
                "profile_rank_ms": round(profile_rank_ms, 3),
                "formula_output_sha256": output_sha,
                "recalc": recalc_summary,
                "scc_tasks_evaluated": recalc.scc_tasks_evaluated,
                "scc_member_evaluations": recalc.scc_member_evaluations,
                "scc_profile_rank": profile_rank,
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
