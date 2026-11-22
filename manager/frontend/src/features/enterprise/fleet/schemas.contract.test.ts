import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import type { z } from 'zod';
import { rustShape, zodShape } from '../../../test/contract';
import { structFields } from '../../../test/rust';
import { AUDIT_ACTIONS, AUDIT_RESOURCE_TYPES } from '../../settings/organization/schemas';
import {
  AvailableLabelSchema,
  FLEET_ADMIN_CODES,
  FLEET_CHANGE_CODES,
  FLEET_CODES,
  FLEET_WORK_CODES,
  FleetCode,
  FleetPolicyResponseSchema,
  FleetRefusalSchema,
  NoRunnerLeftSchema,
  OrganizationRunnerLabelsResponseSchema,
  RUNNER_STATES,
  RunnerResponseSchema,
  RunnersResponseSchema,
  UpdateFleetPolicyRequestSchema,
  UpdateRunnerLabelsRequestSchema,
  UpdateRunnerRequestSchema,
  WorkspaceRunnerLabelsResponseSchema,
} from './schemas';

const RUNNER = join(import.meta.dir, '../../../../../../runner');
const SERVER = join(RUNNER, 'zone_server/src');
const ENTERPRISE = join(RUNNER, 'zone_enterprise/src');
const RESPONSES_RS = join(SERVER, 'routes/fleet/responses.rs');
const REQUESTS_RS = join(SERVER, 'routes/fleet/requests.rs');
const ROUTES_RS = join(SERVER, 'routes/mod.rs');
const COMMON_RS = join(SERVER, 'routes/common.rs');
const STATE_RS = join(SERVER, 'db/fleet/state.rs');
const AUDIT_RS = join(SERVER, 'db/audit.rs');
const CODE_RS = join(ENTERPRISE, 'fleet/code.rs');
const ENTITLEMENT_RS = join(ENTERPRISE, 'entitlement.rs');

const CODE = /pub const ([A-Z_]+): &str = "([^"]+)";/g;
const ARM = /Self::[A-Za-z]+ => "([^"]+)"/g;
const DEFAULTED = /#\[serde\([^)]*\bdefault\b/;
const DENY_UNKNOWN = /#\[serde\(deny_unknown_fields\)\]\s*pub struct /g;

type Shaped = z.ZodObject<z.ZodRawShape>;

const RESPONSES: [string, Shaped][] = [
  ['FleetPolicyResponse', FleetPolicyResponseSchema],
  ['RunnersResponse', RunnersResponseSchema],
  ['RunnerResponse', RunnerResponseSchema],
  ['AvailableLabel', AvailableLabelSchema],
  ['OrganizationRunnerLabelsResponse', OrganizationRunnerLabelsResponseSchema],
  ['WorkspaceRunnerLabelsResponse', WorkspaceRunnerLabelsResponseSchema],
  ['NoRunnerLeftResponse', NoRunnerLeftSchema],
];

const REQUESTS: [string, Shaped, Record<string, unknown>][] = [
  ['UpdateFleetPolicyRequest', UpdateFleetPolicyRequestSchema, { minimum_version: null }],
  ['UpdateRunnerRequest', UpdateRunnerRequestSchema, { state: RUNNER_STATES[0] }],
  ['UpdateRunnerLabelsRequest', UpdateRunnerLabelsRequestSchema, { labels: [] }],
];

const FLEET_ORGANIZATION_ACTIONS = ['RUNNER_LABELS_UPDATED', 'FLEET_RUN_REFUSED'];

const FLEET_ORGANIZATION_RESOURCES = ['ORGANIZATION_RUNNER_LABELS', 'WORKSPACE_RUNNER_LABELS'];

const read = (path: string): string => readFileSync(path, 'utf8');

const sorted = (values: readonly string[]): string[] => [...values].sort();

const keys = (schema: Shaped): string[] => Object.keys(schema.shape).sort();

function codes(path: string): Map<string, string> {
  return new Map([...read(path).matchAll(CODE)].map((match) => [match[1], match[2]]));
}

function code(path: string, name: string): string {
  const value = codes(path).get(name);
  if (value === undefined) throw new Error(`${name} is not a code constant in ${path}`);
  return value;
}

function module(path: string, name: string): string {
  const source = read(path);
  const start = source.indexOf(`pub mod ${name} {`);
  if (start < 0) throw new Error(`${name} is not a module in ${path}`);
  return source.slice(start, source.indexOf('\n}\n', start));
}

function audited(name: string, constants: readonly string[]): string[] {
  const declared = new Map(
    [...module(AUDIT_RS, name).matchAll(CODE)].map((match) => [match[1], match[2]])
  );
  return constants.map((constant) => {
    const value = declared.get(constant);
    if (value === undefined) throw new Error(`${name}::${constant} is not declared in audit.rs`);
    return value;
  });
}

function literals(path: string, enumName: string): string[] {
  const source = read(path);
  const implementation = source.indexOf(`impl ${enumName} {`);
  if (implementation < 0) throw new Error(`${enumName} has no impl block in ${path}`);
  const method = source.indexOf('fn as_str', implementation);
  const end = source.indexOf('\n    }\n', method);
  return [...source.slice(method, end).matchAll(ARM)].map((match) => match[1]);
}

function tolerated(path: string, structName: string): string[] {
  const declaration = new RegExp(`struct ${structName}\\s*\\{([^}]*)\\}`).exec(read(path));
  if (!declaration) throw new Error(`${structName} is not a struct in ${path}`);
  const names: string[] = [];
  let defaulted = false;
  for (const raw of declaration[1].split('\n')) {
    const line = raw.trim();
    if (line.startsWith('#[')) {
      defaulted ||= DEFAULTED.test(line);
      continue;
    }
    const field = /^pub\s+([a-z_][a-z_0-9]*)\s*:/.exec(line);
    if (!field) continue;
    if (defaulted) names.push(field[1]);
    defaulted = false;
  }
  return names.sort();
}

function optional(schema: Shaped): string[] {
  return Object.entries(schema.shape)
    .filter(([, value]) => value._zod.optout === 'optional')
    .map(([key]) => key)
    .sort();
}

function nullable(schema: Shaped): string[] {
  return Object.entries(schema.shape)
    .filter(([, value]) => value.safeParse(null).success)
    .map(([key]) => key)
    .sort();
}

describe('the fleet reads as the server serialises it', () => {
  test.each(RESPONSES)(
    '%s carries the same fields, optional and nullable alike',
    (structName, schema) => {
      expect(zodShape(schema)).toEqual(rustShape(read(RESPONSES_RS), structName));
    }
  );

  test('a runner state is one the server stores', () => {
    expect(literals(STATE_RS, 'State')).toEqual([...RUNNER_STATES]);
  });

  test('a refusal carries the error and code the server writes', () => {
    expect(keys(FleetRefusalSchema)).toEqual(
      sorted(structFields(read(COMMON_RS), 'ErrorResponse').map((field) => field.name))
    );
  });
});

describe('the console sends the fleet requests the server reads', () => {
  test.each(REQUESTS)('%s carries exactly the fields the server declares', (structName, schema) => {
    expect(keys(schema)).toEqual(
      sorted(structFields(read(REQUESTS_RS), structName).map((field) => field.name))
    );
  });

  test.each(REQUESTS)('%s leaves out only what the server defaults', (structName, schema) => {
    expect(optional(schema)).toEqual(tolerated(REQUESTS_RS, structName));
  });

  test.each(REQUESTS)(
    '%s sends null only where the server reads an Option',
    (structName, schema) => {
      expect(nullable(schema)).toEqual(
        structFields(read(REQUESTS_RS), structName)
          .filter((field) => field.nullable)
          .map((field) => field.name)
          .sort()
      );
    }
  );

  test('every request refuses unknown fields on both sides', () => {
    expect([...read(REQUESTS_RS).matchAll(DENY_UNKNOWN)]).toHaveLength(REQUESTS.length);
    for (const [structName, schema, request] of REQUESTS) {
      expect(schema.safeParse(request).success, structName).toBe(true);
      expect(schema.safeParse({ ...request, extra: true }).success, structName).toBe(false);
    }
  });

  test('the routes the console calls are the ones the server serves', () => {
    const routes = read(ROUTES_RS);
    for (const path of [
      '"/api/fleet-policy"',
      '"/api/runners"',
      '"/api/runners/{runner_id}"',
      '"/api/organizations/{org_id}/runner-labels"',
      '"/api/organizations/{org_id}/workspaces/{ws_id}/runner-labels"',
    ]) {
      expect(routes).toContain(path);
    }
  });
});

describe('the fleet codes match the server', () => {
  test('the console models every fleet code, and no other', () => {
    expect(sorted([...FLEET_WORK_CODES, ...FLEET_CHANGE_CODES])).toEqual(
      sorted([...codes(CODE_RS).values()])
    );
  });

  test('each code has the value of its server constant', () => {
    const server = codes(CODE_RS);
    expect(FleetCode.RunnerUnregistered).toBe(server.get('RUNNER_UNREGISTERED') ?? '');
    expect(FleetCode.RunnerCordoned).toBe(server.get('RUNNER_CORDONED') ?? '');
    expect(FleetCode.RunnerRevoked).toBe(server.get('RUNNER_REVOKED') ?? '');
    expect(FleetCode.RunnerOutdated).toBe(server.get('RUNNER_OUTDATED') ?? '');
    expect(FleetCode.LabelsMissing).toBe(server.get('LABELS_MISSING') ?? '');
    expect(FleetCode.NoRunner).toBe(server.get('NO_RUNNER') ?? '');
    expect(FleetCode.BodyInvalid).toBe(server.get('BODY_INVALID') ?? '');
    expect(FleetCode.VersionInvalid).toBe(server.get('VERSION_INVALID') ?? '');
    expect(FleetCode.LabelInvalid).toBe(server.get('LABEL_INVALID') ?? '');
    expect(FleetCode.StateInvalid).toBe(server.get('STATE_INVALID') ?? '');
    expect(FleetCode.RunnerOnline).toBe(server.get('RUNNER_ONLINE') ?? '');
    expect(FleetCode.RunnerBusy).toBe(server.get('RUNNER_BUSY') ?? '');
    expect(FleetCode.NoRunnerLeft).toBe(server.get('NO_RUNNER_LEFT') ?? '');
  });

  test('license_required is the entitlement code', () => {
    expect(FleetCode.LicenseRequired).toBe(code(ENTITLEMENT_RS, 'LICENSE_REQUIRED'));
  });

  test('a change is refused with the license code or a change code, never a work code', () => {
    expect(sorted(FLEET_ADMIN_CODES)).toEqual(
      sorted([FleetCode.LicenseRequired, ...FLEET_CHANGE_CODES])
    );
    expect(sorted(FLEET_CODES)).toEqual(sorted([...FLEET_WORK_CODES, ...FLEET_ADMIN_CODES]));
  });
});

describe('the organization audit log offers the fleet entries recorded in it', () => {
  test('runner label saves and refused runs are filterable actions', () => {
    for (const action of audited('actions', FLEET_ORGANIZATION_ACTIONS)) {
      expect(AUDIT_ACTIONS).toContain(action);
    }
  });

  test('organization and workspace runner labels are filterable resources', () => {
    for (const resource of audited('resources', FLEET_ORGANIZATION_RESOURCES)) {
      expect(AUDIT_RESOURCE_TYPES).toContain(resource);
    }
  });
});
