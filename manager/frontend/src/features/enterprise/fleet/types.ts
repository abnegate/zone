import type { z } from 'zod';
import type {
  AvailableLabelSchema,
  FleetAdminCodeSchema,
  FleetPolicyResponseSchema,
  FleetRefusalSchema,
  FleetWorkCodeSchema,
  OrganizationRunnerLabelsResponseSchema,
  RunnerResponseSchema,
  RunnerStateSchema,
  RunnersResponseSchema,
  UpdateFleetPolicyRequestSchema,
  UpdateRunnerLabelsRequestSchema,
  UpdateRunnerRequestSchema,
  WorkspaceRunnerLabelsResponseSchema,
} from './schemas';

export type RunnerState = z.infer<typeof RunnerStateSchema>;
export type FleetWorkCode = z.infer<typeof FleetWorkCodeSchema>;
export type FleetAdminCode = z.infer<typeof FleetAdminCodeSchema>;
export type FleetPolicyResponse = z.infer<typeof FleetPolicyResponseSchema>;
export type RunnerResponse = z.infer<typeof RunnerResponseSchema>;
export type RunnersResponse = z.infer<typeof RunnersResponseSchema>;
export type AvailableLabel = z.infer<typeof AvailableLabelSchema>;
export type OrganizationRunnerLabelsResponse = z.infer<
  typeof OrganizationRunnerLabelsResponseSchema
>;
export type WorkspaceRunnerLabelsResponse = z.infer<typeof WorkspaceRunnerLabelsResponseSchema>;
export type UpdateFleetPolicyRequest = z.infer<typeof UpdateFleetPolicyRequestSchema>;
export type UpdateRunnerRequest = z.infer<typeof UpdateRunnerRequestSchema>;
export type UpdateRunnerLabelsRequest = z.infer<typeof UpdateRunnerLabelsRequestSchema>;
export type FleetRefusalBody = z.infer<typeof FleetRefusalSchema>;
