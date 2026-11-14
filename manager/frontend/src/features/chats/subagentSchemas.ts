import { z } from 'zod';
import { tolerantArray } from './schemas';
import { STEP_STATUSES, SUBAGENT_STATUSES, type SubagentList } from './subagentTypes';

export const SUBAGENT_FRAME = 'subagent';
export const SUBAGENTS_REMOVED_FRAME = 'subagents_removed';
export const INIT_FRAME = 'init';
export const MESSAGE_START_FRAME = 'message_start';

export const SUBAGENTS_LOAD_FAILED = 'Failed to load agents';
export const STEPS_LOAD_FAILED = 'Failed to load steps';

/**
 * A status this client does not know yet is read as settled rather than
 * dropping the row, so a newer server costs the label, never the agent.
 */
const UNKNOWN_SUBAGENT_STATUS = 'stopped';
const UNKNOWN_STEP_STATUS = 'running';

/** A bad value in a descriptive field costs that field, never the row. */
const forgivingText = z.string().nullable().catch(null);
const forgivingCount = z.number().nullable().catch(null);
const forgivingTally = z.number().int().nonnegative().catch(0);

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
  step_count: forgivingTally,
  revision: forgivingTally,
  started_at: z.string(),
  finished_at: forgivingText,
  updated_at: z.string(),
});

const ReadableSubagentsSchema = tolerantArray(ChatSubagentSchema);

export const ChatSubagentsResponseSchema = z
  .object({ subagents: z.array(z.unknown()).optional() })
  .transform(({ subagents: rows }): SubagentList => {
    const subagents = ReadableSubagentsSchema.parse(rows ?? []);
    return { subagents, complete: rows !== undefined && subagents.length === rows.length };
  });

export const SubagentStepsResponseSchema = z.object({
  steps: tolerantArray(SubagentStepSchema).default([]),
});

/** One write to a subagent: its new row, and only the steps that write changed. */
export const SubagentFrameSchema = z.object({
  type: z.literal(SUBAGENT_FRAME),
  chat_id: z.string(),
  subagent: ChatSubagentSchema,
  steps: tolerantArray(SubagentStepSchema).catch([]),
});

export const SubagentsRemovedFrameSchema = z.object({
  type: z.literal(SUBAGENTS_REMOVED_FRAME),
  chat_id: z.string(),
  ids: z.array(z.string()),
});

/** A reply picked back up after a dropped socket, after which stored agents may have moved on. */
export const ResumedStartFrameSchema = z.object({
  type: z.literal(MESSAGE_START_FRAME),
  resumed: z.literal(true),
});
