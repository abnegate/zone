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
  /**
   * The agent CLI this chat's turns run on first; the router may move a turn to
   * another agent's login. Null when they go over HTTP or cannot run.
   */
  agent?: AiProvider | null;
}

export interface WorkspaceChatProviders {
  default: ChatProviderOffer;
  providers: ChatProviderOffer[];
}

export interface SetChatProviderRequest {
  provider: AiProvider | null;
  model?: string | null;
}
