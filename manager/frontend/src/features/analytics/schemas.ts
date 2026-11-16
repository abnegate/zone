import { z } from 'zod';

export const TimePeriodSchema = z.enum(['day', 'week', 'month', 'quarter']);

export const TrendDirectionSchema = z.enum(['improving', 'steady', 'declining']);

export const ErrorCategorySchema = z.enum([
  'build',
  'test_failure',
  'dependency',
  'permission',
  'timeout',
  'rate_limit',
  'network',
  'merge_conflict',
  'configuration',
  'refusal',
  'unknown',
]);

const KnownErrorCategorySchema = ErrorCategorySchema.catch(ErrorCategorySchema.enum.unknown);

const KnownTrendDirectionSchema = TrendDirectionSchema.catch(TrendDirectionSchema.enum.steady);

const CountSchema = z.number().int().nonnegative();

const ShareSchema = z.number();

const QuantitySchema = z.number().nonnegative();

export const TimeWindowSchema = z.object({
  start: z.string(),
  end: z.string(),
});

export const CategoryStatisticsSchema = z.object({
  category: KnownErrorCategorySchema,
  occurrences: CountSchema,
  recent_weight: QuantitySchema,
  distinct_tasks: CountSchema,
  last_seen: z.string(),
});

export const OutcomeStatisticsSchema = z.object({
  total: CountSchema,
  succeeded: CountSchema,
  failed: CountSchema,
  cancelled: CountSchema,
  success_rate: ShareSchema,
  recent_success_rate: ShareSchema,
  categories: z.array(CategoryStatisticsSchema),
});

export const CompletionTimesSchema = z.object({
  measured: CountSchema,
  median_seconds: QuantitySchema,
  ninetieth_seconds: QuantitySchema,
  mean_seconds: QuantitySchema,
});

export const SeriesPointSchema = z.object({
  window: TimeWindowSchema,
  total: CountSchema,
  succeeded: CountSchema,
  failed: CountSchema,
  cancelled: CountSchema,
});

export const TrendSchema = z.object({
  direction: KnownTrendDirectionSchema,
  earlier_success_rate: ShareSchema.nullable(),
  later_success_rate: ShareSchema.nullable(),
  change: z.number(),
});

export const AgentAnalyticsSchema = z.object({
  period: TimePeriodSchema,
  window: TimeWindowSchema,
  outcomes: OutcomeStatisticsSchema,
  completion: CompletionTimesSchema,
  series: z.array(SeriesPointSchema),
  trend: TrendSchema,
});

export const TallySchema = z.object({
  name: z.string(),
  count: CountSchema,
});

export const SourceBreakdownSchema = z.object({
  source_id: z.string().nullable(),
  name: z.string().nullable(),
  source_type: z.string().nullable(),
  outcomes: OutcomeStatisticsSchema,
  merged_pull_requests: CountSchema,
});

export const ModelBreakdownSchema = z.object({
  model: z.string().nullable(),
  outcomes: OutcomeStatisticsSchema,
  completion: CompletionTimesSchema,
});

export const TaskSummarySchema = z.object({
  statuses: z.array(TallySchema),
  origins: z.array(TallySchema),
  created: CountSchema,
  shipped: CountSchema,
  reopened: CountSchema,
});

export const PullRequestSummarySchema = z.object({
  opened: CountSchema,
  merged: CountSchema,
  closed: CountSchema,
  open: CountSchema,
  merge_rate: ShareSchema.nullable(),
  median_minutes_to_merge: z.number().nullable(),
  mean_review_cycles: z.number().nullable(),
  mean_approvals: z.number().nullable(),
  quality: z.array(TallySchema),
});

export const EvaluationSummarySchema = z.object({
  improved: CountSchema,
  unchanged: CountSchema,
  regressed: CountSchema,
  not_measured: CountSchema,
});

export const OverviewSchema = z.object({
  period: TimePeriodSchema,
  window: TimeWindowSchema,
  truncated: z.boolean(),
  runs: AgentAnalyticsSchema,
  sources: z.array(SourceBreakdownSchema),
  models: z.array(ModelBreakdownSchema),
  tasks: TaskSummarySchema,
  pull_requests: PullRequestSummarySchema,
  evaluations: EvaluationSummarySchema,
});
