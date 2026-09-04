import { describe, expect, it } from 'bun:test';
import { modelDownload } from './download';

describe('modelDownload', () => {
  it('preserves Ollama names, including namespaces and selected size tags', () => {
    expect(modelDownload({ name: 'qwen3.8:27b', source: 'ollama' }).name).toBe('qwen3.8:27b');
    expect(modelDownload({ name: 'team/Model:Q4_K_M', source: 'ollama' }).name).toBe(
      'team/Model:Q4_K_M'
    );
    expect(
      modelDownload(
        {
          name: 'qwen3.8',
          source: 'ollama',
          sizes: [
            { name: 'qwen3.8:9b', label: '9B' },
            { name: 'qwen3.8:27b', label: '27B' },
          ],
        },
        'qwen3.8:27b'
      ).name
    ).toBe('qwen3.8:27b');
  });

  it('qualifies HuggingFace repositories without changing case or quantization tags', () => {
    expect(modelDownload({ name: 'Qwen/Qwen3-GGUF:Q4_K_M', source: 'huggingface' }).name).toBe(
      'hf.co/Qwen/Qwen3-GGUF:Q4_K_M'
    );
    expect(modelDownload({ name: 'hf.co/Qwen/Qwen3-GGUF:Q8_0', source: 'huggingface' }).name).toBe(
      'hf.co/Qwen/Qwen3-GGUF:Q8_0'
    );
    expect(
      modelDownload({ name: 'huggingface.co/Qwen/Qwen3-GGUF:Q8_0', source: 'huggingface' }).name
    ).toBe('huggingface.co/Qwen/Qwen3-GGUF:Q8_0');
    expect(
      modelDownload({ name: 'Qwen/Qwen3-GGUF', source: 'huggingface' }, 'Qwen/Qwen3-GGUF:Q8_0').name
    ).toBe('hf.co/Qwen/Qwen3-GGUF:Q8_0');
  });

  it('uses the active source when an older browse result has no source', () => {
    expect(modelDownload({ name: 'Qwen/Qwen3-GGUF' }, undefined, 'huggingface').name).toBe(
      'hf.co/Qwen/Qwen3-GGUF'
    );
    expect(modelDownload({ name: 'qwen/qwen3.8-27b' }, undefined, 'openrouter').name).toBeNull();
  });

  it('uses model provenance ahead of the currently selected source', () => {
    expect(
      modelDownload({ name: 'qwen/qwen3.8-27b', source: 'openrouter' }, undefined, 'ollama').name
    ).toBeNull();
    expect(
      modelDownload({ name: 'qwen3.8:27b', source: 'ollama' }, undefined, 'openrouter').name
    ).toBe('qwen3.8:27b');
  });

  it('does not invent local names for remote APIs or GPT4All downloads', () => {
    expect(modelDownload({ name: 'qwen/qwen3.8-27b', source: 'openrouter' }).name).toBeNull();
    expect(modelDownload({ name: 'model.gguf', source: 'gpt4all' }).name).toBeNull();
    expect(modelDownload({ name: 'qwen/qwen3.8-27b', details: { format: 'api' } }).name).toBeNull();
  });
});
