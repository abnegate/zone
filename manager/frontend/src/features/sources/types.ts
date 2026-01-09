/**
 * Source Types
 * All source-related types for the sources feature.
 */

export type SourceCategory = 'file' | 'calendar' | 'mail' | 'chat' | 'web' | 'text';

export type SourceType =
  | 'github'
  | 'gitlab'
  | 'filesystem' // File sources
  | 'ical' // Calendar sources
  | 'imap' // Mail sources
  | 'discord'
  | 'slack' // Chat sources
  | 'web'
  | 'text'; // Simple sources

// =============================================================================
// Source Configuration Types
// =============================================================================

// File source configs
export interface GitHubConfig {
  owner: string;
  repo: string;
  branch?: string;
  base_path?: string;
}

export interface GitLabConfig {
  project_id: string;
  host?: string;
  branch?: string;
  base_path?: string;
}

export interface FilesystemConfig {
  base_path: string;
  allow_writes?: boolean;
}

// Calendar source configs
export interface ICalConfig {
  url: string;
  refresh_interval?: number;
}

// Mail source configs
export interface IMAPConfig {
  host: string;
  port?: number;
  username: string;
  use_ssl?: boolean;
  folder?: string;
}

// Chat source configs
export interface DiscordConfig {
  server_id: string;
  channel_ids?: string[];
}

export interface SlackConfig {
  workspace_id: string;
  channel_ids?: string[];
}

// Simple source configs
export interface WebConfig {
  url: string;
  headers?: Record<string, string>;
}

export interface TextConfig {
  content: string;
  label?: string;
}

export type SourceConfig =
  | GitHubConfig
  | GitLabConfig
  | FilesystemConfig
  | ICalConfig
  | IMAPConfig
  | DiscordConfig
  | SlackConfig
  | WebConfig
  | TextConfig;

// =============================================================================
// Main Source Type
// =============================================================================

export interface Source {
  id: string;
  name: string;
  source_type: SourceType;
  category: SourceCategory;
  config: SourceConfig;
  description: string | null;
  url: string;
  is_active: boolean;
  last_verified_at: string | null;
  last_error: string | null;
  created_at: string;
  updated_at: string;
}

// =============================================================================
// Request/Response Types
// =============================================================================

export interface CreateSourceRequest {
  name: string;
  source_type: SourceType;
  config: SourceConfig;
  credentials?: string;
  description?: string;
}

export interface UpdateSourceRequest {
  name?: string;
  config?: SourceConfig;
  credentials?: string;
  description?: string;
  is_active?: boolean;
}

export interface SourceType_Info {
  id: SourceType;
  name: string;
  category: SourceCategory;
  enabled: boolean;
}

// API Response types
export interface ApiResponse {
  success?: boolean;
  error?: string;
}

// The following Response types are now exported from schemas.ts as Zod-inferred types:
// - SourcesResponse
// - SourceResponse
// - SourceTypesResponse
// - SourceVerifyResponse

// =============================================================================
// Content Types (unified content from all source types)
// =============================================================================

export interface ContentItem {
  id: string;
  source_id: string;
  category: SourceCategory;
  title: string;
  content: string;
  content_type: string;
  timestamp: string | null;
  url: string | null;
  metadata: ContentMetadata;
}

export type ContentMetadata =
  | FileMetadata
  | CalendarMetadata
  | MailMetadata
  | ChatMetadata
  | WebMetadata
  | TextMetadataType;

export interface FileMetadata {
  type: 'file';
  path: string;
  size: number;
  sha: string | null;
  is_directory: boolean;
}

export interface CalendarMetadata {
  type: 'calendar';
  start_time: string;
  end_time: string;
  location: string | null;
  attendees: string[];
  recurrence: string | null;
  all_day: boolean;
}

export interface MailMetadata {
  type: 'mail';
  from: string;
  to: string[];
  cc: string[];
  subject: string;
  thread_id: string | null;
  attachments: string[];
  is_read: boolean;
}

export interface ChatMetadata {
  type: 'chat';
  channel_id: string;
  channel_name: string | null;
  author_id: string;
  author_name: string;
  thread_id: string | null;
  reactions: string[];
}

export interface WebMetadata {
  type: 'web';
  status_code: number;
  headers: Record<string, string>;
  fetched_at: string;
}

export interface TextMetadataType {
  type: 'text';
  label: string | null;
}

export interface ContentListResult {
  items: ContentItem[];
  total: number;
  has_more: boolean;
}

// ContentResponse is now exported from schemas.ts as a Zod-inferred type
