import type { SelectOption } from '@zone/ui';
import { isAgentProvider } from '../../settings/ai/options';
import {
  type ChatProviderModel,
  type ChatProviderOffer,
  type ChatProviderSelection,
  type ChatProviders,
  REASONING_EFFORT_LABELS,
  type ReasoningEffort,
} from '../provider';
import type { Chat } from '../types';
import { AUTO_MODEL, modelLabel } from './modelOptions';

export const DEFAULT_PROVIDER = 'default';

export const MODEL_PLACEHOLDER = 'Choose a model';

export const MODEL_NAME_PLACEHOLDER = 'Type a model name';

export const AUTO_EFFORT: ReasoningEffort = 'auto';

export function isAgentOffer(offer: ChatProviderOffer): boolean {
  return isAgentProvider(offer.provider);
}

export function requestedModel(
  chat: Pick<Chat, 'model_name' | 'provider' | 'provider_model'>
): string {
  return chat.provider ? (chat.provider_model ?? AUTO_MODEL) : chat.model_name;
}

export function effectiveOffer(providers: ChatProviders): ChatProviderOffer {
  return providers.selected?.usable ? providers.selected : providers.default;
}

export function modelChoices(offer: ChatProviderOffer): ChatProviderModel[] {
  return offer.models;
}

export function asksModelName(offer: ChatProviderOffer): boolean {
  return offer.requires_model && offer.models.length === 0;
}

export function selectionLabel(selection: ChatProviderSelection): string {
  const model = selection.model
    ? (modelChoices(selection).find((choice) => choice.name === selection.model)?.label ??
      selection.model)
    : modelLabel(AUTO_MODEL);
  return `${selection.label} · ${model}`;
}

export function effortsFor(
  offer: ChatProviderOffer,
  model: string | null | undefined
): ReasoningEffort[] {
  return modelChoices(offer).find((choice) => choice.name === model)?.efforts ?? offer.efforts;
}

export function reasoningOptions(efforts: ReasoningEffort[]): SelectOption[] {
  return [AUTO_EFFORT, ...efforts.filter((effort) => effort !== AUTO_EFFORT)].map((effort) => ({
    value: effort,
    label: REASONING_EFFORT_LABELS[effort],
  }));
}

export function keepEffort(
  current: ReasoningEffort | null | undefined,
  efforts: ReasoningEffort[]
): ReasoningEffort {
  if (!current || !efforts.includes(current)) return AUTO_EFFORT;
  return current;
}

export function providerChoices(
  fallback: ChatProviderOffer,
  offers: ChatProviderOffer[],
  selected: ChatProviderSelection | null = null
): SelectOption[] {
  const options = [
    { value: DEFAULT_PROVIDER, label: `Default (${fallback.label})` },
    ...offers.map((offer) => ({ value: offer.provider, label: offer.label })),
  ];
  if (selected && !offers.some((offer) => offer.provider === selected.provider)) {
    options.push({
      value: selected.provider,
      label: selected.usable ? selected.label : `${selected.label} (unavailable)`,
    });
  }
  return options;
}

export function modelSelectOptions(
  offer: ChatProviderOffer,
  current: string | null | undefined
): SelectOption[] {
  const automatic = offer.requires_model
    ? []
    : [{ value: AUTO_MODEL, label: modelLabel(AUTO_MODEL) }];
  const choices = modelChoices(offer).map((choice) => ({
    value: choice.name,
    label: choice.label,
  }));
  const unlisted =
    current && current !== AUTO_MODEL && !choices.some((choice) => choice.value === current)
      ? [{ value: current, label: current }]
      : [];
  return [...automatic, ...choices, ...unlisted];
}
