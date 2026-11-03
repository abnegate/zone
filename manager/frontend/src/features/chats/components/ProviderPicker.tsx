import type { SelectOption } from '@zone/ui';
import { useState } from 'react';
import type { AiProvider } from '../../settings/workspace/types';
import type { ChatProviderOffer, ChatProviders } from '../provider';
import {
  AUTO_MODEL,
  asksModelName,
  DEFAULT_PROVIDER,
  isAgentOffer,
  MODEL_PLACEHOLDER,
  modelSelectOptions,
  providerChoices,
} from '../utils';
import { HeaderSelect } from './HeaderSelect';
import { ModelNameInput } from './ModelNameInput';

const DEFAULT_LABEL = 'Default';

interface ModelSelect {
  value: string;
  options: SelectOption[];
  onValueChange: (value: string) => void;
}

interface ModelName {
  key: string;
  provider: string;
  value: string;
  onSubmit: (name: string) => void;
}

export interface ProviderPickerProps {
  providers: ChatProviders;
  modelName: string;
  disabled?: boolean;
  onChoose: (provider: AiProvider | null, model: string | null) => void;
  onModel: (name: string) => void;
}

export function ProviderPicker({
  providers,
  modelName,
  disabled = false,
  onChoose,
  onModel,
}: ProviderPickerProps) {
  const { selected } = providers;
  const [pending, setPending] = useState<ChatProviderOffer | null>(null);

  const handleProvider = (value: string) => {
    if (value === DEFAULT_PROVIDER) {
      setPending(null);
      onChoose(null, null);
      return;
    }
    const offer = providers.providers.find((candidate) => candidate.provider === value);
    if (!offer) return;
    if (offer.requires_model) {
      setPending(offer);
      return;
    }
    setPending(null);
    onChoose(offer.provider, null);
  };

  const nameField = (): ModelName | null => {
    if (pending && asksModelName(pending)) {
      return {
        key: pending.provider,
        provider: pending.label,
        value: '',
        onSubmit: (name) => {
          setPending(null);
          onChoose(pending.provider, name);
        },
      };
    }
    if (!pending && selected && asksModelName(selected)) {
      return {
        key: `${selected.provider}:${selected.model ?? ''}`,
        provider: selected.label,
        value: selected.model ?? '',
        onSubmit: (name) => onChoose(selected.provider, name),
      };
    }
    return null;
  };

  const modelSelect = (): ModelSelect | null => {
    if (pending) {
      return {
        value: '',
        options: modelSelectOptions(pending, null),
        onValueChange: (value) => {
          setPending(null);
          onChoose(pending.provider, value);
        },
      };
    }
    if (selected) {
      return {
        value: selected.model ?? (selected.requires_model ? '' : AUTO_MODEL),
        options: modelSelectOptions(selected, selected.model),
        onValueChange: (value) => onChoose(selected.provider, value === AUTO_MODEL ? null : value),
      };
    }
    if (!isAgentOffer(providers.default)) return null;
    return {
      value: modelName || AUTO_MODEL,
      options: modelSelectOptions(providers.default, modelName),
      onValueChange: onModel,
    };
  };
  const named = nameField();
  const model = named ? null : modelSelect();
  const provider = pending?.provider ?? selected?.provider ?? DEFAULT_PROVIDER;

  return (
    <>
      <HeaderSelect
        className="chat-provider"
        label="Provider"
        ariaLabel="AI provider"
        testId="chat-provider"
        title={`Which AI provider answers in this chat. Default follows the workspace settings (${providers.default.label}).`}
        value={provider}
        display={provider === DEFAULT_PROVIDER ? DEFAULT_LABEL : undefined}
        options={providerChoices(providers.default, providers.providers, selected)}
        onValueChange={handleProvider}
        disabled={disabled}
      />
      {named && (
        <ModelNameInput
          key={named.key}
          provider={named.provider}
          value={named.value}
          onSubmit={named.onSubmit}
          disabled={disabled}
        />
      )}
      {model && (
        <HeaderSelect
          className="chat-provider-model"
          label="Model"
          ariaLabel="Provider model"
          testId="chat-provider-model"
          title="Which model the provider runs. Automatic lets the provider pick."
          value={model.value}
          placeholder={MODEL_PLACEHOLDER}
          options={model.options}
          onValueChange={model.onValueChange}
          disabled={disabled}
        />
      )}
      {!pending && selected && !selected.usable && (
        <p className="chat-provider-error" role="alert" data-testid="chat-provider-error">
          {selected.reason ?? `${selected.label} can't be used for this chat.`}
        </p>
      )}
    </>
  );
}
