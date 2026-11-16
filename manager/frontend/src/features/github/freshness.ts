import { z } from 'zod';

export const FreshnessSchema = z.enum(['cached', 'revalidating', 'refreshed', 'floored', 'stale']);

export type Freshness = z.infer<typeof FreshnessSchema>;
