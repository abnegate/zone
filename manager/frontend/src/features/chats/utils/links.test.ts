import { describe, expect, it } from 'bun:test';
import type { Citation } from '../types';
import { canonicalUrl, resolvesToCitation } from './links';

const citation = (overrides: Partial<Citation> = {}): Citation => ({
  kind: 'github_file',
  title: 'owner/repository src/main.rs',
  url: 'https://github.com/owner/repository/blob/main/src/Main.rs?plain=1',
  revision: null,
  observed_at: '2026-09-05T00:00:00.000Z',
  complete: true,
  outcome: 'observed',
  provenance: 'server_execution',
  ...overrides,
});

const sources = [citation()];

describe('canonicalUrl', () => {
  it('lowercases scheme and host while preserving path and query case', () => {
    expect(canonicalUrl('HTTPS://GitHub.COM/Owner/Repository?Plain=1')).toBe(
      'https://github.com/Owner/Repository?Plain=1'
    );
  });

  it('drops the fragment and a trailing slash on an empty path', () => {
    expect(canonicalUrl('https://example.com/docs#install')).toBe('https://example.com/docs');
    expect(canonicalUrl('https://example.com/')).toBe('https://example.com');
    expect(canonicalUrl('https://example.com')).toBe('https://example.com');
    expect(canonicalUrl('https://example.com/#top')).toBe('https://example.com');
  });

  it('refuses anything that is not an absolute http or https URL', () => {
    expect(canonicalUrl('')).toBeNull();
    expect(canonicalUrl(undefined)).toBeNull();
    expect(canonicalUrl(null)).toBeNull();
    expect(canonicalUrl('javascript:alert(1)')).toBeNull();
    expect(canonicalUrl('//evil.com/x')).toBeNull();
    expect(canonicalUrl('/wiki/page')).toBeNull();
    expect(canonicalUrl('docs/guide.md')).toBeNull();
    expect(canonicalUrl('mailto:someone@example.com')).toBeNull();
    expect(canonicalUrl('knowledge://11111111-1111-1111-1111-111111111111')).toBeNull();
  });
});

describe('resolvesToCitation', () => {
  it('resolves an href that matches a cited URL exactly', () => {
    expect(
      resolvesToCitation(
        'https://github.com/owner/repository/blob/main/src/Main.rs?plain=1',
        sources
      )
    ).toBe(true);
  });

  it('resolves a host that differs only in case', () => {
    expect(
      resolvesToCitation(
        'https://GitHub.com/owner/repository/blob/main/src/Main.rs?plain=1',
        sources
      )
    ).toBe(true);
  });

  it('resolves across a trailing slash and a fragment-only difference', () => {
    const cited = [citation({ url: 'https://example.com' })];
    expect(resolvesToCitation('https://example.com/', cited)).toBe(true);
    expect(resolvesToCitation('https://example.com/#section', cited)).toBe(true);
    expect(
      resolvesToCitation(
        'https://github.com/owner/repository/blob/main/src/Main.rs?plain=1#L42',
        sources
      )
    ).toBe(true);
  });

  it('does not resolve a differing query', () => {
    expect(
      resolvesToCitation(
        'https://github.com/owner/repository/blob/main/src/Main.rs?plain=0',
        sources
      )
    ).toBe(false);
    expect(
      resolvesToCitation('https://github.com/owner/repository/blob/main/src/Main.rs', sources)
    ).toBe(false);
  });

  it('does not resolve a path that differs only in case', () => {
    expect(
      resolvesToCitation(
        'https://github.com/owner/repository/blob/main/src/main.rs?plain=1',
        sources
      )
    ).toBe(false);
  });

  it('never resolves an empty, scriptable, protocol-relative or relative href', () => {
    const cited = [citation({ url: 'https://evil.com/x' })];
    expect(resolvesToCitation('', sources)).toBe(false);
    expect(resolvesToCitation(undefined, sources)).toBe(false);
    expect(resolvesToCitation('javascript:alert(1)', sources)).toBe(false);
    expect(resolvesToCitation('//evil.com/x', cited)).toBe(false);
    expect(resolvesToCitation('/wiki/page', sources)).toBe(false);
  });

  it('never resolves against a citation that is not itself an http URL', () => {
    const cited = [citation({ kind: 'workspace_document', url: 'knowledge://11111111' })];
    expect(resolvesToCitation('https://example.com/a', cited)).toBe(false);
  });

  it('does not resolve when the reply cites nothing', () => {
    expect(resolvesToCitation('https://example.com/a', [])).toBe(false);
  });
});
