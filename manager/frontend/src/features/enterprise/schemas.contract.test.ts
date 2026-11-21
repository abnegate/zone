import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import type { z } from 'zod';

import { rustVariants, structFields } from '../../test/rust';
import { COMMUNITY_MEMBERS } from './limits';
import {
  EDITIONS,
  LICENSE_REFUSAL_CODES,
  LICENSE_STATUSES,
  LicenseDetailsSchema,
  LicenseRefusalSchema,
  LicenseResponseSchema,
  MemberCapCode,
  MembersSchema,
  PLANS,
  RENEWAL_MODES,
  RENEWAL_OUTCOMES,
  RenewalSchema,
  SeatsSchema,
  SetLicenseRequestSchema,
  UNVERIFIED_CODES,
  UnverifiedSchema,
} from './schemas';

const RUNNER = join(import.meta.dir, '../../../../../runner');
const SERVER = join(RUNNER, 'zone_server/src');
const ENTERPRISE = join(RUNNER, 'zone_enterprise/src');
const ROUTE = join(SERVER, 'routes/license.rs');
const COMMON = join(SERVER, 'routes/common.rs');
const SERVICE = join(SERVER, 'services/license/error.rs');
const RENEWAL = join(SERVER, 'services/license/renewal');
const ERRORS = join(ENTERPRISE, 'error.rs');
const MEMBERS = join(ENTERPRISE, 'members.rs');

const CODE = /pub const [A-Z_]+: &str = "([^"]+)";/g;
const MEMBER_CAP = /pub const COMMUNITY_MEMBERS: u32 = (\d+);/;
const OPTION = /^pub\s+([a-z_][a-z_0-9]*)\s*:\s*Option</;

const read = (path: string): string => readFileSync(path, 'utf8');

const sorted = (values: readonly string[]): string[] => [...values].sort();

const keys = (shape: Record<string, unknown>): string[] => Object.keys(shape).sort();

type Shaped = z.ZodObject<z.ZodRawShape>;

function rustShape(path: string, structName: string): Record<string, boolean> {
  return Object.fromEntries(
    structFields(read(path), structName).map((field) => [field.name, field.optional])
  );
}

function zodShape(schema: Shaped): Record<string, boolean> {
  return Object.fromEntries(
    Object.entries(schema.shape).map(([key, value]) => [key, value._zod.optout === 'optional'])
  );
}

function rustNullable(path: string, structName: string): string[] {
  const body = new RegExp(`struct ${structName}\\s*\\{([^}]*)\\}`).exec(read(path))?.[1];
  if (!body) throw new Error(`${structName} is not a struct in ${path}`);
  return body
    .split('\n')
    .map((line) => OPTION.exec(line.trim())?.[1])
    .filter((name): name is string => name !== undefined)
    .sort();
}

function zodNullable(schema: Shaped): string[] {
  return Object.entries(schema.shape)
    .filter(([, value]) => value.safeParse(null).success)
    .map(([key]) => key)
    .sort();
}

function codes(path: string): string[] {
  return [...read(path).matchAll(CODE)].map((match) => match[1]);
}

function rustMemberCap(path: string): number {
  const cap = MEMBER_CAP.exec(read(path))?.[1];
  if (cap === undefined) throw new Error(`COMMUNITY_MEMBERS is not a u32 constant in ${path}`);
  return Number(cap);
}

describe('the license reads as the server serialises it', () => {
  test.each<[string, Shaped]>([
    ['LicenseResponse', LicenseResponseSchema],
    ['LicenseDetails', LicenseDetailsSchema],
    ['SeatsResponse', SeatsSchema],
    ['MembersResponse', MembersSchema],
    ['UnverifiedResponse', UnverifiedSchema],
    ['RenewalResponse', RenewalSchema],
  ])('%s carries the same fields on both sides, none of them omitted', (structName, schema) => {
    expect(zodShape(schema)).toEqual(rustShape(ROUTE, structName));
  });

  test.each<[string, Shaped]>([
    ['LicenseResponse', LicenseResponseSchema],
    ['LicenseDetails', LicenseDetailsSchema],
    ['SeatsResponse', SeatsSchema],
    ['MembersResponse', MembersSchema],
    ['UnverifiedResponse', UnverifiedSchema],
    ['RenewalResponse', RenewalSchema],
  ])('%s is null in exactly the fields the server may send as null', (structName, schema) => {
    expect(zodNullable(schema)).toEqual(rustNullable(ROUTE, structName));
  });

  test('seat usage is null when the server could not count it', () => {
    expect(rustNullable(ROUTE, 'SeatsResponse')).toEqual(['licensed', 'used']);
    expect(zodNullable(SeatsSchema)).toEqual(['licensed', 'used']);
  });

  test('active members are null when uncounted and the cap is null when there is none', () => {
    expect(rustNullable(ROUTE, 'MembersResponse')).toEqual(['active', 'cap']);
    expect(zodNullable(MembersSchema)).toEqual(['active', 'cap']);
  });

  test('SetLicenseRequest is sent with the fields the server reads', () => {
    expect(keys(SetLicenseRequestSchema.shape)).toEqual(
      keys(rustShape(ROUTE, 'SetLicenseRequest'))
    );
  });

  test('a refusal carries the error and code the server writes', () => {
    expect(keys(LicenseRefusalSchema.shape)).toEqual(keys(rustShape(COMMON, 'ErrorResponse')));
  });
});

describe('the license vocabularies match the server', () => {
  test('editions', () => {
    expect(sorted(EDITIONS)).toEqual(
      sorted(rustVariants(read(join(ENTERPRISE, 'edition.rs')), 'Edition'))
    );
    expect(EDITIONS).toHaveLength(3);
  });

  test('plans', () => {
    expect(sorted(PLANS)).toEqual(sorted(rustVariants(read(join(ENTERPRISE, 'plan.rs')), 'Plan')));
  });

  test('a refusal at the member cap carries the code the server sends', () => {
    expect(codes(MEMBERS)).toEqual([MemberCapCode]);
  });

  test('the member cap without a license is the one the server enforces', () => {
    expect(COMMUNITY_MEMBERS).toBe(rustMemberCap(MEMBERS));
  });

  test('statuses', () => {
    expect(sorted(LICENSE_STATUSES)).toEqual(
      sorted(rustVariants(read(join(ENTERPRISE, 'status.rs')), 'Status'))
    );
  });

  test('renewal modes', () => {
    expect(sorted(RENEWAL_MODES)).toEqual(
      sorted(rustVariants(read(join(RENEWAL, 'mode.rs')), 'Mode'))
    );
  });

  test('renewal outcomes', () => {
    expect(sorted(RENEWAL_OUTCOMES)).toEqual(
      sorted(rustVariants(read(join(RENEWAL, 'outcome.rs')), 'Outcome'))
    );
  });

  test('the console models every refusal code the server sends, and no other', () => {
    const service = codes(SERVICE);

    expect(service).toHaveLength(1);
    expect(sorted(LICENSE_REFUSAL_CODES)).toEqual(sorted([...codes(ERRORS), ...service]));
    expect(LICENSE_REFUSAL_CODES).toHaveLength(5);
  });

  test('a stored key that stops verifying carries one of the verifier codes', () => {
    expect(sorted(UNVERIFIED_CODES)).toEqual(sorted(codes(ERRORS)));
  });
});
