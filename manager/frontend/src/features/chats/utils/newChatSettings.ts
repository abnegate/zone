import type {
  ChatProviderOffer,
  ReasoningEffort,
  RecentChat,
  WorkspaceChatProviders,
} from '../provider';
import { type CreateChatRequest, REASONING_EFFORT_OPTIONS } from '../types';
import {
  AUTO_MODEL,
  chatShowsReasoning,
  contextTokenOptions,
  defaultContextTokens,
  findInstalledModel,
  type LocalContextModel,
  type ModelHints,
} from './modelOptions';
import {
  AUTO_EFFORT,
  DEFAULT_PROVIDER,
  effortsFor,
  isAgentOffer,
  keepEffort,
  offersAgentOn,
  offersContextOn,
} from './providerOptions';

export interface NewChatSettings {
  provider: string;
  model: string;
  reasoning: ReasoningEffort;
  agent: boolean;
  offline: boolean;
  context: string;
}

type ListedModel = LocalContextModel & ModelHints & { name: string };

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

/** The provider the settings chose for this chat alone, null while they follow the workspace's. */
function chosenOffer(
  settings: NewChatSettings,
  providers: WorkspaceChatProviders | null
): ChatProviderOffer | null {
  return providers?.providers.find((offer) => offer.provider === settings.provider) ?? null;
}

/** Whether a reasoning effort means anything to what `settings` would run on. */
function showsReasoning(
  settings: NewChatSettings,
  efforts: ReasoningEffort[] | null,
  installed: ListedModel[]
): boolean {
  if (efforts) return efforts.length > 0;
  if (settings.model === AUTO_MODEL) {
    return installed.some((model) => chatShowsReasoning({}, model));
  }
  const selected = findInstalledModel(installed, settings.model);
  return selected ? chatShowsReasoning({}, selected) : false;
}

/**
 * The chat `+` creates from `settings`: whatever the chat created last ran on,
 * carrying only the fields that mean anything to it. There is no dialog to
 * refuse a model in, so the settings are taken as the workspace offers them
 * and anything it no longer offers has already fallen back to Automatic.
 */
export function newChatRequest(
  workspaceId: string,
  settings: NewChatSettings,
  providers: WorkspaceChatProviders | null,
  installed: ListedModel[]
): CreateChatRequest {
  const chosen = chosenOffer(settings, providers);
  const fallback = providers?.default ?? null;
  const offer = chosen ?? (fallback && isAgentOffer(fallback) ? fallback : null);
  const model = settings.model.trim() || AUTO_MODEL;
  const efforts = offer ? effortsFor(offer, model) : null;
  const effort = efforts ? keepEffort(settings.reasoning, efforts) : settings.reasoning;
  const workspaceProvider = fallback?.provider ?? null;
  const agent = offersAgentOn(
    offer,
    offer === null || offer.provider === workspaceProvider ? installed : null,
    model
  )
    ? settings.agent
    : false;
  const context = offersContextOn(
    chosen?.provider ?? workspaceProvider,
    workspaceProvider,
    installed,
    model
  )
    ? Number(settings.context)
    : null;
  return {
    workspace_id: workspaceId,
    title: model === AUTO_MODEL ? 'New chat' : `Chat with ${model}`,
    automatic_title: true,
    model_name: chosen ? AUTO_MODEL : model,
    ...(chosen
      ? { provider: chosen.provider, provider_model: model === AUTO_MODEL ? null : model }
      : {}),
    agent_enabled: agent,
    // Never carried: chats have no owner, so the chat created last may be
    // another member's, and approving its writes is not approving theirs.
    auto_approve: false,
    ...(showsReasoning(settings, efforts, installed) && effort !== AUTO_EFFORT
      ? { reasoning_effort: effort }
      : {}),
    ...(settings.offline ? { offline: true } : {}),
    ...(context !== null ? { context_tokens: context } : {}),
  };
}
