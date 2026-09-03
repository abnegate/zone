// =============================================================================
// Chat Types
// =============================================================================

export type MessageRole = 'user' | 'assistant' | 'system';

export interface MessageAttachment {
  name: string;
  mime: string;
  url: string;
}

export interface MessageMetadata {
  attachments?: MessageAttachment[];
  /** When true, zone-server queries SearXNG (through Gluetun) before answering. */
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
}

export interface ChatWithMessages extends Chat {
  messages: Message[];
}

export interface CreateChatRequest {
  workspace_id: string;
  title: string;
  model_name: string;
  first_message?: string;
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
