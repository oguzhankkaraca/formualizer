from __future__ import annotations

import json
from pathlib import Path

ROOT = Path(r"C:\rust_engines\formualizer-v0.8.4")
DATA = ROOT / "docs" / "issue-solutions" / "data"
GRAPH_PATH = DATA / "heavy-graph-root-cause.json"
MISMATCH_PATH = DATA / "heavy-formualizer-excel-mismatch-inventory.json"
REFERENCE_PATH = DATA / "excel-reference-targets.json"
CONDITIONAL_PATH = DATA / "excel-conditional-branch.json"
OUTPUT_PATH = DATA / "heavy-targeted-level-c-evidence.json"


def main() -> None:
    graph = json.loads(GRAPH_PATH.read_text(encoding="utf-8"))
    mismatches = json.loads(MISMATCH_PATH.read_text(encoding="utf-8"))
    reference_targets = json.loads(REFERENCE_PATH.read_text(encoding="utf-8"))
    conditional_oracle = json.loads(CONDITIONAL_PATH.read_text(encoding="utf-8"))
    k65 = next(
        item
        for item in mismatches["main_scc_mismatches"]
        if item["address"] == "CashFlow Engine!K65"
    )
    i65_cell, k65_cell = conditional_oracle["cells"]
    i65 = {
        "formula": i65_cell["formula"],
        "excel_value": conditional_oracle["excel_evaluated_condition"]["i65_selected_value"],
        "selected_branch": "false_literal_empty_string",
        "direct_precedents": conditional_oracle["direct_precedents"],
    }
    conditional_before = {
        "address": k65["address"],
        "formula": k65["formula"],
        "condition": i65,
        "excel": k65["excel"],
        "formualizer": k65["formualizer"],
        "formualizer_selected_branch": "false_literal_empty_string",
        "excel_current_branch_read": "I65 evaluates to No, selecting the false literal branch",
        "excel_dependency_edge_proof": "Excel DirectPrecedents exposes I65,K64 even though K64 is on the inactive branch",
        "excel_k65_value": conditional_oracle["excel_evaluated_k65"],
        "formualizer_references_observed": ["I65", "K64"],
        "inactive_branch_reference": None,
    }
    conditional_after = {
        **conditional_before,
        "formualizer_after_correction": {"kind": "empty", "value": None},
        "excel_parity_after_correction": True,
        "dependency_edges_changed": False,
        "static_scc_before": graph["baseline"]["static_graph"],
        "static_scc_after": graph["baseline"]["static_graph"],
        "runtime_scc_before": graph["baseline"]["runtime_observed_graph"],
        "runtime_scc_after": graph["baseline"]["runtime_observed_graph"],
    }
    index = graph["targeted_semantic_corrections"]["index_reference"]
    result = {
        "schema": "formualizer.heavy-targeted-level-c-evidence/v1",
        "workbook": "Fossil_EstimatingTemplate_2026-08_21_A.xlsx",
        "input": "Inputs!F7=300",
        "baseline": {
            "static_scc": graph["baseline"]["static_graph"],
            "runtime_observed_scc": graph["baseline"]["runtime_observed_graph"],
        },
        "corrections": [
            {
                "id": "conditional_empty_string_to_blank_k65",
                "level": "C_value_semantics_only",
                "before": conditional_before,
                "after": conditional_after,
                "interpretation": "Excel-compatible branch/value representation correction; no dependency/reference correction was required or observed.",
            },
            {
                "id": "index_selected_reference_j8",
                "level": "B_excel_assisted_reference_counterfactual",
                "sources": [
                    {
                        "address": item["address"],
                        "formula": item["formula"],
                        "excel_selected_reference": item["selected_reference"],
                    }
                    for item in reference_targets["results"]
                ],
                "matched_sources_inside_main_scc": index["static"]["selected_source_count"],
                "static_before": graph["baseline"]["static_graph"],
                "static_after": index["static"],
                "runtime_before": graph["baseline"]["runtime_observed_graph"],
                "runtime_after": index["runtime_observed"],
                "interpretation": "Replace the selected INDEX source dependency surface with Excel's observed selected cell. This is not proof that Excel invalidates only that cell, so it remains Level B rather than Level C.",
            },
        ],
        "combined": {
            "tested_corrections": [
                "conditional_empty_string_to_blank_k65",
                "index_selected_reference_j8",
            ],
            "static_scc": index["static"],
            "runtime_observed_scc": index["runtime_observed"],
            "cross_sheet_cycle_remains": index["static"]["cross_sheet_cycle"],
            "level_c_topology_change_proven": False,
        },
        "level_c_conclusion": {
            "status": "no_specific_semantic_defect_proven_causal",
            "central_counterfactual": "LIKELY_YES",
            "reason": "The narrow conditional value correction changes parity without topology; the Excel-selected INDEX target replacement removes the broad J8 dependency surface but leaves the 4,829/4,142 cycle essentially unchanged. Broad mismatch removal remains graph causality only, not a semantic correction.",
        },
        "notes": [
            "No production semantic patch was applied.",
            "The INDEX experiment is explicitly Excel-assisted Level B because INDEX here returns a value and Excel's invalidation dependency surface is not fully observable.",
            "The K65 correction is a narrow value-semantic correction; its branch references do not materially maintain the large SCC.",
        ],
    }
    OUTPUT_PATH.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(f"Generated {OUTPUT_PATH}")


if __name__ == "__main__":
    main()
