import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import type { z } from 'zod';

import { rustVariants, structFields } from '../../../test/rust';
import type { DeniedMessage } from '../../chats/hooks/useChat';
import { ChatProviderOfferSchema } from '../../chats/schemas';
import { InstalledModelSchema } from '../../models/schemas';
import { AgentStatusSchema } from '../../settings/ai/schemas';
import { EDITIONS, LICENSE_STATUSES } from '../schemas';
import {
  OrganizationPolicyResponseSchema,
  POLICY_ADMIN_CODES,
  POLICY_AGENTS,
  POLICY_CEILINGS,
  POLICY_DENIAL_CODES,
  POLICY_FIELDS,
  POLICY_PROVIDERS,
  POLICY_TIERS,
  PolicyCode,
  PolicyRefusalSchema,
  PolicyRulesSchema,
  UpdatePolicyRequestSchema,
  WorkspacePolicyResponseSchema,
} from './schemas';

const RUNNER = join(import.meta.dir, '../../../../../../runner');
const SERVER = join(RUNNER, 'zone_server/src');
const ENTERPRISE = join(RUNNER, 'zone_enterprise/src');
const POLICY = join(ENTERPRISE, 'policy');
const ORGANIZATION_RS = join(SERVER, 'routes/policy/organization.rs');
const WORKSPACE_RS = join(SERVER, 'routes/policy/workspace.rs');
const ROUTES_RS = join(SERVER, 'routes/mod.rs');
const COMMON_RS = join(SERVER, 'routes/common.rs');
const ENTITLEMENT_RS = join(ENTERPRISE, 'entitlement.rs');
const EDITION_RS = join(ENTERPRISE, 'edition.rs');
const STATUS_RS = join(ENTERPRISE, 'status.rs');
const RULES_RS = join(POLICY, 'rules.rs');
const PATCH_RS = join(POLICY, 'patch.rs');
const PROVIDER_RS = join(POLICY, 'provider.rs');
const AGENT_RS = join(POLICY, 'agent.rs');
const TIER_RS = join(POLICY, 'tier.rs');
const FIELD_RS = join(POLICY, 'field.rs');
const DENIAL_RS = join(POLICY, 'denial.rs');
const ERROR_RS = join(POLICY, 'error.rs');
const OFFER_RS = join(SERVER, 'services/provider/offer.rs');
const MODELS_RS = join(SERVER, 'routes/models/types.rs');
const LOGIN_STATUS_RS = join(SERVER, 'services/login/status.rs');
const CHAT_WS_RS = join(SERVER, 'ws/chat.rs');

const CODE = /pub const ([A-Z_]+): &str = "([^"]+)";/g;
const FIELD = /^pub\s+([a-z_][a-z_0-9]*)\s*:\s*(.+?),?$/;
const OPTION = /^Option</;
const DEFAULTED = /#\[serde\([^)]*\bdefault\b/;
const DEFAULTED_RULES = /#\[serde\(default\)\]\s*pub struct Rules\b/;
const ARM = /Self::[A-Za-z]+ => "([^"]+)"/g;
const CEILINGS = /pub const CEILINGS: \[Self; \d+\] = \[([^\]]*)\]/;
const SELF = 'Self::';
const DENIAL_FIELD = 'denial';
const DENIED_VARIANT = 'Denied';
const TAGGED_MESSAGES =
  /#\[serde\(tag = "type", rename_all = "snake_case"\)\]\s*pub enum ServerMessage \{/;

const RESPONSES: [string, string, Shaped][] = [
  ['OrganizationResponse', ORGANIZATION_RS, OrganizationPolicyResponseSchema],
  ['WorkspaceResponse', WORKSPACE_RS, WorkspacePolicyResponseSchema],
  ['Rules', RULES_RS, PolicyRulesSchema],
];

type Shaped = z.ZodObject<z.ZodRawShape>;

const DENIED_BY: [string, string, Shaped][] = [
  ['Offer', OFFER_RS, ChatProviderOfferSchema],
  ['ModelResponse', MODELS_RS, InstalledModelSchema],
  ['AgentStatus', LOGIN_STATUS_RS, AgentStatusSchema],
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

function literals(path: string, enumName: string): string[] {
  const source = read(path);
  const implementation = source.indexOf(`impl ${enumName} {`);
  if (implementation < 0) throw new Error(`${enumName} has no impl block in ${path}`);
  const method = source.indexOf('fn as_str', implementation);
  if (method < 0) throw new Error(`${enumName} has no as_str in ${path}`);
  const end = source.indexOf('\n    }\n', method);
  return [...source.slice(method, end).matchAll(ARM)].map((match) => match[1]);
}

function ceilings(): string[] {
  const declared = CEILINGS.exec(read(TIER_RS))?.[1];
  if (declared === undefined) throw new Error(`Tier::CEILINGS is not declared in ${TIER_RS}`);
  return declared
    .split(',')
    .map((variant) => variant.trim().replace(SELF, '').toLowerCase())
    .filter((variant) => variant.length > 0);
}

function variantFields(path: string, enumName: string, variant: string): string[] {
  const source = read(path);
  const declared = source.indexOf(`pub enum ${enumName} {`);
  if (declared < 0) throw new Error(`${enumName} is not an enum in ${path}`);
  const fields = new RegExp(`\\n\\s*${variant} \\{([^}]*)\\}`).exec(source.slice(declared));
  if (!fields) throw new Error(`${enumName}::${variant} has no fields in ${path}`);
  return fields[1]
    .split(',')
    .map((field) => field.split(':')[0].trim())
    .filter((field) => field.length > 0)
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

describe('the agent policy reads as the server serialises it', () => {
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

  test('every rule may be null, which leaves the field open at that layer', () => {
    expect(zodNullable(PolicyRulesSchema)).toEqual(keys(PolicyRulesSchema));
    expect(read(RULES_RS)).toMatch(DEFAULTED_RULES);
  });

  test('a policy response carries the editions and statuses the license reads', () => {
    expect(sorted(EDITIONS)).toEqual(sorted(rustVariants(read(EDITION_RS), 'Edition')));
    expect(sorted(LICENSE_STATUSES)).toEqual(sorted(rustVariants(read(STATUS_RS), 'Status')));
  });
});

describe('the console sends the agent policy patch the server reads', () => {
  test('a patch carries exactly the fields the server declares', () => {
    expect(keys(UpdatePolicyRequestSchema)).toEqual(
      sorted(Object.keys(rustShape(PATCH_RS, 'Patch')))
    );
  });

  test('a patch is sparse on both sides', () => {
    const fields = keys(UpdatePolicyRequestSchema);

    expect(zodOptional(UpdatePolicyRequestSchema)).toEqual(fields);
    expect(tolerated(PATCH_RS, 'Patch')).toEqual(fields);
  });

  test('a patch clears a field with null', () => {
    expect(zodNullable(UpdatePolicyRequestSchema)).toEqual(keys(UpdatePolicyRequestSchema));
  });

  test('a patch names the fields the rules hold', () => {
    expect(keys(UpdatePolicyRequestSchema)).toEqual(keys(PolicyRulesSchema));
  });

  test('the routes the console calls are the ones the server serves', () => {
    const routes = read(ROUTES_RS);

    for (const path of [
      '"/api/organizations/{org_id}/agent-policy"',
      '"/api/organizations/{org_id}/workspaces/{ws_id}/agent-policy"',
    ]) {
      expect(routes).toContain(path);
    }
  });
});

describe('the agent policy vocabularies match the server', () => {
  test('providers are the wire values as_str writes', () => {
    expect(literals(PROVIDER_RS, 'Provider')).toEqual([...POLICY_PROVIDERS]);
  });

  test('agents', () => {
    expect(rustVariants(read(AGENT_RS), 'Agent')).toEqual([...POLICY_AGENTS]);
    expect(literals(AGENT_RS, 'Agent')).toEqual([...POLICY_AGENTS]);
  });

  test('tiers', () => {
    expect(rustVariants(read(TIER_RS), 'Tier')).toEqual([...POLICY_TIERS]);
    expect(literals(TIER_RS, 'Tier')).toEqual([...POLICY_TIERS]);
  });

  test('the editor offers as a ceiling only the tiers the server accepts as one', () => {
    expect(ceilings()).toEqual([...POLICY_CEILINGS]);
  });

  test('fields', () => {
    expect(rustVariants(read(FIELD_RS), 'Field')).toEqual([...POLICY_FIELDS]);
  });
});

describe('the agent policy codes match the server', () => {
  test('license_required is the entitlement code', () => {
    expect(PolicyCode.LicenseRequired).toBe(code(ENTITLEMENT_RS, 'LICENSE_REQUIRED'));
  });

  test('the console models every denial code, and no other', () => {
    expect(sorted(POLICY_DENIAL_CODES)).toEqual(sorted([...codes(DENIAL_RS).values()]));
  });

  test('the console models every code the admin routes send, and no other', () => {
    expect(sorted(POLICY_ADMIN_CODES)).toEqual(
      sorted([code(ENTITLEMENT_RS, 'LICENSE_REQUIRED'), ...codes(ERROR_RS).values()])
    );
  });

  test('an unreadable body is refused with the body code', () => {
    expect(PolicyCode.BodyInvalid).toBe(code(ERROR_RS, 'BODY_INVALID'));
  });

  test('a refusal carries the error and code the server writes', () => {
    expect(keys(PolicyRefusalSchema)).toEqual(
      sorted(Object.keys(rustShape(COMMON_RS, 'ErrorResponse')))
    );
  });
});

describe('a policy denial reaches the console under the name the server sends it', () => {
  test.each(DENIED_BY)('%s carries an optional denial', (structName, path, schema) => {
    expect(rustShape(path, structName)[DENIAL_FIELD]).toBe(true);
    expect(zodShape(schema)[DENIAL_FIELD]).toBe(true);
    expect(zodNullable(schema)).toContain(DENIAL_FIELD);
  });

  test('a denied turn arrives as a denied frame with its message and code', () => {
    const frame: DeniedMessage = { type: 'denied', message: '', code: '' };

    expect(frame.type).toBe(DENIED_VARIANT.toLowerCase());
    expect(read(CHAT_WS_RS)).toMatch(TAGGED_MESSAGES);
    expect(variantFields(CHAT_WS_RS, 'ServerMessage', DENIED_VARIANT)).toEqual(
      sorted(Object.keys(frame).filter((key) => key !== 'type'))
    );
  });
});
