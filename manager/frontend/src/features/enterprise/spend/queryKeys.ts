import type { SpendQuery } from './schemas';

export const spendQueryKeys = {
  reports: (organizationId: string) => ['spend', 'reports', organizationId] as const,
  report: (organizationId: string, query: SpendQuery) =>
    ['spend', 'reports', organizationId, query] as const,
  budgets: (organizationId: string) => ['spend', 'budgets', organizationId] as const,
  workspaces: (organizationId: string) => ['spend', 'workspaces', organizationId] as const,
  members: (organizationId: string) => ['spend', 'members', organizationId] as const,
};
