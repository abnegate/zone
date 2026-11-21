export const rolesQueryKeys = {
  all: ['roles'] as const,
  organization: (organizationId: string) => ['roles', organizationId] as const,
};
