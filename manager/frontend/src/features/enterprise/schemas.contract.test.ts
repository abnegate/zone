import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import type { z } from 'zod';

import { rustVariants, structFields } from '../../test/rust';
import {
  EDITIONS,
  LICENSE_REFUSAL_CODES,
  LICENSE_STATUSES,
  LicenseDetailsSchema,
  LicenseRefusalSchema,
  LicenseResponseSchema,
  SeatsSchema,
  SetLicenseRequestSchema,
  TIERS,
  UNVERIFIED_CODES,
  UnverifiedSchema,
} from './schemas';

const RUNNER = join(import.meta.dir, '../../../../../runner');
const SERVER = join(RUNNER, 'zone_server/src');
const ENTERPRISE = join(RUNNER, 'zone_enterprise/src');
const ROUTE = join(SERVER, 'routes/license.rs');
const COMMON = join(SERVER, 'routes/common.rs');
const SERVICE = join(SERVER, 'services/license/error.rs');
const ERRORS = join(ENTERPRISE, 'error.rs');

const CODE = /pub const [A-Z_]+: &str = "([^"]+)";/g;
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

describe('the license reads as the server serialises it', () => {
  test.each<[string, Shaped]>([
    ['LicenseResponse', LicenseResponseSchema],
    ['LicenseDetails', LicenseDetailsSchema],
    ['SeatsResponse', SeatsSchema],
    ['UnverifiedResponse', UnverifiedSchema],
  ])('%s carries the same fields on both sides, none of them omitted', (structName, schema) => {
    expect(zodShape(schema)).toEqual(rustShape(ROUTE, structName));
  });

  test.each<[string, Shaped]>([
    ['LicenseResponse', LicenseResponseSchema],
    ['LicenseDetails', LicenseDetailsSchema],
    ['SeatsResponse', SeatsSchema],
    ['UnverifiedResponse', UnverifiedSchema],
  ])('%s is null in exactly the fields the server may send as null', (structName, schema) => {
    expect(zodNullable(schema)).toEqual(rustNullable(ROUTE, structName));
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
  });

  test('statuses', () => {
    expect(sorted(LICENSE_STATUSES)).toEqual(
      sorted(rustVariants(read(join(ENTERPRISE, 'status.rs')), 'Status'))
    );
  });

  test('tiers', () => {
    expect(sorted(TIERS)).toEqual(sorted(rustVariants(read(join(ENTERPRISE, 'tier.rs')), 'Tier')));
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
