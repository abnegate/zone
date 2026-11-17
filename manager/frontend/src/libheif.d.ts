/**
 * libheif-js ships types for its own entry point but not for the WASM bundle
 * subpath, which is the one worth importing: it loads a 1.4 MB `.wasm`
 * alongside 91 KB of glue instead of a 3.1 MB asm.js translation.
 *
 * Only the surface Zone calls is declared.
 */
declare module 'libheif-js/wasm-bundle.js' {
  export interface HeifImage {
    get_width: () => number;
    get_height: () => number;
    display: (
      into: { width: number; height: number; data: Uint8ClampedArray<ArrayBuffer> },
      done: (result: unknown) => void
    ) => void;
  }

  export class HeifDecoder {
    decode: (bytes: Uint8Array) => HeifImage[];
  }

  const libheif: { HeifDecoder: typeof HeifDecoder };
  export default libheif;
}
