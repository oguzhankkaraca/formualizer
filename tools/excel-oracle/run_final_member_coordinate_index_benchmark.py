from __future__ import annotations

import argparse
import json
import os
import random
import statistics
import subprocess
import sys
import time
from pathlib import Path
from typing import Any


MAIN_SCC_ID = 1321560910633541638
SAMPLES = 7

TEMPLATES = {
    "heavy": {
        "path": r"C:\Users\OXK0A0A\Downloads\Fossil_EstimatingTemplate_2026-08_21_A.xlsx",
        "input": ("Inputs", 7, 6, 300),
    },
    "light": {
        "path": r"C:\Users\OXK0A0A\Downloads\Fossil_EstimatingTemplate_2026-06-25_X_Fossil.xlsx",
        "input": ("Inputs", 6, 6, 300),
    },
}


def worker_config() -> Any:
    import formualizer as fz

    evaluation = fz.EvaluationConfig()
    evaluation.enable_parallel = True
    evaluation.cycle_detection = "runtime"
    evaluation.cycle_policy = "iterate"
    evaluation.iterate_max_iterations = 100
    evaluation.iterate_max_change = 0.001
    return fz.WorkbookConfig(eval_config=evaluation)


def main_scc_id_and_runtime(workbook: Any, pass_rows: list[dict[str, Any]]) -> tuple[int | None, dict[str, Any] | None]:
    runtime_rows = workbook.last_scc_dirty_telemetry()["per_scc"]
    if runtime_rows:
        runtime = max(runtime_rows, key=lambda row: row["member_count"])
        main_id = runtime["stable_id"]
        if pass_rows:
            pass_ids = {row["stable_id"] for row in pass_rows}
            if main_id not in pass_ids:
                main_id = max(pass_rows, key=lambda row: row["evaluated_members"])["stable_id"]
                runtime = next((row for row in runtime_rows if row["stable_id"] == main_id), runtime)
        return main_id, {
            "member_count": runtime["member_count"],
            "static_member_count": runtime["static_member_count"],
            "live_cycle_count": runtime["live_cycle_count"],
            "live_cycle_member_count": runtime["live_cycle_member_count"],
            "live_edge_fingerprint": runtime["live_edge_fingerprint"],
        }
    if pass_rows:
        main_id = max(pass_rows, key=lambda row: row["evaluated_members"])["stable_id"]
        return main_id, None
    return None, None


def run_worker(template: str, mode: str, compare: bool) -> dict[str, Any]:
    os.environ["FZ_SCC_MEMBER_COORDINATE_INDEX_MODE"] = mode
    os.environ["FZ_TRACE_SCC_PASS_PROFILE"] = "1"
    os.environ["FZ_TRACE_SCC_ITERATIONS"] = "1"
    if compare:
        os.environ["FZ_TRACE_EDGE_ORIGINS"] = "1"
    else:
        os.environ.pop("FZ_TRACE_EDGE_ORIGINS", None)

    import formualizer as fz

    definition = TEMPLATES[template]
    workbook = fz.Workbook.load_path(
        definition["path"], backend="calamine", config=worker_config()
    )
    input_sheet, input_row, input_col, input_value = definition["input"]
    phases = []
    for label, edit in [
        ("initial", None),
        ("capacity_edit", (input_sheet, input_row, input_col, input_value)),
        ("noop", None),
        ("same_value_write", (input_sheet, input_row, input_col, input_value)),
    ]:
        if edit is not None:
            workbook.set_value(*edit)
        started = time.perf_counter()
        workbook.evaluate_all()
        wall_ms = (time.perf_counter() - started) * 1000
        all_passes = list(workbook.last_scc_pass_profile())
        main_id, runtime = main_scc_id_and_runtime(workbook, all_passes)
        main_passes = [row for row in all_passes if row["stable_id"] == main_id]
        main_members = [
            row
            for row in workbook.last_scc_slowest_members(10000)
            if row["stable_id"] == main_id
        ]
        recalc = workbook.last_recalc_telemetry()
        phases.append(
            {
                "label": label,
                "wall_ms": round(wall_ms, 3),
                "formula_value_fingerprint": workbook.formula_value_fingerprint(),
                "coordinate_index_build_ns": workbook.last_scc_coordinate_index_build_ns(),
                "recalc": {
                    "planned_sccs": recalc.planned_sccs,
                    "scc_tasks_evaluated": recalc.scc_tasks_evaluated,
                    "scc_member_count": recalc.scc_member_count,
                    "scc_member_evaluations": recalc.scc_member_evaluations,
                },
                "main_scc_id": main_id,
                "runtime": runtime,
                "main_passes": main_passes,
                "top_slow_members": sorted(
                    main_members, key=lambda row: row["elapsed_ns"], reverse=True
                )[:10],
                "collector_parity": [
                    row
                    for row in workbook.last_scc_collector_parity()
                    if row["stable_id"] == main_id
                ],
            }
        )
    return {"template": template, "mode": mode, "compare": compare, "phases": phases}


def run_child(template: str, mode: str, compare: bool) -> dict[str, Any]:
    command = [
        sys.executable,
        str(Path(__file__).resolve()),
        "--worker",
        "--template",
        template,
        "--mode",
        mode,
    ]
    if compare:
        command.append("--compare")
    env = os.environ.copy()
    completed = subprocess.run(
        command,
        cwd=str(Path(__file__).resolve().parents[2]),
        env=env,
        capture_output=True,
        text=True,
        check=True,
        timeout=300,
    )
    return json.loads(completed.stdout)


def percentile(values: list[float], p: float) -> float:
    ordered = sorted(values)
    if len(ordered) == 1:
        return ordered[0]
    position = (len(ordered) - 1) * p
    lower = int(position)
    upper = min(lower + 1, len(ordered) - 1)
    fraction = position - lower
    return ordered[lower] + (ordered[upper] - ordered[lower]) * fraction


def summarize(rows: list[dict[str, Any]], metric: str) -> dict[str, Any]:
    values = [float(row[metric]) for row in rows]
    return {
        "n": len(values),
        "median": round(statistics.median(values), 3),
        "p95": round(percentile(values, 0.95), 3),
        "min": round(min(values), 3),
        "max": round(max(values), 3),
    }


def phase_metrics(result: dict[str, Any], phase: str) -> dict[str, Any]:
    row = next(item for item in result["phases"] if item["label"] == phase)
    passes = row["main_passes"]
    return {
        "wall_ms": row["wall_ms"],
        "scc_members": row["runtime"]["member_count"] if row["runtime"] else max(
            (item["evaluated_members"] for item in row["main_passes"]), default=0
        ),
        "membership_checks": sum(item["range_membership_checks"] for item in passes),
        "collector_ms": sum(item["collection_ns"] for item in passes) / 1_000_000,
        "main_scc_pass_ms": sum(item["elapsed_ns"] for item in passes) / 1_000_000,
        "index_build_ms": row["coordinate_index_build_ns"] / 1_000_000,
        "formula_fingerprint": row["formula_value_fingerprint"],
        "runtime_signature": row["runtime"],
    }


def run_parent() -> dict[str, Any]:
    rng = random.Random(20260823)
    output: dict[str, Any] = {
        "schema": "formualizer.final-scc-member-coordinate-index/v1",
        "samples": SAMPLES,
        "templates": {},
        "parity": {},
    }
    phases = ["initial", "capacity_edit", "noop", "same_value_write"]
    for template in TEMPLATES:
        output["templates"][template] = {"samples": [], "summary": {}}
        for sample in range(1, SAMPLES + 1):
            order = ["legacy", "indexed"]
            rng.shuffle(order)
            sample_row: dict[str, Any] = {"sample": sample, "order": order, "modes": {}}
            for mode in order:
                sample_row["modes"][mode] = run_child(template, mode, False)
            output["templates"][template]["samples"].append(sample_row)
        for phase in phases:
            output["templates"][template]["summary"][phase] = {}
            for mode in ["legacy", "indexed"]:
                rows = [
                    phase_metrics(sample["modes"][mode], phase)
                    for sample in output["templates"][template]["samples"]
                ]
                output["templates"][template]["summary"][phase][mode] = {
                    metric: summarize(rows, metric)
                    for metric in [
                        "scc_members",
                        "membership_checks",
                        "collector_ms",
                        "main_scc_pass_ms",
                        "index_build_ms",
                        "wall_ms",
                    ]
                }
        output["parity"][template] = {
            "legacy_indexed_formula_outputs": all(
                phase_metrics(sample["modes"]["legacy"], phase)["formula_fingerprint"]
                == phase_metrics(sample["modes"]["indexed"], phase)["formula_fingerprint"]
                for sample in output["templates"][template]["samples"]
                for phase in phases
            ),
            "legacy_indexed_runtime_signatures": all(
                phase_metrics(sample["modes"]["legacy"], phase)["runtime_signature"]
                == phase_metrics(sample["modes"]["indexed"], phase)["runtime_signature"]
                for sample in output["templates"][template]["samples"]
                for phase in phases
            ),
        }
        compare = run_child(template, "compare", True)
        compare_parity = [
            row
            for phase in compare["phases"]
            for row in phase["collector_parity"]
        ]
        output["parity"][template].update(
            {
                "compare_edge_sets": all(row["edge_set_equal"] for row in compare_parity),
                "compare_edge_fingerprints": all(
                    row["indexed_edge_fingerprint"] == row["legacy_edge_fingerprint"]
                    for row in compare_parity
                ),
                "compare_origin_masks": all(row["origin_map_equal"] for row in compare_parity),
                "compare_collector_exercised_or_no_scc": bool(compare_parity)
                or not any(phase["runtime"] for phase in compare["phases"]),
                "compare_records": len(compare_parity),
            }
        )
        output["parity"][template]["all_required_parity"] = all(
            value
            for key, value in output["parity"][template].items()
            if key != "compare_records"
        )
    output["all_required_parity"] = all(
        output["parity"][template]["all_required_parity"] for template in TEMPLATES
    )
    return output


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--worker", action="store_true")
    parser.add_argument("--template", choices=sorted(TEMPLATES))
    parser.add_argument("--mode", choices=["legacy", "indexed", "compare"])
    parser.add_argument("--compare", action="store_true")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.worker:
        print(json.dumps(run_worker(args.template, args.mode, args.compare), separators=(",", ":")))
        return
    if args.output is None:
        parser.error("--output is required outside worker mode")
    args.output.write_text(json.dumps(run_parent(), indent=2) + "\n", encoding="utf-8")
    print(f"Generated {args.output}")


if __name__ == "__main__":
    main()
