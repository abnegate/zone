import { describe, expect, it } from 'bun:test';
import type { ChatProviderOffer, ChatProviderSelection, ReasoningEffort } from '../provider';
import {
  asksModelName,
  chatModelName,
  chatProviderName,
  DEFAULT_PROVIDER,
  effectiveOffer,
  effortsFor,
  isAgentOffer,
  keepEffort,
  modelChoices,
  modelSelectOptions,
  offersAgentOn,
  providerChoices,
  reasoningOptions,
  selectionModel,
  shortModelName,
  withChoice,
} from './providerOptions';

const HTTP: ReasoningEffort[] = ['off', 'low', 'medium', 'high'];

const offer = (overrides: Partial<ChatProviderOffer> = {}): ChatProviderOffer => ({
  provider: 'self_hosted',
  label: 'Self-Hosted',
  usable: true,
  reason: null,
  models: [],
  efforts: HTTP,
  requires_model: false,
  ...overrides,
});

const codex = offer({
  provider: 'codex',
  label: 'Codex',
  models: [
    {
      name: 'gpt-6-sol',
      label: 'gpt-6-sol',
      efforts: ['low', 'medium', 'high', 'xhigh', 'max', 'ultra'],
    },
    { name: 'gpt-5.5', label: 'gpt-5.5', efforts: ['low', 'medium', 'high', 'xhigh'] },
  ],
  efforts: ['low', 'medium', 'high', 'xhigh'],
});

const claude = offer({
  provider: 'claude_code',
  label: 'Claude Code',
  models: [{ name: 'sonnet', label: 'Sonnet', efforts: ['low', 'medium', 'high', 'xhigh', 'max'] }],
  efforts: ['low', 'medium', 'high', 'xhigh', 'max'],
});

const select = (
  base: ChatProviderOffer,
  overrides: Partial<ChatProviderSelection> = {}
): ChatProviderSelection => ({ ...base, model: null, ...overrides });

describe('isAgentOffer', () => {
  it('recognises coding agents only', () => {
    expect(isAgentOffer(codex)).toBe(true);
    expect(isAgentOffer(claude)).toBe(true);
    expect(isAgentOffer(offer())).toBe(false);
    expect(isAgentOffer(offer({ provider: 'openai' }))).toBe(false);
  });
});

describe('offersAgentOn', () => {
  const installed = [
    { name: 'qwen3', tools: true },
    { name: 'gemma3', tools: false },
  ];

  it('always offers Agent on an agent offer', () => {
    expect(offersAgentOn(offer({ provider: 'codex' }), [], 'gemma3')).toBe(true);
  });

  it('reads the installed models when they list what the provider runs', () => {
    expect(offersAgentOn(offer(), installed, 'qwen3')).toBe(true);
    expect(offersAgentOn(offer(), installed, 'gemma3')).toBe(false);
    expect(offersAgentOn(null, installed, ' gemma3 ')).toBe(false);
    expect(offersAgentOn(null, [{ name: 'gemma3', tools: false }], '  ')).toBe(false);
  });

  it('offers Agent when the installed models do not describe the provider', () => {
    expect(offersAgentOn(offer({ provider: 'openai' }), null, 'gemma3')).toBe(true);
    expect(offersAgentOn(offer({ provider: 'openai' }), null, 'auto')).toBe(true);
  });
});

describe('effectiveOffer', () => {
  it('uses a usable selection', () => {
    const selected = select(codex);
    expect(effectiveOffer({ default: offer(), selected, providers: [codex] })).toBe(selected);
  });

  it('falls back to the default without a selection', () => {
    const fallback = offer();
    expect(effectiveOffer({ default: fallback, selected: null, providers: [] })).toBe(fallback);
  });

  it('falls back to the default when the selection is unusable', () => {
    const fallback = offer();
    const selected = select(codex, { usable: false, reason: 'Signed out.' });
    expect(effectiveOffer({ default: fallback, selected, providers: [] })).toBe(fallback);
  });
});

describe('modelChoices', () => {
  it('returns the server models', () => {
    expect(modelChoices(codex)).toBe(codex.models);
  });

  it('is empty for a provider that needs no model', () => {
    expect(modelChoices(offer())).toEqual([]);
  });

  it('invents no models when a model is required and none are saved', () => {
    expect(
      modelChoices(offer({ provider: 'openai', label: 'OpenAI', models: [], requires_model: true }))
    ).toEqual([]);
  });
});

describe('asksModelName', () => {
  it('asks for a typed name only when a model is required and none are saved', () => {
    expect(asksModelName(offer({ provider: 'openai', requires_model: true }))).toBe(true);
    expect(asksModelName(offer({ provider: 'openai' }))).toBe(false);
    expect(asksModelName(codex)).toBe(false);
    expect(asksModelName(offer())).toBe(false);
  });
});

describe('selectionModel', () => {
  it('names the model by its server label', () => {
    expect(selectionModel(select(claude, { model: 'sonnet' }))).toBe('Sonnet');
  });

  it('calls a missing model Automatic', () => {
    expect(selectionModel(select(codex))).toBe('Automatic');
  });

  it('keeps a model the provider no longer lists', () => {
    expect(selectionModel(select(codex, { model: 'gpt-4.1' }))).toBe('gpt-4.1');
  });
});

describe('shortModelName', () => {
  it('drops the routing prefix and keeps the tag', () => {
    expect(shortModelName('orcarouter/Qwen3.8-27B-Uncensored:Q4_K_M')).toBe(
      'Qwen3.8-27B-Uncensored:Q4_K_M'
    );
    expect(shortModelName('hf.co/org/model:latest')).toBe('model:latest');
  });

  it('leaves a bare name alone', () => {
    expect(shortModelName('llama3.2:3b')).toBe('llama3.2:3b');
  });

  it('keeps a name with nothing after its last slash', () => {
    expect(shortModelName('foo/')).toBe('foo/');
  });
});

describe('withChoice', () => {
  const listing = { default: codex, selected: null, providers: [codex, claude] };

  it('selects the saved provider and model', () => {
    const chosen = withChoice(listing, { provider: 'claude_code', provider_model: 'sonnet' });
    expect(chosen.selected).toEqual({ ...claude, model: 'sonnet' });
    expect(chosen.providers).toEqual(listing.providers);
  });

  it('returns to the default when nothing is saved', () => {
    const selected = select(claude, { model: 'sonnet' });
    expect(
      withChoice({ ...listing, selected }, { provider: null, provider_model: null }).selected
    ).toBeNull();
  });

  it('keeps the selection it can still describe when no offer lists the provider', () => {
    const selected = select(claude, { model: 'sonnet' });
    const unlisted = { ...listing, selected, providers: [codex] };
    expect(
      withChoice(unlisted, { provider: 'claude_code', provider_model: 'opus' }).selected
    ).toEqual({ ...selected, model: 'opus' });
    expect(withChoice(listing, { provider: 'openai', provider_model: 'gpt-4o' })).toEqual(listing);
  });
});

describe('the summary before the providers load', () => {
  const stored = { model_name: 'llama2', provider: null, provider_model: null };
  const chosen = { model_name: 'llama2', provider: 'codex' as const, provider_model: 'gpt-5.5' };

  it('never calls a chat that chose its provider Default', () => {
    expect(chatProviderName(null, chosen)).toBeNull();
    expect(chatProviderName(null, stored)).toBe('Default');
  });

  it('names the model the chat asks its own provider for', () => {
    expect(chatModelName(null, chosen)).toBe('gpt-5.5');
    expect(chatModelName(null, { ...chosen, provider_model: null })).toBe('Automatic');
    expect(chatModelName(null, stored)).toBe('llama2');
  });

  it('trusts the loaded providers over the chat they correct', () => {
    const providers = { default: codex, selected: null, providers: [] };
    expect(chatProviderName(providers, chosen)).toBe('Default');
    expect(chatModelName(providers, chosen)).toBe('llama2');
  });
});

describe('effortsFor', () => {
  it('gives each codex model its own efforts', () => {
    expect(effortsFor(codex, 'gpt-6-sol')).toEqual([
      'low',
      'medium',
      'high',
      'xhigh',
      'max',
      'ultra',
    ]);
    expect(effortsFor(codex, 'gpt-5.5')).toEqual(['low', 'medium', 'high', 'xhigh']);
  });

  it('uses the offer efforts for Automatic or an unknown model', () => {
    expect(effortsFor(codex, null)).toBe(codex.efforts);
    expect(effortsFor(codex, 'auto')).toBe(codex.efforts);
    expect(effortsFor(codex, 'gpt-unknown')).toBe(codex.efforts);
  });
});

describe('reasoningOptions', () => {
  it('leads with Auto and labels every effort', () => {
    expect(reasoningOptions(['low', 'xhigh', 'max', 'ultra'])).toEqual([
      { value: 'auto', label: 'Auto' },
      { value: 'low', label: 'Low' },
      { value: 'xhigh', label: 'Extra high' },
      { value: 'max', label: 'Max' },
      { value: 'ultra', label: 'Ultra' },
    ]);
  });

  it('lists Auto once', () => {
    expect(reasoningOptions(['auto', 'off']).map((option) => option.value)).toEqual([
      'auto',
      'off',
    ]);
  });
});

describe('keepEffort', () => {
  it('keeps a supported effort', () => {
    expect(keepEffort('ultra', effortsFor(codex, 'gpt-6-sol'))).toBe('ultra');
  });

  it('resets an unsupported effort to auto', () => {
    expect(keepEffort('max', effortsFor(codex, 'gpt-5.5'))).toBe('auto');
    expect(keepEffort('off', claude.efforts)).toBe('auto');
  });

  it('keeps auto and treats a missing effort as auto', () => {
    expect(keepEffort('auto', codex.efforts)).toBe('auto');
    expect(keepEffort(null, codex.efforts)).toBe('auto');
    expect(keepEffort(undefined, codex.efforts)).toBe('auto');
  });
});

describe('providerChoices', () => {
  it('lists Default with the server label, then the offers', () => {
    expect(providerChoices(claude, [offer(), codex])).toEqual([
      { value: DEFAULT_PROVIDER, label: 'Default (Claude Code)' },
      { value: 'self_hosted', label: 'Self-Hosted' },
      { value: 'codex', label: 'Codex' },
    ]);
  });

  it('marks an unusable selection that is not offered', () => {
    const selected = select(codex, { usable: false });
    expect(providerChoices(claude, [offer()], selected).at(-1)).toEqual({
      value: 'codex',
      label: 'Codex (unavailable)',
    });
  });

  it('does not repeat an offered selection', () => {
    expect(providerChoices(claude, [codex], select(codex))).toHaveLength(2);
  });
});

describe('modelSelectOptions', () => {
  it('offers Automatic before the models', () => {
    expect(modelSelectOptions(claude, null)).toEqual([
      { value: 'auto', label: 'Automatic' },
      { value: 'sonnet', label: 'Sonnet' },
    ]);
  });

  it('omits Automatic when a model is required', () => {
    const openai = offer({
      provider: 'openai',
      label: 'OpenAI',
      requires_model: true,
      models: [{ name: 'gpt-4o', label: 'gpt-4o', efforts: HTTP }],
    });
    expect(modelSelectOptions(openai, 'gpt-4o')).toEqual([{ value: 'gpt-4o', label: 'gpt-4o' }]);
  });

  it('keeps an unlisted current model selectable', () => {
    expect(modelSelectOptions(claude, 'claude-opus-4-1').at(-1)).toEqual({
      value: 'claude-opus-4-1',
      label: 'claude-opus-4-1',
    });
  });
});
