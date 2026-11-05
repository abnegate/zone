import { describe, expect, it } from 'bun:test';
import type { Citation, Message } from '../types';
import { chatSources, observationKey, sourceKey } from './sources';

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

const urls = (sources: { citation: Citation }[]) => sources.map((source) => source.citation.url);

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

    const { all } = chatSources([
      message('message-1', [older, newest]),
      message('message-2', [newer]),
    ]);

    expect(urls(all)).toEqual([
      'https://example.test/newest',
      'https://example.test/newer',
      'https://example.test/older',
    ]);
    expect(all.map((source) => source.messageId)).toEqual(['message-1', 'message-2', 'message-1']);
  });

  it('shows a source cited twice once in the chat, as it was last observed', () => {
    const pending = citation({ outcome: 'pending', observed_at: '2026-11-05T09:14:00Z' });
    const passing = citation({ outcome: 'success', observed_at: '2026-11-05T09:40:00Z' });

    const { all } = chatSources([message('message-1', [pending]), message('message-2', [passing])]);

    expect(all).toHaveLength(1);
    expect(all[0].citation.outcome).toBe('success');
    expect(all[0].messageId).toBe('message-2');
  });

  it("keeps each reply to its own evidence, never a later reply's observation of it", () => {
    const incomplete = citation({
      complete: false,
      outcome: 'incomplete',
      observed_at: '2026-11-05T09:14:00Z',
    });
    const passing = citation({ outcome: 'success', observed_at: '2026-11-05T09:40:00Z' });

    const { replies } = chatSources([
      message('message-1', [incomplete]),
      message('message-2', [passing]),
    ]);

    expect(replies.get('message-1')?.map((source) => source.citation)).toEqual([incomplete]);
    expect(replies.get('message-2')?.map((source) => source.citation)).toEqual([passing]);
  });

  it('keeps a document and its knowledge passage apart in a reply, but once in the chat', () => {
    const document = citation({
      kind: 'workspace_document',
      url: 'knowledge://11111111-1111-1111-1111-111111111111',
      identifier: 'doc:aaaaaa',
      observed_at: '2026-11-05T09:00:00Z',
    });
    const passage = citation({
      kind: 'knowledge_passage',
      url: 'knowledge://11111111-1111-1111-1111-111111111111',
      identifier: 'kb:bbbbbb',
      observed_at: '2026-11-05T09:05:00Z',
    });

    const { all, replies } = chatSources([message('message-1', [document, passage])]);

    expect(replies.get('message-1')?.map((source) => source.key)).toEqual([
      'kb:bbbbbb',
      'doc:aaaaaa',
    ]);
    expect(all.map((source) => source.citation.identifier)).toEqual(['kb:bbbbbb']);
    expect(all[0].key).toBe('knowledge://11111111-1111-1111-1111-111111111111');
  });

  it('keeps the later observation when a later reply cites an older one', () => {
    const recent = citation({ outcome: 'success', observed_at: '2026-11-05T09:40:00Z' });
    const stale = citation({ outcome: 'pending', observed_at: '2026-11-05T09:14:00Z' });

    const { all } = chatSources([message('message-1', [recent]), message('message-2', [stale])]);

    expect(all[0].citation.outcome).toBe('success');
    expect(all[0].messageId).toBe('message-1');
  });

  it('lets the later citation win an observation tie, and puts it first', () => {
    const first = citation({ url: 'https://example.test/a', note: 'first' });
    const other = citation({ url: 'https://example.test/b', note: 'other' });
    const repeat = citation({ url: 'https://example.test/a', note: 'repeat' });

    const { all } = chatSources([
      message('message-1', [first, other]),
      message('message-2', [repeat]),
    ]);

    expect(all.map((source) => source.citation.note)).toEqual(['repeat', 'other']);
  });

  it('keeps the order a reply cited same-time sources in, latest cited first', () => {
    const a = citation({ url: 'https://example.test/a' });
    const b = citation({ url: 'https://example.test/b' });
    const c = citation({ url: 'https://example.test/c' });

    const { all, replies } = chatSources([message('message-1', [a, b]), message('message-2', [c])]);

    expect(urls(all)).toEqual([
      'https://example.test/c',
      'https://example.test/b',
      'https://example.test/a',
    ]);
    expect(urls(replies.get('message-1') ?? [])).toEqual([
      'https://example.test/b',
      'https://example.test/a',
    ]);
  });

  it('orders an unreadable observation time last', () => {
    const unreadable = citation({ url: 'https://example.test/a', observed_at: 'not a time' });
    const dated = citation({ url: 'https://example.test/b' });

    const { all } = chatSources([message('message-1', [unreadable, dated])]);

    expect(urls(all)).toEqual(['https://example.test/b', 'https://example.test/a']);
  });

  it('reuses a reply whose citations did not change, so streaming text does not rebuild it', () => {
    const cited = message('message-1', [citation()]);
    const first = chatSources([cited]).replies.get('message-1');
    const streamed = { ...cited, content: 'more text' };

    expect(chatSources([streamed]).replies.get('message-1')).toBe(first);
    expect(
      chatSources([{ ...cited, metadata: { citations: [citation()] } }]).replies.get('message-1')
    ).not.toBe(first);
  });

  it('is empty for a chat whose replies cite nothing', () => {
    const { all, replies } = chatSources([
      message('message-1', undefined),
      message('message-2', []),
    ]);

    expect(all).toEqual([]);
    expect(replies.size).toBe(0);
  });
});

describe('observationKey', () => {
  it('names an observation by its identifier, falling back to its address', () => {
    expect(observationKey(citation({ identifier: 'kb:bbbbbb' }))).toBe('kb:bbbbbb');
    expect(observationKey(citation({ identifier: null }))).toBe(
      'https://github.com/abnegate/zone/pull/160'
    );
  });
});

describe('sourceKey', () => {
  it('names a source by its address, falling back to its identifier and title', () => {
    expect(sourceKey(citation())).toBe('https://github.com/abnegate/zone/pull/160');
    expect(sourceKey(citation({ url: '', identifier: 'web:a3f21c' }))).toBe('web:a3f21c');
    expect(sourceKey(citation({ url: '', identifier: null, title: 'Runbook' }))).toBe('Runbook');
  });
});
