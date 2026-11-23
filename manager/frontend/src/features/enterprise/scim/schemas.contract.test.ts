import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import type { z } from 'zod';

import { rustShape, zodShape } from '../../../test/contract';
import { structFields } from '../../../test/rust';
import { AUDIT_ACTIONS, AUDIT_RESOURCE_TYPES } from '../../settings/organization/schemas';
import { CustomRoleReferenceSchema } from '../roles/schemas';
import { BASE_URL } from './fixtures';
import {
  CreateTokenRequestSchema,
  FailureResponseSchema,
  GroupResponseSchema,
  IssuedResponseSchema,
  MAPPED_ROLES,
  MappingRequestSchema,
  SCIM_ROUTE_CODES,
  ScimCode,
  SettingsResponseSchema,
  TokenResponseSchema,
} from './schemas';

const RUNNER = join(import.meta.dir, '../../../../../../runner');
const SERVER = join(RUNNER, 'zone_server/src');
const ENTERPRISE = join(RUNNER, 'zone_enterprise/src');
const CONSOLE = join(SERVER, 'routes/scim/console');
const ROUTES_RS = join(SERVER, 'routes/mod.rs');
const SCIM_RS = join(SERVER, 'routes/scim/mod.rs');
const REFUSAL_RS = join(CONSOLE, 'refusal.rs');
const AUDIT_RS = join(SERVER, 'db/audit.rs');
const CUSTOM_ROLES_RS = join(SERVER, 'db/custom_roles.rs');
const ORGANIZATION_MEMBERS_RS = join(SERVER, 'db/organization_members.rs');
const ENTITLEMENT_RS = join(ENTERPRISE, 'entitlement.rs');

const CODE = /pub const ([A-Z_]+): &str = "([^"]+)";/g;
const AUDITED = /^(?:SCIM_[A-Z_]+|WORKSPACE_OWNER_SEATED)$/;
const OWNER = 'owner';
const ROUTE = '.route(';
const SUMMARY = 'Summary';
const ROLE_NAME = /"([a-z]+)" => Ok\(Self::[A-Za-z]+\)/g;

type Shaped = z.ZodObject<z.ZodRawShape>;

const read = (path: string): string => readFileSync(path, 'utf8');

const sorted = (values: readonly string[]): string[] => [...values].sort();

function codes(path: string): Map<string, string> {
  return new Map([...read(path).matchAll(CODE)].map((match) => [match[1], match[2]]));
}

function code(path: string, name: string): string {
  const value = codes(path).get(name);
  if (value === undefined) throw new Error(`${name} is not a code constant in ${path}`);
  return value;
}

function route(path: string): string {
  const source = read(ROUTES_RS);
  const start = source.indexOf(`"${path}"`);
  if (start < 0) throw new Error(`${path} is not routed`);
  const end = source.indexOf(ROUTE, start);
  return source.slice(start, end < 0 ? undefined : end);
}

const ROUTES: [string, string, string][] = [
  ['/api/organizations/{org_id}/scim', 'get', 'settings::get'],
  ['/api/organizations/{org_id}/scim/tokens', 'post', 'tokens::create'],
  ['/api/organizations/{org_id}/scim/tokens/{token_id}', 'delete', 'tokens::revoke'],
  ['/api/organizations/{org_id}/scim/tokens/{token_id}/rotations', 'post', 'tokens::rotate'],
  ['/api/organizations/{org_id}/scim/groups/{group_id}/mapping', 'put', 'mappings::put'],
  ['/api/organizations/{org_id}/scim/groups/{group_id}/mapping', 'delete', 'mappings::delete'],
];

const SHAPES: [string, string, Shaped][] = [
  ['settings.rs', 'SettingsResponse', SettingsResponseSchema],
  ['token_response.rs', 'TokenResponse', TokenResponseSchema],
  ['issued_response.rs', 'IssuedResponse', IssuedResponseSchema],
  ['group_response.rs', 'GroupResponse', GroupResponseSchema],
  ['failure_response.rs', 'FailureResponse', FailureResponseSchema],
  ['create_token_request.rs', 'CreateTokenRequest', CreateTokenRequestSchema],
];

describe('the console calls the SCIM routes the server serves', () => {
  test.each(ROUTES)('%s answers %s', (path, method, handler) => {
    expect(route(path)).toContain(`${method}(scim::console::${handler})`);
  });

  test('the base URL the fixtures show sits under the SCIM base path', () => {
    const basePath = code(SCIM_RS, 'BASE_PATH');
    expect(new URL(BASE_URL).pathname.startsWith(`${basePath}/`)).toBe(true);
  });
});

describe('the SCIM console structs carry what the console models', () => {
  test.each(SHAPES)('%s %s', (file, structName, schema) => {
    expect(zodShape(schema)).toEqual(rustShape(read(join(CONSOLE, file)), structName));
  });

  test('a mapped custom role is the summary the server sends', () => {
    expect(sorted(structFields(read(CUSTOM_ROLES_RS), SUMMARY).map((field) => field.name))).toEqual(
      sorted(Object.keys(CustomRoleReferenceSchema.shape))
    );
  });

  test('a mapping request names a built-in role or a custom role, never both or neither', () => {
    const fields = structFields(read(join(CONSOLE, 'mapping_request.rs')), 'MappingRequest');
    expect(sorted(fields.map((field) => field.name))).toEqual(
      sorted(MappingRequestSchema.options.flatMap((option) => Object.keys(option.shape)))
    );
    expect(fields.every((field) => field.nullable)).toBe(true);
    expect(MappingRequestSchema.safeParse({ role: 'admin' }).success).toBe(true);
    expect(MappingRequestSchema.safeParse({ custom_role_id: 'role-1' }).success).toBe(true);
    expect(
      MappingRequestSchema.safeParse({ role: 'admin', custom_role_id: 'role-1' }).success
    ).toBe(false);
    expect(MappingRequestSchema.safeParse({}).success).toBe(false);
    expect(MappingRequestSchema.safeParse({ role: OWNER }).success).toBe(false);
  });

  test('a group maps to every organization role but owner', () => {
    const roles = [...read(ORGANIZATION_MEMBERS_RS).matchAll(ROLE_NAME)].map((arm) => arm[1]);
    expect(roles).toContain(OWNER);
    expect(sorted(MAPPED_ROLES)).toEqual(sorted(roles.filter((role) => role !== OWNER)));
  });
});

describe('the SCIM refusal codes match the server', () => {
  test('the console models every code the console routes send', () => {
    expect(sorted(SCIM_ROUTE_CODES)).toEqual(sorted([...codes(REFUSAL_RS).values()]));
  });

  test('license_required is the entitlement code', () => {
    expect(ScimCode.LicenseRequired).toBe(code(ENTITLEMENT_RS, 'LICENSE_REQUIRED'));
  });

  test('role_exceeds is the code a mapping past the caller refuses with', () => {
    expect(ScimCode.Exceeds).toBe(code(CUSTOM_ROLES_RS, 'EXCEEDS'));
  });
});

describe('the audit log filters offer what SCIM records', () => {
  const audit = codes(AUDIT_RS);

  test('every SCIM action and the owner seating', () => {
    const actions = [...audit]
      .filter(([name, value]) => AUDITED.test(name) && value.includes('.'))
      .map(([, value]) => value);
    expect(actions.length).toBe(14);
    for (const action of actions) {
      expect(AUDIT_ACTIONS, action).toContain(action as (typeof AUDIT_ACTIONS)[number]);
    }
  });

  test('the SCIM token and group resources', () => {
    for (const name of ['SCIM_TOKEN', 'SCIM_GROUP']) {
      const resource = audit.get(name);
      expect(resource, name).toBeDefined();
      expect(AUDIT_RESOURCE_TYPES).toContain(resource as (typeof AUDIT_RESOURCE_TYPES)[number]);
    }
  });
});
