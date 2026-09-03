import { z } from 'zod';
import { WorkspaceRoleSchema } from '../../auth/schemas';

// Workspace Schemas
export const WorkspaceSchema = z.object({
  id: z.string(),
  organization_id: z.string(),
  name: z.string(),
  slug: z.string(),
  description: z.string().nullable(),
  is_active: z.boolean(),
  created_at: z.string(),
  updated_at: z.string(),
});

export const CreateWorkspaceRequestSchema = z.object({
  name: z.string().min(1, 'Name is required'),
  slug: z.string().min(1, 'Slug is required'),
  description: z.string().optional(),
});

export const UpdateWorkspaceRequestSchema = z.object({
  name: z.string().min(1).optional(),
  slug: z.string().min(1).optional(),
  description: z.string().optional(),
  is_active: z.boolean().optional(),
});

export const WorkspacesResponseSchema = z.object({
  success: z.boolean().optional(),
  error: z.string().optional(),
  workspaces: z.array(WorkspaceSchema),
});

export const WorkspaceResponseSchema = z.object({
  success: z.boolean().optional(),
  error: z.string().optional(),
  workspace: WorkspaceSchema,
});

// Workspace Member Schemas
export const WorkspaceMemberSchema = z
  .object({
    id: z.string().min(1),
    user_id: z.string().min(1),
    workspace_id: z.string().min(1),
    role: WorkspaceRoleSchema,
    is_active: z.boolean().optional(),
    invited_by: z.string().nullable().optional(),
    email: z.string().nullable().optional(),
    display_name: z.string().nullable().optional(),
    joined_at: z.string().optional(),
    created_at: z.string().optional(),
    updated_at: z.string().optional(),
    deleted_at: z.string().nullable().optional(),
  })
  .transform((member) => ({
    id: member.id,
    user_id: member.user_id,
    workspace_id: member.workspace_id,
    role: member.role,
    email: member.email || '',
    display_name: member.display_name ?? null,
    joined_at: member.joined_at || member.created_at || '',
  }));

export const AddWorkspaceMemberRequestSchema = z.object({
  user_id: z.string().min(1, 'User is required'),
  role: WorkspaceRoleSchema,
});

export const UpdateWorkspaceMemberRequestSchema = z.object({
  role: WorkspaceRoleSchema,
});

export const WorkspaceMembersResponseSchema = z.object({
  members: z.array(WorkspaceMemberSchema),
});

// Workspace Theme Schemas
export const FontFamilySchema = z.enum([
  'system',
  'inter',
  'roboto',
  'open-sans',
  'lato',
  'nunito',
]);

export const BorderRadiusSchema = z.enum(['none', 'small', 'medium', 'large']);

const hexColorRegex = /^#([A-Fa-f0-9]{6}|[A-Fa-f0-9]{3})$/;

export const WorkspaceThemeSchema = z.object({
  id: z.string(),
  workspace_id: z.string(),
  primary_color_light: z.string().regex(hexColorRegex, 'Invalid hex color'),
  secondary_color_light: z.string().regex(hexColorRegex, 'Invalid hex color'),
  primary_color_dark: z.string().regex(hexColorRegex, 'Invalid hex color'),
  secondary_color_dark: z.string().regex(hexColorRegex, 'Invalid hex color'),
  font_family: FontFamilySchema,
  font_size_base: z.string(),
  border_radius: BorderRadiusSchema,
  created_at: z.string(),
  updated_at: z.string(),
});

export const UpdateWorkspaceThemeRequestSchema = z.object({
  primary_color_light: z.string().regex(hexColorRegex, 'Invalid hex color').optional(),
  secondary_color_light: z.string().regex(hexColorRegex, 'Invalid hex color').optional(),
  primary_color_dark: z.string().regex(hexColorRegex, 'Invalid hex color').optional(),
  secondary_color_dark: z.string().regex(hexColorRegex, 'Invalid hex color').optional(),
  font_family: FontFamilySchema.optional(),
  font_size_base: z.string().optional(),
  border_radius: BorderRadiusSchema.optional(),
});

export const WorkspaceThemeResponseSchema = z.object({
  success: z.boolean().optional(),
  error: z.string().optional(),
  theme: WorkspaceThemeSchema,
});

// AI Settings Schemas
export const AiProviderSchema = z.enum(['self_hosted', 'openai', 'anthropic', 'bedrock']);

export const AiSettingsSchema = z.object({
  provider: AiProviderSchema,
  has_litellm_key: z.boolean(),
  litellm_host: z.string().nullable(),
  has_openai_api_key: z.boolean(),
  openai_base_url: z.string().nullable(),
  has_anthropic_api_key: z.boolean(),
  anthropic_base_url: z.string().nullable(),
  bedrock_region: z.string().nullable(),
  bedrock_use_iam_role: z.boolean(),
  has_bedrock_credentials: z.boolean(),
  model_fast: z.string().nullable(),
  model_reasoning: z.string().nullable(),
  model_embedding: z.string().nullable(),
});

export const UpdateAiSettingsRequestSchema = z.object({
  provider: AiProviderSchema.optional(),
  litellm_host: z.string().optional(),
  litellm_key: z.string().optional(),
  openai_api_key: z.string().optional(),
  openai_base_url: z.string().optional(),
  anthropic_api_key: z.string().optional(),
  anthropic_base_url: z.string().optional(),
  bedrock_region: z.string().optional(),
  bedrock_access_key: z.string().optional(),
  bedrock_secret_key: z.string().optional(),
  bedrock_use_iam_role: z.boolean().optional(),
  model_fast: z.string().optional(),
  model_reasoning: z.string().optional(),
  model_embedding: z.string().optional(),
});

export const AiSettingsResponseSchema = z.object({
  success: z.boolean().optional(),
  error: z.string().optional(),
  provider: AiProviderSchema,
  has_litellm_key: z.boolean(),
  litellm_host: z.string().nullable(),
  has_openai_api_key: z.boolean(),
  openai_base_url: z.string().nullable(),
  has_anthropic_api_key: z.boolean(),
  anthropic_base_url: z.string().nullable(),
  bedrock_region: z.string().nullable(),
  bedrock_use_iam_role: z.boolean(),
  has_bedrock_credentials: z.boolean(),
  model_fast: z.string().nullable(),
  model_reasoning: z.string().nullable(),
  model_embedding: z.string().nullable(),
});

// Type exports
export type WorkspaceZ = z.infer<typeof WorkspaceSchema>;
export type WorkspaceMemberZ = z.infer<typeof WorkspaceMemberSchema>;
export type WorkspaceMembersResponse = z.infer<typeof WorkspaceMembersResponseSchema>;
export type WorkspaceThemeZ = z.infer<typeof WorkspaceThemeSchema>;
export type WorkspaceThemeResponse = z.infer<typeof WorkspaceThemeResponseSchema>;
export type AiSettingsZ = z.infer<typeof AiSettingsSchema>;
export type AiSettingsResponse = z.infer<typeof AiSettingsResponseSchema>;
export type WorkspacesResponse = z.infer<typeof WorkspacesResponseSchema>;
export type WorkspaceResponse = z.infer<typeof WorkspaceResponseSchema>;
