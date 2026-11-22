import type { Edition, LicenseStatus } from '../types';
import type {
  AvailableLabel,
  OrganizationRunnerLabelsResponse,
  WorkspaceRunnerLabelsResponse,
} from './types';

export const PlacementKind = {
  Organization: 'organization',
  Workspace: 'workspace',
} as const;

export type PlacementKind = (typeof PlacementKind)[keyof typeof PlacementKind];

export type PlacementScope =
  | { kind: typeof PlacementKind.Organization; organizationId: string }
  | { kind: typeof PlacementKind.Workspace; organizationId: string; workspaceId: string };

export interface Placement {
  entitled: boolean;
  enforced: boolean;
  edition: Edition;
  status: LicenseStatus;
  editable: boolean;
  inherited: string[];
  own: string[];
  effective: string[];
  available: AvailableLabel[];
  matching: number;
  updated_at: string | null;
}

export function organizationPlacement(organizationId: string): PlacementScope {
  return { kind: PlacementKind.Organization, organizationId };
}

export function workspacePlacement(organizationId: string, workspaceId: string): PlacementScope {
  return { kind: PlacementKind.Workspace, organizationId, workspaceId };
}

export function fromOrganization(response: OrganizationRunnerLabelsResponse): Placement {
  const { labels, ...rest } = response;
  return { ...rest, inherited: [], own: labels, effective: labels };
}

export function fromWorkspace(response: WorkspaceRunnerLabelsResponse): Placement {
  const { organization, workspace, ...rest } = response;
  return { ...rest, inherited: organization, own: workspace };
}

const SEPARATOR = /[\s,]+/;

const LABEL = /^[a-z0-9][a-z0-9._\-=/]{0,62}$/;

export function parseLabels(text: string): string[] {
  return [
    ...new Set(
      text
        .split(SEPARATOR)
        .map((entry) => entry.trim())
        .filter((entry) => entry.length > 0)
    ),
  ];
}

export function validLabel(label: string): boolean {
  return LABEL.test(label);
}

export function withLabels(list: readonly string[], added: readonly string[]): string[] {
  return [...new Set([...list, ...added])].sort();
}

export function withoutLabel(list: readonly string[], label: string): string[] {
  return list.filter((entry) => entry !== label);
}

export function sameLabels(left: readonly string[], right: readonly string[]): boolean {
  const sorted = (list: readonly string[]) => [...new Set(list)].sort().join('\n');
  return sorted(left) === sorted(right);
}
