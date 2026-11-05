import { describe, expect, it } from 'bun:test';
import type { ChatProviderOffer, RecentChat, WorkspaceChatProviders } from '../provider';
import { DEFAULT_NEW_CHAT, newChatSettings } from './newChatSettings';

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
