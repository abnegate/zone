import { describe, expect, it } from 'bun:test';
import type { Citation } from '../types';
import {
  citationEvidence,
  citationEvidenceLabel,
  citationHref,
  formatRevision,
  mergeCitations,
} from './citations';

const citation = (overrides: Partial<Citation> = {}): Citation => ({
  kind: 'github_build',
  title: 'repository main@aaaaaaa',
  url: 'https://github.com/owner/repository/commit/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  revision: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  observed_at: '2026-09-05T00:00:00+00:00',
  complete: true,
  outcome: 'success',
  ...overrides,
});

describe('citation evidence', () => {
  it('treats only a complete success as a passing result', () => {
    expect(citationEvidence(citation())).toBe('passing');
    expect(citationEvidenceLabel('passing')).toBe('Passing');
  });

  it('never presents incomplete or unknown evidence as passing', () => {
    expect(citationEvidence(citation({ complete: false, outcome: 'success' }))).toBe('incomplete');
    expect(citationEvidence(citation({ complete: false, outcome: 'incomplete' }))).toBe(
      'incomplete'
    );
    expect(citationEvidence(citation({ complete: true, outcome: 'incomplete' }))).toBe(
      'incomplete'
    );
    expect(citationEvidenceLabel('incomplete')).toBe('Incomplete evidence');
  });

  it('keeps pending and failed complete observations distinct from a pass', () => {
    expect(citationEvidence(citation({ outcome: 'pending' }))).toBe('pending');
    expect(citationEvidence(citation({ outcome: 'failure' }))).toBe('failed');
    expect(citationEvidence(citation({ kind: 'workspace_document', outcome: 'observed' }))).toBe(
      'observed'
    );
  });
});

describe('citation presentation', () => {
  it('makes http and wiki sources clickable and shortens commit SHAs', () => {
    expect(citationHref({ kind: 'github_build', url: 'https://github.com/owner/repository' })).toBe(
      'https://github.com/owner/repository'
    );
    expect(
      citationHref({
        kind: 'workspace_document',
        url: 'knowledge://11111111-1111-1111-1111-111111111111',
      })
    ).toBe('/wiki');
    expect(citationHref({ kind: 'workspace_document', url: 'src/guide.md' })).toBe('/wiki');
    expect(citationHref({ kind: 'github_file', url: 'src/guide.md' })).toBeNull();
    expect(formatRevision('aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa')).toBe('aaaaaaa');
    expect(formatRevision('content-hash')).toBe('content-hash');
  });

  it('deduplicates streamed citations by url and revision', () => {
    const first = citation();
    expect(mergeCitations([first], [first, citation({ title: 'duplicate' })])).toEqual([first]);
    expect(mergeCitations([first], [citation({ revision: 'bbbb' })])).toHaveLength(2);
  });
});
