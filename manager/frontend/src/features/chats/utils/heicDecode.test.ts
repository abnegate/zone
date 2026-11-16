import { describe, expect, it } from 'bun:test';
import type { Pixels } from './heic';
import { decodeHeic, type Spawn } from './heicDecode';

/** A worker that answers however the test tells it to, and records the rest. */
class FakeWorker {
  onmessage: ((event: MessageEvent) => void) | null = null;
  onerror: ((event: unknown) => void) | null = null;
  posted: unknown[] = [];
  terminated = 0;

  constructor(private readonly answer: (worker: FakeWorker) => void) {}

  postMessage(message: unknown) {
    this.posted.push(message);
    this.answer(this);
  }

  terminate() {
    this.terminated += 1;
  }
}

function pixels(): Pixels {
  return { width: 1, height: 1, data: new Uint8ClampedArray(4) };
}

function spawning(answer: (worker: FakeWorker) => void): { spawn: Spawn; worker: FakeWorker } {
  const worker = new FakeWorker(answer);
  return { spawn: (() => worker) as unknown as Spawn, worker };
}

describe('decodeHeic', () => {
  it('answers with the pixels the worker decoded, and shuts it down', async () => {
    const decoded = pixels();
    const { spawn, worker } = spawning((w) =>
      w.onmessage?.({ data: { pixels: decoded } } as MessageEvent)
    );

    expect(await decodeHeic(new ArrayBuffer(8), { spawn })).toBe(decoded);
    expect(worker.terminated).toBe(1);
  });

  it('copies the bytes rather than transferring them, so the caller keeps them', async () => {
    const bytes = new ArrayBuffer(8);
    const { spawn, worker } = spawning((w) =>
      w.onmessage?.({ data: { pixels: pixels() } } as MessageEvent)
    );

    await decodeHeic(bytes, { spawn });

    expect(worker.posted).toEqual([{ bytes }]);
    expect(bytes.byteLength).toBe(8);
  });

  it('declines a file the worker reports it could not decode', async () => {
    const { spawn, worker } = spawning((w) =>
      w.onmessage?.({ data: { error: 'This HEIC could not be decoded' } } as MessageEvent)
    );

    expect(await decodeHeic(new ArrayBuffer(8), { spawn })).toBeNull();
    expect(worker.terminated).toBe(1);
  });

  it('declines when the worker itself fails', async () => {
    const { spawn, worker } = spawning((w) => w.onerror?.({ message: 'boom' }));

    expect(await decodeHeic(new ArrayBuffer(8), { spawn })).toBeNull();
    expect(worker.terminated).toBe(1);
  });

  it('declines when a worker cannot be started at all', async () => {
    const spawn = (() => {
      throw new Error('no worker');
    }) as unknown as Spawn;

    expect(await decodeHeic(new ArrayBuffer(8), { spawn })).toBeNull();
  });

  it('gives up on a decode that outruns its deadline, and terminates it', async () => {
    // The worker never answers, which is the case a main-thread decode could
    // not have escaped: terminating is what stops the work.
    const { spawn, worker } = spawning(() => {});

    expect(await decodeHeic(new ArrayBuffer(8), { spawn, timeout: 1 })).toBeNull();
    expect(worker.terminated).toBe(1);
  });

  it('answers once when a worker both answers and then fails', async () => {
    const decoded = pixels();
    const { spawn, worker } = spawning((w) => {
      w.onmessage?.({ data: { pixels: decoded } } as MessageEvent);
      w.onerror?.({ message: 'late' });
    });

    expect(await decodeHeic(new ArrayBuffer(8), { spawn })).toBe(decoded);
    expect(worker.terminated).toBe(1);
  });
});
