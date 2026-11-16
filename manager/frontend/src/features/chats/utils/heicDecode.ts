/**
 * Runs a HEIC decode in a worker. Separate from `heic.ts` so the worker can
 * import the decoding itself without importing the code that starts workers.
 *
 * The worker is the only place a decode happens. Decoding a full-size photo
 * is about a second of solid CPU and a malformed one can be far worse, and
 * nothing can interrupt that work once it starts: a timeout around a
 * synchronous decode on the main thread would measure the freeze rather than
 * prevent it. So a worker that cannot start, dies, or outlasts its deadline
 * gives up and the attachment is declined, which the composer already has a
 * message for.
 */

import type { DecodeResponse, Pixels } from './heic';

/** Long enough for a large photo on a slow phone, short enough to give up on. */
export const DECODE_TIMEOUT_MS = 30_000;

/** How a decoding worker is started. A seam, so tests need no bundler. */
export type Spawn = () => Worker;

export const spawnDecoder: Spawn = () =>
  new Worker(new URL('./heicWorker.ts', import.meta.url), { type: 'module' });

export interface DecodeOptions {
  spawn?: Spawn;
  timeout?: number;
}

/**
 * Decode HEIC bytes to RGBA, off the main thread.
 *
 * Null means this file could not be decoded here, for any reason: no worker,
 * a worker that failed, a decoder that could not read it, or a decode that
 * ran past its deadline. The caller reports an attachment it cannot take.
 */
export function decodeHeic(
  bytes: ArrayBuffer,
  options: DecodeOptions = {}
): Promise<Pixels | null> {
  const { spawn = spawnDecoder, timeout = DECODE_TIMEOUT_MS } = options;
  if (typeof Worker !== 'function') return Promise.resolve(null);

  let worker: Worker;
  try {
    worker = spawn();
  } catch {
    return Promise.resolve(null);
  }

  return new Promise((resolve) => {
    let settled = false;
    const finish = (pixels: Pixels | null) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      // Terminating is what actually stops a decode that overran, and what
      // keeps a worker per attachment from outliving the composer.
      worker.terminate();
      resolve(pixels);
    };
    const timer = setTimeout(() => finish(null), timeout);

    worker.onmessage = (event: MessageEvent<DecodeResponse>) => {
      finish('pixels' in event.data ? event.data.pixels : null);
    };
    worker.onerror = () => finish(null);

    // Copied rather than transferred: transferring detaches the caller's
    // buffer, and the caller may still read it after this answers.
    worker.postMessage({ bytes });
  });
}
