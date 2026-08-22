# Formualizer UI Worker

This package is the UI-independent browser boundary for Formualizer.

The worker owns the WASM workbook. UI code communicates through `SpreadsheetBackend` and never
holds or calls the Formualizer workbook directly. Canvas, Handsontable, and custom views can use
the same backend.

## Development

Build the WASM package first, then install and build this package:

```bash
pnpm --dir bindings/wasm install --frozen-lockfile
pnpm --dir bindings/wasm build
pnpm --dir webapp/formualizer install
pnpm --dir webapp/formualizer build
```

The current protocol supports:

- XLSX byte loading;
- typed viewport reads through `readCellWindow`;
- raw user input commits;
- full or targeted evaluation;
- revision stamps;
- undo and redo;
- structured worker errors.
