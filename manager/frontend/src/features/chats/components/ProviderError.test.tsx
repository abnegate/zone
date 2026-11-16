import { describe, expect, it } from 'bun:test';
import { render, screen } from '@testing-library/react';
import type { ChatProviderOffer, ChatProviders } from '../provider';
import { ProviderError } from './ProviderError';

const offer = (overrides: Partial<ChatProviderOffer> = {}): ChatProviderOffer => ({
  provider: 'self_hosted',
  label: 'Self-Hosted',
  usable: true,
  reason: null,
  models: [],
  efforts: [],
  requires_model: false,
  ...overrides,
});

const DENIED = "Your organization's policy doesn't allow Codex in this workspace.";

const denied = offer({
  provider: 'codex',
  label: 'Codex',
  usable: false,
  reason: DENIED,
  denial: 'policy_agent_denied',
});

function renderError(providers: ChatProviders | null) {
  return render(<ProviderError providers={providers} />);
}

describe('ProviderError', () => {
  it('says nothing while every choice in effect is usable', () => {
    renderError({ default: offer(), selected: null, providers: [offer()] });

    expect(screen.queryByTestId('chat-provider-error')).toBeNull();
  });

  it("says why a chat's own provider can't be used", () => {
    renderError({ default: offer(), selected: { ...denied, model: null }, providers: [] });

    expect(screen.getByRole('alert')).toHaveTextContent(DENIED);
  });

  it('says why the default provider is denied when the chat follows it', () => {
    renderError({ default: denied, selected: null, providers: [offer()] });

    expect(screen.getByRole('alert')).toHaveTextContent(DENIED);
  });
});
