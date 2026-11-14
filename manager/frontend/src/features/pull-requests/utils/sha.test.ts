import { describe, expect, it } from 'bun:test';
import { firstLine } from './sha';

describe('firstLine', () => {
  it('keeps the subject of a commit message', () => {
    expect(firstLine('feat: add the page\n\nBody text')).toBe('feat: add the page');
  });

  it('trims a one-line message', () => {
    expect(firstLine('  fix: typo  ')).toBe('fix: typo');
  });
});
