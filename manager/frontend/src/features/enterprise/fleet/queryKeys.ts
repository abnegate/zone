import { PlacementKind, type PlacementScope } from './placement';

const LABELS = ['fleet', 'labels'] as const;

function organization(organizationId: string) {
  return [...LABELS, PlacementKind.Organization, organizationId] as const;
}

function workspaces(organizationId: string) {
  return [...LABELS, PlacementKind.Workspace, organizationId] as const;
}

export const fleetQueryKeys = {
  all: ['fleet'] as const,
  policy: ['fleet', 'policy'] as const,
  runners: ['fleet', 'runners'] as const,
  labels: LABELS,
  organization,
  workspaces,
  placement: (scope: PlacementScope): readonly string[] =>
    scope.kind === PlacementKind.Organization
      ? organization(scope.organizationId)
      : [...workspaces(scope.organizationId), scope.workspaceId],
};
