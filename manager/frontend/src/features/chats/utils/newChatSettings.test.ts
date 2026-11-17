import { describe, expect, it } from 'bun:test';
import type { ChatProviderOffer, RecentChat, WorkspaceChatProviders } from '../provider';
import { DEFAULT_NEW_CHAT, newChatRequest, newChatSettings } from './newChatSettings';

const offer = (overrides: Partial<ChatProviderOffer> = {}): ChatProviderOffer => ({
  provider: 'self_hosted',
  label: 'Self-Hosted',
  usable: true,
  reason: null,
  models: [],
  efforts: ['off', 'low', 'medium', 'high'],
  requires_model: false,
  ...overrides,
});

const selfHosted = offer();
const codex = offer({
  provider: 'codex',
  label: 'Codex',
  models: [{ name: 'gpt-6-sol', label: 'gpt-6-sol', efforts: ['low', 'high'] }],
  efforts: ['low', 'high'],
});
const openAi = offer({ provider: 'openai', label: 'OpenAI', requires_model: true });
const claudeCode = offer({
  provider: 'claude_code',
  label: 'Claude Code',
  models: [{ name: 'opus', label: 'Opus', efforts: ['high'] }],
});

const recent = (overrides: Partial<RecentChat> = {}): RecentChat => ({
  model_name: 'auto',
  provider: null,
  provider_model: null,
  reasoning_effort: 'high',
  agent_enabled: true,
  offline: true,
  context_tokens: null,
  ...overrides,
});

const providers = (
  latest: RecentChat | null,
  fallback: ChatProviderOffer = selfHosted,
  offered: ChatProviderOffer[] = [selfHosted, codex, openAi]
): WorkspaceChatProviders => ({ default: fallback, providers: offered, recent: latest });

const llama = { name: 'llama3.2:latest', completion: true, details: { context_length: 131_072 } };
const embedder = { name: 'nomic-embed-text', completion: false };

describe('newChatSettings', () => {
  it('starts from the defaults before the providers load or the workspace has a chat', () => {
    expect(newChatSettings(null, [llama])).toEqual(DEFAULT_NEW_CHAT);
    expect(newChatSettings(providers(null), [llama])).toEqual(DEFAULT_NEW_CHAT);
    const older = { default: selfHosted, providers: [selfHosted] };
    expect(newChatSettings(older, [llama])).toEqual(DEFAULT_NEW_CHAT);
  });

  it("starts from the recent chat's chosen provider, model and settings", () => {
    const latest = recent({ provider: 'codex', provider_model: 'gpt-6-sol' });

    expect(newChatSettings(providers(latest), [])).toEqual({
      provider: 'codex',
      model: 'gpt-6-sol',
      reasoning: 'high',
      agent: true,
      offline: true,
      context: DEFAULT_NEW_CHAT.context,
    });
  });

  it('keeps Automatic on a chosen provider that picks its own model', () => {
    const settings = newChatSettings(providers(recent({ provider: 'codex' })), []);
    expect([settings.provider, settings.model]).toEqual(['codex', 'auto']);
  });

  it('keeps a typed model on a provider that names none of its own', () => {
    const latest = recent({ provider: 'openai', provider_model: 'my-deployment' });
    const settings = newChatSettings(providers(latest), []);
    expect([settings.provider, settings.model]).toEqual(['openai', 'my-deployment']);
  });

  it('falls back to Default when the recent provider is no longer offered', () => {
    const latest = recent({ provider: 'claude_code', provider_model: 'opus' });
    const settings = newChatSettings(providers(latest), [llama]);

    expect([settings.provider, settings.model]).toEqual(['default', 'auto']);
    expect(settings.reasoning).toBe('high');
    expect(settings.offline).toBe(true);
  });

  it('falls back to Default when the recent provider no longer offers its model', () => {
    const latest = recent({ provider: 'codex', provider_model: 'gpt-5-retired' });
    const settings = newChatSettings(providers(latest), []);
    expect([settings.provider, settings.model]).toEqual(['default', 'auto']);
  });

  it('falls back to Default when the recent provider now needs a model it never named', () => {
    const settings = newChatSettings(providers(recent({ provider: 'openai' })), []);
    expect([settings.provider, settings.model]).toEqual(['default', 'auto']);
  });

  it("starts from the recent chat's installed model and its chosen context", () => {
    const latest = recent({ model_name: 'llama3.2', context_tokens: 65_536 });
    const settings = newChatSettings(providers(latest), [llama]);
    expect([settings.provider, settings.model, settings.context]).toEqual([
      'default',
      'llama3.2:latest',
      '65536',
    ]);
  });

  it('falls back to Automatic when the recent model is gone or cannot chat', () => {
    for (const name of ['qwen3:8b', 'nomic-embed-text']) {
      const latest = recent({ model_name: name, context_tokens: 65_536 });
      const settings = newChatSettings(providers(latest), [llama, embedder]);
      expect([settings.provider, settings.model]).toEqual(['default', 'auto']);
      expect(settings.context).toBe(DEFAULT_NEW_CHAT.context);
    }
  });

  it("checks a Default model against an agent default's own models", () => {
    const listed = providers(recent({ model_name: 'opus' }), claudeCode);
    expect(newChatSettings(listed, []).model).toBe('opus');
    const retired = providers(recent({ model_name: 'llama3.2' }), claudeCode);
    expect(newChatSettings(retired, [llama]).model).toBe('auto');
  });

  it('replaces a context the model cannot hold with its default', () => {
    const latest = recent({ model_name: 'llama3.2', context_tokens: 1_048_576 });
    expect(newChatSettings(providers(latest), [llama]).context).toBe('32768');
  });

  it('keeps an effort only where the resolved provider offers it', () => {
    const fromClaude = recent({
      provider: 'claude_code',
      provider_model: 'opus',
      reasoning_effort: 'max',
    });
    expect(newChatSettings(providers(fromClaude), []).reasoning).toBe('auto');

    const onCodex = recent({
      provider: 'codex',
      provider_model: 'gpt-6-sol',
      reasoning_effort: 'medium',
    });
    expect(newChatSettings(providers(onCodex), []).reasoning).toBe('auto');
    const kept = recent({
      provider: 'codex',
      provider_model: 'gpt-6-sol',
      reasoning_effort: 'low',
    });
    expect(newChatSettings(providers(kept), []).reasoning).toBe('low');

    const onDefault = recent({ model_name: 'opus', reasoning_effort: 'low' });
    expect(newChatSettings(providers(onDefault, claudeCode), []).reasoning).toBe('auto');
  });

  it('starts from the default context when the recent chat named none', () => {
    const latest = recent({ model_name: 'llama3.2', context_tokens: null });
    expect(newChatSettings(providers(latest), [llama]).context).toBe('32768');
  });
});

describe('newChatRequest', () => {
  const requestFor = (latest: RecentChat | null, installed = [llama], fallback = selfHosted) => {
    const listed = providers(latest, fallback);
    return newChatRequest('ws-1', newChatSettings(listed, installed), listed, installed);
  };

  it('names the workspace, an automatic title and no auto-approval', () => {
    expect(requestFor(null)).toEqual({
      workspace_id: 'ws-1',
      title: 'New chat',
      automatic_title: true,
      model_name: 'auto',
      agent_enabled: false,
      auto_approve: false,
    });
  });

  it('sends a chosen provider and its model, titled after the model', () => {
    const latest = recent({ provider: 'codex', provider_model: 'gpt-6-sol' });

    expect(requestFor(latest, [])).toMatchObject({
      provider: 'codex',
      provider_model: 'gpt-6-sol',
      model_name: 'auto',
      title: 'Chat with gpt-6-sol',
      offline: true,
    });
  });

  it('sends a chosen provider that picks its own model as no model', () => {
    expect(requestFor(recent({ provider: 'codex' }), [])).toMatchObject({
      provider: 'codex',
      provider_model: null,
      model_name: 'auto',
      title: 'New chat',
    });
  });

  it('never carries auto-approval, which belongs to whoever approved it', () => {
    const latest = recent({ model_name: 'llama3.2', agent_enabled: true });
    expect(requestFor(latest)).toMatchObject({ agent_enabled: true, auto_approve: false });
  });

  it('leaves offline out of an online chat rather than sending false', () => {
    const latest = recent({ model_name: 'llama3.2', offline: false });
    expect(requestFor(latest)).not.toHaveProperty('offline');
  });

  it('sends the context window of a local model and none of a provider', () => {
    const local = recent({ model_name: 'llama3.2', context_tokens: 65_536 });
    expect(requestFor(local)).toMatchObject({
      model_name: 'llama3.2:latest',
      context_tokens: 65_536,
    });

    const chosen = recent({
      provider: 'codex',
      provider_model: 'gpt-6-sol',
      context_tokens: 65_536,
    });
    expect(requestFor(chosen, [])).not.toHaveProperty('context_tokens');
  });

  it('sends an effort the provider offers and nothing when it is Auto', () => {
    const kept = recent({
      provider: 'codex',
      provider_model: 'gpt-6-sol',
      reasoning_effort: 'low',
    });
    expect(requestFor(kept, [])).toMatchObject({ reasoning_effort: 'low' });

    const dropped = recent({
      provider: 'codex',
      provider_model: 'gpt-6-sol',
      reasoning_effort: 'medium',
    });
    expect(requestFor(dropped, [])).not.toHaveProperty('reasoning_effort');
  });

  it('drops agent mode for a model the instance says cannot call tools', () => {
    const toolless = { name: 'gemma3', completion: true, tools: false };
    const latest = recent({ model_name: 'gemma3', agent_enabled: true });
    expect(requestFor(latest, [llama, toolless])).toMatchObject({
      model_name: 'gemma3',
      agent_enabled: false,
    });
  });
});
