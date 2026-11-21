import { afterAll, afterEach, describe, expect, spyOn, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import type { z } from 'zod';

import { API_BASE } from '../../../api/client';
import { rustVariants, structFields } from '../../../test/rust';
import { auditApi } from './api';
import * as fixtures from './fixtures';
import {
  AUDIT_ADMIN_CODES,
  AUDIT_KINDS,
  AuditCode,
  AuditKind,
  AuditRefusalSchema,
  AuditScopeKind,
  CreateDestinationRequestSchema,
  DEFAULT_ADDRESSING,
  DEFAULT_FACILITY,
  DEFAULT_SYSLOG_HOSTNAME,
  DELIVERY_FAILURES,
  DELIVERY_HEALTHS,
  DeliveryResponseSchema,
  DestinationResponseSchema,
  DestinationsResponseSchema,
  EXPORT_FORMATS,
  ExportQuerySchema,
  FACILITY_MAXIMUM,
  HolderResponseSchema,
  KIND_SECRETS,
  REQUIRED_SECRETS,
  RETENTION_DAYS_MAXIMUM,
  RETENTION_WAITS,
  RetentionResponseSchema,
  S3_ADDRESSINGS,
  S3SecretsSchema,
  S3SettingsRequestSchema,
  S3SettingsSchema,
  SECRET_NAMES,
  SettingsSchema,
  STREAMING_PAUSES,
  StandingSchema,
  SYSLOG_PROTOCOLS,
  SyslogSecretsSchema,
  SyslogSettingsRequestSchema,
  SyslogSettingsSchema,
  TestResponseSchema,
  UpdateDestinationRequestSchema,
  UpdateRetentionRequestSchema,
  WebhookSecretsSchema,
  WebhookSettingsRequestSchema,
  WebhookSettingsSchema,
} from './schemas';
import { organizationScope, SERVER_SCOPE } from './scope';
import type { AuditScope, DeliveryResponse, AuditKind as Kind } from './types';

const RUNNER = join(import.meta.dir, '../../../../../../runner');
const SERVER = join(RUNNER, 'zone_server/src');
const ENTERPRISE = join(RUNNER, 'zone_enterprise/src');
const AUDIT = join(ENTERPRISE, 'audit');
const DESTINATION = join(AUDIT, 'destination');
const ROUTES = join(SERVER, 'routes');
const AUDIT_ROUTES = join(ROUTES, 'audit');
const ROUTES_RS = join(ROUTES, 'mod.rs');
const COMMON_RS = join(ROUTES, 'common.rs');
const DESTINATIONS_RS = join(AUDIT_ROUTES, 'destinations.rs');
const RETENTION_ROUTE_RS = join(AUDIT_ROUTES, 'retention.rs');
const EXPORTS_RS = join(AUDIT_ROUTES, 'exports.rs');
const REFUSAL_RS = join(AUDIT_ROUTES, 'refusal.rs');
const ENTITLEMENT_RS = join(ENTERPRISE, 'entitlement.rs');
const STANDING_RS = join(AUDIT, 'standing.rs');
const HEALTH_RS = join(AUDIT, 'health.rs');
const RETENTION_RS = join(AUDIT, 'retention.rs');
const FAILURE_RS = join(AUDIT, 'failure.rs');
const KIND_RS = join(AUDIT, 'kind.rs');
const ERROR_RS = join(AUDIT, 'error.rs');
const EXPORT_RS = join(AUDIT, 'export.rs');
const DESTINATION_RS = join(DESTINATION, 'mod.rs');
const WEBHOOK_RS = join(DESTINATION, 'webhook.rs');
const S3_RS = join(DESTINATION, 's3.rs');
const SYSLOG_RS = join(DESTINATION, 'syslog.rs');

const CODE = /pub const ([A-Z_]+): &str = "([^"]+)";/g;
const FIELD = /^pub\s+([a-z_][a-z_0-9]*)\s*:\s*(.+?),?$/;
const DEFAULTED = /#\[serde\([^)]*\bdefault\b/;
const OPTION = /^Option</;
const ARM = /Self::[A-Za-z0-9]+ => "([^"]+)"/g;
const SECRET_KIND_ARM = /((?:Self::[A-Za-z]+\s*\|?\s*)+)=> Kind::([A-Za-z0-9]+)/g;
const VARIANT = /Self::([A-Za-z]+)/g;
const HEALTH_VARIANT = /Health::([A-Za-z]+)/g;
const TAGGED_SETTINGS =
  /#\[serde\(tag = "kind", rename_all = "lowercase"\)\]\s*pub enum Settings \{([^}]*)\}/;
const WRAPPED_VARIANT = /^([A-Za-z0-9]+)\(([A-Za-z0-9]+)\),?$/;
const DEFAULT_VARIANT = /#\[default\]\s*([A-Za-z]+)/;
const PROCEEDING = /let proceeding = matches!\(health, ([^)]*)\);/;
const ROUTE = '.route(';
const ROUTE_END = /\.layer\(|\.merge\(|\.nest\(|\n\s*\n/;
const PATH_LITERAL = /"([^"]+)"/;
const HANDLER = /\b(get|post|put|patch|delete)\((audit::[a-z_]+::[a-z_]+)\)/g;
const AUDIT_ROUTE = /\/audit-(?:destinations|retention|exports)\b/;
const STATUS_CODE = /StatusCode::([A-Z_]+)/;
const REMOVED = 'removed(';
const KIND_TAG = 'kind';
const ORGANIZATIONS = '/api/organizations/';
const DESTINATIONS = '/audit-destinations/';
const SERVER_ROUTES = '/api/audit-';
const SAMPLE_ORIGIN = 'http://zone.test';
const I32_MAXIMUM = 2 ** 31 - 1;

const STATUS: Record<string, number> = { OK: 200, CREATED: 201, NO_CONTENT: 204 };

const ROUTE_PATHS = [
  '/audit-destinations',
  '/audit-destinations/{destination_id}',
  '/audit-destinations/{destination_id}/tests',
  '/audit-retention',
  '/audit-exports',
];

const SCOPE_PREFIXES = ['/api', '/api/organizations/{org_id}'];

type Shaped = z.ZodObject<z.ZodRawShape>;

interface Route {
  method: string;
  path: string;
  handler: string;
}

interface Call {
  body: unknown;
  send: (scope: AuditScope) => Promise<unknown>;
}

const DESTINATION_ID = fixtures.FAILING_WEBHOOK.id;

const SCOPES: [string, AuditScope][] = [
  ['an organization', organizationScope(fixtures.ORGANIZATION_ID)],
  ['the server', SERVER_SCOPE],
];

const CALLS: Call[] = [
  {
    body: fixtures.ENTERPRISE_DESTINATIONS,
    send: (scope) => auditApi.destinations(scope),
  },
  {
    body: fixtures.FAILING_WEBHOOK,
    send: (scope) => auditApi.destination(scope, DESTINATION_ID),
  },
  {
    body: fixtures.FAILING_WEBHOOK,
    send: (scope) =>
      auditApi.createDestination(scope, {
        name: fixtures.FAILING_WEBHOOK.name,
        settings: fixtures.FAILING_WEBHOOK.settings,
        secrets: { signing_secret: 'signing-secret' },
      }),
  },
  {
    body: fixtures.FAILING_WEBHOOK,
    send: (scope) => auditApi.updateDestination(scope, DESTINATION_ID, { enabled: false }),
  },
  {
    body: null,
    send: (scope) => auditApi.deleteDestination(scope, DESTINATION_ID),
  },
  {
    body: fixtures.FAILED_TEST,
    send: (scope) => auditApi.testDestination(scope, DESTINATION_ID),
  },
  {
    body: fixtures.WAITING_RETENTION,
    send: (scope) => auditApi.retention(scope),
  },
  {
    body: fixtures.WAITING_RETENTION,
    send: (scope) => auditApi.setRetention(scope, { days: 90 }),
  },
  {
    body: '{}\n',
    send: (scope) => auditApi.download(scope, fixtures.EXPORT_QUERY),
  },
];

const RESPONSES: [string, string, Shaped][] = [
  ['DestinationsResponse', DESTINATIONS_RS, DestinationsResponseSchema],
  ['DestinationResponse', DESTINATIONS_RS, DestinationResponseSchema],
  ['DeliveryResponse', DESTINATIONS_RS, DeliveryResponseSchema],
  ['TestResponse', DESTINATIONS_RS, TestResponseSchema],
  ['RetentionResponse', RETENTION_ROUTE_RS, RetentionResponseSchema],
  ['HolderResponse', RETENTION_ROUTE_RS, HolderResponseSchema],
  ['Standing', STANDING_RS, StandingSchema],
];

const REQUESTS: [string, string, Shaped][] = [
  ['CreateDestinationRequest', DESTINATIONS_RS, CreateDestinationRequestSchema],
  ['UpdateDestinationRequest', DESTINATIONS_RS, UpdateDestinationRequestSchema],
  ['UpdateRetentionRequest', RETENTION_ROUTE_RS, UpdateRetentionRequestSchema],
  ['ExportQuery', EXPORTS_RS, ExportQuerySchema],
];

const SETTINGS: [string, string, Kind, Shaped, Shaped][] = [
  [
    'WebhookSettings',
    WEBHOOK_RS,
    AuditKind.Webhook,
    WebhookSettingsSchema,
    WebhookSettingsRequestSchema,
  ],
  ['S3Settings', S3_RS, AuditKind.S3, S3SettingsSchema, S3SettingsRequestSchema],
  [
    'SyslogSettings',
    SYSLOG_RS,
    AuditKind.Syslog,
    SyslogSettingsSchema,
    SyslogSettingsRequestSchema,
  ],
];

const SECRETS: [string, string, Kind, Shaped][] = [
  ['WebhookSecrets', WEBHOOK_RS, AuditKind.Webhook, WebhookSecretsSchema],
  ['S3Secrets', S3_RS, AuditKind.S3, S3SecretsSchema],
  ['SyslogSecrets', SYSLOG_RS, AuditKind.Syslog, SyslogSecretsSchema],
];

const VOCABULARIES: [string, string, readonly string[]][] = [
  ['Kind', KIND_RS, AUDIT_KINDS],
  ['Health', HEALTH_RS, DELIVERY_HEALTHS],
  ['Pause', STANDING_RS, STREAMING_PAUSES],
  ['Wait', RETENTION_RS, RETENTION_WAITS],
  ['FailureCode', FAILURE_RS, DELIVERY_FAILURES],
  ['Format', EXPORT_RS, EXPORT_FORMATS],
  ['Addressing', S3_RS, S3_ADDRESSINGS],
  ['Protocol', SYSLOG_RS, SYSLOG_PROTOCOLS],
  ['SecretName', DESTINATION_RS, SECRET_NAMES],
];

const read = (path: string): string => readFileSync(path, 'utf8');

const sorted = (values: readonly string[]): string[] => [...values].sort();

const keys = (schema: Shaped): string[] => Object.keys(schema.shape).sort();

const untagged = (names: string[]): string[] => names.filter((name) => name !== KIND_TAG);

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

function rustKeys(path: string, structName: string): string[] {
  return sorted(Object.keys(rustShape(path, structName)));
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

function rustTypes(path: string, structName: string): Map<string, string> {
  return new Map(
    body(path, structName)
      .map((line) => FIELD.exec(line))
      .filter((field): field is RegExpExecArray => field !== null)
      .map((field) => [field[1], field[2]])
  );
}

function rustNullable(path: string, structName: string): string[] {
  return [...rustTypes(path, structName)]
    .filter(([, type]) => OPTION.test(type))
    .map(([name]) => name)
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
    if (defaulted || OPTION.test(field[2])) names.push(field[1]);
    defaulted = false;
  }
  return names.sort();
}

function functionSource(source: string, signature: string, indent = ''): string {
  const start = source.indexOf(signature);
  if (start < 0) throw new Error(`${signature} is not in the Rust source`);
  return source.slice(start, source.indexOf(`\n${indent}}\n`, start));
}

function implementation(path: string, enumName: string): string {
  return functionSource(read(path), `impl ${enumName} {`);
}

function literals(path: string, enumName: string): string[] {
  const asStr = functionSource(implementation(path, enumName), 'fn as_str(', '    ');
  return [...asStr.matchAll(ARM)].map((match) => match[1]);
}

function codes(path: string): Map<string, string> {
  return new Map([...read(path).matchAll(CODE)].map((match) => [match[1], match[2]]));
}

function code(path: string, name: string): string {
  const value = codes(path).get(name);
  if (value === undefined) throw new Error(`${name} is not a code constant in ${path}`);
  return value;
}

function constant(path: string, name: string): string {
  const value = new RegExp(`const ${name}: [^=]+= "?([^";]+)"?;`).exec(read(path))?.[1];
  if (value === undefined) throw new Error(`${name} is not a constant in ${path}`);
  return value;
}

function snakeCase(variant: string): string {
  return variant.replace(/([a-z0-9])([A-Z])/g, '$1_$2').toLowerCase();
}

function wrappedSettings(): [string, string][] {
  const declared = TAGGED_SETTINGS.exec(read(DESTINATION_RS))?.[1];
  if (declared === undefined)
    throw new Error(`Settings is not tagged by kind in ${DESTINATION_RS}`);
  return declared
    .split('\n')
    .map((line) => WRAPPED_VARIANT.exec(line.trim()))
    .filter((variant): variant is RegExpExecArray => variant !== null)
    .map((variant) => [variant[1].toLowerCase(), variant[2]]);
}

function secretKinds(): Map<string, string[]> {
  const kindOf = functionSource(implementation(DESTINATION_RS, 'SecretName'), 'fn kind(', '    ');
  return new Map(
    [...kindOf.matchAll(SECRET_KIND_ARM)].map((arm) => [
      arm[2].toLowerCase(),
      [...arm[1].matchAll(VARIANT)].map((variant) => snakeCase(variant[1])),
    ])
  );
}

function served(): Route[] {
  return read(ROUTES_RS)
    .split(ROUTE)
    .slice(1)
    .map((chunk) => chunk.split(ROUTE_END)[0])
    .flatMap((chunk) => {
      const path = PATH_LITERAL.exec(chunk)?.[1];
      if (path === undefined || !AUDIT_ROUTE.test(path)) return [];
      return [...chunk.matchAll(HANDLER)].map((handler) => ({
        method: handler[1].toUpperCase(),
        path,
        handler: handler[2],
      }));
    });
}

function route(method: string, path: string): Route {
  const found = served().find(
    (candidate) => candidate.method === method && candidate.path === path
  );
  if (!found) throw new Error(`the server serves no ${method} ${path}`);
  return found;
}

function status(handler: string): number {
  const [, module, name] = handler.split('::');
  const source = read(join(AUDIT_ROUTES, `${module}.rs`));
  const handled = functionSource(source, `pub async fn ${name}(`);
  const answered = handled.includes(REMOVED) ? functionSource(source, 'fn removed(') : handled;
  const named = STATUS_CODE.exec(answered)?.[1];
  if (named === undefined) return STATUS.OK;
  const value = STATUS[named];
  if (value === undefined) throw new Error(`${handler} answers with an unmapped ${named}`);
  return value;
}

function template(url: string): string {
  return url
    .slice(API_BASE.length)
    .split('?')[0]
    .replace(`${ORGANIZATIONS}${fixtures.ORGANIZATION_ID}`, `${ORGANIZATIONS}{org_id}`)
    .replace(`${DESTINATIONS}${DESTINATION_ID}`, `${DESTINATIONS}{destination_id}`);
}

function answer(call: Call, handler: string): Response {
  const code = status(handler);
  if (code === STATUS.NO_CONTENT) return new Response(null, { status: code });
  if (typeof call.body === 'string') return new Response(call.body, { status: code });
  return Response.json(call.body, { status: code });
}

function deliveries(): DeliveryResponse[] {
  return Object.values(fixtures).flatMap((value) => {
    const many = DestinationsResponseSchema.safeParse(value);
    if (many.success) return many.data.destinations.map((destination) => destination.delivery);
    const one = DestinationResponseSchema.safeParse(value);
    return one.success ? [one.data.delivery] : [];
  });
}

const fetchSpy = spyOn(globalThis, 'fetch');

afterEach(() => {
  fetchSpy.mockReset();
});

afterAll(() => {
  fetchSpy.mockRestore();
});

describe('the audit settings read as the server serialises them', () => {
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

  test('the vocabulary fields are typed by the enums the vocabularies pin', () => {
    const delivery = rustTypes(DESTINATIONS_RS, 'DeliveryResponse');
    const destination = rustTypes(DESTINATIONS_RS, 'DestinationResponse');

    expect(delivery.get('health')).toBe('Health');
    expect(delivery.get('last_error_code')).toBe('Option<FailureCode>');
    expect(destination.get('kind')).toBe('Kind');
    expect(destination.get('secrets')).toBe('Vec<SecretName>');
    expect(rustTypes(DESTINATIONS_RS, 'TestResponse').get('error_code')).toBe(
      'Option<FailureCode>'
    );
    expect(rustTypes(RETENTION_ROUTE_RS, 'HolderResponse').get('wait')).toBe('Wait');
    expect(rustTypes(STANDING_RS, 'Standing').get('pause')).toBe('Option<Pause>');
  });

  test('settings are tagged by their kind, one variant per kind wrapping its own settings', () => {
    expect(wrappedSettings()).toEqual(SETTINGS.map(([structName, , kind]) => [kind, structName]));
    expect(SettingsSchema.options.map((option) => keys(option))).toEqual(
      SETTINGS.map(([, , , schema]) => keys(schema))
    );
    for (const [, , kind, schema] of SETTINGS) {
      expect(schema.shape[KIND_TAG].safeParse(kind).success).toBe(true);
    }
  });

  test.each(SETTINGS)(
    '%s carries the same fields on both sides beside the kind',
    (structName, path, _, schema) => {
      expect(untagged(keys(schema))).toEqual(rustKeys(path, structName));
      expect(zodOptional(schema)).toEqual([]);
      expect(zodNullable(schema)).toEqual(rustNullable(path, structName));
    }
  );

  test('every fixture schedules a next attempt exactly while delivery goes ahead', () => {
    const proceeding = PROCEEDING.exec(read(DESTINATIONS_RS))?.[1];
    if (proceeding === undefined)
      throw new Error('DeliveryResponse no longer names when it proceeds');
    const healths = [...proceeding.matchAll(HEALTH_VARIANT)].map((match) => snakeCase(match[1]));
    const scheduled = deliveries();

    expect(healths.length).toBeGreaterThan(0);
    expect(scheduled.length).toBeGreaterThan(0);
    for (const delivery of scheduled) {
      expect({ health: delivery.health, scheduled: delivery.next_attempt_at !== null }).toEqual({
        health: delivery.health,
        scheduled: healths.includes(delivery.health),
      });
    }
  });
});

describe('the console sends the audit requests the server reads', () => {
  test.each(REQUESTS)(
    '%s carries exactly the fields the server declares',
    (structName, path, schema) => {
      expect(keys(schema)).toEqual(rustKeys(path, structName));
    }
  );

  test.each(REQUESTS)(
    '%s leaves out only fields the server tolerates missing',
    (structName, path, schema) => {
      const missing = tolerated(path, structName);

      for (const field of zodOptional(schema)) {
        expect(missing).toContain(field);
      }
    }
  );

  test.each(REQUESTS)(
    '%s sends null only where the server reads null',
    (structName, path, schema) => {
      const nullable = rustNullable(path, structName);

      for (const field of zodNullable(schema)) {
        expect(nullable).toContain(field);
      }
    }
  );

  test('a creation may leave out enabled and the secrets, and the server defaults every field', () => {
    expect(zodOptional(CreateDestinationRequestSchema)).toEqual(['enabled', 'secrets']);
    expect(tolerated(DESTINATIONS_RS, 'CreateDestinationRequest')).toEqual(
      keys(CreateDestinationRequestSchema)
    );
  });

  test('an update is sparse on both sides', () => {
    const fields = keys(UpdateDestinationRequestSchema);

    expect(zodOptional(UpdateDestinationRequestSchema)).toEqual(fields);
    expect(tolerated(DESTINATIONS_RS, 'UpdateDestinationRequest')).toEqual(fields);
  });

  test('retention is always sent, as whole days or null to keep events forever', () => {
    expect(zodOptional(UpdateRetentionRequestSchema)).toEqual([]);
    expect(zodNullable(UpdateRetentionRequestSchema)).toEqual(['days']);
    expect(rustNullable(RETENTION_ROUTE_RS, 'UpdateRetentionRequest')).toEqual(['days']);
    expect(rustTypes(RETENTION_ROUTE_RS, 'RetentionResponse').get('days')).toBe('Option<i32>');
    expect(RETENTION_DAYS_MAXIMUM).toBe(I32_MAXIMUM);
  });

  test.each(SETTINGS)(
    '%s requests carry the stored fields and leave out only the defaulted ones',
    (structName, path, _, __, request) => {
      expect(untagged(keys(request))).toEqual(rustKeys(path, structName));
      expect(zodOptional(request)).toEqual(tolerated(path, structName));
      for (const field of zodNullable(request)) {
        expect(rustNullable(path, structName)).toContain(field);
      }
    }
  );

  test.each(SECRETS)(
    '%s are the secrets the server seals, required where it requires them',
    (structName, path, kind, schema) => {
      const fields = structFields(read(path), structName).map((field) => field.name);
      const optional = rustNullable(path, structName);

      expect(keys(schema)).toEqual(sorted(fields));
      expect([...KIND_SECRETS[kind]]).toEqual(fields);
      expect(secretKinds().get(kind)).toEqual(fields);
      expect([...REQUIRED_SECRETS[kind]]).toEqual(
        fields.filter((field) => !optional.includes(field))
      );
    }
  );

  test('the syslog and S3 defaults are the server defaults', () => {
    expect(String(DEFAULT_FACILITY)).toBe(constant(SYSLOG_RS, 'DEFAULT_FACILITY'));
    expect(String(FACILITY_MAXIMUM)).toBe(constant(SYSLOG_RS, 'MAXIMUM_FACILITY'));
    expect(DEFAULT_SYSLOG_HOSTNAME).toBe(constant(SYSLOG_RS, 'DEFAULT_HOSTNAME'));
    expect(DEFAULT_ADDRESSING).toBe(snakeCase(DEFAULT_VARIANT.exec(read(S3_RS))?.[1] ?? ''));
  });
});

describe('the audit vocabularies match the server', () => {
  test.each(VOCABULARIES)('%s', (enumName, path, values) => {
    expect(rustVariants(read(path), enumName)).toEqual([...values]);
    expect(literals(path, enumName)).toEqual([...values]);
  });
});

describe('the audit refusal codes match the server', () => {
  test('license_required is the entitlement code', () => {
    expect(AuditCode.LicenseRequired).toBe(code(ENTITLEMENT_RS, 'LICENSE_REQUIRED'));
  });

  test('an unreadable body is refused with the body code', () => {
    expect(AuditCode.BodyInvalid).toBe(code(REFUSAL_RS, 'BODY_INVALID'));
  });

  test('the console models every code the audit routes send, and no other', () => {
    expect(sorted(AUDIT_ADMIN_CODES)).toEqual(
      sorted([
        code(ENTITLEMENT_RS, 'LICENSE_REQUIRED'),
        code(REFUSAL_RS, 'BODY_INVALID'),
        ...codes(ERROR_RS).values(),
      ])
    );
  });

  test('a refusal carries the error and code the server writes', () => {
    expect(keys(AuditRefusalSchema)).toEqual(rustKeys(COMMON_RS, 'ErrorResponse'));
  });
});

describe('the console calls the audit routes the server serves', () => {
  test('every audit route is served under both scopes', () => {
    const routes = read(ROUTES_RS);

    for (const prefix of SCOPE_PREFIXES) {
      for (const path of ROUTE_PATHS) {
        expect(routes).toContain(`"${prefix}${path}"`);
      }
    }
  });

  test('a destination is created with 201 and deleted with 204', () => {
    for (const prefix of SCOPE_PREFIXES) {
      expect(status(route('POST', `${prefix}/audit-destinations`).handler)).toBe(STATUS.CREATED);
      expect(status(route('DELETE', `${prefix}/audit-destinations/{destination_id}`).handler)).toBe(
        STATUS.NO_CONTENT
      );
    }
  });

  test.each(SCOPES)('the console calls exactly the routes %s serves', async (_, scope) => {
    const called: string[] = [];
    fetchSpy.mockImplementation(async (input, init) => {
      const method = init?.method ?? 'GET';
      const path = template(String(input));
      const call = CALLS[called.length];
      called.push(`${method} ${path}`);
      return answer(call, route(method, path).handler);
    });

    for (const call of CALLS) {
      await call.send(scope);
    }

    const prefix = scope.kind === AuditScopeKind.Server ? SERVER_ROUTES : ORGANIZATIONS;
    expect(sorted(called)).toEqual(
      sorted(
        served()
          .filter((candidate) => candidate.path.startsWith(prefix))
          .map((candidate) => `${candidate.method} ${candidate.path}`)
      )
    );
  });

  test('a download asks with the query fields the server reads', async () => {
    fetchSpy.mockImplementation(async () => new Response('{}\n'));

    await auditApi.download(SERVER_SCOPE, fixtures.EXPORT_QUERY);

    const [url] = fetchSpy.mock.calls[0];
    expect(sorted([...new URL(String(url), SAMPLE_ORIGIN).searchParams.keys()])).toEqual(
      rustKeys(EXPORTS_RS, 'ExportQuery')
    );
  });
});
