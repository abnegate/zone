import { describe, expect, it } from 'bun:test';
import { mergeStageOptions } from './stageOptions';
import type { InstalledModel } from '../types';

const model = (name: string, extra: Partial<InstalledModel> = {}): InstalledModel => ({
  name,
  size: 1,
  modified_at: '',
  ...extra,
});

describe('mergeStageOptions', () => {
  it('keeps suggested models and installed chat models', () => {
    expect(
      mergeStageOptions(
        ['llama3.2:3b'],
        [
          model('qwen3.8:27b', { completion: true }),
          model('nomic-embed-text', { completion: false, capabilities: ['embeddings'] }),
        ],
        '',
        'chat'
      )
    ).toEqual(['llama3.2:3b', 'qwen3.8:27b']);
  });

  it('includes the current pin even when it is not suggested', () => {
    expect(
      mergeStageOptions(['llama3.2:3b'], [], 'custom:7b', 'chat')
    ).toEqual(['llama3.2:3b', 'custom:7b']);
  });
});
