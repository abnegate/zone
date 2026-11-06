import { describe, expect, it } from 'bun:test';
import {
  chatShowsAgent,
  chatShowsCharacter,
  chatShowsReasoning,
  contextTokenOptions,
  defaultContextTokens,
  findInstalledModel,
  offersAgent,
  offersAgentFor,
  offersLocalContext,
  sameModelName,
  selectedContextTokens,
} from './modelOptions';

describe('sameModelName', () => {
  it('treats a missing latest tag as the same installed model', () => {
    expect(sameModelName('llama3.1:latest', 'llama3.1')).toBe(true);
    expect(sameModelName('llama3.1', 'mistral')).toBe(false);
  });
});

describe('findInstalledModel', () => {
  it('matches the chat model against installed names', () => {
    const models = [{ name: 'llama3.1:latest' }, { name: 'mistral' }];
    expect(findInstalledModel(models, 'llama3.1')?.name).toBe('llama3.1:latest');
  });
});

describe('chatShowsAgent', () => {
  it('shows Agent when the model can call tools or the chat already uses them', () => {
    expect(chatShowsAgent({ agent_enabled: false }, { tools: true })).toBe(true);
    expect(chatShowsAgent({ agent_enabled: true, tools: false })).toBe(true);
    expect(chatShowsAgent({ agent_enabled: false, tools: true })).toBe(true);
    expect(chatShowsAgent({ agent_enabled: false }, { tools: false })).toBe(false);
    expect(chatShowsAgent({ agent_enabled: false })).toBe(false);
  });

  it('shows Agent for a listed model whose tool calling is unknown', () => {
    expect(chatShowsAgent({ agent_enabled: false, tools: null }, {})).toBe(true);
    expect(chatShowsAgent({ agent_enabled: false, tools: false }, {})).toBe(false);
  });
});

describe('offersAgent', () => {
  it('offers Agent unless the model is known to lack tools or chat', () => {
    expect(offersAgent({ tools: true })).toBe(true);
    expect(offersAgent({})).toBe(true);
    expect(offersAgent({ tools: false })).toBe(false);
    expect(offersAgent({ completion: false })).toBe(false);
    expect(offersAgent(undefined)).toBe(false);
  });
});

describe('offersAgentFor', () => {
  const models = [
    { name: 'qwen3:latest', tools: true },
    { name: 'gemma3', tools: false },
    { name: 'nomic-embed-text', completion: false },
  ];

  it('offers Agent for an installed model unless it is known to lack tools or chat', () => {
    expect(offersAgentFor(models, 'qwen3')).toBe(true);
    expect(offersAgentFor(models, 'gemma3')).toBe(false);
    expect(offersAgentFor(models, 'nomic-embed-text')).toBe(false);
  });

  it('offers Agent for a model zone has no details for', () => {
    expect(offersAgentFor([...models, { name: 'mistral' }], 'mistral')).toBe(true);
    expect(offersAgentFor(models, 'gpt-4o')).toBe(true);
    expect(offersAgentFor([], 'my-deployment')).toBe(true);
  });

  it('offers Agent on Automatic only when an installed model can call tools', () => {
    expect(offersAgentFor(models, 'auto')).toBe(true);
    expect(offersAgentFor([{ name: 'gemma3', tools: false }], 'auto')).toBe(false);
    expect(offersAgentFor([], 'auto')).toBe(false);
  });
});

describe('chatShowsReasoning', () => {
  it('shows the control when the engine advertised thinking', () => {
    expect(chatShowsReasoning({}, { capabilities: ['completion', 'reasoning'] })).toBe(true);
    expect(chatShowsReasoning({ reasoning: true })).toBe(true);
    expect(chatShowsReasoning({}, { reasoning: true })).toBe(true);
    expect(chatShowsReasoning({}, { capabilities: ['completion'] })).toBe(false);
    expect(chatShowsReasoning({ reasoning: false })).toBe(false);
    expect(chatShowsReasoning({})).toBe(false);
  });
});

describe('offersLocalContext', () => {
  it('offers a context picker for an installed local weight', () => {
    expect(offersLocalContext({ size: 17_741_860_762 })).toBe(true);
    expect(offersLocalContext({ details: { context_length: 262144 } })).toBe(true);
    expect(offersLocalContext({ size: 0 })).toBe(false);
    expect(offersLocalContext({})).toBe(false);
    expect(offersLocalContext(undefined)).toBe(false);
  });
});

describe('contextTokenOptions', () => {
  it('lists windows up to the native length and includes an odd native size', () => {
    expect(contextTokenOptions(32768).map((option) => option.value)).toEqual([
      '2048',
      '4096',
      '8192',
      '16384',
      '32768',
    ]);
    expect(contextTokenOptions(128000).map((option) => option.value)).toContain('128000');
    expect(contextTokenOptions(128000).at(-1)).toEqual({
      value: '128000',
      label: '128K tokens',
    });
  });
});

describe('defaultContextTokens', () => {
  it('defaults to 32k when the native window is larger', () => {
    expect(defaultContextTokens(262144)).toBe(32768);
    expect(defaultContextTokens(8192)).toBe(8192);
    expect(defaultContextTokens(null)).toBe(32768);
  });
});

describe('selectedContextTokens', () => {
  it('keeps an existing choice and otherwise shows the native window', () => {
    expect(selectedContextTokens(8192, 262144)).toBe(8192);
    expect(selectedContextTokens(null, 262144)).toBe(262144);
    expect(selectedContextTokens(undefined, null)).toBe(32768);
  });

  it('never shows more than the native window', () => {
    expect(selectedContextTokens(32768, 4096)).toBe(4096);
    expect(selectedContextTokens(32768, null)).toBe(32768);
  });
});

describe('chatShowsCharacter', () => {
  it('shows Character when the model needs a card or one is already attached', () => {
    expect(chatShowsCharacter({ needs_character: true })).toBe(true);
    expect(chatShowsCharacter({}, { needs_character: true })).toBe(true);
    expect(chatShowsCharacter({ character: { name: 'Ada' } })).toBe(true);
    expect(chatShowsCharacter({ needs_character: false })).toBe(false);
    expect(chatShowsCharacter({})).toBe(false);
  });
});
