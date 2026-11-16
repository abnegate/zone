import { describe, expect, it } from 'bun:test';
import {
  allows,
  ceilingAllowed,
  changed,
  OPEN_RULES,
  parseEntries,
  patchFrom,
  toggled,
  withEntries,
  withoutEntry,
} from './draft';
import { ORGANIZATION_RULES } from './fixtures';
import { PolicyField, PolicyProvider, PolicyTier } from './schemas';

describe('patchFrom', () => {
  it('sends nothing when nothing changed', () => {
    const request = patchFrom(ORGANIZATION_RULES, { ...ORGANIZATION_RULES });
    expect(request).toEqual({});
    expect(changed(request)).toBe(false);
  });

  it('sends only the keys that changed', () => {
    const request = patchFrom(ORGANIZATION_RULES, {
      ...ORGANIZATION_RULES,
      models: ['claude-*'],
      ceiling: PolicyTier.Write,
    });
    expect(request).toEqual({ models: ['claude-*'], ceiling: PolicyTier.Write });
    expect(changed(request)).toBe(true);
  });

  it('sends null to stop restricting a field', () => {
    expect(patchFrom(ORGANIZATION_RULES, { ...ORGANIZATION_RULES, domains: null })).toEqual({
      domains: null,
    });
    expect(patchFrom(ORGANIZATION_RULES, { ...ORGANIZATION_RULES, ceiling: null })).toEqual({
      ceiling: null,
    });
  });

  it('tells an empty allowlist apart from no restriction', () => {
    expect(patchFrom(OPEN_RULES, { ...OPEN_RULES, mcp_servers: [] })).toEqual({ mcp_servers: [] });
    expect(patchFrom({ ...OPEN_RULES, mcp_servers: [] }, OPEN_RULES)).toEqual({
      mcp_servers: null,
    });
  });

  it('ignores the order of entries', () => {
    expect(
      patchFrom(ORGANIZATION_RULES, {
        ...ORGANIZATION_RULES,
        providers: [PolicyProvider.Bedrock, PolicyProvider.Local, PolicyProvider.Anthropic],
      })
    ).toEqual({});
  });
});

describe('parseEntries', () => {
  it('splits on commas and spaces, trims and drops repeats', () => {
    expect(parseEntries(' claude-*, gpt-5*  claude-* ,,', PolicyField.Models)).toEqual([
      'claude-*',
      'gpt-5*',
    ]);
  });

  it('lowercases domains only', () => {
    expect(parseEntries('GitHub.com', PolicyField.Domains)).toEqual(['github.com']);
    expect(parseEntries('Linear', PolicyField.McpServers)).toEqual(['Linear']);
  });
});

describe('list edits', () => {
  it('adds entries once, removes one and toggles one', () => {
    expect(withEntries(['a'], ['b', 'a'])).toEqual(['a', 'b']);
    expect(withoutEntry(['a', 'b'], 'a')).toEqual(['b']);
    expect(toggled(['a'], 'b', true)).toEqual(['a', 'b']);
    expect(toggled(['a', 'b'], 'a', false)).toEqual(['b']);
  });

  it('allows anything under no limit and only listed entries under one', () => {
    expect(allows(null, 'openai')).toBe(true);
    expect(allows(['local'], 'local')).toBe(true);
    expect(allows(['local'], 'openai')).toBe(false);
  });
});

describe('ceilingAllowed', () => {
  it('allows any ceiling under no organization limit', () => {
    expect(ceilingAllowed(null, null)).toBe(true);
    expect(ceilingAllowed(null, PolicyTier.Host)).toBe(true);
  });

  it('allows only a ceiling no higher than the organization', () => {
    expect(ceilingAllowed(PolicyTier.Write, PolicyTier.Write)).toBe(true);
    expect(ceilingAllowed(PolicyTier.Write, PolicyTier.Host)).toBe(false);
    expect(ceilingAllowed(PolicyTier.Host, PolicyTier.Write)).toBe(true);
    expect(ceilingAllowed(PolicyTier.Host, null)).toBe(false);
  });
});
