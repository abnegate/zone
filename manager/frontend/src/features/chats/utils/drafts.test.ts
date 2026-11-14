import { afterEach, beforeEach, describe, expect, it, mock, spyOn } from 'bun:test';
import { CHAT_DRAFT_PREFIX, draftKey, readDraft, writeDraft } from './drafts';

describe('chat drafts', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  afterEach(() => {
    mock.restore();
  });

  it('keeps a draft per chat and per viewer', () => {
    expect(draftKey('user-1', 'chat-7')).toBe(`${CHAT_DRAFT_PREFIX}:user-1:chat-7`);
    expect(draftKey(null, 'chat-7')).toBe(`${CHAT_DRAFT_PREFIX}:signed-out:chat-7`);
    expect(draftKey('user-1', 'chat-7')).not.toBe(draftKey('user-2', 'chat-7'));
    expect(draftKey('user-1', 'chat-7')).not.toBe(draftKey('user-1', 'chat-8'));
  });

  it('reads back what was written, chat by chat', () => {
    writeDraft(draftKey('user-1', 'chat-7'), 'half a question');
    writeDraft(draftKey('user-1', 'chat-8'), 'another thought');

    expect(readDraft(draftKey('user-1', 'chat-7'))).toBe('half a question');
    expect(readDraft(draftKey('user-1', 'chat-8'))).toBe('another thought');
    expect(readDraft(draftKey('user-2', 'chat-7'))).toBe('');
    expect(readDraft(draftKey('user-1', 'chat-9'))).toBe('');
  });

  it('forgets the key once the draft is empty, and ignores a chat that is not there', () => {
    const key = draftKey('user-1', 'chat-7');
    writeDraft(key, 'half a question');
    writeDraft(key, '');

    expect(localStorage.getItem(key)).toBeNull();
    expect(readDraft(key)).toBe('');
    expect(readDraft(null)).toBe('');
    expect(() => writeDraft(null, 'nowhere')).not.toThrow();
  });

  it('carries on when the browser refuses storage', () => {
    spyOn(localStorage, 'getItem').mockImplementation(() => {
      throw new Error('denied');
    });
    spyOn(localStorage, 'setItem').mockImplementation(() => {
      throw new Error('denied');
    });

    const key = draftKey('user-1', 'chat-7');
    expect(() => writeDraft(key, 'half a question')).not.toThrow();
    expect(readDraft(key)).toBe('');
  });
});
