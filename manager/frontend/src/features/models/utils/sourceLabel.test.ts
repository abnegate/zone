import { describe, expect, it } from 'bun:test';
import { sourceLabel } from './sourceLabel';

describe('sourceLabel', () => {
  it('names a source in sentence case', () => {
    expect(sourceLabel('ollama')).toBe('Ollama');
    expect(sourceLabel('huggingface')).toBe('Hugging Face');
    expect(sourceLabel('all')).toBe('All');
  });

  it('is empty for no source', () => {
    expect(sourceLabel(null)).toBe('');
    expect(sourceLabel(undefined)).toBe('');
  });
});
