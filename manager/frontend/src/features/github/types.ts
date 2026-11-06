import type { z } from 'zod';
import type {
  GitHubAccountTypeSchema,
  GitHubAppOwnerTypeSchema,
  GitHubAppRegistrationSchema,
  GitHubAppSchema,
  GitHubBranchesResponseSchema,
  GitHubBranchSchema,
  GitHubConnectionSchema,
  GitHubInstallationSchema,
  GitHubRepositoriesResponseSchema,
  GitHubRepositoryInstallationErrorSchema,
  GitHubRepositoryInstallationSchema,
  GitHubRepositorySchema,
} from './schemas';

export type GitHubAppOwnerType = z.infer<typeof GitHubAppOwnerTypeSchema>;
export type GitHubAccountType = z.infer<typeof GitHubAccountTypeSchema>;
export type GitHubApp = z.infer<typeof GitHubAppSchema>;
export type GitHubInstallation = z.infer<typeof GitHubInstallationSchema>;
export type GitHubConnection = z.infer<typeof GitHubConnectionSchema>;
export type GitHubAppRegistration = z.infer<typeof GitHubAppRegistrationSchema>;
export type GitHubRepository = z.infer<typeof GitHubRepositorySchema>;
export type GitHubRepositoryInstallationError = z.infer<
  typeof GitHubRepositoryInstallationErrorSchema
>;
export type GitHubRepositoryInstallation = z.infer<typeof GitHubRepositoryInstallationSchema>;
export type GitHubRepositoriesResponse = z.infer<typeof GitHubRepositoriesResponseSchema>;
export type GitHubBranch = z.infer<typeof GitHubBranchSchema>;
export type GitHubBranchesResponse = z.infer<typeof GitHubBranchesResponseSchema>;
