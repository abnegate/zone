import type {
  ChatProviderOffer,
  ReasoningEffort,
  RecentChat,
  WorkspaceChatProviders,
} from '../provider';
import { REASONING_EFFORT_OPTIONS } from '../types';
import {
  AUTO_MODEL,
  contextTokenOptions,
  defaultContextTokens,
  findInstalledModel,
  type LocalContextModel,
} from './modelOptions';
import {
  AUTO_EFFORT,
  DEFAULT_PROVIDER,
  effortsFor,
  isAgentOffer,
  keepEffort,
} from './providerOptions';

export interface NewChatSettings {
  provider: string;
  model: string;
  reasoning: ReasoningEffort;
  agent: boolean;
  offline: boolean;
  context: string;
}

type ListedModel = LocalContextModel & { name: string; completion?: boolean };

interface Resolved {
  provider: string;
  model: string;
  offer: ChatProviderOffer | null;
}

export const DEFAULT_NEW_CHAT: NewChatSettings = {
  provider: DEFAULT_PROVIDER,
  model: AUTO_MODEL,
  reasoning: AUTO_EFFORT,
  agent: false,
  offline: false,
  context: String(defaultContextTokens()),
};

const LOCAL_EFFORTS = REASONING_EFFORT_OPTIONS.map((option) => option.value);

function offersModel(offer: ChatProviderOffer, model: string): boolean {
  if (model === AUTO_MODEL) return !offer.requires_model;
  return offer.models.length === 0 || offer.models.some((choice) => choice.name === model);
}

function installedName(installed: ListedModel[], model: string): string | null {
  if (model === AUTO_MODEL) return AUTO_MODEL;
  const found = findInstalledModel(installed, model);
  return found && found.completion !== false ? found.name : null;
}

function resolve(
  recent: RecentChat,
  providers: WorkspaceChatProviders,
  installed: ListedModel[]
): Resolved {
  const fallback = providers.default;
  const automatic: Resolved = {
    provider: DEFAULT_PROVIDER,
    model: AUTO_MODEL,
    offer: isAgentOffer(fallback) ? fallback : null,
  };
  if (recent.provider) {
    const offer = providers.providers.find((candidate) => candidate.provider === recent.provider);
    const model = recent.provider_model ?? AUTO_MODEL;
    return offer && offersModel(offer, model)
      ? { provider: offer.provider, model, offer }
      : automatic;
  }
  if (isAgentOffer(fallback)) {
    return offersModel(fallback, recent.model_name)
      ? { ...automatic, model: recent.model_name }
      : automatic;
  }
  const model = installedName(installed, recent.model_name);
  return model ? { ...automatic, model } : automatic;
}

/**
 * The settings of the chat the workspace used last, with anything the
 * workspace no longer offers put back to Default, Automatic or Auto.
 */
export function newChatSettings(
  providers: WorkspaceChatProviders | null,
  installed: ListedModel[]
): NewChatSettings {
  const recent = providers?.recent;
  if (!providers || !recent) return DEFAULT_NEW_CHAT;
  const { provider, model, offer } = resolve(recent, providers, installed);
  const efforts = offer ? effortsFor(offer, model) : LOCAL_EFFORTS;
  const native = findInstalledModel(installed, model)?.details?.context_length;
  const context = recent.context_tokens === null ? null : String(recent.context_tokens);
  const keepsContext =
    context !== null &&
    model !== AUTO_MODEL &&
    contextTokenOptions(native).some((option) => option.value === context);
  return {
    provider,
    model,
    reasoning: keepEffort(recent.reasoning_effort, efforts),
    agent: recent.agent_enabled,
    offline: recent.offline,
    context: keepsContext ? context : String(defaultContextTokens(native)),
  };
}
