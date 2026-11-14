import { describe, expect, it } from 'bun:test';
import { shortSha } from './sha';

describe('shortSha', () => {
  it('keeps the first seven characters', () => {
    expect(shortSha('0123456789abcdef')).toBe('0123456');
  });

  it('keeps a shorter value whole', () => {
    expect(shortSha('abc')).toBe('abc');
  });
});
