import type {
  ChatProviderOffer,
  ReasoningEffort,
  RecentChat,
  WorkspaceChatProviders,
} from '../provider';
import {
  AUTO_MODEL,
  contextTokenOptions,
  defaultContextTokens,
  findInstalledModel,
  type LocalContextModel,
} from './modelOptions';
import { AUTO_EFFORT, asksModelName, DEFAULT_PROVIDER, isAgentOffer } from './providerOptions';

export interface NewChatSettings {
  provider: string;
  model: string;
  reasoning: ReasoningEffort;
  agent: boolean;
  autoApprove: boolean;
  offline: boolean;
  context: string;
}

type InstalledModel = LocalContextModel & { name: string; completion?: boolean };

export const DEFAULT_NEW_CHAT: NewChatSettings = {
  provider: DEFAULT_PROVIDER,
  model: AUTO_MODEL,
  reasoning: AUTO_EFFORT,
  agent: false,
  autoApprove: false,
  offline: false,
  context: String(defaultContextTokens()),
};

function offersModel(offer: ChatProviderOffer, model: string): boolean {
  if (model === AUTO_MODEL) return !offer.requires_model;
  if (asksModelName(offer) || offer.models.length === 0) return true;
  return offer.models.some((choice) => choice.name === model);
}

function installs(installed: InstalledModel[], model: string): boolean {
  if (model === AUTO_MODEL) return true;
  const found = findInstalledModel(installed, model);
  return found !== undefined && found.completion !== false;
}

/** The provider and model `recent` ran on, or Default's when the workspace no longer offers them. */
function resolveModel(
  recent: RecentChat,
  providers: WorkspaceChatProviders,
  installed: InstalledModel[]
): Pick<NewChatSettings, 'provider' | 'model'> {
  const fallback = { provider: DEFAULT_PROVIDER, model: AUTO_MODEL };
  if (recent.provider) {
    const offer = providers.providers.find((candidate) => candidate.provider === recent.provider);
    const model = recent.provider_model ?? AUTO_MODEL;
    return offer && offersModel(offer, model) ? { provider: offer.provider, model } : fallback;
  }
  const model = recent.model_name;
  const offered = isAgentOffer(providers.default)
    ? offersModel(providers.default, model)
    : installs(installed, model);
  return offered ? { provider: DEFAULT_PROVIDER, model } : fallback;
}

/**
 * What a new chat starts with: the settings of the chat the workspace used
 * last, with any provider or model it no longer offers put back to Default.
 */
export function newChatSettings(
  providers: WorkspaceChatProviders | null,
  installed: InstalledModel[]
): NewChatSettings {
  const recent = providers?.recent;
  if (!providers || !recent) return DEFAULT_NEW_CHAT;
  const { provider, model } = resolveModel(recent, providers, installed);
  const native = findInstalledModel(installed, model)?.details?.context_length;
  const context = String(recent.context_tokens);
  const offeredContext =
    model === recent.model_name &&
    contextTokenOptions(native).some((option) => option.value === context);
  return {
    provider,
    model,
    reasoning: recent.reasoning_effort,
    agent: recent.agent_enabled,
    autoApprove: recent.agent_enabled && recent.auto_approve,
    offline: recent.offline,
    context: offeredContext ? context : String(defaultContextTokens(native)),
  };
}
