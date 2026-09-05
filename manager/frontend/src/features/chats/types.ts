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
  | 'workspace_document';

export type CitationOutcome = 'success' | 'failure' | 'pending' | 'incomplete' | 'observed';

/// A checkable source behind an agent reply. Incomplete evidence is never a pass.
export interface Citation {
  kind: CitationKind;
  title: string;
  url: string;
  revision?: string | null;
  observed_at: string;
  complete: boolean;
  outcome: CitationOutcome;
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
}

export interface ChatWithMessages extends Chat {
  messages: Message[];
}

export interface CreateChatRequest {
  workspace_id: string;
  title: string;
  model_name: string;
  first_message?: string;
  automatic_title?: boolean;
  agent_enabled?: boolean;
  auto_approve?: boolean;
  character?: ChatCharacter;
}

export interface UpdateChatRequest {
  title?: string;
  agent_enabled?: boolean;
  auto_approve?: boolean;
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
