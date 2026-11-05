import { z } from 'zod';
import { tolerantArray } from './schemas';
import { STEP_STATUSES, SUBAGENT_STATUSES } from './subagentTypes';

export const SUBAGENT_FRAME = 'subagent';
export const INIT_FRAME = 'init';
export const MESSAGE_START_FRAME = 'message_start';

/**
 * A status this client does not know yet is read as settled rather than
 * dropping the row, so a newer server costs the label, never the agent.
 */
const UNKNOWN_SUBAGENT_STATUS = 'stopped';
const UNKNOWN_STEP_STATUS = 'running';

/** A bad value in a descriptive field costs that field, never the row. */
const forgivingText = z.string().nullable().catch(null);
const forgivingCount = z.number().nullable().catch(null);

export const SubagentStepSchema = z.object({
  id: z.string(),
  name: z.string(),
  subject: forgivingText,
  status: z.enum(STEP_STATUSES).catch(UNKNOWN_STEP_STATUS),
});

export const ChatSubagentSchema = z.object({
  id: z.string(),
  chat_id: z.string(),
  message_id: z.string().nullable(),
  external_id: z.string(),
  description: z.string(),
  kind: forgivingText,
  model: forgivingText,
  background: z.boolean(),
  status: z.enum(SUBAGENT_STATUSES).catch(UNKNOWN_SUBAGENT_STATUS),
  tool_uses: forgivingCount,
  duration_ms: forgivingCount,
  tokens: forgivingCount,
  last_tool: forgivingText,
  summary: forgivingText,
  error: forgivingText,
  steps: tolerantArray(SubagentStepSchema).catch([]),
  started_at: z.string(),
  finished_at: forgivingText,
  updated_at: z.string(),
});

export const ChatSubagentsResponseSchema = z.object({
  subagents: tolerantArray(ChatSubagentSchema).default([]),
});

export const SubagentFrameSchema = z.object({
  type: z.literal(SUBAGENT_FRAME),
  chat_id: z.string(),
  subagent: ChatSubagentSchema,
});

/** A frame after which the stored agents may have moved on without this client seeing it. */
export const ResyncFrameSchema = z.discriminatedUnion('type', [
  z.object({ type: z.literal(INIT_FRAME) }),
  z.object({ type: z.literal(MESSAGE_START_FRAME), resumed: z.literal(true) }),
]);
