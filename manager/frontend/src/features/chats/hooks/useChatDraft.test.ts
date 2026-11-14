import { beforeEach, describe, expect, it } from 'bun:test';
import { act, renderHook } from '@testing-library/react';
import type { Attachment } from '../utils/attachments';
import { draftKey, writeDraft } from '../utils/drafts';
import { useChatDraft } from './useChatDraft';

const notes: Attachment = {
  id: 'notes.md:12',
  name: 'notes.md',
  size: 12,
  type: 'text/markdown',
  text: 'a few notes',
};

describe('useChatDraft', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('holds a draft for each chat and swaps them as the chat changes', () => {
    const { result, rerender } = renderHook(({ chatId }) => useChatDraft(chatId, 'user-1'), {
      initialProps: { chatId: 'chat-1' as string | null },
    });

    act(() => result.current.setDraft('for the first chat'));
    expect(result.current.draft).toBe('for the first chat');

    rerender({ chatId: 'chat-2' });
    expect(result.current.draft).toBe('');
    act(() => result.current.setDraft('for the second chat'));

    rerender({ chatId: 'chat-1' });
    expect(result.current.draft).toBe('for the first chat');
    rerender({ chatId: 'chat-2' });
    expect(result.current.draft).toBe('for the second chat');
  });

  it('reads a draft typed before the page was reloaded', () => {
    writeDraft(draftKey('user-1', 'chat-1'), 'typed last time');

    const { result } = renderHook(() => useChatDraft('chat-1', 'user-1'));

    expect(result.current.draft).toBe('typed last time');
  });

  it('shows the viewer their own draft, not the one left by the last viewer', () => {
    writeDraft(draftKey('user-1', 'chat-1'), 'theirs');

    const { result } = renderHook(() => useChatDraft('chat-1', 'user-2'));

    expect(result.current.draft).toBe('');
  });

  it('stops holding the text once it is sent', () => {
    const key = draftKey('user-1', 'chat-1');
    const { result } = renderHook(() => useChatDraft('chat-1', 'user-1'));

    act(() => result.current.setDraft('on its way'));
    expect(localStorage.getItem(key)).toBe('on its way');

    act(() => result.current.setDraft(''));
    expect(result.current.draft).toBe('');
    expect(localStorage.getItem(key)).toBeNull();
  });

  it('keeps each chat its own attachments, without storing them', () => {
    const { result, rerender } = renderHook(({ chatId }) => useChatDraft(chatId, 'user-1'), {
      initialProps: { chatId: 'chat-1' as string | null },
    });

    act(() => result.current.setAttachments([notes]));
    expect(result.current.attachments).toEqual([notes]);

    rerender({ chatId: 'chat-2' });
    expect(result.current.attachments).toEqual([]);

    rerender({ chatId: 'chat-1' });
    expect(result.current.attachments).toEqual([notes]);
    act(() => result.current.setAttachments((held) => held.filter((a) => a.id !== notes.id)));
    expect(result.current.attachments).toEqual([]);
    expect(localStorage.getItem(draftKey('user-1', 'chat-1'))).toBeNull();
  });

  it('forgets a deleted chat, and keeps out of the way with no chat open', () => {
    const { result, rerender } = renderHook(({ chatId }) => useChatDraft(chatId, 'user-1'), {
      initialProps: { chatId: 'chat-1' as string | null },
    });

    act(() => {
      result.current.setDraft('never sent');
      result.current.setAttachments([notes]);
    });
    act(() => result.current.forgetDraft('chat-1'));

    expect(result.current.draft).toBe('');
    expect(result.current.attachments).toEqual([]);
    expect(localStorage.getItem(draftKey('user-1', 'chat-1'))).toBeNull();

    rerender({ chatId: null });
    act(() => {
      result.current.setDraft('nowhere to put this');
      result.current.setAttachments([notes]);
    });
    expect(result.current.draft).toBe('');
    expect(result.current.attachments).toEqual([]);
    expect(localStorage.getItem(draftKey('user-1', 'chat-1'))).toBeNull();
  });
});
