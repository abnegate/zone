/**
 * The sign-in pages and the server agree on single sign-on only by convention: the public structs
 * and the callback path live in `runner/zone_server/src/routes/sso/sign_in.rs`, the codes in
 * `runner/zone_enterprise/src/sso/error.rs` and `runner/zone_server/src/auth/account.rs`. A field,
 * code or path changed on one side alone hides every sign-in button, sends people back to a page
 * that doesn't exist, or shows a refusal the console can't explain.
 *
 * This reads the server's source and asserts the console mirrors it.
 */

import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import type { z } from 'zod';

import { DEVICE_HEADER, DEVICE_NAME_HEADER, DEVICE_PLATFORM_HEADER } from '../../api/device';
import { TIME_ZONE_HEADER } from '../../api/timeZone';
import { structFields } from '../../test/rust';
import { SsoCode } from './SsoCode';
import {
  SsoAuthorizationSchema,
  SsoCallbackRequestSchema,
  SsoPublicProviderSchema,
  SsoPublicProvidersSchema,
} from './schemas';

const FRONTEND = join(import.meta.dir, '../../..');
const RUNNER = join(FRONTEND, '../../runner');
const SERVER = join(RUNNER, 'zone_server/src');
const ENTERPRISE = join(RUNNER, 'zone_enterprise/src');
const SIGN_IN_RS = join(SERVER, 'routes/sso/sign_in.rs');
const ROUTES_RS = join(SERVER, 'routes/mod.rs');
const ACCOUNT_RS = join(SERVER, 'auth/account.rs');
const DEVICE_RS = join(SERVER, 'auth/device.rs');
const TIME_ZONE_RS = join(SERVER, 'auth/time_zone.rs');
const ERROR_RS = join(ENTERPRISE, 'sso/error.rs');
const APP_TSX = join(FRONTEND, 'src/App.tsx');
const SSO_API_TS = join(FRONTEND, 'src/api/sso.ts');

const CODE = /pub const ([A-Z_]+): &str = "([^"]+)";/g;
const CALLBACK_ROUTE = /<Route path="([^"]+)" element=\{<SsoCallbackPage \/>\} \/>/;

/** The codes the callback answers with, by the name of their constant in sso/error.rs. */
const CALLBACK_CODES = [
  'STATE_INVALID',
  'PROVIDER_NOT_FOUND',
  'DISCOVERY_FAILED',
  'TOKEN_EXCHANGE_FAILED',
  'ID_TOKEN_INVALID',
  'EMAIL_MISSING',
  'EMAIL_UNVERIFIED',
  'DOMAIN_NOT_ALLOWED',
  'EMAIL_CONFLICT',
];

const PUBLIC_ROUTES = [
  '"/api/auth/sso/providers"',
  '"/api/auth/sso/providers/{provider_id}/authorizations"',
  '"/api/auth/sso/callbacks"',
];

type Shaped = z.ZodObject<z.ZodRawShape>;

const read = (path: string): string => readFileSync(path, 'utf8');

const sorted = (values: readonly string[]): string[] => [...values].sort();

function zodShape(schema: Shaped): Record<string, boolean> {
  return Object.fromEntries(
    Object.entries(schema.shape).map(([key, value]) => [key, value._zod.optout === 'optional'])
  );
}

function rustShape(structName: string): Record<string, boolean> {
  return Object.fromEntries(
    structFields(read(SIGN_IN_RS), structName).map((field) => [field.name, field.optional])
  );
}

function zodNullable(schema: Shaped): string[] {
  return Object.entries(schema.shape)
    .filter(([, value]) => value.safeParse(null).success)
    .map(([key]) => key);
}

function code(path: string, name: string): string {
  const value = new Map([...read(path).matchAll(CODE)].map((match) => [match[1], match[2]])).get(
    name
  );
  if (value === undefined) throw new Error(`${name} is not a code constant in ${path}`);
  return value;
}

describe('the public single sign-on routes read and send what the server declares', () => {
  test.each<[string, Shaped]>([
    ['PublicProvidersResponse', SsoPublicProvidersSchema],
    ['PublicProvider', SsoPublicProviderSchema],
    ['AuthorizationResponse', SsoAuthorizationSchema],
    ['CallbackRequest', SsoCallbackRequestSchema],
  ])('%s carries the same fields on both sides, none of them optional', (structName, schema) => {
    expect(zodShape(schema)).toEqual(rustShape(structName));
    expect(Object.values(zodShape(schema))).not.toContain(true);
    expect(zodNullable(schema)).toEqual([]);
  });

  test('the provider list names the origin a sign-in has to start on', () => {
    expect(Object.keys(rustShape('PublicProvidersResponse'))).toContain('callback_origin');
  });

  test('the routes the console calls are the ones the server serves', () => {
    const routes = read(ROUTES_RS);
    const api = read(SSO_API_TS);

    for (const path of PUBLIC_ROUTES) {
      expect(routes).toContain(path);
    }
    expect(api).toContain('/api/auth/sso/providers`');
    expect(api).toContain('/authorizations`');
    expect(api).toContain('/api/auth/sso/callbacks`');
  });

  test('the callback page sits where identity providers send people back', () => {
    const route = CALLBACK_ROUTE.exec(read(APP_TSX))?.[1];

    expect(route).toBe(code(SIGN_IN_RS, 'CALLBACK_PATH'));
  });

  test('the callback carries the device and time-zone headers the server reads', () => {
    expect(
      [DEVICE_HEADER, DEVICE_NAME_HEADER, DEVICE_PLATFORM_HEADER].map((name) => name.toLowerCase())
    ).toEqual([code(DEVICE_RS, 'PUBLIC_ID'), code(DEVICE_RS, 'NAME'), code(DEVICE_RS, 'PLATFORM')]);
    expect(TIME_ZONE_HEADER.toLowerCase()).toBe(code(TIME_ZONE_RS, 'HEADER'));
  });
});

describe('the sign-in refusal codes match the server', () => {
  test('sso_required is the enforcement code', () => {
    expect(SsoCode.Required).toBe(code(ERROR_RS, 'SSO_REQUIRED'));
  });

  test('account_disabled is the disabled-account code', () => {
    expect(SsoCode.AccountDisabled).toBe(code(ACCOUNT_RS, 'DISABLED'));
  });

  test('the console models every code the callback sends, and no other', () => {
    expect(sorted(Object.values(SsoCode))).toEqual(
      sorted([
        code(ERROR_RS, 'SSO_REQUIRED'),
        code(ACCOUNT_RS, 'DISABLED'),
        ...CALLBACK_CODES.map((name) => code(ERROR_RS, name)),
      ])
    );
  });
});
