import { OverviewSchema } from '../features/analytics/schemas';
import type { Overview, TimePeriod } from '../features/analytics/types';
import { parse } from '../validation';
import { ApiError } from './ApiError';
import { AuthorizedApi } from './AuthorizedApi';
import { API_BASE } from './client';

export const LOAD_FAILED = 'Failed to load analytics';

function analyticsUrl(workspaceId: string, period: TimePeriod): string {
  const query = new URLSearchParams({ period });
  return `${API_BASE}/api/workspaces/${encodeURIComponent(workspaceId)}/analytics?${query}`;
}

class AnalyticsApi extends AuthorizedApi {
  async overview(workspaceId: string, period: TimePeriod, signal?: AbortSignal): Promise<Overview> {
    const response = await fetch(analyticsUrl(workspaceId, period), {
      headers: this.headers(),
      signal,
    });
    if (!response.ok) {
      throw await ApiError.from(response, LOAD_FAILED);
    }
    return parse(OverviewSchema, await response.json());
  }
}

export const analyticsApi = new AnalyticsApi();
