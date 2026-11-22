import { z } from 'zod';
import { LicenseStatusSchema } from '../schemas';

export const SpendCode = {
  BodyInvalid: 'spend_body_invalid',
  RangeInvalid: 'spend_range_invalid',
  LimitInvalid: 'budget_limit_invalid',
  ThresholdInvalid: 'budget_threshold_invalid',
  ScopeInvalid: 'budget_scope_invalid',
  BudgetExists: 'budget_exists',
  LicenseRequired: 'license_required',
  BudgetExhausted: 'budget_exhausted',
} as const;

export const BudgetScope = {
  Organization: 'organization',
  Workspace: 'workspace',
  User: 'user',
} as const;

export const BudgetState = {
  Under: 'under',
  Warning: 'warning',
  Exhausted: 'exhausted',
} as const;

export const UncountedReason = {
  CodingAgent: 'coding_agent',
  DirectProvider: 'direct_provider',
  ExternalEndpoint: 'external_endpoint',
  SavedKey: 'saved_key',
} as const;

export const SPEND_REFUSAL_CODES = [
  SpendCode.BodyInvalid,
  SpendCode.RangeInvalid,
  SpendCode.LimitInvalid,
  SpendCode.ThresholdInvalid,
  SpendCode.ScopeInvalid,
  SpendCode.BudgetExists,
  SpendCode.LicenseRequired,
] as const;

export const SPEND_CODES = [...SPEND_REFUSAL_CODES, SpendCode.BudgetExhausted] as const;

export const BUDGET_SCOPES = [
  BudgetScope.Organization,
  BudgetScope.Workspace,
  BudgetScope.User,
] as const;

export const BUDGET_STATES = [
  BudgetState.Under,
  BudgetState.Warning,
  BudgetState.Exhausted,
] as const;

export const UNCOUNTED_REASONS = [
  UncountedReason.CodingAgent,
  UncountedReason.DirectProvider,
  UncountedReason.ExternalEndpoint,
  UncountedReason.SavedKey,
] as const;

export const THRESHOLD_MINIMUM = 1;

export const THRESHOLD_MAXIMUM = 100;

export const THRESHOLD_DEFAULT = 80;

const DATE = /^\d{4}-\d{2}-\d{2}$/;

const MONTH = /^\d{4}-\d{2}$/;

export const SpendCodeSchema = z.enum(SPEND_CODES);

export const SpendRefusalCodeSchema = z.enum(SPEND_REFUSAL_CODES);

export const BudgetScopeSchema = z.enum(BUDGET_SCOPES);

export const BudgetStateSchema = z.enum(BUDGET_STATES);

export const UncountedReasonSchema = z.enum(UNCOUNTED_REASONS);

export const DateSchema = z.string().regex(DATE);

const Count = z.number().int().nonnegative();

const Threshold = z.number().int().min(THRESHOLD_MINIMUM).max(THRESHOLD_MAXIMUM);

const Limit = z.number().int().positive();

const LicenseFields = {
  entitled: z.boolean(),
  enforced: z.boolean(),
  edition: z.string(),
  status: LicenseStatusSchema,
  editable: z.boolean(),
};

const UsageFields = {
  spend_microdollars: Count,
  requests: Count,
  prompt_tokens: Count,
  completion_tokens: Count,
};

export const UsageSchema = z.object(UsageFields);

export const DayUsageSchema = z.object({ date: DateSchema, ...UsageFields });

export const WorkspaceUsageSchema = z.object({
  id: z.string().nullable(),
  name: z.string().nullable(),
  ...UsageFields,
});

export const UserUsageSchema = z.object({
  id: z.string().nullable(),
  name: z.string().nullable(),
  email: z.string().nullable(),
  automated: z.boolean(),
  ...UsageFields,
});

export const NamedUsageSchema = z.object({ name: z.string(), ...UsageFields });

export const SpendReportSchema = z.object({
  from: DateSchema,
  to: DateSchema,
  ingested_through: z.string().nullable(),
  total: UsageSchema,
  daily: z.array(DayUsageSchema),
  workspaces: z.array(WorkspaceUsageSchema),
  users: z.array(UserUsageSchema),
  models: z.array(NamedUsageSchema),
  providers: z.array(NamedUsageSchema),
  uncounted: z.array(UncountedReasonSchema),
});

export const SpendReportResponseSchema = z.object({
  ...LicenseFields,
  report: SpendReportSchema.nullable(),
});

export const SpendQuerySchema = z.strictObject({
  from: DateSchema,
  to: DateSchema,
  workspace_id: z.string().min(1).optional(),
  user_id: z.string().min(1).optional(),
});

export const BudgetSchema = z.object({
  id: z.string(),
  scope: BudgetScopeSchema,
  workspace_id: z.string().nullable(),
  workspace_name: z.string().nullable(),
  user_id: z.string().nullable(),
  user_name: z.string().nullable(),
  user_email: z.string().nullable(),
  limit_microdollars: Limit,
  threshold_percent: Threshold,
  hard: z.boolean(),
  spent_microdollars: Count,
  state: BudgetStateSchema,
  updated_at: z.string(),
});

export const BudgetsResponseSchema = z.object({
  ...LicenseFields,
  month: z.string().regex(MONTH),
  budgets: z.array(BudgetSchema),
});

export const CreateBudgetRequestSchema = z.strictObject({
  scope: BudgetScopeSchema,
  workspace_id: z.string().min(1).optional(),
  user_id: z.string().min(1).optional(),
  limit_microdollars: Limit,
  threshold_percent: Threshold.optional(),
  hard: z.boolean().optional(),
});

export const UpdateBudgetRequestSchema = z.strictObject({
  limit_microdollars: Limit.optional(),
  threshold_percent: Threshold.optional(),
  hard: z.boolean().optional(),
});

export const SpendRefusalSchema = z.object({
  error: z.string(),
  code: SpendRefusalCodeSchema,
});

export type SpendCode = z.infer<typeof SpendCodeSchema>;
export type SpendRefusalCode = z.infer<typeof SpendRefusalCodeSchema>;
export type BudgetScope = z.infer<typeof BudgetScopeSchema>;
export type BudgetState = z.infer<typeof BudgetStateSchema>;
export type UncountedReason = z.infer<typeof UncountedReasonSchema>;
export type Usage = z.infer<typeof UsageSchema>;
export type DayUsage = z.infer<typeof DayUsageSchema>;
export type WorkspaceUsage = z.infer<typeof WorkspaceUsageSchema>;
export type UserUsage = z.infer<typeof UserUsageSchema>;
export type NamedUsage = z.infer<typeof NamedUsageSchema>;
export type SpendReport = z.infer<typeof SpendReportSchema>;
export type SpendReportResponse = z.infer<typeof SpendReportResponseSchema>;
export type SpendQuery = z.infer<typeof SpendQuerySchema>;
export type Budget = z.infer<typeof BudgetSchema>;
export type BudgetsResponse = z.infer<typeof BudgetsResponseSchema>;
export type CreateBudgetRequest = z.infer<typeof CreateBudgetRequestSchema>;
export type UpdateBudgetRequest = z.infer<typeof UpdateBudgetRequestSchema>;
export type SpendRefusalBody = z.infer<typeof SpendRefusalSchema>;
export type SpendLicense = Pick<
  SpendReportResponse,
  'entitled' | 'enforced' | 'edition' | 'status' | 'editable'
>;
