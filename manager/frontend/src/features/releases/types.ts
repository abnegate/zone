import type { z } from 'zod';
import type {
  CreatedSchema,
  CreateReleaseRequestSchema,
  IncludedPullSchema,
  LookupSchema,
  OptionsResponseSchema,
  PipelineBlockerCodeSchema,
  PipelineBlockerSchema,
  PipelineResponseSchema,
  PipelineRunSchema,
  PipelineStateSchema,
  PreviewRequestSchema,
  PreviewSchema,
  RECONCILIATION_CODE,
  RefusalSchema,
  ReleaseIdentitySchema,
  ReleasePipelineSchema,
  ReleaseSchema,
  ReleasesResponseSchema,
  SourceOptionsSchema,
  TagLineSchema,
  TagRefusalCode,
} from './schemas';

export type PipelineState = z.infer<typeof PipelineStateSchema>;
export type Lookup = z.infer<typeof LookupSchema>;
export type PipelineBlockerCode = z.infer<typeof PipelineBlockerCodeSchema>;
export type PipelineBlocker = z.infer<typeof PipelineBlockerSchema>;
export type ReleaseIdentity = z.infer<typeof ReleaseIdentitySchema>;
export type PipelineRun = z.infer<typeof PipelineRunSchema>;
export type ReleasePipeline = z.infer<typeof ReleasePipelineSchema>;
export type IncludedPull = z.infer<typeof IncludedPullSchema>;
export type Release = z.infer<typeof ReleaseSchema>;
export type ReleasesResponse = z.infer<typeof ReleasesResponseSchema>;
export type TagLine = z.infer<typeof TagLineSchema>;
export type SourceOptions = z.infer<typeof SourceOptionsSchema>;
export type OptionsResponse = z.infer<typeof OptionsResponseSchema>;
export type PipelineResponse = z.infer<typeof PipelineResponseSchema>;
export type Preview = z.infer<typeof PreviewSchema>;
export type PreviewRequest = z.input<typeof PreviewRequestSchema>;
export type CreateReleaseRequest = z.input<typeof CreateReleaseRequestSchema>;
export type Created = z.infer<typeof CreatedSchema>;
export type Refusal = z.infer<typeof RefusalSchema>;
export type RefusalCode = Refusal['code'];
export type Reconciliation = Extract<Refusal, { code: typeof RECONCILIATION_CODE }>;
export type TagRefusal = Extract<
  Refusal,
  { code: (typeof TagRefusalCode)[keyof typeof TagRefusalCode] }
>;

export interface ReleasesQuery {
  days?: number;
  refresh?: boolean;
}

export interface RefreshQuery {
  refresh?: boolean;
}
