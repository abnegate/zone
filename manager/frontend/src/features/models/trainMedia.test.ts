import { describe, expect, it } from 'bun:test';
import {
  blobFromBase64,
  captionBatches,
  poolMap,
  shouldRecompress,
  TRAIN_EDGE,
} from './trainMedia';

describe('trainMedia', () => {
  it('turns frame base64 into a blob without keeping the string', () => {
    const blob = blobFromBase64('YWFh', 'image/png');
    expect(blob).toBeInstanceOf(Blob);
    expect(blob.type).toBe('image/png');
    expect(blob.size).toBe(3);
  });

  it('packs caption groups so a shot is never split across requests', () => {
    const items = [
      { id: 1, group: 0 },
      { id: 2, group: 0 },
      { id: 3, group: 1 },
      { id: 4 },
      { id: 5, group: 2 },
      { id: 6, group: 2 },
    ];
    expect(captionBatches(items, 3).map((batch) => batch.map((item) => item.id))).toEqual([
      [1, 2, 3],
      [4, 5, 6],
    ]);
  });

  it('keeps clip frames and 1536 PNGs as-is', () => {
    expect(TRAIN_EDGE).toBe(1536);
    expect(
      shouldRecompress({ type: 'image/png', width: 1536, height: 1024, size: 800_000, clip: true })
    ).toBe(false);
    expect(shouldRecompress({ type: 'image/png', width: 1536, height: 1024, size: 800_000 })).toBe(
      false
    );
    expect(shouldRecompress({ type: 'image/jpeg', width: 1024, height: 768, size: 80_000 })).toBe(
      false
    );
    expect(shouldRecompress({ type: 'image/jpeg', width: 4000, height: 3000, size: 80_000 })).toBe(
      true
    );
    expect(shouldRecompress({ type: 'image/jpeg', width: 1536, height: 1024, size: 400_000 })).toBe(
      true
    );
  });

  it('runs a bounded worker pool in source order', async () => {
    const seen: number[] = [];
    const results = await poolMap([10, 20, 30, 40], 2, async (item, index) => {
      seen.push(index);
      return item + 1;
    });
    expect(results).toEqual([11, 21, 31, 41]);
    expect(seen).toHaveLength(4);
  });
});
