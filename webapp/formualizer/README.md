# Formualizer UI Worker

This package is the UI-independent browser boundary for Formualizer.

The worker owns the WASM workbook. UI code communicates through `SpreadsheetBackend` and never
holds or calls the Formualizer workbook directly. Canvas, Handsontable, and custom views can use
the same backend.

## Development

Install and build this package; its prebuild step creates the browser-target WASM package:

```bash
pnpm --dir webapp/formualizer install
pnpm --dir webapp/formualizer build
pnpm --dir webapp/formualizer dev
```

The current protocol supports:

- iterative calculation with runtime cycle detection (`100` passes, `0.001` threshold);
- empty workbook creation with a default `Sheet1`;
- XLSX byte loading without blocking load on full recalculation;
- explicit full recalculation through the UI action;
- typed viewport reads through `readCellWindow`;
- raw user input commits;
- full or targeted evaluation;
- revision stamps;
- undo and redo;
- structured worker errors.
