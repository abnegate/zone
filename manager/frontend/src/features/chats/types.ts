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
