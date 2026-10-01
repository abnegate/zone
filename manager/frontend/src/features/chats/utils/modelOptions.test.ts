import { describe, expect, it } from 'bun:test';
import {
  chatShowsAgent,
  chatShowsCharacter,
  chatShowsReasoning,
  findInstalledModel,
  offersAgent,
  sameModelName,
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

describe('chatShowsCharacter', () => {
  it('shows Character when the model needs a card or one is already attached', () => {
    expect(chatShowsCharacter({ needs_character: true })).toBe(true);
    expect(chatShowsCharacter({}, { needs_character: true })).toBe(true);
    expect(chatShowsCharacter({ character: { name: 'Ada' } })).toBe(true);
    expect(chatShowsCharacter({ needs_character: false })).toBe(false);
    expect(chatShowsCharacter({})).toBe(false);
  });
});
