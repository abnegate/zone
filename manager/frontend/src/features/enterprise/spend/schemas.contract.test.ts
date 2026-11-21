import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import type { z } from 'zod';

import { rustVariants, structFields } from '../../../test/rust';
import { exportFilename } from './api';
import {
  BUDGET_SCOPES,
  BUDGET_STATES,
  BudgetSchema,
  BudgetsResponseSchema,
  CreateBudgetRequestSchema,
  DayUsageSchema,
  NamedUsageSchema,
  SPEND_REFUSAL_CODES,
  SpendCode,
  SpendQuerySchema,
  SpendRefusalSchema,
  SpendReportResponseSchema,
  SpendReportSchema,
  THRESHOLD_DEFAULT,
  UNCOUNTED_REASONS,
  UpdateBudgetRequestSchema,
  UsageSchema,
  UserUsageSchema,
  WorkspaceUsageSchema,
} from './schemas';

const RUNNER = join(import.meta.dir, '../../../../../../runner');
const SERVER = join(RUNNER, 'zone_server/src');
const ENTERPRISE = join(RUNNER, 'zone_enterprise/src');
const SPEND = join(ENTERPRISE, 'spend');
const ROUTES = join(SERVER, 'routes/spend');
const REPORT = join(SERVER, 'db/spend/report');
const REPORTS_RS = join(ROUTES, 'reports.rs');
const BUDGETS_RS = join(ROUTES, 'budgets.rs');
const EXPORTS_RS = join(ROUTES, 'exports.rs');
const QUERY_RS = join(ROUTES, 'query.rs');
const ROUTES_RS = join(SERVER, 'routes/mod.rs');
const COMMON_RS = join(SERVER, 'routes/common.rs');
const ENTITLEMENT_RS = join(ENTERPRISE, 'entitlement.rs');
const ERROR_RS = join(SPEND, 'error.rs');
const EXHAUSTED_RS = join(SPEND, 'exhausted.rs');
const DRAFT_RS = join(SPEND, 'draft.rs');
const PATCH_RS = join(SPEND, 'patch.rs');
const SCOPE_RS = join(SPEND, 'scope.rs');
const STATE_RS = join(SPEND, 'state.rs');
const UNCOUNTED_RS = join(SPEND, 'uncounted.rs');
const BUDGET_RS = join(SPEND, 'budget.rs');

const CODE = /pub const ([A-Z_]+): &str = "([^"]+)";/g;
const PARAMETER = /const [A-Z_]+: &str = "([^"]+)";/g;
const DEFAULT_THRESHOLD = /pub const DEFAULT_THRESHOLD: u8 = (\d+);/;
const FIELD = /^pub\s+([a-z_][a-z_0-9]*)\s*:\s*(.+?),?$/;
const OPTION = /^Option</;
const DEFAULTED = /#\[serde\([^)]*\bdefault\b/;
const DENY_UNKNOWN = /#\[serde\(deny_unknown_fields\)\]\s*pub struct (\w+)/;
const FILENAME = '"zone-spend-{}-{}.csv"';

type Shaped = z.ZodObject<z.ZodRawShape>;

const RESPONSES: [string, string, Shaped][] = [
  ['ReportResponse', REPORTS_RS, SpendReportResponseSchema],
  ['Report', REPORTS_RS, SpendReportSchema],
  ['Usage', join(REPORT, 'usage.rs'), UsageSchema],
  ['Day', join(REPORT, 'day.rs'), DayUsageSchema],
  ['WorkspaceUsage', join(REPORT, 'workspace_usage.rs'), WorkspaceUsageSchema],
  ['UserUsage', join(REPORT, 'user_usage.rs'), UserUsageSchema],
  ['NamedUsage', join(REPORT, 'named_usage.rs'), NamedUsageSchema],
  ['BudgetsResponse', BUDGETS_RS, BudgetsResponseSchema],
  ['BudgetResponse', BUDGETS_RS, BudgetSchema],
];

const REQUESTS: [string, string, Shaped, Record<string, unknown>][] = [
  ['Draft', DRAFT_RS, CreateBudgetRequestSchema, { scope: 'organization', limit_microdollars: 1 }],
  ['Patch', PATCH_RS, UpdateBudgetRequestSchema, {}],
];

const ROUTE_PATHS = [
  '"/api/organizations/{org_id}/spend-reports"',
  '"/api/organizations/{org_id}/spend-exports"',
  '"/api/organizations/{org_id}/budgets"',
  '"/api/organizations/{org_id}/budgets/{budget_id}"',
];

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

function rustNullable(path: string, structName: string): string[] {
  return body(path, structName)
    .map((line) => FIELD.exec(line))
    .filter((field): field is RegExpExecArray => field !== null && OPTION.test(field[2]))
    .map((field) => field[1])
    .sort();
}

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
    if (defaulted) names.push(field[1]);
    defaulted = false;
  }
  return names.sort();
}

function codes(path: string): Map<string, string> {
  return new Map([...read(path).matchAll(CODE)].map((match) => [match[1], match[2]]));
}

function code(path: string, name: string): string {
  const value = codes(path).get(name);
  if (value === undefined) throw new Error(`${name} is not a code constant in ${path}`);
  return value;
}

describe('spend reads as the server serialises it', () => {
  test.each(RESPONSES)(
    '%s carries the same fields on both sides, none of them omitted',
    (structName, path, schema) => {
      expect(zodShape(schema)).toEqual(rustShape(path, structName));
    }
  );

  test.each(RESPONSES)(
    '%s is null in exactly the fields the server may send as null',
    (structName, path, schema) => {
      expect(zodNullable(schema)).toEqual(rustNullable(path, structName));
    }
  );

  test('a refusal carries the error and code the server writes', () => {
    expect(keys(SpendRefusalSchema)).toEqual(
      sorted(Object.keys(rustShape(COMMON_RS, 'ErrorResponse')))
    );
  });
});

describe('the console sends the budgets the server reads', () => {
  test.each(REQUESTS)(
    '%s carries exactly the fields the server declares',
    (structName, path, schema) => {
      expect(keys(schema)).toEqual(sorted(Object.keys(rustShape(path, structName))));
    }
  );

  test.each(REQUESTS)(
    '%s leaves out exactly the fields the server defaults',
    (structName, path, schema) => {
      expect(zodOptional(schema)).toEqual(tolerated(path, structName));
    }
  );

  test.each(REQUESTS)(
    '%s refuses unknown fields on both sides',
    (structName, path, schema, valid) => {
      expect(DENY_UNKNOWN.exec(read(path))?.[1]).toBe(structName);
      expect(schema.safeParse(valid).success).toBe(true);
      expect(schema.safeParse({ ...valid, unknown: true }).success).toBe(false);
    }
  );

  test('a budget the console leaves the threshold off warns where the server defaults it', () => {
    expect(Number(DEFAULT_THRESHOLD.exec(read(BUDGET_RS))?.[1])).toBe(THRESHOLD_DEFAULT);
  });

  test('a report is asked for with the parameters the server reads', () => {
    expect(keys(SpendQuerySchema)).toEqual(
      sorted([...read(QUERY_RS).matchAll(PARAMETER)].map((match) => match[1]))
    );
  });

  test('the routes the console calls are the ones the server serves', () => {
    const routes = read(ROUTES_RS);

    for (const path of ROUTE_PATHS) {
      expect(routes).toContain(path);
    }
  });

  test('an export is saved under the name the server gives it', () => {
    expect(read(EXPORTS_RS)).toContain(FILENAME);
    expect(exportFilename({ from: '2026-11-01', to: '2026-11-21' })).toBe(
      'zone-spend-2026-11-01-2026-11-21.csv'
    );
  });
});

describe('the spend vocabularies match the server', () => {
  test('scopes', () => {
    expect(rustVariants(read(SCOPE_RS), 'Scope')).toEqual([...BUDGET_SCOPES]);
  });

  test('states', () => {
    expect(rustVariants(read(STATE_RS), 'State')).toEqual([...BUDGET_STATES]);
  });

  test('uncounted reasons', () => {
    expect(rustVariants(read(UNCOUNTED_RS), 'Uncounted')).toEqual([...UNCOUNTED_REASONS]);
  });
});

describe('the spend codes match the server', () => {
  test('license_required is the entitlement code', () => {
    expect(SpendCode.LicenseRequired).toBe(code(ENTITLEMENT_RS, 'LICENSE_REQUIRED'));
  });

  test('the console models every code the spend routes refuse with, and no other', () => {
    expect(sorted(SPEND_REFUSAL_CODES)).toEqual(
      sorted([code(ENTITLEMENT_RS, 'LICENSE_REQUIRED'), ...codes(ERROR_RS).values()])
    );
  });

  test('each refusal code is the one the server names for it', () => {
    expect(SpendCode.BodyInvalid).toBe(code(ERROR_RS, 'BODY_INVALID'));
    expect(SpendCode.RangeInvalid).toBe(code(ERROR_RS, 'RANGE_INVALID'));
    expect(SpendCode.LimitInvalid).toBe(code(ERROR_RS, 'LIMIT_INVALID'));
    expect(SpendCode.ThresholdInvalid).toBe(code(ERROR_RS, 'THRESHOLD_INVALID'));
    expect(SpendCode.ScopeInvalid).toBe(code(ERROR_RS, 'SCOPE_INVALID'));
    expect(SpendCode.BudgetExists).toBe(code(ERROR_RS, 'BUDGET_EXISTS'));
  });

  test('a blocked request carries the exhausted code', () => {
    expect(SpendCode.BudgetExhausted).toBe(code(EXHAUSTED_RS, 'BUDGET_EXHAUSTED'));
  });
});
