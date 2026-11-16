/**
 * Decodes one HEIC off the main thread and hands back its pixels.
 *
 * A worker per file, started by `decodeHeic` and terminated by it, so a
 * decoder this big is never held open for a composer that may send nothing
 * else. The pixel buffer is transferred rather than copied.
 */

import { type DecodeRequest, type DecodeResponse, decodeHeicPixels } from './heic';

self.onmessage = async (event: MessageEvent<DecodeRequest>) => {
  try {
    const pixels = await decodeHeicPixels(event.data.bytes);
    if (!pixels) {
      post({ error: 'This HEIC could not be decoded' });
      return;
    }
    post({ pixels }, [pixels.data.buffer]);
  } catch (error) {
    post({ error: error instanceof Error ? error.message : 'HEIC decoding failed' });
  }
};

function post(response: DecodeResponse, transfer: Transferable[] = []) {
  (self as unknown as Worker).postMessage(response, transfer);
}
