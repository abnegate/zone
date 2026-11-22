import { z } from 'zod';
import { EditionSchema, LicenseStatusSchema } from '../schemas';

export const RunnerState = {
  Active: 'active',
  Cordoned: 'cordoned',
  Revoked: 'revoked',
} as const;

export const FleetCode = {
  RunnerUnregistered: 'fleet_runner_unregistered',
  RunnerCordoned: 'fleet_runner_cordoned',
  RunnerRevoked: 'fleet_runner_revoked',
  RunnerOutdated: 'fleet_runner_outdated',
  LabelsMissing: 'fleet_labels_missing',
  NoRunner: 'fleet_no_runner',
  LicenseRequired: 'license_required',
  BodyInvalid: 'fleet_body_invalid',
  VersionInvalid: 'fleet_version_invalid',
  LabelInvalid: 'fleet_label_invalid',
  StateInvalid: 'fleet_state_invalid',
  RunnerOnline: 'fleet_runner_online',
  RunnerBusy: 'fleet_runner_busy',
  NoRunnerLeft: 'fleet_no_runner_left',
} as const;

export const RUNNER_STATES = [
  RunnerState.Active,
  RunnerState.Cordoned,
  RunnerState.Revoked,
] as const;

export const FLEET_WORK_CODES = [
  FleetCode.RunnerUnregistered,
  FleetCode.RunnerCordoned,
  FleetCode.RunnerRevoked,
  FleetCode.RunnerOutdated,
  FleetCode.LabelsMissing,
  FleetCode.NoRunner,
] as const;

export const FLEET_CHANGE_CODES = [
  FleetCode.BodyInvalid,
  FleetCode.VersionInvalid,
  FleetCode.LabelInvalid,
  FleetCode.StateInvalid,
  FleetCode.RunnerOnline,
  FleetCode.RunnerBusy,
  FleetCode.NoRunnerLeft,
] as const;

export const FLEET_ADMIN_CODES = [FleetCode.LicenseRequired, ...FLEET_CHANGE_CODES] as const;

export const FLEET_CODES = [...FLEET_WORK_CODES, ...FLEET_ADMIN_CODES] as const;

export const RunnerStateSchema = z.enum(RUNNER_STATES);

export const FleetWorkCodeSchema = z.enum(FLEET_WORK_CODES);

export const FleetAdminCodeSchema = z.enum(FLEET_ADMIN_CODES);

const LicenseFields = {
  entitled: z.boolean(),
  enforced: z.boolean(),
  edition: EditionSchema,
  status: LicenseStatusSchema,
  editable: z.boolean(),
};

export const FleetPolicyResponseSchema = z.object({
  ...LicenseFields,
  minimum_version: z.string().nullable(),
  updated_at: z.string().nullable(),
});

export const RunnerResponseSchema = z.object({
  id: z.string(),
  name: z.string(),
  version: z.string(),
  labels: z.array(z.string()),
  state: RunnerStateSchema,
  connected: z.boolean(),
  outdated: z.boolean(),
  serving: z.boolean(),
  code: FleetWorkCodeSchema.nullable(),
  current: z.boolean(),
  runs: z.number().int().nonnegative(),
  started_at: z.string(),
  last_seen_at: z.string(),
  state_changed_at: z.string().nullable(),
});

export const RunnersResponseSchema = z.object({
  editable: z.boolean(),
  enforced: z.boolean(),
  current: z.string().nullable(),
  runners: z.array(RunnerResponseSchema),
});

export const AvailableLabelSchema = z.object({
  label: z.string(),
  runners: z.number().int().nonnegative(),
});

export const OrganizationRunnerLabelsResponseSchema = z.object({
  ...LicenseFields,
  labels: z.array(z.string()),
  available: z.array(AvailableLabelSchema),
  matching: z.number().int().nonnegative(),
  updated_at: z.string().nullable(),
});

export const WorkspaceRunnerLabelsResponseSchema = z.object({
  ...LicenseFields,
  organization: z.array(z.string()),
  workspace: z.array(z.string()),
  effective: z.array(z.string()),
  available: z.array(AvailableLabelSchema),
  matching: z.number().int().nonnegative(),
  updated_at: z.string().nullable(),
});

export const UpdateFleetPolicyRequestSchema = z.strictObject({
  minimum_version: z.string().nullable(),
  confirm: z.boolean().optional(),
});

export const UpdateRunnerRequestSchema = z.strictObject({
  state: RunnerStateSchema,
  confirm: z.boolean().optional(),
});

export const UpdateRunnerLabelsRequestSchema = z.strictObject({
  labels: z.array(z.string()),
});

export const FleetRefusalSchema = z.object({
  error: z.string(),
  code: FleetAdminCodeSchema,
});

export const NoRunnerLeftSchema = z.object({
  error: z.string(),
  code: z.literal(FleetCode.NoRunnerLeft),
  remaining: z.number().int(),
});
