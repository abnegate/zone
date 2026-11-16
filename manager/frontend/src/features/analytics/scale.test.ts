import { describe, expect, it } from 'bun:test';
import { labelEvery, niceCeiling, proportion } from './scale';

describe('chart scale', () => {
  it('rounds the axis up to 1, 2 or 5 times a power of ten', () => {
    expect(niceCeiling(0)).toBe(1);
    expect(niceCeiling(1)).toBe(1);
    expect(niceCeiling(3)).toBe(5);
    expect(niceCeiling(9)).toBe(10);
    expect(niceCeiling(10)).toBe(10);
    expect(niceCeiling(11)).toBe(20);
    expect(niceCeiling(240)).toBe(500);
  });

  it('bounds a proportion to the track', () => {
    expect(proportion(5, 10)).toBe(0.5);
    expect(proportion(15, 10)).toBe(1);
    expect(proportion(3, 0)).toBe(0);
  });

  it('labels every point when they fit and thins them when they do not', () => {
    expect(labelEvery(7, 7)).toBe(1);
    expect(labelEvery(30, 7)).toBe(5);
    expect(labelEvery(24, 8)).toBe(3);
  });
});
