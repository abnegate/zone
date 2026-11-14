import { z } from 'zod';
import { FreshnessSchema } from './freshness';
import { WebUrlSchema } from './schemas';

export const SourceStatusSchema = z.object({
  source_id: z.string(),
  name: z.string(),
  repository: z.string(),
  repository_url: WebUrlSchema,
  freshness: FreshnessSchema.catch(FreshnessSchema.enum.stale),
  generated_at: z.string().nullable(),
  warning: z.string().nullable(),
});

export type SourceStatus = z.infer<typeof SourceStatusSchema>;
