import { z } from 'zod';

export const GitHubInstallationStatusSchema = z.enum(['active', 'revoked', 'suspended']);

export type GitHubInstallationStatus = z.infer<typeof GitHubInstallationStatusSchema>;
