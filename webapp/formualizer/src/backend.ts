import type {
  CellWindowOptions,
  CellWindowRequest,
  EvaluateRequest,
  EvaluationResult,
  MutationResult,
  RevisionStamp,
  ViewportSnapshot,
  WorkerError,
  WorkerRequest,
  WorkerRequestPayload,
  WorkerResponse,
  WorkbookSnapshot,
} from './protocol.js';

export interface SpreadsheetBackend {
  createWorkbook(): Promise<WorkbookSnapshot>;
  loadXlsx(bytes: Uint8Array): Promise<WorkbookSnapshot>;
  readCellWindow(
    window: CellWindowRequest,
    options?: CellWindowOptions,
  ): Promise<ViewportSnapshot>;
  commitUserInput(
    cell: { sheet: string; row: number; column: number },
    input: string,
    evaluate?: boolean,
  ): Promise<MutationResult>;
  evaluate(request?: EvaluateRequest): Promise<EvaluationResult>;
  stateStamp(): Promise<RevisionStamp>;
  undo(): Promise<MutationResult>;
  redo(): Promise<MutationResult>;
  dispose(): void;
}

type MessageListener = (event: MessageEvent<WorkerResponse>) => void;
type ErrorListener = (event: ErrorEvent) => void;

export interface WorkerLike {
  postMessage(message: WorkerRequest, transfer?: Transferable[]): void;
  addEventListener(type: 'message', listener: MessageListener): void;
  addEventListener(type: 'error', listener: ErrorListener): void;
  removeEventListener(type: 'message', listener: MessageListener): void;
  removeEventListener(type: 'error', listener: ErrorListener): void;
  terminate(): void;
}

export class SpreadsheetBackendError extends Error {
  readonly code?: string;

  constructor(error: WorkerError) {
    super(error.message);
    this.name = error.name;
    this.code = error.code;
  }
}

interface PendingRequest {
  resolve: (value: unknown) => void;
  reject: (reason: unknown) => void;
}

export class WorkerBackend implements SpreadsheetBackend {
  private readonly pending = new Map<string, PendingRequest>();
  private nextRequestId = 0;
  private disposed = false;

  private readonly onMessage = (event: MessageEvent<WorkerResponse>): void => {
    const response = event.data;
    const pending = this.pending.get(response.requestId);
    if (!pending) {
      return;
    }
    this.pending.delete(response.requestId);
    if (response.type === 'error') {
      pending.reject(new SpreadsheetBackendError(response.error));
    } else {
      pending.resolve(response.result);
    }
  };

  private readonly onError = (event: ErrorEvent): void => {
    const error = new SpreadsheetBackendError({
      name: 'WorkerError',
      message: event.message || 'Formualizer worker failed',
      code: 'WORKER_ERROR',
    });
    for (const pending of this.pending.values()) {
      pending.reject(error);
    }
    this.pending.clear();
  };

  constructor(private readonly worker: WorkerLike) {
    worker.addEventListener('message', this.onMessage);
    worker.addEventListener('error', this.onError);
  }

  createWorkbook(): Promise<WorkbookSnapshot> {
    return this.request<WorkbookSnapshot>({ type: 'createWorkbook' });
  }

  loadXlsx(bytes: Uint8Array): Promise<WorkbookSnapshot> {
    const buffer = bytes.slice().buffer as ArrayBuffer;
    return this.request<WorkbookSnapshot>({ type: 'loadXlsx', bytes: buffer }, [buffer]);
  }

  readCellWindow(
    window: CellWindowRequest,
    options?: CellWindowOptions,
  ): Promise<ViewportSnapshot> {
    return this.request<ViewportSnapshot>({ type: 'readCellWindow', window, options });
  }

  commitUserInput(
    cell: { sheet: string; row: number; column: number },
    input: string,
    evaluate = true,
  ): Promise<MutationResult> {
    return this.request<MutationResult>({
      type: 'setUserInput',
      cell,
      input,
      evaluate,
    });
  }

  evaluate(request?: EvaluateRequest): Promise<EvaluationResult> {
    return this.request<EvaluationResult>({ type: 'evaluate', request });
  }

  stateStamp(): Promise<RevisionStamp> {
    return this.request<RevisionStamp>({ type: 'stateStamp' });
  }

  undo(): Promise<MutationResult> {
    return this.request<MutationResult>({ type: 'undo' });
  }

  redo(): Promise<MutationResult> {
    return this.request<MutationResult>({ type: 'redo' });
  }

  dispose(): void {
    if (this.disposed) {
      return;
    }
    this.disposed = true;
    this.worker.removeEventListener('message', this.onMessage);
    this.worker.removeEventListener('error', this.onError);
    this.worker.terminate();
    const error = new SpreadsheetBackendError({
      name: 'BackendDisposedError',
      message: 'Spreadsheet backend has been disposed',
      code: 'BACKEND_DISPOSED',
    });
    for (const pending of this.pending.values()) {
      pending.reject(error);
    }
    this.pending.clear();
  }

  private request<T>(
    payload: WorkerRequestPayload,
    transfer: Transferable[] = [],
  ): Promise<T> {
    if (this.disposed) {
      return Promise.reject(
        new SpreadsheetBackendError({
          name: 'BackendDisposedError',
          message: 'Spreadsheet backend has been disposed',
          code: 'BACKEND_DISPOSED',
        }),
      );
    }
    const requestId = String(++this.nextRequestId);
    const request = { ...payload, requestId } as WorkerRequest;
    return new Promise<T>((resolve, reject) => {
      this.pending.set(requestId, {
        resolve: (value) => resolve(value as T),
        reject,
      });
      try {
        this.worker.postMessage(request, transfer);
      } catch (error) {
        this.pending.delete(requestId);
        reject(error);
      }
    });
  }
}

export function createFormualizerWorkerBackend(): WorkerBackend {
  const worker = new Worker(new URL('./worker.js', import.meta.url), { type: 'module' });
  return new WorkerBackend(worker);
}
