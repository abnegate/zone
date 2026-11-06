import type { AiProvider } from '../settings/workspace/types';

export type ReasoningEffort =
  | 'auto'
  | 'off'
  | 'low'
  | 'medium'
  | 'high'
  | 'xhigh'
  | 'max'
  | 'ultra';

export const REASONING_EFFORT_LABELS: Record<ReasoningEffort, string> = {
  auto: 'Auto',
  off: 'Off',
  low: 'Low',
  medium: 'Medium',
  high: 'High',
  xhigh: 'Extra high',
  max: 'Max',
  ultra: 'Ultra',
};

export interface ChatProviderModel {
  name: string;
  label: string;
  efforts: ReasoningEffort[];
}

export interface ChatProviderOffer {
  provider: AiProvider;
  label: string;
  usable: boolean;
  reason?: string | null;
  models: ChatProviderModel[];
  efforts: ReasoningEffort[];
  requires_model: boolean;
}

export interface ChatProviderSelection extends ChatProviderOffer {
  model: string | null;
}

export interface ChatProviders {
  default: ChatProviderOffer;
  selected: ChatProviderSelection | null;
  providers: ChatProviderOffer[];
  /** Whether this chat's turns run on a coding agent CLI. Omitted when the server cannot say. */
  cli?: boolean;
}

export interface RecentChat {
  model_name: string;
  provider: AiProvider | null;
  provider_model: string | null;
  reasoning_effort: ReasoningEffort;
  agent_enabled: boolean;
  offline: boolean;
  context_tokens: number | null;
}

export interface WorkspaceChatProviders {
  default: ChatProviderOffer;
  providers: ChatProviderOffer[];
  recent?: RecentChat | null;
}

export interface SetChatProviderRequest {
  provider: AiProvider | null;
  model?: string | null;
}
