import { PolicySource } from './source';

export const SAVE_LABEL = 'Save policy';

export const DISCARD_LABEL = 'Discard changes';

export const ORGANIZATION_SUMMARY = 'Organization';

export const EFFECTIVE_SUMMARY = 'In effect';

export const LAPSED_NOTICE =
  'Your Zone license has lapsed. Policies are still enforced; editing needs an active or grace license.';

export const UNVERIFIED_NOTICE =
  'The stored Zone license key no longer verifies. Policies are still enforced, and editing is blocked until a server admin replaces or removes the key.';

export const UPGRADE_NOTICE =
  'Agent policy is part of Zone Enterprise. Add a license to choose which providers, models and coding agents people can use, and when tools need approval.';

export const SOURCE_LABELS: Readonly<Record<PolicySource, string>> = {
  [PolicySource.Organization]: 'Inherited from organization',
  [PolicySource.Workspace]: 'Set for this workspace',
  [PolicySource.None]: 'Not restricted',
};
