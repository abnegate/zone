import { z } from 'zod';
import { isWebUrl, NOT_WEB_URL_MESSAGE } from './utils/webUrl';

const WebUrlSchema = z.string().refine(isWebUrl, { error: NOT_WEB_URL_MESSAGE });

export const GitHubAppOwnerTypeSchema = z.enum(['User', 'Organization']);
export const GitHubAccountTypeSchema = z.enum(['User', 'Organization', 'Enterprise']);
export const GitHubRepositorySelectionSchema = z.enum(['all', 'selected']);
export const GitHubRepositoryInstallationErrorSchema = z.enum([
  'revoked',
  'suspended',
  'unavailable',
]);

export const GitHubAppSchema = z.object({
  id: z.string(),
  app_id: z.number().int(),
  slug: z.string(),
  name: z.string(),
  owner_login: z.string(),
  owner_type: GitHubAppOwnerTypeSchema,
  html_url: WebUrlSchema,
  settings_url: WebUrlSchema,
  created_at: z.string(),
});

export const GitHubInstallationSchema = z.object({
  id: z.string(),
  github_app_id: z.string(),
  installation_id: z.number().int(),
  account_login: z.string(),
  account_type: GitHubAccountTypeSchema,
  repository_selection: GitHubRepositorySelectionSchema,
  html_url: WebUrlSchema,
  suspended_at: z.string().nullable(),
  revoked_at: z.string().nullable(),
  created_at: z.string(),
});

export const GitHubConnectionSchema = z.object({
  apps: z.array(GitHubAppSchema),
  installations: z.array(GitHubInstallationSchema),
});

export const GitHubAppRegistrationSchema = z.object({
  action: WebUrlSchema,
  manifest: z.string(),
  state: z.string(),
});

export const GitHubInstallationUrlSchema = z.object({
  url: WebUrlSchema,
});

export const GitHubRepositorySchema = z.object({
  installation_id: z.string(),
  id: z.number().int(),
  owner: z.string(),
  name: z.string(),
  full_name: z.string(),
  private: z.boolean(),
  archived: z.boolean(),
  default_branch: z.string(),
  html_url: WebUrlSchema,
});

export const GitHubRepositoryInstallationSchema = z.object({
  id: z.string(),
  account_login: z.string(),
  html_url: WebUrlSchema,
  error: GitHubRepositoryInstallationErrorSchema.nullable(),
});

export const GitHubRepositoriesResponseSchema = z.object({
  repositories: z.array(GitHubRepositorySchema),
  installations: z.array(GitHubRepositoryInstallationSchema),
  total: z.number().int(),
  next_page: z.number().int().nullable(),
});

export const GitHubBranchSchema = z.object({
  name: z.string(),
  protected: z.boolean(),
});

export const GitHubBranchesResponseSchema = z.object({
  branches: z.array(GitHubBranchSchema),
  next_page: z.number().int().nullable(),
});
