import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ChangeEvent,
  type KeyboardEvent,
} from 'react';
import { CanvasGrid, VISIBLE_COLUMNS, VISIBLE_ROWS } from './CanvasGrid.js';
import { createFormualizerWorkerBackend, type SpreadsheetBackend } from './backend.js';
import type {
  CellSnapshot,
  CellWindowRequest,
  RevisionStamp,
  ViewportSnapshot,
  WorkbookSnapshot,
} from './protocol.js';
import './styles.css';

const DEFAULT_WINDOW_HEIGHT = VISIBLE_ROWS;
const DEFAULT_WINDOW_WIDTH = VISIBLE_COLUMNS;

type SelectedCell = CellWindowRequest;

type AppStatus = 'loading' | 'ready' | 'busy' | 'error';

function valueToInput(value: unknown): string {
  if (value === null || value === undefined) {
    return '';
  }
  if (typeof value === 'object') {
    const candidate = value as { kind?: string; code?: string };
    if (candidate.kind === 'error') {
      return `#${(candidate.code ?? 'VALUE').toUpperCase()}!`;
    }
    return '';
  }
  return String(value);
}

function inputForCell(cell: CellSnapshot | undefined): string {
  if (!cell) {
    return '';
  }
  return cell.formula ?? valueToInput(cell.value);
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function columnName(column: number): string {
  let value = column;
  let name = '';
  while (value > 0) {
    const remainder = (value - 1) % 26;
    name = String.fromCharCode(65 + remainder) + name;
    value = Math.floor((value - 1) / 26);
  }
  return name;
}

export default function App() {
  const backendRef = useRef<SpreadsheetBackend | null>(null);
  const refreshSequence = useRef(0);
  const stampRef = useRef<RevisionStamp | null>(null);
  const [workbook, setWorkbook] = useState<WorkbookSnapshot | null>(null);
  const [snapshot, setSnapshot] = useState<ViewportSnapshot | null>(null);
  const [selected, setSelected] = useState<SelectedCell>({
    sheet: 'Sheet1',
    startRow: 1,
    startColumn: 1,
    endRow: 1,
    endColumn: 1,
  });
  const [startRow, setStartRow] = useState(1);
  const [startColumn, setStartColumn] = useState(1);
  const [draft, setDraft] = useState('');
  const [editing, setEditing] = useState(false);
  const [status, setStatus] = useState<AppStatus>('loading');
  const [message, setMessage] = useState('Starting Formualizer…');

  const selectedSnapshot = useMemo(
    () =>
      snapshot?.items.find(
        (cell) =>
          cell.address.sheet === selected.sheet &&
          cell.address.row === selected.startRow &&
          cell.address.column === selected.startColumn,
      ),
    [selected, snapshot],
  );

  const refreshWindow = useCallback(
    async (
      sheet: string,
      row: number,
      column: number,
      expectedStamp?: RevisionStamp | null,
    ) => {
      const backend = backendRef.current;
      if (!backend) {
        return;
      }
      const sequence = ++refreshSequence.current;
      try {
        const result = await backend.readCellWindow(
          {
            sheet,
            startRow: row,
            startColumn: column,
            endRow: row + DEFAULT_WINDOW_HEIGHT - 1,
            endColumn: column + DEFAULT_WINDOW_WIDTH - 1,
          },
          expectedStamp ? { expectedStamp } : undefined,
        );
        if (sequence !== refreshSequence.current) {
          return;
        }
        stampRef.current = result.stamp;
        setSnapshot(result);
        setStatus('ready');
        setMessage('Ready');
      } catch (error) {
        if (sequence !== refreshSequence.current) {
          return;
        }
        setStatus('error');
        setMessage(errorMessage(error));
      }
    },
    [],
  );

  const applyWorkbookSnapshot = useCallback(
    (nextWorkbook: WorkbookSnapshot) => {
      const nextSheet = nextWorkbook.sheetNames[0] ?? 'Sheet1';
      stampRef.current = nextWorkbook.stamp;
      setWorkbook(nextWorkbook);
      setSnapshot(null);
      setStartRow(1);
      setStartColumn(1);
      setSelected({
        sheet: nextSheet,
        startRow: 1,
        startColumn: 1,
        endRow: 1,
        endColumn: 1,
      });
      setEditing(false);
    },
    [],
  );

  useEffect(() => {
    const backend = createFormualizerWorkerBackend();
    backendRef.current = backend;
    let active = true;
    void backend
      .createWorkbook()
      .then((nextWorkbook) => {
        if (active) {
          applyWorkbookSnapshot(nextWorkbook);
        }
      })
      .catch((error) => {
        if (active) {
          setStatus('error');
          setMessage(errorMessage(error));
        }
      });
    return () => {
      active = false;
      backend.dispose();
      backendRef.current = null;
    };
  }, [applyWorkbookSnapshot]);

  useEffect(() => {
    const declared = snapshot?.declared;
    const needsRefresh =
      workbook &&
      (!declared ||
        declared.sheet !== selected.sheet ||
        declared.startRow !== startRow ||
        declared.startColumn !== startColumn);
    if (needsRefresh) {
      void refreshWindow(selected.sheet, startRow, startColumn, stampRef.current);
    }
  }, [selected.sheet, startColumn, startRow, snapshot?.declared, workbook, refreshWindow]);

  useEffect(() => {
    setDraft(inputForCell(selectedSnapshot));
  }, [selectedSnapshot]);

  const selectCell = useCallback((row: number, column: number) => {
    setSelected((current) => ({
      ...current,
      startRow: row,
      startColumn: column,
      endRow: row,
      endColumn: column,
    }));
    setEditing(false);
  }, []);

  const navigate = useCallback((rowDelta: number, columnDelta: number) => {
    setSelected((current) => {
      const row = Math.max(1, current.startRow + rowDelta);
      const column = Math.max(1, current.startColumn + columnDelta);
      setStartRow((value) => {
        if (row < value) return row;
        if (row >= value + DEFAULT_WINDOW_HEIGHT) return row - DEFAULT_WINDOW_HEIGHT + 1;
        return value;
      });
      setStartColumn((value) => {
        if (column < value) return column;
        if (column >= value + DEFAULT_WINDOW_WIDTH) return column - DEFAULT_WINDOW_WIDTH + 1;
        return value;
      });
      return {
        ...current,
        startRow: row,
        startColumn: column,
        endRow: row,
        endColumn: column,
      };
    });
    setEditing(false);
  }, []);

  const commitInput = useCallback(
    async (input: string) => {
      const backend = backendRef.current;
      if (!backend) {
        return;
      }
      setStatus('busy');
      setMessage('Calculating…');
      try {
        const result = await backend.commitUserInput(
          {
            sheet: selected.sheet,
            row: selected.startRow,
            column: selected.startColumn,
          },
          input,
          true,
        );
        stampRef.current = result.stamp;
        setEditing(false);
        setDraft(input);
        await refreshWindow(selected.sheet, startRow, startColumn, result.stamp);
      } catch (error) {
        setStatus('error');
        setMessage(errorMessage(error));
      }
    },
    [refreshWindow, selected, startColumn, startRow],
  );

  const runHistory = useCallback(
    async (operation: 'undo' | 'redo') => {
      const backend = backendRef.current;
      if (!backend) {
        return;
      }
      setStatus('busy');
      setMessage(operation === 'undo' ? 'Undoing…' : 'Redoing…');
      try {
        const result = await backend[operation]();
        stampRef.current = result.stamp;
        await refreshWindow(selected.sheet, startRow, startColumn, result.stamp);
      } catch (error) {
        setStatus('error');
        setMessage(errorMessage(error));
      }
    },
    [refreshWindow, selected.sheet, startColumn, startRow],
  );

  const loadFile = useCallback(
    async (event: ChangeEvent<HTMLInputElement>) => {
      const file = event.target.files?.[0];
      const backend = backendRef.current;
      if (!file || !backend) {
        return;
      }
      setStatus('loading');
      setMessage(`Loading ${file.name}…`);
      try {
        const result = await backend.loadXlsx(new Uint8Array(await file.arrayBuffer()));
        applyWorkbookSnapshot(result);
      } catch (error) {
        setStatus('error');
        setMessage(errorMessage(error));
      } finally {
        event.target.value = '';
      }
    },
    [applyWorkbookSnapshot],
  );

  const handleGridKeyDown = useCallback(
    (event: KeyboardEvent<HTMLCanvasElement>) => {
      if (editing) {
        return;
      }
      if (event.key === 'Enter' || event.key === 'F2') {
        event.preventDefault();
        setEditing(true);
        return;
      }
      if (event.key === 'ArrowUp') {
        event.preventDefault();
        navigate(-1, 0);
      } else if (event.key === 'ArrowDown') {
        event.preventDefault();
        navigate(1, 0);
      } else if (event.key === 'ArrowLeft') {
        event.preventDefault();
        navigate(0, -1);
      } else if (event.key === 'ArrowRight') {
        event.preventDefault();
        navigate(0, 1);
      } else if (event.key === 'Delete' || event.key === 'Backspace') {
        event.preventDefault();
        void commitInput('');
      } else if (event.key.length === 1 && !event.ctrlKey && !event.metaKey && !event.altKey) {
        event.preventDefault();
        setDraft(event.key);
        setEditing(true);
      }
    },
    [commitInput, editing, navigate],
  );

  const handleEditorKeyDown = useCallback(
    (event: KeyboardEvent<HTMLInputElement>) => {
      if (event.key === 'Enter') {
        event.preventDefault();
        void commitInput(draft);
      } else if (event.key === 'Escape') {
        event.preventDefault();
        setDraft(inputForCell(selectedSnapshot));
        setEditing(false);
      }
    },
    [commitInput, draft, selectedSnapshot],
  );

  const activeSheet = selected.sheet;
  const stamp = stampRef.current;

  return (
    <main className="app-shell">
      <header className="app-toolbar">
        <div className="brand">Formualizer</div>
        <label className="toolbar-button">
          Open XLSX
          <input type="file" accept=".xlsx" onChange={loadFile} />
        </label>
        <button type="button" className="toolbar-button" onClick={() => void runHistory('undo')}>
          Undo
        </button>
        <button type="button" className="toolbar-button" onClick={() => void runHistory('redo')}>
          Redo
        </button>
        <span className={`status status-${status}`}>{message}</span>
      </header>

      <section className="formula-toolbar">
        <div className="name-box">
          {activeSheet}!{columnName(selected.startColumn)}{selected.startRow}
        </div>
        <div className="formula-symbol">fx</div>
        <input
          className="formula-input"
          aria-label="Formula bar"
          value={draft}
          readOnly={!editing}
          onFocus={() => setEditing(true)}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={handleEditorKeyDown}
        />
        {editing && (
          <button type="button" className="commit-button" onClick={() => void commitInput(draft)}>
            Apply
          </button>
        )}
      </section>

      <section className="grid-panel">
        <CanvasGrid
          snapshot={snapshot}
          selected={selected}
          onSelect={selectCell}
          onEdit={() => setEditing(true)}
          onNavigate={navigate}
          onKeyDown={handleGridKeyDown}
        />
      </section>

      <footer className="sheet-tabs">
        <div className="sheet-tab-list">
          {(workbook?.sheetNames ?? [activeSheet]).map((sheetName) => (
            <button
              type="button"
              className={sheetName === activeSheet ? 'sheet-tab active' : 'sheet-tab'}
              key={sheetName}
              onClick={() => {
                setSelected({
                  sheet: sheetName,
                  startRow: 1,
                  startColumn: 1,
                  endRow: 1,
                  endColumn: 1,
                });
                setStartRow(1);
                setStartColumn(1);
                setEditing(false);
              }}
            >
              {sheetName}
            </button>
          ))}
        </div>
        <span className="revision">
          rev {stamp?.mutationRevision ?? '—'} · recalc {stamp?.recalcEpoch ?? '—'}
        </span>
      </footer>
    </main>
  );
}
