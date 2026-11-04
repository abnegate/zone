import { describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen } from '@testing-library/react';
import { chooseSelect, openSelect } from '../../../test/select';
import type {
  ChatProviderOffer,
  ChatProviderSelection,
  ChatProviders,
  ReasoningEffort,
} from '../provider';
import { ProviderPicker } from './ProviderPicker';

const HTTP: ReasoningEffort[] = ['off', 'low', 'medium', 'high'];

const selfHosted: ChatProviderOffer = {
  provider: 'self_hosted',
  label: 'Self-Hosted',
  usable: true,
  reason: null,
  models: [{ name: 'llama3.2:3b', label: 'llama3.2:3b', efforts: HTTP }],
  efforts: HTTP,
  requires_model: false,
};

const claude: ChatProviderOffer = {
  provider: 'claude_code',
  label: 'Claude Code',
  usable: true,
  reason: null,
  models: [
    { name: 'sonnet', label: 'Sonnet', efforts: ['low', 'medium', 'high', 'xhigh', 'max'] },
    { name: 'opus', label: 'Opus', efforts: ['low', 'medium', 'high', 'xhigh', 'max'] },
  ],
  efforts: ['low', 'medium', 'high', 'xhigh', 'max'],
  requires_model: false,
};

const codex: ChatProviderOffer = {
  provider: 'codex',
  label: 'Codex',
  usable: true,
  reason: null,
  models: [
    {
      name: 'gpt-6-sol',
      label: 'gpt-6-sol',
      efforts: ['low', 'medium', 'high', 'xhigh', 'max', 'ultra'],
    },
  ],
  efforts: ['low', 'medium', 'high', 'xhigh'],
  requires_model: false,
};

const openai: ChatProviderOffer = {
  provider: 'openai',
  label: 'OpenAI',
  usable: true,
  reason: null,
  models: [],
  efforts: HTTP,
  requires_model: true,
};

const selection = (
  offer: ChatProviderOffer,
  overrides: Partial<ChatProviderSelection> = {}
): ChatProviderSelection => ({ ...offer, model: null, ...overrides });

function optionNames(label: string): string[] {
  const trigger = openSelect(label);
  const names = screen.getAllByRole('option').map((option) => option.textContent ?? '');
  fireEvent.keyDown(trigger, { key: 'Escape' });
  return names;
}

function renderPicker(providers: Partial<ChatProviders> = {}, modelName = 'auto') {
  const onChoose = mock((_provider: string | null, _model: string | null) => {});
  const onModel = mock((_name: string) => {});
  render(
    <ProviderPicker
      providers={{
        default: selfHosted,
        selected: null,
        providers: [selfHosted, claude, codex],
        ...providers,
      }}
      modelName={modelName}
      onChoose={onChoose}
      onModel={onModel}
    />
  );
  return { onChoose, onModel };
}

describe('ProviderPicker', () => {
  it('shows Default on the trigger and names the default provider in its list and tooltip', () => {
    renderPicker({ default: claude, providers: [claude] });

    const trigger = screen.getByTestId('chat-provider');
    expect(trigger).toHaveTextContent('ProviderDefault');
    expect(trigger).not.toHaveTextContent('Claude Code');
    expect(trigger.getAttribute('title')).toContain('(Claude Code)');
    expect(optionNames('AI provider')).toEqual(['Default (Claude Code)', 'Claude Code']);
  });

  it('lists only the offered providers', () => {
    renderPicker({ providers: [selfHosted, codex] });

    expect(optionNames('AI provider')).toEqual(['Default (Self-Hosted)', 'Self-Hosted', 'Codex']);
  });

  it('chooses a provider with Automatic', () => {
    const { onChoose } = renderPicker();

    chooseSelect('AI provider', 'Claude Code');

    expect(onChoose).toHaveBeenCalledWith('claude_code', null);
  });

  it('asks for a typed model name before choosing a provider with none saved', () => {
    const { onChoose } = renderPicker({ providers: [selfHosted, openai] });

    chooseSelect('AI provider', 'OpenAI');

    expect(onChoose).not.toHaveBeenCalled();
    expect(screen.getByTestId('chat-provider')).toHaveTextContent('OpenAI');
    expect(screen.queryByTestId('chat-provider-model')).not.toBeInTheDocument();
    const field = screen.getByRole('textbox', { name: 'Provider model name' });
    expect(field).toHaveValue('');
    expect(field).toHaveAttribute('placeholder', 'Type a model name');

    fireEvent.change(field, { target: { value: '   ' } });
    fireEvent.keyDown(field, { key: 'Enter' });
    fireEvent.blur(field);
    expect(onChoose).not.toHaveBeenCalled();

    fireEvent.change(field, { target: { value: ' my-deployment ' } });
    fireEvent.keyDown(field, { key: 'Enter' });

    expect(onChoose).toHaveBeenCalledTimes(1);
    expect(onChoose).toHaveBeenCalledWith('openai', 'my-deployment');
  });

  it('saves a typed model name only on Enter, and says so while it is unsaved', () => {
    const { onChoose } = renderPicker({ providers: [selfHosted, openai] });

    chooseSelect('AI provider', 'OpenAI');
    const field = screen.getByRole('textbox', { name: 'Provider model name' });
    expect(screen.queryByText('Enter to save')).toBeNull();

    fireEvent.change(field, { target: { value: 'gp' } });
    fireEvent.blur(field);

    expect(onChoose).not.toHaveBeenCalled();
    expect(field).toHaveValue('gp');
    expect(screen.getByText('Enter to save')).toBeInTheDocument();

    fireEvent.change(field, { target: { value: 'gpt-4o' } });
    fireEvent.keyDown(field, { key: 'Enter' });

    expect(onChoose).toHaveBeenCalledTimes(1);
    expect(onChoose).toHaveBeenCalledWith('openai', 'gpt-4o');
  });

  it('after a refused name, reverts to the saved model and can send the name again', () => {
    const { onChoose } = renderPicker({
      providers: [selfHosted, openai],
      selected: selection(openai, { model: 'gpt-4o' }),
    });
    const field = screen.getByRole('textbox', { name: 'Provider model name' });

    fireEvent.change(field, { target: { value: 'refused' } });
    fireEvent.keyDown(field, { key: 'Enter' });
    expect(onChoose).toHaveBeenCalledTimes(1);

    fireEvent.keyDown(field, { key: 'Enter' });
    expect(onChoose).toHaveBeenCalledTimes(2);
    expect(onChoose).toHaveBeenLastCalledWith('openai', 'refused');

    fireEvent.keyDown(field, { key: 'Escape' });
    expect(field).toHaveValue('gpt-4o');
    expect(screen.queryByText('Enter to save')).toBeNull();
  });

  it('drops a pending provider that needs a model when another is chosen', () => {
    const { onChoose } = renderPicker({ providers: [selfHosted, openai, claude] });

    chooseSelect('AI provider', 'OpenAI');
    chooseSelect('AI provider', 'Claude Code');

    expect(onChoose).toHaveBeenCalledTimes(1);
    expect(onChoose).toHaveBeenCalledWith('claude_code', null);
  });

  it('shows an empty name field for a selected provider that requires a model and has none', () => {
    renderPicker({ providers: [selfHosted, openai], selected: selection(openai) });

    expect(screen.getByRole('textbox', { name: 'Provider model name' })).toHaveValue('');
  });

  it('clears the choice when Default is picked', () => {
    const { onChoose } = renderPicker({ selected: selection(codex) });

    chooseSelect('AI provider', 'Default (Self-Hosted)');

    expect(onChoose).toHaveBeenCalledWith(null, null);
  });

  it('chooses a model for the selected provider and Automatic clears it', () => {
    const { onChoose } = renderPicker({ selected: selection(claude, { model: 'sonnet' }) });

    expect(optionNames('Provider model')).toEqual(['Automatic', 'Sonnet', 'Opus']);
    chooseSelect('Provider model', 'Opus');
    expect(onChoose).toHaveBeenLastCalledWith('claude_code', 'opus');

    chooseSelect('Provider model', 'Automatic');
    expect(onChoose).toHaveBeenLastCalledWith('claude_code', null);
  });

  it('renames the model of a selected provider with none saved, but never to a blank one', () => {
    const { onChoose } = renderPicker({
      providers: [selfHosted, openai],
      selected: selection(openai, { model: 'gpt-4o' }),
    });

    expect(screen.queryByTestId('chat-provider-model')).not.toBeInTheDocument();
    const field = screen.getByRole('textbox', { name: 'Provider model name' });
    expect(field).toHaveValue('gpt-4o');

    fireEvent.blur(field);
    fireEvent.change(field, { target: { value: '' } });
    fireEvent.blur(field);
    expect(onChoose).not.toHaveBeenCalled();

    fireEvent.keyDown(field, { key: 'Enter' });
    expect(onChoose).not.toHaveBeenCalled();

    fireEvent.change(field, { target: { value: 'my-deployment' } });
    fireEvent.keyDown(field, { key: 'Enter' });
    fireEvent.blur(field);

    expect(onChoose).toHaveBeenCalledTimes(1);
    expect(onChoose).toHaveBeenCalledWith('openai', 'my-deployment');
  });

  it('shows the agent models on a Default chat whose default is an agent', () => {
    const { onChoose, onModel } = renderPicker({ default: claude }, 'sonnet');

    expect(screen.getByTestId('chat-provider-model')).toHaveTextContent('Sonnet');
    expect(optionNames('Provider model')).toEqual(['Automatic', 'Sonnet', 'Opus']);

    chooseSelect('Provider model', 'Opus');

    expect(onModel).toHaveBeenCalledWith('opus');
    expect(onChoose).not.toHaveBeenCalled();
  });

  it('shows no model select on a Default chat whose default is not an agent', () => {
    renderPicker();

    expect(screen.queryByTestId('chat-provider-model')).toBeNull();
  });

  it('drops a half-typed name on Escape instead of choosing it when focus leaves', () => {
    const { onChoose } = renderPicker({ selected: selection(openai) });
    const field = screen.getByRole('textbox', { name: 'Provider model name' });

    fireEvent.change(field, { target: { value: 'gpt-' } });
    fireEvent.keyDown(field, { key: 'Escape' });
    fireEvent.blur(field);

    expect(onChoose).not.toHaveBeenCalled();
    expect(field).toHaveValue('');
  });

  it('names an unusable selection in the picker and raises no alert of its own', () => {
    renderPicker({
      providers: [selfHosted],
      selected: selection(codex, {
        usable: false,
        reason: 'Codex is not signed in for this organization.',
      }),
    });

    expect(screen.queryByRole('alert')).toBeNull();
    expect(screen.getByTestId('chat-provider')).toHaveTextContent('Codex (unavailable)');
  });

  it('shows no alert for a usable selection', () => {
    renderPicker({ selected: selection(codex) });

    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('disables both selects', () => {
    render(
      <ProviderPicker
        providers={{ default: claude, selected: null, providers: [claude] }}
        modelName="auto"
        disabled
        onChoose={() => {}}
        onModel={() => {}}
      />
    );

    expect(screen.getByTestId('chat-provider')).toBeDisabled();
    expect(screen.getByTestId('chat-provider-model')).toBeDisabled();
  });

  it('disables the model name field', () => {
    render(
      <ProviderPicker
        providers={{
          default: selfHosted,
          selected: selection(openai, { model: 'gpt-4o' }),
          providers: [selfHosted, openai],
        }}
        modelName="auto"
        disabled
        onChoose={() => {}}
        onModel={() => {}}
      />
    );

    expect(screen.getByRole('textbox', { name: 'Provider model name' })).toBeDisabled();
  });
});
