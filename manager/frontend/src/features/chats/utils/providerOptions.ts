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

export const DEFAULT_PROVIDER_LABEL = 'Default';

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

export function selectionModel(selection: ChatProviderSelection): string {
  return selection.model
    ? (modelChoices(selection).find((choice) => choice.name === selection.model)?.label ??
        selection.model)
    : modelLabel(AUTO_MODEL);
}

/** `providers` with the provider a chat saved selected, for when it can't be read back. */
export function withChoice(
  providers: ChatProviders,
  chat: Pick<Chat, 'provider' | 'provider_model'>
): ChatProviders {
  if (!chat.provider) return { ...providers, selected: null };
  const offer =
    providers.providers.find((candidate) => candidate.provider === chat.provider) ??
    (providers.selected?.provider === chat.provider ? providers.selected : null);
  if (!offer) return providers;
  return { ...providers, selected: { ...offer, model: chat.provider_model ?? null } };
}

export function shortModelName(name: string): string {
  return name.slice(name.lastIndexOf('/') + 1) || name;
}

/** The provider's name for the summary. Null while a chat's own choice has no loaded label. */
export function chatProviderName(
  providers: ChatProviders | null,
  chat: Pick<Chat, 'provider'> | null
): string | null {
  if (providers) return providers.selected?.label ?? DEFAULT_PROVIDER_LABEL;
  return chat?.provider ? null : DEFAULT_PROVIDER_LABEL;
}

export function chatModelName(
  providers: ChatProviders | null,
  chat: Pick<Chat, 'model_name' | 'provider' | 'provider_model'> | null
): string {
  if (providers?.selected) return selectionModel(providers.selected);
  if (!chat) return modelLabel(AUTO_MODEL);
  return modelLabel(providers ? chat.model_name : requestedModel(chat));
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
