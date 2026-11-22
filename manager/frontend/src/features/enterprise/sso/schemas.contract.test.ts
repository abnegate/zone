/**
 * The single sign-on section and the server agree on the admin API only by convention: the
 * structs live in `runner/zone_server/src/routes/sso/` and the codes in
 * `runner/zone_enterprise/src/{entitlement.rs,sso/error.rs}`. A field or code changed on one side
 * alone either fails every read of the settings or shows a refusal the console can't explain.
 *
 * This reads the server's source and asserts the console mirrors it.
 */

import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import type { z } from 'zod';

import { rustVariants, structFields } from '../../../test/rust';
import {
  CreateProviderRequestSchema,
  EnforcementRequestSchema,
  EnforcementResponseSchema,
  PROTOCOLS,
  ProviderResponseSchema,
  ServiceProviderResponseSchema,
  SettingsResponseSchema,
  SSO_ADMIN_CODES,
  SsoAdminCode,
  SsoRefusalSchema,
  UpdateProviderRequestSchema,
} from './schemas';

const RUNNER = join(import.meta.dir, '../../../../../../runner');
const SERVER = join(RUNNER, 'zone_server/src');
const ENTERPRISE = join(RUNNER, 'zone_enterprise/src');
const SETTINGS_RS = join(SERVER, 'routes/sso/settings.rs');
const PROVIDERS_RS = join(SERVER, 'routes/sso/providers.rs');
const ENFORCEMENT_RS = join(SERVER, 'routes/sso/enforcement.rs');
const ROUTES_RS = join(SERVER, 'routes/mod.rs');
const COMMON_RS = join(SERVER, 'routes/common.rs');
const ENTITLEMENT_RS = join(ENTERPRISE, 'entitlement.rs');
const ERROR_RS = join(ENTERPRISE, 'sso/error.rs');
const PROTOCOL_RS = join(ENTERPRISE, 'sso/protocol.rs');

const CODE = /pub const ([A-Z_]+): &str = "([^"]+)";/g;
const FIELD = /^pub\s+([a-z_][a-z_0-9]*)\s*:\s*(.+?),?$/;
const DEFAULTED = /#\[serde\([^)]*\bdefault\b/;
const OPTION = /^Option</;

/** The codes the admin routes answer with, by the name of their constant in sso/error.rs. */
const ADMIN_CODES = [
  'PROVIDER_NOT_FOUND',
  'PROVIDER_REQUIRED',
  'PROVIDER_DUPLICATE',
  'DISPLAY_NAME_INVALID',
  'ISSUER_INVALID',
  'CLIENT_ID_INVALID',
  'CLIENT_SECRET_INVALID',
  'DOMAIN_INVALID',
  'DISCOVERY_FAILED',
  'METADATA_INVALID',
  'DOMAIN_REQUIRED',
  'VERIFIED_EMAIL_UNSUPPORTED',
  'PROTOCOL_MISMATCH',
];

/** The codes only public sign-in and the password paths answer with. */
const SIGN_IN_CODES = [
  'SSO_REQUIRED',
  'STATE_INVALID',
  'TOKEN_EXCHANGE_FAILED',
  'ID_TOKEN_INVALID',
  'EMAIL_MISSING',
  'EMAIL_UNVERIFIED',
  'DOMAIN_NOT_ALLOWED',
  'EMAIL_CONFLICT',
  'ASSERTION_INVALID',
  'SIGN_IN_REFUSED',
  'SIGN_IN_FAILED',
];

/** The connection fields a provider of the other protocol sends as null. */
const CONNECTION_NULLABLE = ['client_id', 'metadata_url', 'sso_url'];

type Shaped = z.ZodObject<z.ZodRawShape>;

const read = (path: string): string => readFileSync(path, 'utf8');

const sorted = (values: readonly string[]): string[] => [...values].sort();

const keys = (schema: Shaped): string[] => Object.keys(schema.shape).sort();

function zodShape(schema: Shaped): Record<string, boolean> {
  return Object.fromEntries(
    Object.entries(schema.shape).map(([key, value]) => [key, value._zod.optout === 'optional'])
  );
}

function rustShape(path: string, structName: string): Record<string, boolean> {
  return Object.fromEntries(
    structFields(read(path), structName).map((field) => [field.name, field.optional])
  );
}

function zodOptional(schema: Shaped): string[] {
  return Object.entries(zodShape(schema))
    .filter(([, optional]) => optional)
    .map(([key]) => key)
    .sort();
}

function zodNullable(schema: Shaped): string[] {
  return Object.entries(schema.shape)
    .filter(([, value]) => value.safeParse(null).success)
    .map(([key]) => key)
    .sort();
}

function body(path: string, structName: string): string[] {
  const declaration = new RegExp(`struct ${structName}\\s*\\{([^}]*)\\}`).exec(read(path));
  if (!declaration) throw new Error(`${structName} is not a struct in ${path}`);
  return declaration[1].split('\n').map((line) => line.trim());
}

/** The fields of a request struct the server reads when the console leaves them out. */
function tolerated(path: string, structName: string): string[] {
  const names: string[] = [];
  let defaulted = false;
  for (const line of body(path, structName)) {
    if (line.startsWith('#[')) {
      defaulted ||= DEFAULTED.test(line);
      continue;
    }
    const field = FIELD.exec(line);
    if (!field) continue;
    if (defaulted || OPTION.test(field[2])) names.push(field[1]);
    defaulted = false;
  }
  return names.sort();
}

function rustNullable(path: string, structName: string): string[] {
  return body(path, structName)
    .map((line) => FIELD.exec(line))
    .filter((field): field is RegExpExecArray => field !== null && OPTION.test(field[2]))
    .map((field) => field[1])
    .sort();
}

function codes(path: string): Map<string, string> {
  return new Map([...read(path).matchAll(CODE)].map((match) => [match[1], match[2]]));
}

function code(path: string, name: string): string {
  const value = codes(path).get(name);
  if (value === undefined) throw new Error(`${name} is not a code constant in ${path}`);
  return value;
}

describe('the single sign-on settings read as the server serialises them', () => {
  test.each<[string, string, Shaped]>([
    ['SettingsResponse', SETTINGS_RS, SettingsResponseSchema],
    ['ServiceProviderResponse', SETTINGS_RS, ServiceProviderResponseSchema],
    ['ProviderResponse', PROVIDERS_RS, ProviderResponseSchema],
    ['EnforcementResponse', ENFORCEMENT_RS, EnforcementResponseSchema],
  ])(
    '%s carries the same fields on both sides, none of them omitted',
    (structName, path, schema) => {
      expect(zodShape(schema)).toEqual(rustShape(path, structName));
    }
  );

  test.each<[string, string, Shaped]>([
    ['SettingsResponse', SETTINGS_RS, SettingsResponseSchema],
    ['ServiceProviderResponse', SETTINGS_RS, ServiceProviderResponseSchema],
    ['EnforcementResponse', ENFORCEMENT_RS, EnforcementResponseSchema],
  ])('%s is never null', (structName, path, schema) => {
    expect(rustNullable(path, structName)).toEqual([]);
    expect(zodNullable(schema)).toEqual([]);
  });

  test('a provider sends null only for the connection fields of the other protocol', () => {
    expect(rustNullable(PROVIDERS_RS, 'ProviderResponse')).toEqual(CONNECTION_NULLABLE);
    expect(zodNullable(ProviderResponseSchema)).toEqual(CONNECTION_NULLABLE);
  });

  test('a provider never carries its client secret or its raw certificates', () => {
    for (const secret of ['client_secret', 'certificates']) {
      expect(keys(ProviderResponseSchema)).not.toContain(secret);
      expect(Object.keys(rustShape(PROVIDERS_RS, 'ProviderResponse'))).not.toContain(secret);
    }
  });

  test('protocols', () => {
    expect(sorted(PROTOCOLS)).toEqual(sorted(rustVariants(read(PROTOCOL_RS), 'Protocol')));
  });
});

describe('the console sends the single sign-on requests the server reads', () => {
  test.each<[string, string, Shaped]>([
    ['CreateProviderRequest', PROVIDERS_RS, CreateProviderRequestSchema],
    ['UpdateProviderRequest', PROVIDERS_RS, UpdateProviderRequestSchema],
    ['EnforcementRequest', ENFORCEMENT_RS, EnforcementRequestSchema],
  ])('%s carries exactly the fields the server declares', (structName, path, schema) => {
    expect(keys(schema)).toEqual(sorted(Object.keys(rustShape(path, structName))));
  });

  test.each<[string, string, Shaped]>([
    ['CreateProviderRequest', PROVIDERS_RS, CreateProviderRequestSchema],
    ['UpdateProviderRequest', PROVIDERS_RS, UpdateProviderRequestSchema],
    ['EnforcementRequest', ENFORCEMENT_RS, EnforcementRequestSchema],
  ])('%s leaves out only fields the server tolerates missing', (structName, path, schema) => {
    const missing = tolerated(path, structName);

    for (const field of zodOptional(schema)) {
      expect(missing).toContain(field);
    }
  });

  test('a creation must name the provider and may leave out every protocol field', () => {
    expect(zodOptional(CreateProviderRequestSchema)).toEqual([
      'allowed_domains',
      'client_id',
      'client_secret',
      'enabled',
      'issuer',
      'metadata',
      'metadata_url',
      'protocol',
      'require_verified_email',
    ]);
  });

  test('an update is sparse on both sides', () => {
    const fields = keys(UpdateProviderRequestSchema);

    expect(zodOptional(UpdateProviderRequestSchema)).toEqual(fields);
    expect(tolerated(PROVIDERS_RS, 'UpdateProviderRequest')).toEqual(fields);
  });

  test('enforcement is always sent', () => {
    expect(zodOptional(EnforcementRequestSchema)).toEqual([]);
    expect(tolerated(ENFORCEMENT_RS, 'EnforcementRequest')).toEqual([]);
  });

  test('the routes the console calls are the ones the server serves', () => {
    const routes = read(ROUTES_RS);

    for (const path of [
      '"/api/sso"',
      '"/api/sso/providers"',
      '"/api/sso/providers/{provider_id}"',
      '"/api/sso/enforcement"',
    ]) {
      expect(routes).toContain(path);
    }
  });
});

describe('the single sign-on refusal codes match the server', () => {
  test('license_required is the entitlement code', () => {
    expect(SsoAdminCode.LicenseRequired).toBe(code(ENTITLEMENT_RS, 'LICENSE_REQUIRED'));
  });

  test('the console models every code the admin routes send, and no other', () => {
    expect(sorted(SSO_ADMIN_CODES)).toEqual(
      sorted([
        code(ENTITLEMENT_RS, 'LICENSE_REQUIRED'),
        ...ADMIN_CODES.map((name) => code(ERROR_RS, name)),
      ])
    );
  });

  test('every code in sso/error.rs belongs to the admin routes or to sign-in', () => {
    expect(sorted([...codes(ERROR_RS).keys()])).toEqual(sorted([...ADMIN_CODES, ...SIGN_IN_CODES]));
  });

  test('a refusal carries the error and code the server writes', () => {
    expect(keys(SsoRefusalSchema)).toEqual(
      sorted(Object.keys(rustShape(COMMON_RS, 'ErrorResponse')))
    );
  });
});
