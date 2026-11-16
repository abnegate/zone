import { FreshnessSchema } from './freshness';
import type { SourceStatus } from './sourceStatus';

export const REVALIDATION_REFETCH_MS = 2_000;

export function revalidating(sources: readonly SourceStatus[] | undefined): boolean {
  return sources?.some((source) => source.freshness === FreshnessSchema.enum.revalidating) ?? false;
}

export function pollAfter(sources: readonly SourceStatus[] | undefined, interval: number): number {
  return revalidating(sources) ? REVALIDATION_REFETCH_MS : interval;
}
