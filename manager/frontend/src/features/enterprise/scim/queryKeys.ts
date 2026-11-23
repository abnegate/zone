export const scimQueryKeys = {
  all: ['scim'] as const,
  organization: (organizationId: string) => ['scim', organizationId] as const,
};
