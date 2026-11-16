import type { z } from 'zod';
import type {
  AgentAnalyticsSchema,
  CategoryStatisticsSchema,
  CompletionTimesSchema,
  ErrorCategorySchema,
  EvaluationSummarySchema,
  ModelBreakdownSchema,
  OriginSchema,
  OutcomeStatisticsSchema,
  OverviewSchema,
  PullRequestSummarySchema,
  QualityBandSchema,
  SeriesPointSchema,
  SourceBreakdownSchema,
  TallySchema,
  TaskSummarySchema,
  TimePeriodSchema,
  TimeWindowSchema,
  TrendDirectionSchema,
  TrendSchema,
  TruncationSchema,
} from './schemas';

export type TimePeriod = z.infer<typeof TimePeriodSchema>;
export type TrendDirection = z.infer<typeof TrendDirectionSchema>;
export type ErrorCategory = z.infer<typeof ErrorCategorySchema>;
export type TimeWindow = z.infer<typeof TimeWindowSchema>;
export type CategoryStatistics = z.infer<typeof CategoryStatisticsSchema>;
export type OutcomeStatistics = z.infer<typeof OutcomeStatisticsSchema>;
export type CompletionTimes = z.infer<typeof CompletionTimesSchema>;
export type SeriesPoint = z.infer<typeof SeriesPointSchema>;
export type Trend = z.infer<typeof TrendSchema>;
export type AgentAnalytics = z.infer<typeof AgentAnalyticsSchema>;
export type Tally = z.infer<typeof TallySchema>;
export type SourceBreakdown = z.infer<typeof SourceBreakdownSchema>;
export type ModelBreakdown = z.infer<typeof ModelBreakdownSchema>;
export type TaskSummary = z.infer<typeof TaskSummarySchema>;
export type PullRequestSummary = z.infer<typeof PullRequestSummarySchema>;
export type EvaluationSummary = z.infer<typeof EvaluationSummarySchema>;
export type Overview = z.infer<typeof OverviewSchema>;
export type Origin = z.infer<typeof OriginSchema>;
export type QualityBand = z.infer<typeof QualityBandSchema>;
export type Truncation = z.infer<typeof TruncationSchema>;
