import { strict as assert } from 'node:assert';
import { test } from 'node:test';
import { SpreadsheetBackendError, WorkerBackend, type WorkerLike } from './backend.js';
import type { WorkerRequest, WorkerResponse } from './protocol.js';

class FakeWorker implements WorkerLike {
  readonly requests: WorkerRequest[] = [];
  readonly transfers: Transferable[][] = [];
  terminated = false;
  private messageListener: ((event: MessageEvent<WorkerResponse>) => void) | null = null;
  private errorListener: ((event: ErrorEvent) => void) | null = null;

  postMessage(message: WorkerRequest, transfer: Transferable[] = []): void {
    this.requests.push(message);
    this.transfers.push(transfer);
  }

  addEventListener(
    type: 'message',
    listener: (event: MessageEvent<WorkerResponse>) => void,
  ): void;
  addEventListener(type: 'error', listener: (event: ErrorEvent) => void): void;
  addEventListener(
    type: 'message' | 'error',
    listener:
      | ((event: MessageEvent<WorkerResponse>) => void)
      | ((event: ErrorEvent) => void),
  ): void {
    if (type === 'message') {
      this.messageListener = listener as (event: MessageEvent<WorkerResponse>) => void;
    } else {
      this.errorListener = listener as (event: ErrorEvent) => void;
    }
  }

  removeEventListener(
    type: 'message',
    listener: (event: MessageEvent<WorkerResponse>) => void,
  ): void;
  removeEventListener(type: 'error', listener: (event: ErrorEvent) => void): void;
  removeEventListener(
    type: 'message' | 'error',
    listener:
      | ((event: MessageEvent<WorkerResponse>) => void)
      | ((event: ErrorEvent) => void),
  ): void {
    if (type === 'message' && this.messageListener === listener) {
      this.messageListener = null;
    }
    if (type === 'error' && this.errorListener === listener) {
      this.errorListener = null;
    }
  }

  terminate(): void {
    this.terminated = true;
  }

  respond(response: WorkerResponse): void {
    this.messageListener?.(new MessageEvent('message', { data: response }));
  }
}

test('WorkerBackend transfers workbook bytes and resolves typed replies', async () => {
  const worker = new FakeWorker();
  const backend = new WorkerBackend(worker);
  const bytes = new Uint8Array([1, 2, 3]);
  const pending = backend.loadXlsx(bytes);

  assert.equal(worker.requests.length, 1);
  const request = worker.requests[0];
  assert.equal(request.type, 'loadXlsx');
  assert.deepEqual(Array.from(new Uint8Array(request.bytes)), [1, 2, 3]);
  assert.equal(worker.transfers[0].length, 1);
  assert.equal(worker.transfers[0][0], request.bytes);

  worker.respond({
    requestId: request.requestId,
    type: 'result',
    result: {
      sheetNames: ['Sheet1'],
      stamp: { mutationRevision: '2', recalcEpoch: '1' },
    },
  });

  assert.deepEqual(await pending, {
    sheetNames: ['Sheet1'],
    stamp: { mutationRevision: '2', recalcEpoch: '1' },
  });
  backend.dispose();
});

test('WorkerBackend maps worker errors and rejects disposed requests', async () => {
  const worker = new FakeWorker();
  const backend = new WorkerBackend(worker);
  const pending = backend.stateStamp();
  const request = worker.requests[0];
  worker.respond({
    requestId: request.requestId,
    type: 'error',
    error: {
      name: 'InspectError',
      message: 'revision changed',
      code: 'REVISION_MISMATCH',
    },
  });

  await assert.rejects(pending, (error: unknown) => {
    assert.ok(error instanceof SpreadsheetBackendError);
    assert.equal(error.code, 'REVISION_MISMATCH');
    assert.equal(error.message, 'revision changed');
    return true;
  });

  const disposedRequest = backend.readCellWindow({
    sheet: 'Sheet1',
    startRow: 1,
    startColumn: 1,
    endRow: 1,
    endColumn: 1,
  });
  backend.dispose();
  assert.equal(worker.terminated, true);
  await assert.rejects(disposedRequest, (error: unknown) => {
    assert.ok(error instanceof SpreadsheetBackendError);
    assert.equal(error.code, 'BACKEND_DISPOSED');
    return true;
  });
});
