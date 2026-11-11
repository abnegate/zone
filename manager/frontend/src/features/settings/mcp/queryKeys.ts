export const mcpQueryKeys = {
  organization: (organizationId: string) => ['mcp', organizationId] as const,
  servers: (organizationId: string) => ['mcp', organizationId, 'servers'] as const,
  workspace: (organizationId: string, workspaceId: string) =>
    ['mcp', organizationId, 'workspaces', workspaceId] as const,
};
