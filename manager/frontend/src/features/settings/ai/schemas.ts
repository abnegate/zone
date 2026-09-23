import { z } from 'zod';
import { AiProviderSchema } from '../workspace/schemas';

const WebUrlSchema = z.url({ protocol: /^https?$/ });
const TimestampSchema = z.iso.datetime({ offset: true });

export const AgentSchema = z.enum(['claude', 'codex']);
export const AgentProviderSchema = AiProviderSchema.extract(['claude_code', 'codex']);
export const AgentStateSchema = z.enum(['signed_in', 'signed_out', 'pending', 'expired']);
export const AgentSourceSchema = z.enum(['zone', 'host']);
export const ClaudeScopeSchema = z.enum(['inference', 'full']);
export const CodeFailureSchema = z.enum(['invalid_code', 'start_again']);

export const AgentFailureSchema = z.object({
  error: z.string(),
  kind: CodeFailureSchema.optional().catch(undefined),
});

export const DevicePromptSchema = z.object({
  verification_url: WebUrlSchema,
  user_code: z.string().nullable(),
  expires_at: TimestampSchema,
});

export const AgentStatusSchema = z.object({
  agent: AgentSchema,
  provider: AgentProviderSchema,
  state: AgentStateSchema,
  source: AgentSourceSchema.nullable(),
  label: z.string().nullable(),
  expires_at: TimestampSchema.nullable(),
  models: z.array(z.string()),
  pending: DevicePromptSchema.nullable(),
  error: z.string().nullable(),
});

export const AgentStatusesSchema = z.object({
  agents: z.array(AgentStatusSchema),
});

export const ClaudeLoginSchema = z.object({
  agent: z.literal('claude'),
  authorize_url: WebUrlSchema,
  expires_at: TimestampSchema,
});

export const CodexLoginSchema = z.object({
  agent: z.literal('codex'),
  verification_url: WebUrlSchema,
  user_code: z.string(),
  expires_at: TimestampSchema,
});

export const AgentLoginSchema = z.discriminatedUnion('agent', [
  ClaudeLoginSchema,
  CodexLoginSchema,
]);

export type Agent = z.infer<typeof AgentSchema>;
export type AgentProvider = z.infer<typeof AgentProviderSchema>;
export type AgentState = z.infer<typeof AgentStateSchema>;
export type ClaudeScope = z.infer<typeof ClaudeScopeSchema>;
export type CodeFailure = z.infer<typeof CodeFailureSchema>;
export type DevicePrompt = z.infer<typeof DevicePromptSchema>;
export type AgentStatus = z.infer<typeof AgentStatusSchema>;
export type ClaudeLogin = z.infer<typeof ClaudeLoginSchema>;
export type CodexLogin = z.infer<typeof CodexLoginSchema>;
export type AgentLogin = z.infer<typeof AgentLoginSchema>;
