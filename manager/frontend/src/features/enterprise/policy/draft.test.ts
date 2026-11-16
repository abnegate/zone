import { describe, expect, it } from 'bun:test';
import {
  allows,
  ceilingAllowed,
  changed,
  covers,
  OPEN_RULES,
  parseEntries,
  patchFrom,
  toggled,
  typed,
  uncovered,
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

describe('covers', () => {
  it('lets a model prefix cover every name and narrower prefix it starts', () => {
    expect(covers(PolicyField.Models, 'claude-*', 'claude-opus')).toBe(true);
    expect(covers(PolicyField.Models, 'claude-*', 'claude-opus-*')).toBe(true);
    expect(covers(PolicyField.Models, '*', 'gpt-*')).toBe(true);
    expect(covers(PolicyField.Models, 'claude-opus', 'claude-*')).toBe(false);
    expect(covers(PolicyField.Models, 'sonnet', 'sonnet-4')).toBe(false);
  });

  it('lets a domain cover its subdomains but never an IP literal', () => {
    expect(covers(PolicyField.Domains, 'example.com', 'api.example.com')).toBe(true);
    expect(covers(PolicyField.Domains, 'example.com', 'badexample.com')).toBe(false);
    expect(covers(PolicyField.Domains, '0.0.1', '10.0.0.1')).toBe(false);
    expect(covers(PolicyField.Domains, '10.0.0.1', '10.0.0.1')).toBe(true);
  });

  it('matches MCP servers by name', () => {
    expect(covers(PolicyField.McpServers, 'github', 'github')).toBe(true);
    expect(covers(PolicyField.McpServers, 'github', 'github-enterprise')).toBe(false);
  });
});

describe('uncovered', () => {
  it('names the entries no organization entry covers', () => {
    expect(uncovered(PolicyField.Domains, ['github.com'], ['api.github.com', 'evil.com'])).toEqual([
      'evil.com',
    ]);
  });

  it('covers everything while the organization leaves the field open', () => {
    expect(uncovered(PolicyField.Models, null, ['anything'])).toEqual([]);
  });
});

describe('parseEntries for domains', () => {
  it('lowercases and drops a leading wildcard and trailing dot, as the server does', () => {
    expect(parseEntries('*.Example.com. GitHub.com', PolicyField.Domains)).toEqual([
      'example.com',
      'github.com',
    ]);
  });
});

describe('typed', () => {
  it('is true only while an entry field holds text', () => {
    expect(typed({})).toBe(false);
    expect(typed({ [PolicyField.Models]: '  ' })).toBe(false);
    expect(typed({ [PolicyField.Domains]: 'github.com' })).toBe(true);
  });
});
