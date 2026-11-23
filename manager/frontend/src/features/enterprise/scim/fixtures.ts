import { AUDITOR } from '../roles/fixtures';
import { Edition, LicenseStatus } from '../schemas';
import { MappedRole } from './schemas';
import type {
  FailureResponse,
  GroupResponse,
  IssuedResponse,
  ScimSettings,
  TokenResponse,
} from './types';

export const ORGANIZATION_ID = '9a8b7c6d-5e4f-4a3b-8c2d-1e0f9a8b7c6d';

export const OWNER_ID = 'c0ffee00-0000-4000-8000-000000000001';

export const BASE_URL = `https://zone.example.com/api/scim/v2/organizations/${ORGANIZATION_ID}`;

export const CREATED_AT = '2026-11-15T10:30:00Z';

export const ROTATED_AT = '2026-11-20T09:15:00Z';

export const SYNCED_AT = '2026-11-21T16:45:00Z';

export const SECRET = `zscim_${'7f3a'.repeat(16)}`;

export const OKTA: TokenResponse = {
  id: 'd1e2f3a4-0000-4000-8000-000000000001',
  name: 'Okta',
  hint: '9c4e',
  created_by: OWNER_ID,
  created_at: CREATED_AT,
  rotated_at: ROTATED_AT,
  last_used_at: SYNCED_AT,
};

export const ENTRA: TokenResponse = {
  id: 'd1e2f3a4-0000-4000-8000-000000000002',
  name: 'Microsoft Entra ID',
  hint: '51b0',
  created_by: OWNER_ID,
  created_at: CREATED_AT,
  rotated_at: null,
  last_used_at: null,
};

export const ENGINEERING: GroupResponse = {
  id: 'e1f2a3b4-0000-4000-8000-000000000001',
  display_name: 'Engineering',
  external_id: '00g1a2b3c4',
  members: 42,
  role: MappedRole.Member,
  custom_role: null,
  updated_at: SYNCED_AT,
};

export const PLATFORM_ADMINS: GroupResponse = {
  id: 'e1f2a3b4-0000-4000-8000-000000000002',
  display_name: 'Platform admins',
  external_id: '00g5d6e7f8',
  members: 3,
  role: MappedRole.Admin,
  custom_role: null,
  updated_at: SYNCED_AT,
};

export const COMPLIANCE: GroupResponse = {
  id: 'e1f2a3b4-0000-4000-8000-000000000003',
  display_name: 'Compliance',
  external_id: null,
  members: 1,
  role: null,
  custom_role: { id: AUDITOR.id, name: AUDITOR.name },
  updated_at: SYNCED_AT,
};

export const CONTRACTORS: GroupResponse = {
  id: 'e1f2a3b4-0000-4000-8000-000000000004',
  display_name: 'Contractors',
  external_id: null,
  members: 0,
  role: null,
  custom_role: null,
  updated_at: CREATED_AT,
};

export const CONFLICT: FailureResponse = {
  id: 'f1a2b3c4-0000-4000-8000-000000000001',
  method: 'POST',
  path: '/api/scim/v2/organizations/{org_id}/Users',
  status: 409,
  scim_type: 'uniqueness',
  detail: 'A user with this userName already exists',
  occurred_at: SYNCED_AT,
};

export const LAST_OWNER: FailureResponse = {
  id: 'f1a2b3c4-0000-4000-8000-000000000002',
  method: 'PATCH',
  path: '/api/scim/v2/organizations/{org_id}/Users/{user_id}',
  status: 409,
  scim_type: null,
  detail: "The last active owner of the organization can't be deactivated",
  occurred_at: CREATED_AT,
};

export const ACTIVE: ScimSettings = {
  entitled: true,
  edition: Edition.Enterprise,
  status: LicenseStatus.Active,
  base_url: BASE_URL,
  last_synced_at: SYNCED_AT,
  tokens: [OKTA, ENTRA],
  groups: [ENGINEERING, PLATFORM_ADMINS, COMPLIANCE, CONTRACTORS],
  errors: [CONFLICT, LAST_OWNER],
};

export const ACTIVE_EMPTY: ScimSettings = {
  ...ACTIVE,
  last_synced_at: null,
  tokens: [],
  groups: [],
  errors: [],
};

export const GRACE: ScimSettings = { ...ACTIVE, status: LicenseStatus.Grace };

export const LAPSED: ScimSettings = {
  ...ACTIVE,
  entitled: false,
  status: LicenseStatus.Lapsed,
};

export const COMMUNITY: ScimSettings = {
  ...ACTIVE_EMPTY,
  entitled: false,
  edition: Edition.Community,
  status: LicenseStatus.Community,
};

export const TEAM_WITH_TOKENS: ScimSettings = {
  ...ACTIVE,
  entitled: false,
  edition: Edition.Team,
  status: LicenseStatus.Active,
};

export const ISSUED: IssuedResponse = {
  token: {
    id: 'd1e2f3a4-0000-4000-8000-000000000003',
    name: 'Google Workspace',
    hint: SECRET.slice(-4),
    created_by: OWNER_ID,
    created_at: SYNCED_AT,
    rotated_at: null,
    last_used_at: null,
  },
  secret: SECRET,
};

export const ROTATED: IssuedResponse = {
  token: { ...OKTA, hint: SECRET.slice(-4), rotated_at: SYNCED_AT },
  secret: SECRET,
};
