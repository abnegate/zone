import type { QueryState } from '@tanstack/react-query';
import { FreshnessSchema } from './freshness';
import type { SourceStatus } from './sourceStatus';

export const REVALIDATION_REFETCH_MS = 2_000;

interface Listing {
  readonly sources: readonly SourceStatus[];
}

export function revalidating(sources: readonly SourceStatus[] | undefined): boolean {
  return sources?.some((source) => source.freshness === FreshnessSchema.enum.revalidating) ?? false;
}

export function pollAfter(state: QueryState<Listing, unknown>, interval: number): number {
  return state.status === 'success' && revalidating(state.data?.sources)
    ? REVALIDATION_REFETCH_MS
    : interval;
}
