from __future__ import annotations

import os
from pathlib import Path

os.environ["FZ_DIAGNOSTIC_EDGE_DUMP_PATH"] = r"C:\rust_engines\formualizer-v0.8.4\docs\issue-solutions\data\heavy-scc-edge-dump.tsv"
os.environ["FZ_DIAGNOSTIC_STATIC_EDGE_DUMP_PATH"] = r"C:\rust_engines\formualizer-v0.8.4\docs\issue-solutions\data\heavy-static-scc-edge-dump.tsv"
os.environ["FZ_TRACE_EDGE_ORIGINS"] = "1"

import formualizer as fz


def main() -> None:
    evaluation = fz.EvaluationConfig()
    evaluation.enable_parallel = True
    evaluation.cycle_detection = "runtime"
    evaluation.cycle_policy = "iterate"
    evaluation.iterate_max_iterations = 100
    evaluation.iterate_max_change = 0.001
    workbook = fz.Workbook.load_path(
        r"C:\Users\OXK0A0A\Downloads\Fossil_EstimatingTemplate_2026-08_21_A.xlsx",
        backend="calamine",
        config=fz.WorkbookConfig(eval_config=evaluation),
    )
    workbook.set_value("Inputs", 7, 6, 300)
    workbook.evaluate_all()
    workbook.static_scc_probe()
    path = Path(os.environ["FZ_DIAGNOSTIC_EDGE_DUMP_PATH"])
    static_path = Path(os.environ["FZ_DIAGNOSTIC_STATIC_EDGE_DUMP_PATH"])
    print(f"Generated {path} ({path.stat().st_size} bytes)")
    print(f"Generated {static_path} ({static_path.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
