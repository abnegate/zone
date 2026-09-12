import { describe, expect, it } from 'bun:test';
import type { Citation } from '../types';
import {
  citationEvidence,
  citationEvidenceLabel,
  citationHref,
  citationKindLabel,
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
  provenance: 'server_execution',
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
    ).toBe('/wiki?id=11111111-1111-1111-1111-111111111111');
    expect(
      citationHref({
        kind: 'knowledge_passage',
        url: 'knowledge://11111111-1111-1111-1111-111111111111',
      })
    ).toBe('/wiki?id=11111111-1111-1111-1111-111111111111');
    expect(citationHref({ kind: 'knowledge_passage', url: 'knowledge://' })).toBe('/wiki');
    expect(citationHref({ kind: 'knowledge_passage', url: 'notes/guide.md' })).toBe('/wiki');
    expect(citationHref({ kind: 'workspace_document', url: 'src/guide.md' })).toBe('/wiki');
    expect(citationHref({ kind: 'github_file', url: 'src/guide.md' })).toBeNull();
    expect(formatRevision('aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa')).toBe('aaaaaaa');
    expect(formatRevision('content-hash')).toBe('content-hash');
  });

  it('sends a knowledge citation to the index when its suffix cannot name an entry', () => {
    expect(citationHref({ kind: 'knowledge_passage', url: 'knowledge://not-an-entry' })).toBe(
      '/wiki'
    );
    expect(
      citationHref({
        kind: 'knowledge_passage',
        url: 'knowledge://11111111-1111-1111-1111-111111111111',
      })
    ).toBe('/wiki?id=11111111-1111-1111-1111-111111111111');
    expect(
      citationHref({
        kind: 'knowledge_passage',
        url: 'knowledge://A1B2C3D4-1111-4111-8111-111111111111',
      })
    ).toBe('/wiki?id=A1B2C3D4-1111-4111-8111-111111111111');
  });

  it('refuses a protocol-relative url that would navigate off-site', () => {
    expect(citationHref({ kind: 'github_file', url: '//evil.example/x' })).toBeNull();
    expect(citationHref({ kind: 'github_file', url: '/\\evil.example/x' })).toBeNull();
    expect(citationHref({ kind: 'github_file', url: '/wiki/page' })).toBe('/wiki/page');
    expect(citationHref({ kind: 'github_file', url: '/' })).toBe('/');
  });

  it('deduplicates streamed citations by url and revision', () => {
    const first = citation();
    expect(mergeCitations([first], [first, citation({ title: 'duplicate' })])).toEqual([first]);
    expect(mergeCitations([first], [citation({ revision: 'bbbb' })])).toHaveLength(2);
  });

  it('keeps two sources that share an address under different identifiers', () => {
    const url = 'knowledge://11111111-1111-1111-1111-111111111111';
    const document = citation({ kind: 'workspace_document', identifier: 'doc:4a91c2', url });
    const passage = citation({ kind: 'knowledge_passage', identifier: 'kb:9f30ab', url });

    expect(mergeCitations([document], [passage]).map((seen) => seen.identifier)).toEqual([
      'doc:4a91c2',
      'kb:9f30ab',
    ]);
  });

  it('folds one source cited twice under the same identifier', () => {
    const cited = citation({ kind: 'web', identifier: 'web:a3f21c' });
    const again = citation({
      kind: 'web',
      identifier: 'WEB:A3F21C ',
      url: 'https://example.test/elsewhere',
    });

    expect(mergeCitations([cited], [again])).toEqual([cited]);
  });

  it('names a knowledge passage', () => {
    expect(citationKindLabel('knowledge_passage')).toBe('Knowledge passage');
  });

  it('names a web source and links it by the absolute url it was retrieved from', () => {
    expect(citationKindLabel('web')).toBe('Web page');
    expect(citationHref({ kind: 'web', url: 'https://example.test/changelog' })).toBe(
      'https://example.test/changelog'
    );
  });

  it('never routes a web source to the wiki when its url is unusable', () => {
    expect(citationHref({ kind: 'web', url: 'example.test/changelog' })).toBeNull();
    expect(citationHref({ kind: 'web', url: '//evil.example/x' })).toBeNull();
  });

  it('keeps the identifier a reply cites a source by', () => {
    const cited = citation({ kind: 'web', identifier: 'web-1' });

    expect(cited.identifier).toBe('web-1');
    expect(citation().identifier).toBeUndefined();
    expect(mergeCitations([], [cited])).toEqual([cited]);
  });

  it('will not call a model claim passing, however complete and successful', () => {
    // The server's Citation::passing() is complete && authoritative && success.
    // This is the only place a human reads the verdict, so dropping the
    // provenance term here renders an unverified claim as proof.
    expect(citationEvidence(citation({ provenance: 'model_asserted' }))).toBe('claimed');
    expect(citationEvidenceLabel('claimed')).toBe('Claimed');
    expect(citationEvidence(citation({ provenance: 'server_execution' }))).toBe('passing');
  });
});
