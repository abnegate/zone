/**
 * Runs a HEIC decode in a worker, and inline where there is no worker to run
 * it in. Separate from `heic.ts` so the worker can import the decoding itself
 * without importing the code that starts workers.
 */

import { decodeHeicPixels, type DecodeResponse, type Pixels } from './heic';

/** Long enough for a large photo on a slow phone, short enough to give up on. */
const DECODE_TIMEOUT_MS = 30_000;

/**
 * Decode HEIC bytes to RGBA, off the main thread when that is possible.
 *
 * Null means this file could not be decoded, which the caller reports as an
 * attachment it cannot take.
 */
export async function decodeHeic(bytes: ArrayBuffer): Promise<Pixels | null> {
  if (typeof Worker !== 'function') return decodeHeicPixels(bytes);
  try {
    return await inWorker(bytes);
  } catch {
    // A worker that cannot start or dies mid-decode leaves the main thread as
    // the only way to answer; a frozen composer beats a photo that will not go.
    return decodeHeicPixels(bytes);
  }
}

function inWorker(bytes: ArrayBuffer): Promise<Pixels | null> {
  return new Promise((resolve, reject) => {
    const worker = new Worker(new URL('./heicWorker.ts', import.meta.url), { type: 'module' });
    const timer = setTimeout(() => {
      finish();
      reject(new Error('HEIC decoding timed out'));
    }, DECODE_TIMEOUT_MS);

    const finish = () => {
      clearTimeout(timer);
      worker.terminate();
    };

    worker.onmessage = (event: MessageEvent<DecodeResponse>) => {
      finish();
      resolve('pixels' in event.data ? event.data.pixels : null);
    };
    worker.onerror = (event) => {
      finish();
      reject(new Error(event.message || 'HEIC worker failed'));
    };

    // Copied rather than transferred: transferring detaches the caller's
    // buffer, and the fallback below still needs to read it if this fails.
    worker.postMessage({ bytes });
  });
}
