export const policyQueryKeys = {
  all: ['agent-policy'] as const,
  organization: (organizationId: string) =>
    ['agent-policy', 'organization', organizationId] as const,
  workspaces: (organizationId: string) => ['agent-policy', 'workspace', organizationId] as const,
  workspace: (organizationId: string, workspaceId: string) =>
    ['agent-policy', 'workspace', organizationId, workspaceId] as const,
};
