import { describe, expect, it } from 'bun:test';
import { blobFromBase64, captionBatches, poolMap } from './trainMedia';

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
