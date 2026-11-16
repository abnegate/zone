/**
 * HEIC decoding for the browsers that cannot do it themselves.
 *
 * Chrome, Firefox and Edge do not license HEVC, so they decode no HEIC at all;
 * Safari is the only engine `createImageBitmap` handles one in. libheif
 * compiled to WebAssembly covers the rest, at about half a megabyte gzipped,
 * so it is loaded only once a HEIC is actually attached.
 *
 * Decoding a full-size phone photo takes around a second of solid CPU, which
 * is why it happens in a worker. The worker hands back raw pixels rather than
 * an encoded image: that keeps the expensive part off the main thread without
 * needing `OffscreenCanvas`, which this path cannot assume.
 */

/** `ImageData` refuses a buffer that might be shared, so this one may not be. */
import type { HeifImage } from 'libheif-js/wasm-bundle.js';

/** `ImageData` refuses a buffer that might be shared, so this one may not be. */
export type Pixels = { width: number; height: number; data: Uint8ClampedArray<ArrayBuffer> };

/** What the worker is asked to do, and what it answers with. */
export type DecodeRequest = { bytes: ArrayBuffer };
export type DecodeResponse = { pixels: Pixels } | { error: string };

/**
 * Decode HEIC bytes to RGBA.
 *
 * Returns null when this build of libheif cannot read the file, which is a
 * refusal to display rather than an error to report: the caller already has a
 * message for an image it cannot take.
 */
export async function decodeHeicPixels(bytes: ArrayBuffer): Promise<Pixels | null> {
  const { HeifDecoder } = await loadLibheif();
  let images: HeifImage[];

  try {
    images = new HeifDecoder().decode(new Uint8Array(bytes));
  } catch {
    return null;
  }
  // A Live Photo or a burst holds several; the first is the one shown.
  const image = images?.[0];
  if (!image) return null;

  const width = image.get_width();
  const height = image.get_height();
  if (!(width > 0) || !(height > 0)) return null;

  const pixels: Pixels = { width, height, data: new Uint8ClampedArray(width * height * 4) };
  try {
    await new Promise<void>((resolve, reject) => {
      image.display(pixels, (result) =>
        result ? resolve() : reject(new Error('libheif could not draw the image'))
      );
    });
  } catch {
    return null;
  }
  return pixels;
}

/** The decoder, loaded on first use so no one pays for it who never sends a HEIC. */
async function loadLibheif() {
  const module = await import('libheif-js/wasm-bundle.js');
  // The bundle is CommonJS, so a test double supplying it either way is read
  // the same as the real package is.
  return module.default ?? (module as unknown as typeof module.default);
}
