import type { TimePeriod } from './types';

export const ANALYTICS_KEY = 'analytics';

type Workspace = string | undefined;

export const analyticsKeys = {
  all: [ANALYTICS_KEY] as const,
  workspace: (workspaceId: Workspace) => [ANALYTICS_KEY, workspaceId] as const,
  overview: (workspaceId: Workspace, period: TimePeriod) =>
    [ANALYTICS_KEY, workspaceId, period] as const,
};
