import { PolicyField } from './schemas';
import type { PolicyField as PolicyFieldValue, PolicyRules } from './types';

export const PolicyScope = {
  Organization: 'organization',
  Workspace: 'workspace',
} as const;

export type PolicyScope = (typeof PolicyScope)[keyof typeof PolicyScope];

export const PolicySource = {
  Organization: 'organization',
  Workspace: 'workspace',
  None: 'none',
} as const;

export type PolicySource = (typeof PolicySource)[keyof typeof PolicySource];

function restricts(rules: PolicyRules, field: PolicyFieldValue): boolean {
  return field === PolicyField.Ceiling ? rules.ceiling !== null : rules[field] !== null;
}

export function layerSource(workspace: boolean, organization: boolean): PolicySource {
  if (workspace) {
    return PolicySource.Workspace;
  }
  return organization ? PolicySource.Organization : PolicySource.None;
}

export function sourceOf(
  field: PolicyFieldValue,
  organization: PolicyRules,
  workspace: PolicyRules
): PolicySource {
  return layerSource(restricts(workspace, field), restricts(organization, field));
}
