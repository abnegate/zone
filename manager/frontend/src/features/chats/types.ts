// =============================================================================
// Chat Types
// =============================================================================

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
}

export interface MessageMetadata {
  attachments?: MessageAttachment[];
  tool_calls?: ToolCallRecord[];
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

export interface Chat {
  id: string;
  title: string;
  model_name: string;
  created_at: string;
  updated_at: string;
  archived: boolean;
  /** Whether replies run the tool-calling agent loop. */
  agent_enabled: boolean;
  /**
   * Whether the agent is limited to read-only workspace tools. When false it
   * also gets a shell and file access on the machine running the server.
   */
  agent_sandboxed: boolean;
}

export interface ChatWithMessages extends Chat {
  messages: Message[];
}

export interface CreateChatRequest {
  workspace_id: string;
  title: string;
  model_name: string;
  first_message?: string;
  agent_enabled?: boolean;
  agent_sandboxed?: boolean;
}

export interface UpdateChatRequest {
  title?: string;
  agent_enabled?: boolean;
  agent_sandboxed?: boolean;
}

export interface SendMessageRequest {
  content: string;
  metadata?: MessageMetadata;
}

// =============================================================================
// Chat Search Types
// =============================================================================

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
