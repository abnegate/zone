import type { z } from 'zod';
import type {
  CreateTokenRequestSchema,
  FailureResponseSchema,
  GroupResponseSchema,
  IssuedResponseSchema,
  MappingRequestSchema,
  ScimRefusalSchema,
  SettingsResponseSchema,
  TokenResponseSchema,
} from './schemas';

export type TokenResponse = z.infer<typeof TokenResponseSchema>;
export type GroupResponse = z.infer<typeof GroupResponseSchema>;
export type FailureResponse = z.infer<typeof FailureResponseSchema>;
export type ScimSettings = z.infer<typeof SettingsResponseSchema>;
export type IssuedResponse = z.infer<typeof IssuedResponseSchema>;
export type CreateTokenRequest = z.infer<typeof CreateTokenRequestSchema>;
export type MappingRequest = z.infer<typeof MappingRequestSchema>;
export type ScimRefusalBody = z.infer<typeof ScimRefusalSchema>;
