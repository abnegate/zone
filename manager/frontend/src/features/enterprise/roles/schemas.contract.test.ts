import { describe, expect, test } from 'bun:test';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import type { z } from 'zod';

import { type RustField, rustVariants, structFields } from '../../../test/rust';
import { CATALOGUE } from './fixtures';
import {
  COMPOSITION_CODES,
  CreateRoleRequestSchema,
  CustomRoleSchema,
  ORGANIZATION_PERMISSIONS,
  PERMISSIONS,
  PermissionDefinitionSchema,
  ROLE_BASE,
  ROLE_ROUTE_CODES,
  ROLE_SCOPES,
  RoleCode,
  RoleRefusalSchema,
  RoleResponseSchema,
  RolesResponseSchema,
  UpdateRoleRequestSchema,
  WORKSPACE_PERMISSIONS,
} from './schemas';

const RUNNER = join(import.meta.dir, '../../../../../../runner');
const SERVER = join(RUNNER, 'zone_server/src');
const ENTERPRISE = join(RUNNER, 'zone_enterprise/src');
const PERMISSION_RS = join(SERVER, 'auth/permission.rs');
const SCOPE_RS = join(SERVER, 'auth/scope.rs');
const COMPOSITION_RS = join(SERVER, 'auth/composition.rs');
const CUSTOM_ROLES_RS = join(SERVER, 'db/custom_roles.rs');
const ROUTES_RS = join(SERVER, 'routes/mod.rs');
const ROLES_ROUTES = join(SERVER, 'routes/roles');
const ENTITLEMENT_RS = join(ENTERPRISE, 'entitlement.rs');
const MIGRATION_SQL = join(RUNNER, 'zone_server/migrations/0001_v1.sql');

const NAME_ARM = /Self::([A-Za-z]+) => "([^"]+)"/g;
const SCOPE_ARM = /((?:\|?\s*Self::[A-Za-z]+\s*)+)=>\s*Scope::([A-Za-z]+)/g;
const VARIANT = /Self::([A-Za-z]+)/g;
const BASE_ARM = /Self::([A-Za-z]+) => Some\((?:Permission::)?([A-Za-z]+)\)/g;
const CODE_ARM = /=> "([a-z_]+)"/g;
const CODE = /pub const ([A-Z_]+): &str = "([^"]+)";/g;
const STRUCT = /struct ([A-Z][A-Za-z]*)\s*\{/g;
const PERMISSION_CHECK =
  /CONSTRAINT custom_role_permissions_permission_check CHECK \(+permission = ANY \(ARRAY\[([^\]]*)\]/;
const QUOTED = /'([^']+)'/g;
const ROLES_PATH = '"/api/organizations/{org_id}/roles"';
const ROLE_PATH = /"\/api\/organizations\/\{org_id\}\/roles\/\{[a-z_]+\}"/;
const SERVER_SCOPE = 'server';

type Shaped = z.ZodObject<z.ZodRawShape>;

const read = (path: string): string => readFileSync(path, 'utf8');

const sorted = (values: readonly string[]): string[] => [...values].sort();

const keys = (schema: Shaped): string[] => Object.keys(schema.shape).sort();

function method(source: string, name: string): string {
  const start = source.indexOf(`fn ${name}(`);
  if (start < 0) throw new Error(`fn ${name} is not defined`);
  const end = source.indexOf('\n    }\n', start);
  return source.slice(start, end < 0 ? undefined : end);
}

function names(): Map<string, string> {
  return new Map(
    [...method(read(PERMISSION_RS), 'name').matchAll(NAME_ARM)].map((arm) => [arm[1], arm[2]])
  );
}

function scopes(): Map<string, string> {
  const scoped = new Map<string, string>();
  for (const arm of method(read(PERMISSION_RS), 'scope').matchAll(SCOPE_ARM)) {
    for (const variant of arm[1].matchAll(VARIANT)) {
      scoped.set(variant[1], arm[2].toLowerCase());
    }
  }
  return scoped;
}

function scoped(scope: string): string[] {
  const scopeOf = scopes();
  return [...names()].filter(([variant]) => scopeOf.get(variant) === scope).map(([, name]) => name);
}

function codes(path: string): Map<string, string> {
  return new Map([...read(path).matchAll(CODE)].map((match) => [match[1], match[2]]));
}

function code(path: string, name: string): string {
  const value = codes(path).get(name);
  if (value === undefined) throw new Error(`${name} is not a code constant in ${path}`);
  return value;
}

function routeSources(): string {
  if (!existsSync(ROLES_ROUTES)) throw new Error(`${ROLES_ROUTES} does not exist yet`);
  return readdirSync(ROLES_ROUTES, { recursive: true })
    .map(String)
    .filter((file) => file.endsWith('.rs'))
    .map((file) => read(join(ROLES_ROUTES, file)))
    .join('\n');
}

function shapes(source: string): Map<string, RustField[]> {
  const found = new Map<string, RustField[]>();
  for (const [, structName] of source.matchAll(STRUCT)) {
    try {
      found.set(structName, structFields(source, structName));
    } catch {}
  }
  return found;
}

function carrying(source: string, schema: Shaped): string[] {
  const expected = keys(schema);
  return [...shapes(source)]
    .filter(([, fields]) => sorted(fields.map((field) => field.name)).join() === expected.join())
    .map(([structName]) => structName);
}

const SHAPES: [string, Shaped][] = [
  ['the roles response', RolesResponseSchema],
  ['a role', CustomRoleSchema],
  ['a catalogue entry', PermissionDefinitionSchema],
  ['the response to a create or update', RoleResponseSchema],
  ['a create request', CreateRoleRequestSchema],
  ['an update request', UpdateRoleRequestSchema],
  ['the refusal of a role in use', RoleRefusalSchema],
];

describe('the permission names match the catalogue the server declares', () => {
  test('organization permissions, in declaration order', () => {
    expect([...ORGANIZATION_PERMISSIONS]).toEqual(scoped('organization'));
  });

  test('workspace permissions, in declaration order', () => {
    expect([...WORKSPACE_PERMISSIONS]).toEqual(scoped('workspace'));
  });

  test('the console models every permission a role can hold, and no server permission', () => {
    const server = scoped(SERVER_SCOPE);
    expect(server.length).toBeGreaterThan(0);
    expect(sorted(PERMISSIONS)).toEqual(
      sorted([...names().values()].filter((name) => !server.includes(name)))
    );
  });

  test('the scopes a role can take are the non-server scopes', () => {
    expect(sorted(ROLE_SCOPES)).toEqual(
      sorted(rustVariants(read(SCOPE_RS), 'Scope').filter((scope) => scope !== SERVER_SCOPE))
    );
  });

  test('each scope adds the base permission the server adds', () => {
    const nameOf = names();
    const bases = Object.fromEntries(
      [...method(read(SCOPE_RS), 'base').matchAll(BASE_ARM)].map((arm) => [
        arm[1].toLowerCase(),
        nameOf.get(arm[2]),
      ])
    );
    expect(bases).toEqual({ ...ROLE_BASE });
  });

  test('the fixture catalogue stores exactly the names the database accepts in a role', () => {
    const accepted = PERMISSION_CHECK.exec(read(MIGRATION_SQL))?.[1];
    if (accepted === undefined) throw new Error('custom_role_permissions has no permission check');
    const composable = CATALOGUE.filter((entry) => entry.composable).map((entry) => entry.name);

    expect(sorted([...accepted.matchAll(QUOTED)].map((match) => match[1]))).toEqual(
      sorted([...composable, ...Object.values(ROLE_BASE)])
    );
  });
});

describe('the role codes match the server', () => {
  test('license_required is the entitlement code', () => {
    expect(RoleCode.LicenseRequired).toBe(code(ENTITLEMENT_RS, 'LICENSE_REQUIRED'));
  });

  test('the composition codes are the ones Composition::code writes', () => {
    expect(sorted(COMPOSITION_CODES)).toEqual(
      sorted([...method(read(COMPOSITION_RS), 'code').matchAll(CODE_ARM)].map((arm) => arm[1]))
    );
  });

  test('role_exceeds is the code the role store refuses a widening with', () => {
    expect(RoleCode.Exceeds).toBe(code(CUSTOM_ROLES_RS, 'EXCEEDS'));
  });

  test.each([...ROLE_ROUTE_CODES])('the roles routes send %s', (routeCode) => {
    expect(routeSources()).toContain(`"${routeCode}"`);
  });
});

describe('the roles routes read and write what the console models', () => {
  test('the routes the console calls are the ones the server serves', () => {
    const routes = read(ROUTES_RS);
    expect(routes).toContain(ROLES_PATH);
    expect(routes).toMatch(ROLE_PATH);
  });

  test.each(SHAPES)('a server struct carries exactly the fields of %s', (_, schema) => {
    expect(carrying(routeSources(), schema).length).toBeGreaterThan(0);
  });
});
