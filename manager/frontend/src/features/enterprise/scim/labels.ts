import { MappedRole } from './schemas';

export const TITLE = 'SCIM provisioning';

export const DESCRIPTION =
  'Let your identity provider add, update and remove the members of this organization, and map its groups to roles.';

export const LOADING = 'Loading SCIM provisioning...';

export const RETRY_LABEL = 'Retry';

export const UPGRADE_NOTICE =
  'SCIM provisioning is part of Zone Enterprise. Add a license to let Okta, Microsoft Entra ID or another identity provider manage the members of this organization.';

export const LAPSED_NOTICE =
  'Your Zone license has lapsed. Your identity provider can still deactivate and remove people and you can still revoke tokens, but provisioning anyone, issuing or rotating tokens and mapping groups needs a renewed license.';

export const BASE_URL_TITLE = 'Base URL';

export const BASE_URL_NAME = 'base URL';

export const BASE_URL_HINT =
  'Enter this as the SCIM connector base URL or tenant URL in your identity provider, with a token below as the bearer token.';

export const TOKENS_TITLE = 'Tokens';

export const CREATE_LABEL = 'Create token';

export const ROTATE_LABEL = 'Rotate';

export const REVOKE_LABEL = 'Revoke';

export const CANCEL_LABEL = 'Cancel';

export const NO_TOKENS_EDITABLE =
  'No tokens yet. Create one and paste it into your identity provider to start provisioning.';

export const NO_TOKENS_READ_ONLY = 'No tokens.';

export const NEVER_USED = 'Never used';

export const CREATED_LABEL = 'Created';

export const LAST_USED_LABEL = 'Last used';

export const ROTATED_LABEL = 'Rotated';

export const HINT_PREFIX = 'zscim_…';

export const CREATE_TITLE = 'Create SCIM token';

export const NAME_LABEL = 'Name';

export const NAME_PLACEHOLDER = 'Okta';

export const NAME_HINT = 'Name it after the identity provider that will use it.';

export const NAME_REQUIRED = 'Enter a name for the token.';

export const ROTATE_CONFIRM = 'Rotate token';

export const REVOKE_CONFIRM = 'Revoke token';

export const SECRET_NAME = 'token secret';

export const DISMISS_LABEL = 'Dismiss';

export const SECRET_HINT =
  "Paste it into your identity provider as the bearer token now. Zone keeps only a hash, so it can't be shown again.";

export const GROUPS_TITLE = 'Group mappings';

export const GROUPS_HINT =
  'People in a mapped group get its role. Someone in several mapped groups gets the role that grants the most. Once any group is mapped, provisioned members in no mapped group are members. Owners are never changed.';

export const NO_GROUPS = 'Your identity provider has not pushed any groups yet.';

export const UNMAPPED_LABEL = 'Unmapped';

export const CUSTOM_ROLE_DESCRIPTION = 'Custom role';

export const SYNC_TITLE = 'Sync';

export const LAST_SYNC_LABEL = 'Last request';

export const NEVER_SYNCED = 'Your identity provider has not connected yet.';

export const ERRORS_TITLE = 'Recent errors';

export const NO_ERRORS = 'No refused requests.';

export const META_SEPARATOR = ' · ';

export const MAPPED_ROLE_LABELS: Readonly<Record<MappedRole, string>> = {
  [MappedRole.Admin]: 'Admin',
  [MappedRole.Member]: 'Member',
};

function counted(count: number, singular: string, plural: string): string {
  return `${count} ${count === 1 ? singular : plural}`;
}

export function memberCount(count: number): string {
  return counted(count, 'member', 'members');
}

export function hintLabel(hint: string): string {
  return `${HINT_PREFIX}${hint}`;
}

export function mappingLabel(displayName: string): string {
  return `Role for ${displayName}`;
}

export function secretTitle(name: string): string {
  return `${name} secret, shown once`;
}

export function rotateTitle(name: string): string {
  return `Rotate ${name}?`;
}

export function revokeTitle(name: string): string {
  return `Revoke ${name}?`;
}

export function rotateWarning(name: string): string {
  return `${name}'s current secret stops working at once, and your identity provider can't sync until you paste the new one. To switch without a gap, create a second token, move your provider to it, then revoke this one.`;
}

export function revokeWarning(name: string): string {
  return `Your identity provider can no longer provision or deprovision anyone with ${name}. This can't be undone.`;
}
