export type MessageRole = 'user' | 'assistant' | 'system';

export interface MessageAttachment {
  name: string;
  mime: string;
  url: string;
}

/// One tool the agent ran while producing a reply. Streamed over the socket as
/// it happens and stored on the message, so the trace survives a reload.
export interface ToolCallRecord {
  id: string;
  name: string;
  arguments: string;
  success: boolean;
  detail: string;
  duration_ms: number;
  /** Model thinking that immediately preceded this call. */
  reasoning?: string;
  /**
   * Why the model said it was making this call. Side-effecting tools are asked
   * for one. Model-authored prose, so it is shown as stated, never as observed.
   */
  reason?: string;
  /**
   * What the call will do, rendered by the server from the arguments rather
   * than from the model's account of them. Observed, so it is what settles a
   * disagreement with the stated reason above it.
   */
  preview?: string;
  /** Client-only: set while the tool is still running. Never sent by the server. */
  pending?: boolean;
  /** Client-only: mutating file/shell tools wait here for the user. */
  approval?: 'pending' | 'approved' | 'denied';
}

export type CitationKind =
  | 'github_build'
  | 'github_deployment'
  | 'github_issue'
  | 'github_file'
  | 'workspace_document'
  | 'behavioral_verification';

export type CitationOutcome = 'success' | 'failure' | 'pending' | 'incomplete' | 'observed';

/// How the outcome was produced. The server executing something and recording
/// the result proves it; the model saying so is a claim, and a claim never
/// authorises a passing result. Stored citations predate the field and are
/// server-proven, matching the server's own default.
export type CitationProvenance = 'server_execution' | 'model_asserted';

/// A checkable source behind an agent reply. Incomplete evidence is never a pass.
export interface Citation {
  kind: CitationKind;
  title: string;
  url: string;
  revision?: string | null;
  observed_at: string;
  complete: boolean;
  outcome: CitationOutcome;
  provenance: CitationProvenance;
  note?: string | null;
}

/// A stated reason is the model's own sentence, not something the server saw.
/// Citations already draw that line between proof and claim; the same line has
/// to hold here, because a reason is the only part of a receipt the model wrote.
export const REASON_LABEL = 'Reason, stated by the model';

/// Shown instead of a blank when a side-effecting call arrived without one.
/// Silence must read as an absence the reader notices, not as nothing to say.
export const REASON_MISSING = 'No reason given';

/// The counterpart to REASON_LABEL, and the reason the two sit apart on the
/// row: a preview is the server reading the call's own arguments, so it is the
/// half of an approval a reader can trust when the stated reason disagrees.
export const PREVIEW_LABEL = 'Effect, read from the call by the server';

/// Tools that change something outside the conversation and are therefore
/// asked to say why. The trace row is where a reader sees that answer, whether
/// the call is still waiting on them or already done, so an absent reason is
/// called out here rather than passed over in silence.
///
/// A copy of the server's list, which is why `schemas.contract.test.ts` reads
/// the Rust one and compares. An eighth reasoned tool added there and missed
/// here shows nothing where the absence should have been.
export const REASONED_TOOLS: ReadonlySet<string> = new Set([
  'apply_patch',
  'comment_on_issue',
  'create_pull_request',
  'run_command',
  'run_shell',
  'send_message',
  'write_file',
]);

export type ActionTarget = 'task' | 'document' | 'message' | 'reminder';

/// A workspace write the agent completed. Streamed live and stored on the
/// message so the receipt survives a reload.
export interface ActionReceipt {
  id: string;
  action: string;
  target_type: ActionTarget;
  target_id: string;
  target_label: string;
  actor_id: string;
  actor_name: string;
  occurred_at: string;
  success: boolean;
  outcome: string;
  href: string;
  /**
   * Why the model said it was making this write. The one model-authored field
   * on an otherwise server-observed record, and absent on older receipts.
   */
  reason?: string;
}

export interface MessageMetadata {
  attachments?: MessageAttachment[];
  tool_calls?: ToolCallRecord[];
  citations?: Citation[];
  action_receipts?: ActionReceipt[];
  /** Optional API override: force web search on/off for one message. */
  web_search?: boolean;
  /** Model thinking text, when the deployment advertised reasoning. */
  reasoning?: string;
}

export interface Message {
  id: string;
  chat_id: string;
  role: MessageRole;
  content: string;
  created_at: string;
  metadata?: MessageMetadata | null;
}

export interface ChatCharacter {
  name: string;
  description?: string | null;
  personality?: string | null;
  scenario?: string | null;
  first_mes?: string | null;
  mes_example?: string | null;
  system_prompt?: string | null;
  post_history_instructions?: string | null;
  stop_sequences?: string[];
  source_name?: string | null;
}

export interface Chat {
  id: string;
  title: string;
  model_name: string;
  created_at: string;
  updated_at: string;
  archived: boolean;
  /** Persona for models that expect a character card. Absent on ordinary assistant chats. */
  character?: ChatCharacter | null;
  /** Whether the installed model advertised tool calling. */
  tools?: boolean | null;
  /** Whether this model should offer a character card. */
  needs_character?: boolean | null;
  /**
   * Whether replies run the tool-calling agent loop, including workspace tools
   * and server filesystem and shell tools.
   */
  agent_enabled: boolean;
  /**
   * When true, mutating file and shell tools run without a confirmation.
   * Older servers omit this; treat those chats as requiring approval.
   */
  auto_approve?: boolean;
  /** Whether the installed model advertised thinking / extended reasoning. */
  reasoning?: boolean | null;
  /**
   * How much a thinking-capable model should think. Auto inspects the next
   * request. Older servers omit this; treat those chats as Auto.
   */
  reasoning_effort?: ReasoningEffort;
}

export type ReasoningEffort = 'auto' | 'off' | 'low' | 'medium' | 'high';

export const REASONING_EFFORT_OPTIONS: Array<{ value: ReasoningEffort; label: string }> = [
  { value: 'auto', label: 'Auto' },
  { value: 'low', label: 'Low' },
  { value: 'medium', label: 'Medium' },
  { value: 'high', label: 'High' },
  { value: 'off', label: 'Off' },
];

export interface ChatWithMessages extends Chat {
  messages: Message[];
  context?: ContextUsage | null;
}

export interface CreateChatRequest {
  workspace_id: string;
  title: string;
  model_name: string;
  first_message?: string;
  automatic_title?: boolean;
  agent_enabled?: boolean;
  auto_approve?: boolean;
  reasoning_effort?: ReasoningEffort;
  character?: ChatCharacter;
}

export interface UpdateChatRequest {
  title?: string;
  agent_enabled?: boolean;
  auto_approve?: boolean;
  reasoning_effort?: ReasoningEffort;
  character?: ChatCharacter | null;
  clear_character?: boolean;
}

export interface SendMessageRequest {
  content: string;
  metadata?: MessageMetadata;
}

export interface ChatSearchResult {
  message_id: string;
  chat_id: string;
  chat_title: string;
  content: string;
  snippet: string;
  relevance_score: number;
  created_at: string;
}

export interface ChatSearchOptions {
  query: string;
  chat_id?: string;
  limit?: number;
}

export interface ChatSearchResponse {
  results: ChatSearchResult[];
  total: number;
}

export type ContextStatus = 'ready' | 'compacting' | 'compacted' | 'unavailable' | 'blocked';
export type ContextSource = 'runtime' | 'configured' | 'provider' | 'unknown';
export interface ContextBreakdown {
  instructions: number;
  conversation: number;
  tools: number;
  results: number;
  summary: number;
  attachments: number | null;
  overhead: number;
}
export interface ContextUsage {
  model: string;
  used: number;
  limit: number | null;
  reserved: number;
  threshold: number | null;
  remaining: number | null;
  estimated: boolean;
  incomplete: boolean;
  source: ContextSource;
  status: ContextStatus;
  breakdown: ContextBreakdown;
  revision: number;
  compacted_messages: number;
  updated_at: string;
  reason?: string | null;
}
