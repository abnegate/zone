import { describe, expect, it } from 'bun:test';
import type { Citation, Message } from '../types';
import { chatSources, sourceKey, sourceKeys } from './sources';

function citation(overrides: Partial<Citation> = {}): Citation {
  return {
    kind: 'github_issue',
    title: '#160 feat(chat): merge chat pull requests',
    url: 'https://github.com/abnegate/zone/pull/160',
    revision: 'f4c546c2f5270f38f84d1a9121a3f5c6461d1be2',
    observed_at: '2026-11-05T09:14:00Z',
    complete: true,
    outcome: 'pending',
    provenance: 'server_execution',
    ...overrides,
  };
}

function message(id: string, citations: Citation[] | undefined): Message {
  return {
    id,
    chat_id: 'chat-1',
    role: 'assistant',
    content: '',
    created_at: '2026-11-05T10:00:00Z',
    metadata: citations ? { citations } : undefined,
  };
}

describe('chatSources', () => {
  it('lists every cited source in the chat, newest observation first', () => {
    const older = citation({
      url: 'https://example.test/older',
      observed_at: '2026-11-05T08:00:00Z',
    });
    const newer = citation({
      url: 'https://example.test/newer',
      observed_at: '2026-11-05T09:00:00Z',
    });
    const newest = citation({
      url: 'https://example.test/newest',
      observed_at: '2026-11-05T10:00:00Z',
    });

    const sources = chatSources([
      message('message-1', [older, newest]),
      message('message-2', [newer]),
    ]);

    expect(sources.map((source) => source.citation.url)).toEqual([
      'https://example.test/newest',
      'https://example.test/newer',
      'https://example.test/older',
    ]);
    expect(sources.map((source) => source.messageId)).toEqual([
      'message-1',
      'message-2',
      'message-1',
    ]);
  });

  it('shows a source cited twice once, as it was last observed', () => {
    const pending = citation({ outcome: 'pending', observed_at: '2026-11-05T09:14:00Z' });
    const passing = citation({ outcome: 'success', observed_at: '2026-11-05T09:40:00Z' });

    const sources = chatSources([message('message-1', [pending]), message('message-2', [passing])]);

    expect(sources).toHaveLength(1);
    expect(sources[0].citation.outcome).toBe('success');
    expect(sources[0].messageId).toBe('message-2');
  });

  it('keeps the later observation when a later reply cites an older one', () => {
    const recent = citation({ outcome: 'success', observed_at: '2026-11-05T09:40:00Z' });
    const stale = citation({ outcome: 'pending', observed_at: '2026-11-05T09:14:00Z' });

    const [source] = chatSources([message('message-1', [recent]), message('message-2', [stale])]);

    expect(source.citation.outcome).toBe('success');
    expect(source.messageId).toBe('message-1');
  });

  it('lets the later citation win an observation tie, and puts it first', () => {
    const first = citation({ url: 'https://example.test/a', note: 'first' });
    const repeat = citation({ url: 'https://example.test/a', note: 'repeat' });
    const other = citation({ url: 'https://example.test/b' });

    const sources = chatSources([
      message('message-1', [first, other]),
      message('message-2', [repeat]),
    ]);

    expect(sources.map((source) => source.citation.note ?? null)).toEqual(['repeat', null]);
  });

  it('orders an unreadable observation time last', () => {
    const unreadable = citation({ url: 'https://example.test/a', observed_at: 'not a time' });
    const dated = citation({ url: 'https://example.test/b' });

    const sources = chatSources([message('message-1', [unreadable, dated])]);

    expect(sources.map((source) => source.citation.url)).toEqual([
      'https://example.test/b',
      'https://example.test/a',
    ]);
  });

  it('is empty for a chat whose replies cite nothing', () => {
    expect(chatSources([message('message-1', undefined), message('message-2', [])])).toEqual([]);
  });
});

describe('sourceKey', () => {
  it('names a source by its address, falling back to its identifier and title', () => {
    expect(sourceKey(citation())).toBe('https://github.com/abnegate/zone/pull/160');
    expect(sourceKey(citation({ url: '', identifier: 'web:a3f21c' }))).toBe('web:a3f21c');
    expect(sourceKey(citation({ url: '', identifier: null, title: 'Runbook' }))).toBe('Runbook');
  });

  it('lists each source once', () => {
    expect(
      sourceKeys([citation(), citation(), citation({ url: 'https://example.test/b' })])
    ).toEqual(['https://github.com/abnegate/zone/pull/160', 'https://example.test/b']);
  });
});
