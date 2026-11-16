import { describe, expect, it } from 'bun:test';
import { OPEN_RULES } from './draft';
import { PolicyAgent, PolicyCode, PolicyProvider } from './schemas';
import { providerDenial, providerDenialMessage, providerSubject } from './settings';

const NARROW = {
  ...OPEN_RULES,
  providers: [PolicyProvider.Local, PolicyProvider.Anthropic],
  agents: [PolicyAgent.Claude],
};

const CLOSED = { ...OPEN_RULES, providers: [], agents: [] };

describe('providerDenial', () => {
  it('denies nothing when the policy restricts nothing', () => {
    for (const provider of [
      'self_hosted',
      'openai',
      'anthropic',
      'bedrock',
      'claude_code',
      'codex',
    ] as const) {
      expect(providerDenial(OPEN_RULES, provider), provider).toBeNull();
    }
  });

  it('checks the direct OpenAI and Anthropic APIs against the providers', () => {
    expect(providerDenial(NARROW, 'anthropic')).toBeNull();
    expect(providerDenial(NARROW, 'openai')).toBe(PolicyCode.ProviderDenied);
  });

  it('checks Claude Code and Codex against the coding agents', () => {
    expect(providerDenial(NARROW, 'claude_code')).toBeNull();
    expect(providerDenial(NARROW, 'codex')).toBe(PolicyCode.AgentDenied);
  });

  it('never denies the self-hosted gateway or Bedrock, which the server checks per route', () => {
    expect(providerDenial(CLOSED, 'self_hosted')).toBeNull();
    expect(providerDenial(CLOSED, 'bedrock')).toBeNull();
    expect(providerDenial(CLOSED, 'openai')).toBe(PolicyCode.ProviderDenied);
    expect(providerDenial(CLOSED, 'claude_code')).toBe(PolicyCode.AgentDenied);
  });
});

describe('providerSubject', () => {
  it('names a provider the policy governs by its policy label', () => {
    expect(providerSubject('openai')).toBe('OpenAI API');
    expect(providerSubject('anthropic')).toBe('Anthropic API');
    expect(providerSubject('claude_code')).toBe('Claude Code');
    expect(providerSubject('codex')).toBe('Codex');
  });

  it('names nothing for a provider the server checks per route', () => {
    expect(providerSubject('self_hosted')).toBeUndefined();
    expect(providerSubject('bedrock')).toBeUndefined();
  });
});

describe('providerDenialMessage', () => {
  it('words a denial the way the server words it', () => {
    expect(providerDenialMessage(NARROW, 'openai')).toBe(
      "Your organization's policy doesn't allow OpenAI API in this workspace."
    );
    expect(providerDenialMessage(NARROW, 'codex')).toBe(
      "Your organization's policy doesn't allow Codex in this workspace."
    );
    expect(providerDenialMessage(NARROW, 'anthropic')).toBeNull();
  });
});
