import { describe, expect, it } from 'bun:test';
import { type Listed, noticesOf } from './notices';
import { SourceStatusSchema } from './sourceStatus';

const TEXTS = { stale: 'Cached', partial: 'Incomplete' };

const source = SourceStatusSchema.parse({
  source_id: 'source-1',
  name: 'Zone',
  repository: 'acme/zone',
  repository_url: 'https://github.com/acme/zone',
  freshness: 'refreshed',
  generated_at: '2026-11-13T10:00:05Z',
  warning: null,
});

function listed(changes: Partial<Listed> = {}): Listed {
  return { stale: false, partial: false, warnings: [], sources: [source], ...changes };
}

describe('noticesOf', () => {
  it('says nothing about a missing or healthy listing', () => {
    expect(noticesOf(undefined, TEXTS)).toEqual([]);
    expect(noticesOf(listed(), TEXTS)).toEqual([]);
  });

  it('lists stale, partial, listing and source warnings in that order', () => {
    expect(
      noticesOf(
        listed({
          stale: true,
          partial: true,
          warnings: ['acme/api: rate limited'],
          sources: [{ ...source, warning: 'token expired' }],
        }),
        TEXTS
      )
    ).toEqual(['Cached', 'Incomplete', 'acme/api: rate limited', 'acme/zone: token expired']);
  });

  it('names each repeated warning once', () => {
    expect(
      noticesOf(
        listed({
          warnings: ['acme/zone: token expired'],
          sources: [{ ...source, warning: 'token expired' }],
        }),
        TEXTS
      )
    ).toEqual(['acme/zone: token expired']);
  });
});
