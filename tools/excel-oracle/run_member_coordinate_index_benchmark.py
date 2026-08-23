from __future__ import annotations

import argparse
import json
import os
import time
from pathlib import Path
from typing import Any

import formualizer as fz


MAIN_SCC_ID = 1321560910633541638


def config() -> Any:
    evaluation = fz.EvaluationConfig()
    evaluation.enable_parallel = True
    evaluation.cycle_detection = "runtime"
    evaluation.cycle_policy = "iterate"
    evaluation.iterate_max_iterations = 100
    evaluation.iterate_max_change = 0.001
    return fz.WorkbookConfig(eval_config=evaluation)


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
        "member_count": row["member_count"],
        "static_member_count": row["static_member_count"],
        "live_cycle_count": row["live_cycle_count"],
        "live_cycle_member_count": row["live_cycle_member_count"],
        "live_edge_fingerprint": row["live_edge_fingerprint"],
        "static_live_edge_count": row["static_live_edge_count"],
    }


def run_fossil(path: str, mode: str, compare_origins: bool) -> list[dict[str, Any]]:
    os.environ["FZ_SCC_MEMBER_COORDINATE_INDEX_MODE"] = mode
    os.environ["FZ_TRACE_SCC_ITERATIONS"] = "1"
    if compare_origins:
        os.environ["FZ_TRACE_EDGE_ORIGINS"] = "1"
    else:
        os.environ.pop("FZ_TRACE_EDGE_ORIGINS", None)
    workbook = fz.Workbook.load_path(path, backend="calamine", config=config())
    phases = []
    for label, edit in [
        ("initial", None),
        ("f7_300", ("Inputs", 7, 6, 300)),
        ("noop", None),
    ]:
        if edit is not None:
            workbook.set_value(*edit)
        started = time.perf_counter()
        workbook.evaluate_all()
        trace = [
            row
            for row in workbook.last_scc_iteration_trace()
            if row["stable_id"] == MAIN_SCC_ID
        ]
        parity = [
            row
            for row in workbook.last_scc_collector_parity()
            if row["stable_id"] == MAIN_SCC_ID
        ]
        phases.append(
            {
                "label": label,
                "wall_ms": round((time.perf_counter() - started) * 1000, 3),
                "coordinate_index_build_ns": workbook.last_scc_coordinate_index_build_ns(),
                "formula_value_fingerprint": workbook.formula_value_fingerprint(),
                "scc_passes": trace,
                "runtime": main_runtime(workbook),
                "collector_parity": parity,
            }
        )
    return phases


def run_synthetic(mode: str, compare_origins: bool) -> list[dict[str, Any]]:
    os.environ["FZ_SCC_MEMBER_COORDINATE_INDEX_MODE"] = mode
    if compare_origins:
        os.environ["FZ_TRACE_EDGE_ORIGINS"] = "1"
    else:
        os.environ.pop("FZ_TRACE_EDGE_ORIGINS", None)
    workbook = fz.Workbook(config=config())
    workbook.add_sheet("S")
    workbook.set_formula("S", 1, 1, "=B1+1")
    workbook.set_formula("S", 1, 2, "=A1/2")
    phases = []
    for label in ["initial", "noop"]:
        started = time.perf_counter()
        workbook.evaluate_all()
        phases.append(
            {
                "label": label,
                "wall_ms": round((time.perf_counter() - started) * 1000, 3),
                "coordinate_index_build_ns": workbook.last_scc_coordinate_index_build_ns(),
                "formula_value_fingerprint": workbook.formula_value_fingerprint(),
                "collector_parity": workbook.last_scc_collector_parity(),
            }
        )
    return phases


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--fossil", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    result: dict[str, Any] = {
        "schema": "formualizer.scc-member-coordinate-index/v1",
        "settings": {
            "cycle_detection": "runtime",
            "cycle_policy": "iterate",
            "max_iterations": 100,
            "max_change": 0.001,
            "main_scc_id": MAIN_SCC_ID,
        },
        "modes": {},
    }
    for mode in ["indexed", "legacy"]:
        result["modes"][mode] = {
            "fossil": run_fossil(args.fossil, mode, False),
            "synthetic": run_synthetic(mode, False),
        }
    result["modes"]["compare"] = {
        "fossil": run_fossil(args.fossil, "compare", True),
        "synthetic": run_synthetic("compare", True),
    }
    fossil_modes = result["modes"]
    result["parity"] = {
        "legacy_indexed_formula_fingerprints_equal": all(
            left["formula_value_fingerprint"] == right["formula_value_fingerprint"]
            for left, right in zip(
                fossil_modes["legacy"]["fossil"], fossil_modes["indexed"]["fossil"]
            )
        ),
        "legacy_indexed_runtime_equal": all(
            left["runtime"] == right["runtime"]
            for left, right in zip(
                fossil_modes["legacy"]["fossil"], fossil_modes["indexed"]["fossil"]
            )
        ),
        "compare_edge_sets_equal": all(
            row["edge_set_equal"]
            for phase in fossil_modes["compare"]["fossil"]
            for row in phase["collector_parity"]
        ),
        "compare_fingerprints_equal": all(
            row["indexed_edge_fingerprint"] == row["legacy_edge_fingerprint"]
            for phase in fossil_modes["compare"]["fossil"]
            for row in phase["collector_parity"]
        ),
        "compare_origin_masks_equal": all(
            row["origin_map_equal"]
            for phase in fossil_modes["compare"]["fossil"]
            for row in phase["collector_parity"]
        ),
        "compare_formula_fingerprints_equal": all(
            phase["formula_value_fingerprint"]
            == fossil_modes["indexed"]["fossil"][index]["formula_value_fingerprint"]
            for index, phase in enumerate(fossil_modes["compare"]["fossil"])
        ),
        "synthetic_compare_edge_sets_equal": all(
            row["edge_set_equal"]
            for phase in fossil_modes["compare"]["synthetic"]
            for row in phase["collector_parity"]
        ),
    }
    result["parity"]["all_required_parity"] = all(result["parity"].values())
    args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    for variable in [
        "FZ_SCC_MEMBER_COORDINATE_INDEX_MODE",
        "FZ_TRACE_SCC_ITERATIONS",
        "FZ_TRACE_EDGE_ORIGINS",
    ]:
        os.environ.pop(variable, None)
    print(f"Generated {args.output}")


if __name__ == "__main__":
    main()
