import { OverviewSchema } from '../features/analytics/schemas';
import type { Overview, TimePeriod } from '../features/analytics/types';
import { parse } from '../validation';
import { ApiError } from './ApiError';
import { API_BASE } from './client';
import { deviceHeaders } from './device';
import { session } from './session';

export const LOAD_FAILED = 'Failed to load analytics';

function analyticsUrl(workspaceId: string, period: TimePeriod): string {
  const query = new URLSearchParams({ period });
  return `${API_BASE}/api/workspaces/${encodeURIComponent(workspaceId)}/analytics?${query}`;
}

class AnalyticsApi {
  private getAccessToken: () => string | null = () => session.getAccessToken();

  setGetAccessToken(provider: () => string | null): void {
    this.getAccessToken = provider;
  }

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

  private headers(): HeadersInit {
    const headers: HeadersInit = {
      'Content-Type': 'application/json',
      ...deviceHeaders(),
    };
    const token = this.getAccessToken();
    if (token) {
      headers.Authorization = `Bearer ${token}`;
    }
    return headers;
  }
}

export const analyticsApi = new AnalyticsApi();
