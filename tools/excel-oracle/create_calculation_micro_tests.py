from __future__ import annotations

import argparse
import json
from pathlib import Path
from zipfile import ZIP_DEFLATED, ZipFile

from openpyxl import Workbook


CASES = [
    {
        "id": "unused-if-cycle",
        "sheets": ["Sheet1"],
        "cells": {
            "Sheet1": {
                "A1": 0,
                "B1": "=IF(A1=0,0,C1+1)",
                "C1": "=B1/2",
            }
        },
        "targets": ["Sheet1!B1", "Sheet1!C1"],
        "mutations": [{"sheet": "Sheet1", "address": "A1", "value": 1}],
    },
    {
        "id": "active-if-cycle",
        "sheets": ["Sheet1"],
        "cells": {
            "Sheet1": {
                "A1": 1,
                "B1": "=IF(A1=1,C1+1,0)",
                "C1": "=B1/2",
            }
        },
        "targets": ["Sheet1!B1", "Sheet1!C1"],
        "mutations": [{"sheet": "Sheet1", "address": "A1", "value": 0}],
    },
    {
        "id": "indirect-target-change",
        "sheets": ["Sheet1"],
        "cells": {
            "Sheet1": {
                "A1": 10,
                "B1": 20,
                "D1": "A1",
                "C1": "=INDIRECT(D1)",
            }
        },
        "targets": ["Sheet1!C1"],
        "mutations": [{"sheet": "Sheet1", "address": "D1", "value": "B1"}],
    },
    {
        "id": "offset-target-change",
        "sheets": ["Sheet1"],
        "cells": {
            "Sheet1": {
                "A1": 10,
                "A2": 20,
                "B1": 1,
                "C1": "=OFFSET(A1,B1-1,0)",
            }
        },
        "targets": ["Sheet1!C1"],
        "mutations": [{"sheet": "Sheet1", "address": "B1", "value": 2}],
    },
    {
        "id": "filter-shape-change",
        "sheets": ["Sheet1"],
        "cells": {
            "Sheet1": {
                "A1": 10,
                "A2": 20,
                "A3": 30,
                "B1": 1,
                "B2": 1,
                "B3": 0,
                "C1": "=FILTER(A1:A3,B1:B3=1)",
            }
        },
        "targets": ["Sheet1!C1", "Sheet1!C2", "Sheet1!C3"],
        "mutations": [{"sheet": "Sheet1", "address": "B2", "value": 0}],
    },
    {
        "id": "two-cell-cycle",
        "sheets": ["Sheet1"],
        "cells": {"Sheet1": {"A1": "=B1+1", "B1": "=A1/2"}},
        "targets": ["Sheet1!A1", "Sheet1!B1"],
        "mutations": [],
    },
    {
        "id": "same-sheet-cycle",
        "sheets": ["Sheet1"],
        "cells": {
            "Sheet1": {"A1": 10, "B1": "=A1+C1", "C1": "=B1/2"}
        },
        "targets": ["Sheet1!B1", "Sheet1!C1"],
        "mutations": [{"sheet": "Sheet1", "address": "A1", "value": 20}],
    },
    {
        "id": "cross-sheet-cycle",
        "sheets": ["Sheet1", "Sheet2"],
        "cells": {
            "Sheet1": {"A1": "=Sheet2!A1+1"},
            "Sheet2": {"A1": "=Sheet1!A1/2"},
        },
        "targets": ["Sheet1!A1", "Sheet2!A1"],
        "mutations": [],
    },
]


def seed_formula_caches(path: Path) -> None:
    temporary = path.with_suffix(".seeded.xlsx")
    with ZipFile(path) as source, ZipFile(temporary, "w", ZIP_DEFLATED) as destination:
        for item in source.infolist():
            data = source.read(item.filename)
            if item.filename.startswith("xl/worksheets/") and item.filename.endswith(".xml"):
                data = data.replace(b"<v></v>", b"<v>0</v>")
                data = data.replace(b">FILTER(", b">_xlfn._xlws.FILTER(")
            destination.writestr(item, data)
    temporary.replace(path)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output-directory", type=Path, required=True)
    args = parser.parse_args()
    args.output_directory.mkdir(parents=True, exist_ok=True)

    for spec in CASES:
        workbook = Workbook()
        first = workbook.active
        first.title = spec["sheets"][0]
        for sheet_name in spec["sheets"][1:]:
            workbook.create_sheet(sheet_name)
        for sheet_name, cells in spec["cells"].items():
            sheet = workbook[sheet_name]
            for address, value in cells.items():
                sheet[address] = value
        path = args.output_directory / f"{spec['id']}.xlsx"
        workbook.calculation.iterate = True
        workbook.calculation.iterateCount = 100
        workbook.calculation.iterateDelta = 0.001
        workbook.calculation.calcMode = "manual"
        workbook.save(path)
        if spec["id"] in {
            "unused-if-cycle",
            "active-if-cycle",
            "two-cell-cycle",
            "same-sheet-cycle",
            "cross-sheet-cycle",
            "filter-shape-change",
        }:
            seed_formula_caches(path)

    manifest = {
        "schema": "formualizer.excel-calculation-micro-tests/v1",
        "output_directory": str(args.output_directory.resolve()),
        "cases": CASES,
    }
    (args.output_directory / "manifest.json").write_text(
        json.dumps(manifest, indent=2) + "\n", encoding="utf-8"
    )
    print(f"Generated {len(CASES)} micro-workbooks and manifest.json")


if __name__ == "__main__":
    main()
