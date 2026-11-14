import { describe, expect, it } from 'bun:test';
import { SourceStatusSchema } from './sourceStatus';

const source = {
  source_id: 'source-1',
  name: 'Zone',
  repository: 'acme/zone',
  repository_url: 'https://github.com/acme/zone',
  freshness: 'refreshed',
  generated_at: '2026-11-13T10:00:05Z',
  warning: null,
};

describe('source status', () => {
  it.each(['cached', 'refreshed', 'floored', 'stale'])('keeps freshness %s', (freshness) => {
    expect(SourceStatusSchema.parse({ ...source, freshness }).freshness).toBe(freshness);
  });

  it('reads an unknown freshness as stale', () => {
    expect(SourceStatusSchema.parse({ ...source, freshness: 'warming' }).freshness).toBe('stale');
  });

  it('accepts a source that never answered', () => {
    const status = SourceStatusSchema.parse({
      ...source,
      freshness: 'stale',
      generated_at: null,
      warning: 'GitHub refused the token',
    });
    expect(status.generated_at).toBeNull();
    expect(status.warning).toBe('GitHub refused the token');
  });

  it.each(['javascript:alert(1)', 'data:text/html,<b>hi</b>', '/acme/zone'])(
    'refuses a repository url of %s',
    (url) => {
      expect(SourceStatusSchema.safeParse({ ...source, repository_url: url }).success).toBe(false);
    }
  );

  it('refuses a source without a warning field', () => {
    const { warning: _, ...unwarned } = source;
    expect(SourceStatusSchema.safeParse(unwarned).success).toBe(false);
  });
});
