import { AiProviderSchema } from '../../settings/workspace/schemas';
import type { AiProvider as AiProviderValue } from '../../settings/workspace/types';
import { agentLabel, denialMessage, providerLabel } from './messages';
import { PolicyAgent, PolicyCode, PolicyProvider } from './schemas';
import type {
  PolicyAgent as PolicyAgentValue,
  PolicyDenialCode,
  PolicyProvider as PolicyProviderValue,
  PolicyRules,
} from './types';

const AiProvider = AiProviderSchema.enum;

const DIRECT: Readonly<Partial<Record<AiProviderValue, PolicyProviderValue>>> = {
  [AiProvider.openai]: PolicyProvider.OpenAI,
  [AiProvider.anthropic]: PolicyProvider.Anthropic,
};

const AGENTS: Readonly<Partial<Record<AiProviderValue, PolicyAgentValue>>> = {
  [AiProvider.claude_code]: PolicyAgent.Claude,
  [AiProvider.codex]: PolicyAgent.Codex,
};

export function providerDenial(
  rules: PolicyRules,
  provider: AiProviderValue
): PolicyDenialCode | null {
  const direct = DIRECT[provider];
  if (direct !== undefined) {
    return rules.providers === null || rules.providers.includes(direct)
      ? null
      : PolicyCode.ProviderDenied;
  }
  const agent = AGENTS[provider];
  if (agent !== undefined) {
    return rules.agents === null || rules.agents.includes(agent) ? null : PolicyCode.AgentDenied;
  }
  return null;
}

export function providerSubject(provider: AiProviderValue): string | undefined {
  const direct = DIRECT[provider];
  if (direct !== undefined) {
    return providerLabel(direct);
  }
  const agent = AGENTS[provider];
  return agent === undefined ? undefined : agentLabel(agent);
}

export function providerDenialMessage(
  rules: PolicyRules,
  provider: AiProviderValue
): string | null {
  const denial = providerDenial(rules, provider);
  return denial === null ? null : denialMessage(denial, providerSubject(provider));
}
