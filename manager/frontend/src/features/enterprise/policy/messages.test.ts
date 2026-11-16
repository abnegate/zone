import { describe, expect, it } from 'bun:test';
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

describe('labels', () => {
  it('labels providers as the server does', () => {
    expect(POLICY_PROVIDERS.map(providerLabel)).toEqual([
      'Local models',
      'Anthropic API',
      'OpenAI API',
      'AWS Bedrock',
      'Google Vertex AI',
      'Azure OpenAI',
      'Other OpenAI-compatible endpoints',
    ]);
  });

  it('labels coding agents as the server does', () => {
    expect(POLICY_AGENTS.map(agentLabel)).toEqual(['Claude Code', 'Codex']);
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
  it('matches the server wording for every denial', () => {
    expect(denialMessage(PolicyCode.ProviderDenied, 'OpenAI API')).toBe(
      "Your organization's policy doesn't allow OpenAI API in this workspace."
    );
    expect(denialMessage(PolicyCode.AgentDenied, 'Codex')).toBe(
      "Your organization's policy doesn't allow Codex in this workspace."
    );
    expect(denialMessage(PolicyCode.ModelDenied)).toBe(
      "Your organization's policy doesn't allow this model in this workspace. Choose an allowed model."
    );
    expect(denialMessage(PolicyCode.ServerDenied)).toBe(
      "Your organization's policy doesn't allow this MCP server in this workspace."
    );
    expect(denialMessage(PolicyCode.DomainDenied)).toBe(
      "Your organization's policy doesn't allow requests to this domain from this workspace."
    );
    expect(denialMessage(PolicyCode.ApprovalRequired)).toBe(
      "Your organization's policy requires a person to approve this tool call, and nobody can approve it in this run."
    );
    expect(denialMessage(PolicyCode.Unreadable)).toBe(
      "Your organization's agent policy couldn't be read, so this can't start. Try again shortly."
    );
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

  it('names Team and Enterprise when a license is required', () => {
    expect(refusalMessage(PolicyCode.LicenseRequired)).toContain('Team or Enterprise');
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
