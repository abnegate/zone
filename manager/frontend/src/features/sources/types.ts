import type { GitHubInstallationStatus } from '../github/installationStatus';

export type SourceCategory = 'file' | 'calendar' | 'mail' | 'chat' | 'web' | 'text';

export type SourceType =
  | 'github'
  | 'gitlab'
  | 'filesystem'
  | 'ical'
  | 'imap'
  | 'discord'
  | 'slack'
  | 'web'
  | 'text';

export interface GitHubConfig {
  owner: string;
  repo: string;
  branch?: string;
  base_path?: string;
}

export interface GitLabConfig {
  project_id: string;
  host?: string;
  base_url?: string;
  branch?: string;
  base_path?: string;
}

export interface FilesystemConfig {
  base_path: string;
  allow_writes?: boolean;
}

export interface ICalConfig {
  url: string;
  refresh_interval?: number;
}

export interface IMAPConfig {
  host: string;
  port?: number;
  username: string;
  use_ssl?: boolean;
  folder?: string;
}

export interface DiscordConfig {
  server_id: string;
  channel_ids?: string[];
}

export interface SlackConfig {
  workspace_id: string;
  channel_ids?: string[];
}

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
  github_installation_id?: string | null;
  has_credentials?: boolean;
  github_installation_status?: GitHubInstallationStatus | null;
  github_account_login?: string | null;
  created_at: string;
  updated_at: string;
}

export interface CreateSourceRequest {
  name: string;
  source_type: SourceType;
  config: SourceConfig;
  credentials?: string;
  github_installation_id?: string;
  description?: string;
  url?: string;
}

export interface UpdateSourceRequest {
  name?: string;
  config?: SourceConfig;
  credentials?: string;
  github_installation_id?: string | null;
  description?: string;
  is_active?: boolean;
  url?: string;
  instance?: string;
}

export interface SourceType_Info {
  id: SourceType;
  name: string;
  category: SourceCategory;
  enabled: boolean;
}

export interface ApiResponse {
  success?: boolean;
  error?: string;
}

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
