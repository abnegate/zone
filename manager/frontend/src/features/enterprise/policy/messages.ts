import { PolicyAgent, PolicyCode, PolicyField, PolicyProvider, PolicyTier } from './schemas';
import type {
  PolicyAdminCode,
  PolicyAgent as PolicyAgentValue,
  PolicyDenialCode,
  PolicyField as PolicyFieldValue,
  PolicyProvider as PolicyProviderValue,
  PolicyTier as PolicyTierValue,
} from './types';

const PROVIDER_LABELS: Readonly<Record<PolicyProviderValue, string>> = {
  [PolicyProvider.Local]: 'Local models',
  [PolicyProvider.Anthropic]: 'Anthropic API',
  [PolicyProvider.OpenAI]: 'OpenAI API',
  [PolicyProvider.Bedrock]: 'AWS Bedrock',
  [PolicyProvider.Vertex]: 'Google Vertex AI',
  [PolicyProvider.Azure]: 'Azure OpenAI',
  [PolicyProvider.Other]: 'Other OpenAI-compatible endpoints',
};

const AGENT_LABELS: Readonly<Record<PolicyAgentValue, string>> = {
  [PolicyAgent.Claude]: 'Claude Code',
  [PolicyAgent.Codex]: 'Codex',
};

const CEILING_LABELS: Readonly<Record<PolicyTierValue, string>> = {
  [PolicyTier.Read]: 'Ask before any tool that changes something',
  [PolicyTier.Write]: 'Ask before host tools such as shell and file writes',
  [PolicyTier.Host]: 'Ask before outward tools such as messages, pull requests, web and MCP',
  [PolicyTier.Outward]: 'No limit',
};

export const NO_CEILING_LABEL = 'No limit';

const FIELD_LABELS: Readonly<Record<PolicyFieldValue, string>> = {
  [PolicyField.Providers]: 'Providers',
  [PolicyField.Agents]: 'Coding agents',
  [PolicyField.Models]: 'Models',
  [PolicyField.Ceiling]: 'Tool approval',
  [PolicyField.McpServers]: 'MCP servers',
  [PolicyField.Domains]: 'Outbound domains',
};

const FIELD_DESCRIPTIONS: Readonly<Record<PolicyFieldValue, string>> = {
  [PolicyField.Providers]:
    'Where model requests may go. Local models stay on this server; every other provider receives the prompts.',
  [PolicyField.Agents]: "Coding agents that run on someone's own Claude or ChatGPT sign-in.",
  [PolicyField.Models]:
    'Model names people can pick. End an entry with * to allow every model that starts with it.',
  [PolicyField.Ceiling]:
    'Tool calls above this level always wait for a person, even when auto-approve is on.',
  [PolicyField.McpServers]: 'MCP servers agents may use, by name.',
  [PolicyField.Domains]:
    'Hosts agents may reach. A domain covers its subdomains. Restricting this turns off web search and other networked tools.',
};

const UNRESTRICTED: Readonly<Record<PolicyFieldValue, string>> = {
  [PolicyField.Providers]: 'Every provider is allowed.',
  [PolicyField.Agents]: 'Every coding agent is allowed.',
  [PolicyField.Models]: 'Every model is allowed.',
  [PolicyField.Ceiling]: "Tools follow each chat's own approval setting.",
  [PolicyField.McpServers]: 'Every MCP server is allowed.',
  [PolicyField.Domains]: 'Agents can reach any domain.',
};

export const NOTHING_ALLOWED = 'Nothing is allowed.';

export const NOTHING_ALLOWED_WARNING =
  'Nothing is allowed, so everything this field covers is refused. Add an entry, or stop restricting it.';

export const NOT_ALLOWED = 'Not allowed by your organization';

export const AGENT_POLICY_LINK = 'See the agent policy';

const ENTRY_SEPARATOR = ', ';

const DEFAULT_PROVIDER_SUBJECT = 'this provider';

const DEFAULT_AGENT_SUBJECT = 'this coding agent';

const DENIALS: Readonly<Record<PolicyDenialCode, (subject: string | undefined) => string>> = {
  [PolicyCode.ProviderDenied]: (subject) =>
    `Your organization's policy doesn't allow ${subject ?? DEFAULT_PROVIDER_SUBJECT} in this workspace.`,
  [PolicyCode.AgentDenied]: (subject) =>
    `Your organization's policy doesn't allow ${subject ?? DEFAULT_AGENT_SUBJECT} in this workspace.`,
  [PolicyCode.ModelDenied]: () =>
    "Your organization's policy doesn't allow this model in this workspace. Choose an allowed model.",
  [PolicyCode.ServerDenied]: () =>
    "Your organization's policy doesn't allow this MCP server in this workspace.",
  [PolicyCode.DomainDenied]: () =>
    "Your organization's policy doesn't allow requests to this domain from this workspace.",
  [PolicyCode.ApprovalRequired]: () =>
    "Your organization's policy requires a person to approve this tool call, and nobody can approve it in this run.",
  [PolicyCode.Unreadable]: () =>
    "Your organization's agent policy couldn't be read, so this can't start. Try again shortly.",
};

const REFUSALS: Readonly<Record<PolicyAdminCode, string>> = {
  [PolicyCode.LicenseRequired]:
    'Changing the agent policy needs a Zone Enterprise license that is active or in its grace period.',
  [PolicyCode.Wider]:
    "A workspace can only narrow its organization's policy. Remove anything the organization doesn't allow.",
  [PolicyCode.ModelInvalid]:
    'Enter model names like claude-sonnet-4, or a prefix ending in one *, like gpt-5*, with no spaces.',
  [PolicyCode.DomainInvalid]:
    'Enter hostnames like example.com or IP addresses, without a scheme, port or path.',
  [PolicyCode.ServerInvalid]:
    'Enter MCP server names of up to 32 letters, digits, dashes or underscores.',
  [PolicyCode.CeilingInvalid]:
    'Choose to ask before host tools or before outward tools, or set no limit.',
  [PolicyCode.BodyInvalid]: "Zone couldn't read this change. Reload the page and try again.",
};

const REFUSAL_FIELDS: Readonly<Partial<Record<PolicyAdminCode, PolicyFieldValue>>> = {
  [PolicyCode.ModelInvalid]: PolicyField.Models,
  [PolicyCode.DomainInvalid]: PolicyField.Domains,
  [PolicyCode.ServerInvalid]: PolicyField.McpServers,
  [PolicyCode.CeilingInvalid]: PolicyField.Ceiling,
};

const DENIAL_CODES: ReadonlySet<string> = new Set(Object.keys(DENIALS));

const ADMIN_CODES: ReadonlySet<string> = new Set(Object.keys(REFUSALS));

export function isDenialCode(code: string): code is PolicyDenialCode {
  return DENIAL_CODES.has(code);
}

export function isAdminCode(code: string): code is PolicyAdminCode {
  return ADMIN_CODES.has(code);
}

export function providerLabel(provider: PolicyProviderValue): string {
  return PROVIDER_LABELS[provider];
}

export function agentLabel(agent: PolicyAgentValue): string {
  return AGENT_LABELS[agent];
}

export function ceilingLabel(ceiling: PolicyTierValue | null): string {
  return ceiling === null ? NO_CEILING_LABEL : CEILING_LABELS[ceiling];
}

export function fieldLabel(field: PolicyFieldValue): string {
  return FIELD_LABELS[field];
}

export function fieldDescription(field: PolicyFieldValue): string {
  return FIELD_DESCRIPTIONS[field];
}

export function unrestrictedText(field: PolicyFieldValue): string {
  return UNRESTRICTED[field];
}

export function denialMessage(code: PolicyDenialCode, subject?: string): string {
  return DENIALS[code](subject);
}

export function denialReason(denial: string, subject?: string): string {
  return isDenialCode(denial) ? denialMessage(denial, subject) : denial;
}

export function notAllowed(entries: readonly string[]): string {
  return `${NOT_ALLOWED}: ${entries.join(ENTRY_SEPARATOR)}.`;
}

export function pendingText(entries: readonly string[]): string {
  return `Saving adds ${entries.join(ENTRY_SEPARATOR)}.`;
}

export function refusalMessage(code: PolicyAdminCode): string {
  return REFUSALS[code];
}

export function refusalField(code: PolicyAdminCode): PolicyFieldValue | null {
  return REFUSAL_FIELDS[code] ?? null;
}
