// Workspace Member Types - import from auth for consistency
import type { WorkspaceRole } from '../../auth/types';
import type { ApiResponse, OrganizationMember } from '../organization/types';
export type { WorkspaceRole };
// Re-export OrganizationMember for WorkspaceMembersSection
export type { OrganizationMember, ApiResponse };

// Workspace Types
export interface Workspace {
  id: string;
  organization_id: string;
  name: string;
  slug: string;
  description: string | null;
  is_active: boolean;
  created_at: string;
  updated_at: string;
}

export interface CreateWorkspaceRequest {
  name: string;
  slug: string;
  description?: string;
}

export interface UpdateWorkspaceRequest {
  name?: string;
  slug?: string;
  description?: string;
  is_active?: boolean;
}

export interface WorkspaceMember {
  id: string;
  user_id: string;
  workspace_id: string;
  role: WorkspaceRole;
  email: string;
  display_name: string | null;
  joined_at: string;
}

export interface AddWorkspaceMemberRequest {
  user_id: string;
  role: WorkspaceRole;
}

export interface UpdateWorkspaceMemberRequest {
  role: WorkspaceRole;
}

export interface WorkspaceMembersResponse {
  members: WorkspaceMember[];
}

// Workspace Theme Types
export type FontFamily = 'system' | 'inter' | 'roboto' | 'open-sans' | 'lato' | 'nunito';
export type BorderRadius = 'none' | 'small' | 'medium' | 'large';

export interface WorkspaceTheme {
  id: string;
  workspace_id: string;
  primary_color_light: string;
  secondary_color_light: string;
  primary_color_dark: string;
  secondary_color_dark: string;
  font_family: FontFamily;
  font_size_base: string;
  border_radius: BorderRadius;
  created_at: string;
  updated_at: string;
}

export interface UpdateWorkspaceThemeRequest {
  primary_color_light?: string;
  secondary_color_light?: string;
  primary_color_dark?: string;
  secondary_color_dark?: string;
  font_family?: FontFamily;
  font_size_base?: string;
  border_radius?: BorderRadius;
}

export interface WorkspaceThemeResponse {
  success?: boolean;
  error?: string;
  theme: WorkspaceTheme;
}

// AI Provider Types
export type AiProvider = 'self_hosted' | 'openai' | 'anthropic' | 'bedrock';

export interface AiSettings {
  provider: AiProvider;
  has_litellm_key: boolean;
  litellm_host: string | null;
  has_openai_api_key: boolean;
  openai_base_url: string | null;
  has_anthropic_api_key: boolean;
  anthropic_base_url: string | null;
  bedrock_region: string | null;
  bedrock_use_iam_role: boolean;
  has_bedrock_credentials: boolean;
  model_fast: string | null;
  model_reasoning: string | null;
  model_embedding: string | null;
  model_image: string | null;
}

export interface UpdateAiSettingsRequest {
  provider?: AiProvider;
  litellm_host?: string;
  litellm_key?: string;
  openai_api_key?: string;
  openai_base_url?: string;
  anthropic_api_key?: string;
  anthropic_base_url?: string;
  bedrock_region?: string;
  bedrock_access_key?: string;
  bedrock_secret_key?: string;
  bedrock_use_iam_role?: boolean;
  model_fast?: string;
  model_reasoning?: string;
  model_embedding?: string;
  model_image?: string;
}

export interface AiSettingsResponse {
  success?: boolean;
  error?: string;
  settings: AiSettings;
}

// API Response wrappers
export interface WorkspacesResponse extends ApiResponse {
  workspaces: Workspace[];
}

export interface WorkspaceResponse extends ApiResponse {
  workspace: Workspace;
}
