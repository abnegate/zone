import { describe, expect, it, mock } from 'bun:test';

import type { Pixels } from './heic';

/** A stand-in for the WASM decoder, so the tests never load 1.4 MB of it. */
function libheif(image: Partial<StubImage> | null, decode?: () => never) {
  return {
    default: {
      HeifDecoder: class {
        decode() {
          if (decode) decode();
          return image ? [{ ...working(), ...image }] : [];
        }
      },
    },
  };
}

type StubImage = {
  get_width: () => number;
  get_height: () => number;
  display: (into: Pixels, done: (result: Pixels | null) => void) => void;
};

function working(): StubImage {
  return {
    get_width: () => 2,
    get_height: () => 1,
    display: (into, done) => {
      into.data.set([1, 2, 3, 255, 4, 5, 6, 255]);
      done(into);
    },
  };
}

async function decodeWith(module: unknown) {
  mock.module('libheif-js/wasm-bundle.js', () => module);
  const { decodeHeicPixels } = await import('./heic');
  return decodeHeicPixels(new ArrayBuffer(8));
}

describe('decodeHeicPixels', () => {
  it('returns the first image as RGBA', async () => {
    const pixels = await decodeWith(libheif({}));
    expect(pixels?.width).toBe(2);
    expect(pixels?.height).toBe(1);
    expect(pixels && [...pixels.data]).toEqual([1, 2, 3, 255, 4, 5, 6, 255]);
  });

  it('declines a file holding no image', async () => {
    expect(await decodeWith(libheif(null))).toBeNull();
  });

  it('declines an image of no size', async () => {
    expect(await decodeWith(libheif({ get_width: () => 0 }))).toBeNull();
  });

  it('declines when the decoder throws on the bytes', async () => {
    expect(
      await decodeWith(
        libheif({}, () => {
          throw new Error('not a HEIC');
        })
      )
    ).toBeNull();
  });

  it('declines when drawing the image fails', async () => {
    expect(await decodeWith(libheif({ display: (_, done) => done(null) }))).toBeNull();
  });
});
