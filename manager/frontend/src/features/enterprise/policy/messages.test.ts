import { describe, expect, it } from 'bun:test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import {
  agentLabel,
  ceilingLabel,
  denialMessage,
  denialReason,
  fieldDescription,
  fieldLabel,
  isAdminCode,
  isDenialCode,
  NO_CEILING_LABEL,
  notAllowed,
  pendingText,
  providerLabel,
  refusalField,
  refusalMessage,
  unrestrictedText,
} from './messages';
import {
  POLICY_ADMIN_CODES,
  POLICY_AGENTS,
  POLICY_DENIAL_CODES,
  POLICY_FIELDS,
  POLICY_PROVIDERS,
  PolicyCode,
  PolicyField,
} from './schemas';
import type { PolicyDenialCode } from './types';

const POLICY_RS = join(import.meta.dir, '../../../../../../runner/zone_enterprise/src/policy');
const PROVIDER_RS = join(POLICY_RS, 'provider.rs');
const AGENT_RS = join(POLICY_RS, 'agent.rs');
const DENIAL_RS = join(POLICY_RS, 'denial.rs');

const ARM = /Self::([A-Za-z]+)(?:\(_\))? => (?:"([^"]+)"|([A-Z_]+))/g;
const ERROR = /#\[error\(\s*"((?:[^"\\]|\\.)*)"[^\]]*\]\s*([A-Za-z]+)/g;
const CODE = /pub const ([A-Z_]+): &str = "([^"]+)";/g;
const PLACEHOLDER = '{}';

const read = (path: string): string => readFileSync(path, 'utf8');

function method(source: string, name: string): string {
  const start = source.indexOf(`fn ${name}(`);
  if (start < 0) throw new Error(`fn ${name} not found in the Rust source`);
  return source.slice(start, source.indexOf('\n    }\n', start));
}

function arms(source: string, name: string): Map<string, string> {
  return new Map(
    [...method(source, name).matchAll(ARM)].map((match) => [match[1], match[2] ?? match[3]])
  );
}

function labels(path: string): Map<string, string> {
  const source = read(path);
  const values = arms(source, 'as_str');
  const words = arms(source, 'label');
  return new Map([...values].map(([variant, value]) => [value, words.get(variant) ?? '']));
}

interface RustDenial {
  code: PolicyDenialCode;
  template: string;
}

function denials(): Map<string, RustDenial> {
  const source = read(DENIAL_RS);
  const constants = new Map([...source.matchAll(CODE)].map((match) => [match[1], match[2]]));
  const codes = arms(source, 'code');
  return new Map(
    [...source.matchAll(ERROR)].map((match) => {
      const code = constants.get(codes.get(match[2]) ?? '');
      if (code === undefined || !isDenialCode(code)) {
        throw new Error(`Denial::${match[2]} has no denial code`);
      }
      return [match[2], { code, template: match[1] }];
    })
  );
}

function denial(variant: string): RustDenial {
  const found = denials().get(variant);
  if (found === undefined) throw new Error(`Denial::${variant} not found in ${DENIAL_RS}`);
  return found;
}

describe('labels', () => {
  it('labels providers as the server does', () => {
    const server = labels(PROVIDER_RS);

    expect(POLICY_PROVIDERS.map(providerLabel)).toEqual(
      POLICY_PROVIDERS.map((provider) => server.get(provider) ?? '')
    );
  });

  it('labels coding agents as the server does', () => {
    const server = labels(AGENT_RS);

    expect(POLICY_AGENTS.map(agentLabel)).toEqual(
      POLICY_AGENTS.map((agent) => server.get(agent) ?? '')
    );
  });

  it('describes the ceilings the editor offers', () => {
    expect(ceilingLabel(null)).toBe(NO_CEILING_LABEL);
    expect(ceilingLabel('write')).toBe('Ask before host tools such as shell and file writes');
    expect(ceilingLabel('host')).toBe(
      'Ask before outward tools such as messages, pull requests, web and MCP'
    );
  });

  it('labels, describes and explains an unrestricted value for every field', () => {
    for (const field of POLICY_FIELDS) {
      expect(fieldLabel(field).length, field).toBeGreaterThan(0);
      expect(fieldDescription(field), field).toEndWith('.');
      expect(unrestrictedText(field), field).toEndWith('.');
    }
  });
});

describe('denialMessage', () => {
  it('words every denial the server declares', () => {
    expect([...denials().values()].map((rust) => rust.code).sort()).toEqual(
      [...POLICY_DENIAL_CODES].sort()
    );
  });

  it('names a denied provider the way the server does', () => {
    const { code, template } = denial('Provider');

    for (const provider of POLICY_PROVIDERS) {
      const label = providerLabel(provider);
      expect(denialMessage(code, label)).toBe(template.replace(PLACEHOLDER, label));
    }
  });

  it('names a denied coding agent the way the server does', () => {
    const { code, template } = denial('Agent');

    for (const agent of POLICY_AGENTS) {
      const label = agentLabel(agent);
      expect(denialMessage(code, label)).toBe(template.replace(PLACEHOLDER, label));
    }
  });

  it('matches the server wording for every other denial', () => {
    for (const variant of ['Model', 'Server', 'Domain', 'Approval', 'Unreadable']) {
      const { code, template } = denial(variant);
      expect(denialMessage(code), variant).toBe(template);
    }
  });

  it('names the provider or agent generically without a label', () => {
    expect(denialMessage(PolicyCode.ProviderDenied)).toBe(
      "Your organization's policy doesn't allow this provider in this workspace."
    );
    expect(denialMessage(PolicyCode.AgentDenied)).toBe(
      "Your organization's policy doesn't allow this coding agent in this workspace."
    );
  });

  it('recognises only denial codes', () => {
    for (const code of POLICY_DENIAL_CODES) {
      expect(isDenialCode(code), code).toBe(true);
      expect(isAdminCode(code), code).toBe(false);
    }
  });
});

describe('refusalMessage', () => {
  for (const code of POLICY_ADMIN_CODES) {
    it(`explains ${code} in a sentence`, () => {
      expect(refusalMessage(code)).toEndWith('.');
      expect(isAdminCode(code)).toBe(true);
    });
  }

  it('gives every code its own message', () => {
    expect(new Set(POLICY_ADMIN_CODES.map(refusalMessage)).size).toBe(POLICY_ADMIN_CODES.length);
  });

  it('names Zone Enterprise when a license is required', () => {
    expect(refusalMessage(PolicyCode.LicenseRequired)).toContain('Zone Enterprise license');
  });
});

describe('entry messages', () => {
  it('names the entries the organization does not allow', () => {
    expect(notAllowed(['gitlab', 'jira'])).toBe('Not allowed by your organization: gitlab, jira.');
  });

  it('names the entries a save adds', () => {
    expect(pendingText(['github'])).toBe('Saving adds github.');
  });
});

describe('refusalField', () => {
  it('puts the model, domain, server and ceiling refusals on their field', () => {
    expect(refusalField(PolicyCode.ModelInvalid)).toBe(PolicyField.Models);
    expect(refusalField(PolicyCode.DomainInvalid)).toBe(PolicyField.Domains);
    expect(refusalField(PolicyCode.ServerInvalid)).toBe(PolicyField.McpServers);
    expect(refusalField(PolicyCode.CeilingInvalid)).toBe(PolicyField.Ceiling);
  });

  it('leaves every other refusal to the form', () => {
    for (const code of [PolicyCode.LicenseRequired, PolicyCode.Wider, PolicyCode.BodyInvalid]) {
      expect(refusalField(code), code).toBeNull();
    }
  });
});

describe('denialReason', () => {
  it('words a denial code the way the console words it', () => {
    expect(denialReason(PolicyCode.ProviderDenied, 'Anthropic')).toBe(
      "Your organization's policy doesn't allow Anthropic in this workspace."
    );
    expect(denialReason(PolicyCode.ModelDenied)).toBe(denialMessage(PolicyCode.ModelDenied));
  });

  it('shows a reason the server already worded as it is', () => {
    const worded = "Your organization's policy doesn't allow AWS Bedrock in this workspace.";

    expect(denialReason(worded, 'Bedrock')).toBe(worded);
  });
});
