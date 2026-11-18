import type { z } from 'zod';
import type {
  CreateProviderRequestSchema,
  EnforcementRequestSchema,
  EnforcementResponseSchema,
  ProtocolSchema,
  ProviderResponseSchema,
  SettingsResponseSchema,
  SsoAdminCodeSchema,
  SsoRefusalSchema,
  UpdateProviderRequestSchema,
} from './schemas';

export type Protocol = z.infer<typeof ProtocolSchema>;
export type SsoAdminCode = z.infer<typeof SsoAdminCodeSchema>;
export type ProviderResponse = z.infer<typeof ProviderResponseSchema>;
export type SettingsResponse = z.infer<typeof SettingsResponseSchema>;
export type CreateProviderRequest = z.infer<typeof CreateProviderRequestSchema>;
export type UpdateProviderRequest = z.infer<typeof UpdateProviderRequestSchema>;
export type EnforcementRequest = z.infer<typeof EnforcementRequestSchema>;
export type EnforcementResponse = z.infer<typeof EnforcementResponseSchema>;
export type SsoRefusalBody = z.infer<typeof SsoRefusalSchema>;
