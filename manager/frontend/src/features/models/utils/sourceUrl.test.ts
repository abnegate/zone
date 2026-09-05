import { describe, expect, it } from 'bun:test';
import { modelSourceUrl } from './sourceUrl';

describe('modelSourceUrl', () => {
  it('prefers an explicit catalog URL', () => {
    expect(modelSourceUrl('llama3.2', 'https://example.com/model')).toBe(
      'https://example.com/model'
    );
  });

  it('maps Ollama library names and tags', () => {
    expect(modelSourceUrl('llama3.2:latest')).toBe('https://ollama.com/library/llama3.2');
    expect(modelSourceUrl('mistral')).toBe('https://ollama.com/library/mistral');
  });

  it('maps namespaced Ollama models', () => {
    expect(modelSourceUrl('team/custom:latest')).toBe('https://ollama.com/team/custom');
  });

  it('maps HuggingFace GGUF references', () => {
    expect(modelSourceUrl('hf.co/TheBloke/Llama-2-7B-GGUF:Q4_K_M')).toBe(
      'https://huggingface.co/TheBloke/Llama-2-7B-GGUF'
    );
    expect(modelSourceUrl('huggingface.co/Qwen/Qwen3-GGUF')).toBe(
      'https://huggingface.co/Qwen/Qwen3-GGUF'
    );
  });

  it('uses the browse source when the name is unqualified', () => {
    expect(modelSourceUrl('Qwen/Qwen3-GGUF', null, 'huggingface')).toBe(
      'https://huggingface.co/Qwen/Qwen3-GGUF'
    );
    expect(modelSourceUrl('llama3.2', null, 'ollama')).toBe('https://ollama.com/library/llama3.2');
  });

  it('returns null for an empty name', () => {
    expect(modelSourceUrl('')).toBeNull();
    expect(modelSourceUrl('   ')).toBeNull();
  });
});
