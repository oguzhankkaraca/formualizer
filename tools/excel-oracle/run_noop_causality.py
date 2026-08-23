from __future__ import annotations

import argparse
import json
import time
from pathlib import Path
from typing import Any

import formualizer as fz


TEMPLATES = {
    "heavy": {
        "path": r"C:\Users\OXK0A0A\Downloads\Fossil_EstimatingTemplate_2026-08_21_A.xlsx",
        "edit": ("Inputs", 7, 6, 300),
    },
    "light": {
        "path": r"C:\Users\OXK0A0A\Downloads\Fossil_EstimatingTemplate_2026-06-25_X_Fossil.xlsx",
        "edit": ("Inputs", 6, 6, 300),
    },
}


def config() -> Any:
    evaluation = fz.EvaluationConfig()
    evaluation.enable_parallel = True
    evaluation.cycle_detection = "runtime"
    evaluation.cycle_policy = "iterate"
    evaluation.iterate_max_iterations = 100
    evaluation.iterate_max_change = 0.001
    return fz.WorkbookConfig(eval_config=evaluation)


def summarize_main(per_scc: list[dict[str, Any]]) -> dict[str, Any] | None:
    if not per_scc:
        return None
    row = max(per_scc, key=lambda item: item["member_count"])
    return {
        key: row[key]
        for key in [
            "stable_id",
            "member_count",
            "naturally_dirty_member_count",
            "volatile_member_count",
            "dynamic_member_count",
            "volatile_redirty_member_count",
            "iterative_redirty_member_count",
            "volatile_member_samples",
            "dynamic_member_samples",
            "static_member_samples",
            "static_member_count",
            "static_live_edge_count",
            "live_cycle_count",
            "live_cycle_member_count",
            "live_edge_fingerprint",
            "converged",
            "exactly_stable",
            "capped",
            "reason",
        ]
    }


def run_template(name: str) -> dict[str, Any]:
    workbook = fz.Workbook.load_path(
        TEMPLATES[name]["path"], backend="calamine", config=config()
    )
    phases = []
    for label, edit in [
        ("initial", None),
        ("capacity_edit", TEMPLATES[name]["edit"]),
        ("noop", None),
        ("same_value_write", TEMPLATES[name]["edit"]),
    ]:
        if edit is not None:
            workbook.set_value(*edit)
        started = time.perf_counter()
        workbook.evaluate_all()
        recalc = workbook.last_recalc_telemetry()
        dirty = workbook.last_scc_dirty_telemetry()
        phases.append(
            {
                "label": label,
                "wall_ms": round((time.perf_counter() - started) * 1000, 3),
                "recalc": {
                    key: getattr(recalc, key)
                    for key in [
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
                        "volatile_vertices_redirtied",
                        "iterative_vertices_redirtied",
                    ]
                },
                "dirty": {
                    key: dirty[key]
                    for key in [
                        "dirty_at_request_start",
                        "vertices_added_since_attribution_baseline",
                        "naturally_dirty_before_redirty",
                        "dirty_after_volatile_redirty",
                        "dirty_after_iterative_redirty",
                        "vertices_added_solely_by_iterative_policy",
                        "sccs_intersecting_naturally_dirty",
                        "scc_cells_intersecting_naturally_dirty",
                        "sccs_added_solely_by_iterative_policy",
                        "scc_cells_added_solely_by_iterative_policy",
                        "dirty_root_sources",
                        "dirty_root_samples",
                        "iterative_state_value_count",
                        "request_snapshot_id",
                        "topology_epoch",
                        "graph_topology_revision",
                        "graph_symbol_revision",
                    ]
                },
                "main_scc": summarize_main(dirty["per_scc"]),
                "formula_value_fingerprint": workbook.formula_value_fingerprint(),
            }
        )
    return {"template": name, "phases": phases}


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    result = {
        "schema": "formualizer.heavy-light-noop-causality/v1",
        "settings": {
            "cycle_detection": "runtime",
            "cycle_policy": "iterate",
            "max_iterations": 100,
            "max_change": 0.001,
            "heavy_input": "Inputs!F7",
            "light_input": "Inputs!F6",
        },
        "templates": {name: run_template(name) for name in TEMPLATES},
    }
    args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(f"Generated {args.output}")


if __name__ == "__main__":
    main()
