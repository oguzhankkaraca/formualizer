import {
  useEffect,
  useRef,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
} from 'react';
import type { CellSnapshot, CellWindowRequest, ViewportSnapshot } from './protocol.js';

export const VISIBLE_ROWS = 28;
export const VISIBLE_COLUMNS = 14;
export const ROW_HEADER_WIDTH = 54;
export const COLUMN_HEADER_HEIGHT = 26;
export const CELL_WIDTH = 112;
export const CELL_HEIGHT = 26;

interface CanvasGridProps {
  snapshot: ViewportSnapshot | null;
  selected: CellWindowRequest;
  onSelect: (row: number, column: number) => void;
  onEdit: () => void;
  onNavigate: (rowDelta: number, columnDelta: number) => void;
  onKeyDown: (event: ReactKeyboardEvent<HTMLCanvasElement>) => void;
  editing: boolean;
  editValue: string;
  onEditValueChange: (value: string) => void;
  onCommitEdit: () => void;
  onCancelEdit: () => void;
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

function cellText(cell: CellSnapshot | undefined): string {
  if (!cell || !cell.valueIncluded || cell.value === null || cell.value === undefined) {
    return '';
  }
  if (typeof cell.value === 'object') {
    const value = cell.value as { kind?: string; code?: string };
    if (value.kind === 'error') {
      return `#${(value.code ?? 'VALUE').toUpperCase()}!`;
    }
    return '[array]';
  }
  return String(cell.value);
}

function keyFor(row: number, column: number): string {
  return `${row}:${column}`;
}

function viewportFromSnapshot(snapshot: ViewportSnapshot | null): CellWindowRequest | null {
  if (!snapshot?.resolved) {
    return null;
  }
  return snapshot.resolved;
}

export function CanvasGrid({
  snapshot,
  selected,
  onSelect,
  onEdit,
  onNavigate,
  onKeyDown,
  editing,
  editValue,
  onEditValueChange,
  onCommitEdit,
  onCancelEdit,
}: CanvasGridProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const editorRef = useRef<HTMLInputElement>(null);
  const viewport = viewportFromSnapshot(snapshot) ?? {
    sheet: selected.sheet,
    startRow: 1,
    startColumn: 1,
    endRow: VISIBLE_ROWS,
    endColumn: VISIBLE_COLUMNS,
  };

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) {
      return;
    }
    const scale = window.devicePixelRatio || 1;
    const width = ROW_HEADER_WIDTH + VISIBLE_COLUMNS * CELL_WIDTH;
    const height = COLUMN_HEADER_HEIGHT + VISIBLE_ROWS * CELL_HEIGHT;
    canvas.width = width * scale;
    canvas.height = height * scale;
    canvas.style.width = `${width}px`;
    canvas.style.height = `${height}px`;
    const context = canvas.getContext('2d');
    if (!context) {
      return;
    }
    context.resetTransform();
    context.scale(scale, scale);
    context.clearRect(0, 0, width, height);
    context.font = '13px Segoe UI, sans-serif';
    context.textBaseline = 'middle';

    const cells = new Map(
      (snapshot?.items ?? []).map((cell) => [
        keyFor(cell.address.row, cell.address.column),
        cell,
      ]),
    );

    context.fillStyle = '#f4f6f8';
    context.fillRect(0, 0, width, COLUMN_HEADER_HEIGHT);
    context.fillRect(0, 0, ROW_HEADER_WIDTH, height);
    context.fillStyle = '#ffffff';
    context.fillRect(ROW_HEADER_WIDTH, COLUMN_HEADER_HEIGHT, width, height);

    context.strokeStyle = '#d9dee5';
    context.lineWidth = 1;
    for (let column = 0; column <= VISIBLE_COLUMNS; column += 1) {
      const x = ROW_HEADER_WIDTH + column * CELL_WIDTH + 0.5;
      context.beginPath();
      context.moveTo(x, COLUMN_HEADER_HEIGHT);
      context.lineTo(x, height);
      context.stroke();
    }
    for (let row = 0; row <= VISIBLE_ROWS; row += 1) {
      const y = COLUMN_HEADER_HEIGHT + row * CELL_HEIGHT + 0.5;
      context.beginPath();
      context.moveTo(ROW_HEADER_WIDTH, y);
      context.lineTo(width, y);
      context.stroke();
    }
    context.strokeStyle = '#c4cad3';
    context.beginPath();
    context.moveTo(ROW_HEADER_WIDTH + 0.5, 0);
    context.lineTo(ROW_HEADER_WIDTH + 0.5, height);
    context.moveTo(0, COLUMN_HEADER_HEIGHT + 0.5);
    context.lineTo(width, COLUMN_HEADER_HEIGHT + 0.5);
    context.stroke();

    context.fillStyle = '#4b5563';
    context.textAlign = 'center';
    for (let column = 0; column < VISIBLE_COLUMNS; column += 1) {
      context.fillText(
        columnName(viewport.startColumn + column),
        ROW_HEADER_WIDTH + column * CELL_WIDTH + CELL_WIDTH / 2,
        COLUMN_HEADER_HEIGHT / 2,
      );
    }
    for (let row = 0; row < VISIBLE_ROWS; row += 1) {
      context.fillText(
        String(viewport.startRow + row),
        ROW_HEADER_WIDTH / 2,
        COLUMN_HEADER_HEIGHT + row * CELL_HEIGHT + CELL_HEIGHT / 2,
      );
    }

    context.textAlign = 'left';
    for (let row = 0; row < VISIBLE_ROWS; row += 1) {
      for (let column = 0; column < VISIBLE_COLUMNS; column += 1) {
        const rowNumber = viewport.startRow + row;
        const columnNumber = viewport.startColumn + column;
        const cell = cells.get(keyFor(rowNumber, columnNumber));
        const isSelected =
          selected.sheet === viewport.sheet &&
          selected.startRow === rowNumber &&
          selected.startColumn === columnNumber;
        const x = ROW_HEADER_WIDTH + column * CELL_WIDTH;
        const y = COLUMN_HEADER_HEIGHT + row * CELL_HEIGHT;
        if (isSelected) {
          context.fillStyle = '#e8f0fe';
          context.fillRect(x + 1, y + 1, CELL_WIDTH - 2, CELL_HEIGHT - 2);
        }
        const text = cellText(cell);
        if (text) {
          context.fillStyle = text.startsWith('#') ? '#b42318' : '#1f2937';
          context.fillText(text.slice(0, 18), x + 7, y + CELL_HEIGHT / 2);
        }
      }
    }

    const selectedRow = selected.startRow - viewport.startRow;
    const selectedColumn = selected.startColumn - viewport.startColumn;
    if (
      selectedRow >= 0 &&
      selectedRow < VISIBLE_ROWS &&
      selectedColumn >= 0 &&
      selectedColumn < VISIBLE_COLUMNS
    ) {
      context.strokeStyle = '#1769aa';
      context.lineWidth = 2;
      context.strokeRect(
        ROW_HEADER_WIDTH + selectedColumn * CELL_WIDTH + 1,
        COLUMN_HEADER_HEIGHT + selectedRow * CELL_HEIGHT + 1,
        CELL_WIDTH - 2,
        CELL_HEIGHT - 2,
      );
    }
  }, [selected, snapshot, viewport]);

  const cellAtEvent = (event: ReactMouseEvent<HTMLCanvasElement>) => {
    const bounds = event.currentTarget.getBoundingClientRect();
    const x = event.clientX - bounds.left;
    const y = event.clientY - bounds.top;
    if (x < ROW_HEADER_WIDTH || y < COLUMN_HEADER_HEIGHT) {
      return null;
    }
    return {
      row: viewport.startRow + Math.floor((y - COLUMN_HEADER_HEIGHT) / CELL_HEIGHT),
      column: viewport.startColumn + Math.floor((x - ROW_HEADER_WIDTH) / CELL_WIDTH),
    };
  };

  const selectedRowIndex = selected.startRow - viewport.startRow;
  const selectedColumnIndex = selected.startColumn - viewport.startColumn;
  const editorVisible =
    editing &&
    selected.sheet === viewport.sheet &&
    selectedRowIndex >= 0 &&
    selectedRowIndex < VISIBLE_ROWS &&
    selectedColumnIndex >= 0 &&
    selectedColumnIndex < VISIBLE_COLUMNS;

  useEffect(() => {
    if (editorVisible) {
      editorRef.current?.focus();
      editorRef.current?.setSelectionRange(editValue.length, editValue.length);
    }
  }, [editValue.length, editorVisible]);

  return (
    <div className="grid-shell">
      <canvas
        ref={canvasRef}
        className="grid-canvas"
        aria-label="Formualizer spreadsheet grid"
        tabIndex={0}
        onKeyDown={onKeyDown}
        onPointerDown={(event) => {
          const cell = cellAtEvent(event);
          if (cell) {
            onSelect(cell.row, cell.column);
          }
          event.currentTarget.focus();
        }}
        onDoubleClick={(event) => {
          if (cellAtEvent(event)) {
            onEdit();
          }
        }}
        onWheel={(event) => {
          onNavigate(event.deltaY > 0 ? 3 : -3, event.deltaX > 0 ? 3 : -3);
        }}
      />
      {editorVisible && (
        <input
          ref={editorRef}
          className="cell-editor"
          aria-label={`Edit ${columnName(selected.startColumn)}${selected.startRow}`}
          style={{
            left: `${ROW_HEADER_WIDTH + selectedColumnIndex * CELL_WIDTH + 1}px`,
            top: `${COLUMN_HEADER_HEIGHT + selectedRowIndex * CELL_HEIGHT + 1}px`,
            width: `${CELL_WIDTH - 2}px`,
            height: `${CELL_HEIGHT - 2}px`,
          }}
          value={editValue}
          onChange={(event) => onEditValueChange(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter') {
              event.preventDefault();
              onCommitEdit();
            } else if (event.key === 'Tab') {
              event.preventDefault();
              onCommitEdit();
              onNavigate(0, 1);
            } else if (event.key === 'Escape') {
              event.preventDefault();
              onCancelEdit();
            }
          }}
          onBlur={onCommitEdit}
          onPointerDown={(event) => event.stopPropagation()}
        />
      )}
    </div>
  );
}
