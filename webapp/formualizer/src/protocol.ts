export interface RevisionStamp {
  mutationRevision: string;
  recalcEpoch: string;
}

export interface CellAddress {
  sheet: string;
  row: number;
  column: number;
}

export interface CellWindowRequest {
  sheet: string;
  startRow: number;
  startColumn: number;
  endRow: number;
  endColumn: number;
}

export interface CellWindowOptions {
  includeValues?: boolean;
  expectedStamp?: RevisionStamp;
}

export interface CellSnapshot {
  address: CellAddress;
  formula: string | null;
  value: unknown | null;
  valueIncluded: boolean;
  staleness: 'current' | 'dirty' | 'neverEvaluated';
  volatile: boolean;
  spill: unknown | null;
}

export interface ViewportSnapshot {
  stamp: RevisionStamp;
  declared: CellWindowRequest;
  resolved: CellWindowRequest | null;
  total: number;
  offset: number;
  items: CellSnapshot[];
  nextOffset: number | null;
}

export interface WorkbookSnapshot {
  sheetNames: string[];
  stamp: RevisionStamp;
}

export interface CycleTelemetry {
  staticSccs: number;
  phantomSccs: number;
  liveCyclesWitnessed: number;
  circCellsStamped: number;
  settlePassesTotal: number;
  maxPassesSingleScc: number;
  iteratedSccs: number;
  convergedSccs: number;
  cappedSccs: number;
  maxAbsDeltaAtStop: number;
  nanConverged: number;
  elapsedMs: number;
}

export interface EvaluationResult {
  stamp: RevisionStamp;
  telemetry: CycleTelemetry;
}

export interface MutationResult {
  stamp: RevisionStamp;
}

export interface EvaluateRequest {
  targets?: CellAddress[];
}

export type WorkerRequest =
  | {
      requestId: string;
      type: 'loadXlsx';
      bytes: ArrayBuffer;
    }
  | {
      requestId: string;
      type: 'readCellWindow';
      window: CellWindowRequest;
      options?: CellWindowOptions;
    }
  | {
      requestId: string;
      type: 'setUserInput';
      cell: CellAddress;
      input: string;
      evaluate?: boolean;
    }
  | {
      requestId: string;
      type: 'evaluate';
      request?: EvaluateRequest;
    }
  | {
      requestId: string;
      type: 'stateStamp';
    }
  | {
      requestId: string;
      type: 'undo' | 'redo';
    };

type WithoutRequestId<T> = T extends { requestId: string } ? Omit<T, 'requestId'> : never;

export type WorkerRequestPayload = WithoutRequestId<WorkerRequest>;

export interface WorkerError {
  name: string;
  message: string;
  code?: string;
}

export type WorkerResponse =
  | {
      requestId: string;
      type: 'result';
      result: unknown;
    }
  | {
      requestId: string;
      type: 'error';
      error: WorkerError;
    };
