export const githubQueryKeys = {
  organization: (organizationId: string | null) => ['github', organizationId] as const,
  connection: (organizationId: string | null) => ['github', organizationId, 'connection'] as const,
  repositories: (organizationId: string | null, query: string) =>
    ['github', organizationId, 'repositories', query] as const,
  branches: (
    organizationId: string | null,
    installationId: string,
    owner: string,
    repository: string
  ) => ['github', organizationId, 'branches', installationId, owner, repository] as const,
};
