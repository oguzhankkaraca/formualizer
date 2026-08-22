import init, { Workbook } from 'formualizer';
import type {
  CellAddress,
  CellWindowOptions,
  CellWindowRequest,
  CycleTelemetry,
  EvaluationResult,
  MutationResult,
  RevisionStamp,
  ViewportSnapshot,
  WorkerError,
  WorkerRequest,
  WorkerResponse,
  WorkbookSnapshot,
} from './protocol.js';

interface FormualizerWorkbook {
  addSheet(name: string): void;
  evaluateAll(): void;
  evaluateCells(targets: Array<[string, number, number]>): unknown[];
  lastCycleTelemetry(): CycleTelemetry;
  readCellWindow(window: CellWindowRequest, options?: CellWindowOptions): ViewportSnapshot;
  redo(): void;
  setUserInput(sheet: string, row: number, column: number, input: string): void;
  sheetNames(): unknown[];
  stateStamp(): RevisionStamp;
  undo(): void;
}

interface FormualizerWorkbookConstructor {
  new (options?: unknown): FormualizerWorkbook;
  fromXlsxBytesWithOptions(bytes: Uint8Array, options: unknown): FormualizerWorkbook;
}

const workbookConstructor = Workbook as unknown as FormualizerWorkbookConstructor;
const ITERATIVE_OPTIONS = {
  cycleDetection: 'runtime',
  cyclePolicy: 'iterate',
  iterateMaxIterations: 100,
  iterateMaxChange: 0.001,
};

function normalizeError(error: unknown): WorkerError {
  if (error && typeof error === 'object') {
    const candidate = error as {
      name?: unknown;
      message?: unknown;
      code?: unknown;
      inspect_code?: unknown;
    };
    return {
      name: typeof candidate.name === 'string' ? candidate.name : 'WorkerError',
      message: typeof candidate.message === 'string' ? candidate.message : String(error),
      code:
        typeof candidate.inspect_code === 'string'
          ? candidate.inspect_code
          : typeof candidate.code === 'string'
            ? candidate.code
            : undefined,
    };
  }
  return {
    name: 'WorkerError',
    message: error instanceof Error ? error.message : String(error),
    code: 'WORKER_ERROR',
  };
}

class FormualizerWorkerRuntime {
  private workbook: FormualizerWorkbook | null = null;
  private initialized = false;
  private queue: Promise<void> = Promise.resolve();

  handle(request: WorkerRequest): Promise<WorkerResponse> {
    const operation = this.queue.then(() => this.process(request));
    this.queue = operation.then(
      () => undefined,
      () => undefined,
    );
    return operation;
  }

  private async process(request: WorkerRequest): Promise<WorkerResponse> {
    try {
      const result = await this.execute(request);
      return { requestId: request.requestId, type: 'result', result };
    } catch (error) {
      return {
        requestId: request.requestId,
        type: 'error',
        error: normalizeError(error),
      };
    }
  }

  private async execute(request: WorkerRequest): Promise<unknown> {
    if (request.type === 'createWorkbook') {
      await this.ensureInitialized();
      this.workbook = new workbookConstructor(ITERATIVE_OPTIONS);
      this.workbook.addSheet('Sheet1');
      return this.workbookSnapshot();
    }
    if (request.type === 'loadXlsx') {
      await this.ensureInitialized();
      this.workbook = workbookConstructor.fromXlsxBytesWithOptions(
        new Uint8Array(request.bytes),
        ITERATIVE_OPTIONS,
      );
      return this.workbookSnapshot();
    }

    const workbook = this.requireWorkbook();
    switch (request.type) {
      case 'readCellWindow':
        return workbook.readCellWindow(request.window, request.options) as ViewportSnapshot;
      case 'setUserInput':
        return this.setUserInput(workbook, request.cell, request.input, request.evaluate);
      case 'evaluate':
        return this.evaluate(workbook, request.request?.targets);
      case 'stateStamp':
        return workbook.stateStamp() as RevisionStamp;
      case 'undo':
        workbook.undo();
        return this.mutationResult(workbook);
      case 'redo':
        workbook.redo();
        return this.mutationResult(workbook);
    }
  }

  private async ensureInitialized(): Promise<void> {
    if (!this.initialized) {
      await init();
      this.initialized = true;
    }
  }

  private requireWorkbook(): FormualizerWorkbook {
    if (!this.workbook) {
      throw new Error('Workbook has not been loaded');
    }
    return this.workbook;
  }

  private workbookSnapshot(): WorkbookSnapshot {
    const workbook = this.requireWorkbook();
    return {
      sheetNames: Array.from(workbook.sheetNames()) as string[],
      stamp: workbook.stateStamp() as RevisionStamp,
      evaluationMode: 'iterate',
      telemetry: workbook.lastCycleTelemetry(),
    };
  }

  private setUserInput(
    workbook: FormualizerWorkbook,
    cell: { sheet: string; row: number; column: number },
    input: string,
    evaluate = true,
  ): MutationResult {
    workbook.setUserInput(cell.sheet, cell.row, cell.column, input);
    if (evaluate) {
      workbook.evaluateAll();
    }
    return this.mutationResult(workbook);
  }

  private evaluate(workbook: FormualizerWorkbook, targets?: CellAddress[]): EvaluationResult {
    if (targets && targets.length > 0) {
      workbook.evaluateCells(
        targets.map((target) => [target.sheet, target.row, target.column]),
      );
    } else {
      workbook.evaluateAll();
    }
    return {
      stamp: workbook.stateStamp() as RevisionStamp,
      telemetry: workbook.lastCycleTelemetry(),
    };
  }

  private mutationResult(workbook: FormualizerWorkbook): MutationResult {
    return {
      stamp: workbook.stateStamp() as RevisionStamp,
      telemetry: workbook.lastCycleTelemetry(),
    };
  }
}

interface WorkerScope {
  onmessage: ((event: MessageEvent<WorkerRequest>) => void) | null;
  postMessage(message: WorkerResponse): void;
}

const scope = globalThis as unknown as WorkerScope;
const runtime = new FormualizerWorkerRuntime();
scope.onmessage = (event) => {
  void runtime.handle(event.data).then((response) => scope.postMessage(response));
};
