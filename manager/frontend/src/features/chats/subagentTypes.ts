export const SUBAGENT_STATUSES = ['running', 'completed', 'failed', 'stopped'] as const;
export type SubagentStatus = (typeof SUBAGENT_STATUSES)[number];

export const STEP_STATUSES = ['running', 'succeeded', 'failed'] as const;
export type StepStatus = (typeof STEP_STATUSES)[number];

/** One tool call a subagent made, shown under it rather than in the main thread. */
export interface SubagentStep {
  id: string;
  name: string;
  subject: string | null;
  status: StepStatus;
}

/**
 * A Claude Code or Codex subagent, keyed to the assistant message whose turn started it.
 * Its steps are read on demand; `revision` orders two copies of the same row.
 */
export interface ChatSubagent {
  id: string;
  chat_id: string;
  message_id: string | null;
  external_id: string;
  description: string;
  kind: string | null;
  model: string | null;
  background: boolean;
  status: SubagentStatus;
  tool_uses: number | null;
  duration_ms: number | null;
  tokens: number | null;
  last_tool: string | null;
  summary: string | null;
  error: string | null;
  step_count: number;
  revision: number;
  started_at: string;
  finished_at: string | null;
  updated_at: string;
}

/**
 * A chat's agents as one read listed them. A list read only in part adds and
 * updates agents, but is no proof that any it lacks are gone.
 */
export interface SubagentList {
  subagents: ChatSubagent[];
  complete: boolean;
}

/** Asks the panel to reveal one subagent; a new nonce asks again for the same one. */
export interface SubagentRequest {
  id: string;
  nonce: number;
}
