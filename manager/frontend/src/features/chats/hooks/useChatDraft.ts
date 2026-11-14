import { useCallback, useState } from 'react';
import type { Attachment } from '../utils/attachments';
import { draftKey, readDraft, writeDraft } from '../utils/drafts';

const NO_ATTACHMENTS: Attachment[] = [];

export type ChatDraft = {
  /** The unsent text of the chat being shown. */
  draft: string;
  setDraft: (text: string) => void;
  /** The files waiting on that chat's composer. */
  attachments: Attachment[];
  setAttachments: (next: Attachment[] | ((held: Attachment[]) => Attachment[])) => void;
  /** Drops the draft of a chat that is gone. */
  forgetDraft: (chatId: string) => void;
};

/**
 * The unsent message of one chat. Every chat keeps its own, so moving to
 * another chat leaves the text and its files where they were put. The text
 * lives in storage and survives a reload; the files only live in memory,
 * because a data URL is far too large to keep there.
 */
export function useChatDraft(chatId: string | null, userId?: string | null): ChatDraft {
  const key = chatId ? draftKey(userId, chatId) : null;
  const [typed, setTyped] = useState<Record<string, string>>({});
  const [attached, setAttached] = useState<Record<string, Attachment[]>>({});
  // Storage answers for a chat this session has not typed in yet, so the first
  // render after a switch already shows that chat's own draft.
  const draft = key === null ? '' : (typed[key] ?? readDraft(key));
  const attachments = (key === null ? undefined : attached[key]) ?? NO_ATTACHMENTS;

  const setDraft = useCallback(
    (text: string) => {
      if (key === null) return;
      setTyped((current) => ({ ...current, [key]: text }));
      writeDraft(key, text);
    },
    [key]
  );

  const setAttachments = useCallback(
    (next: Attachment[] | ((held: Attachment[]) => Attachment[])) => {
      if (key === null) return;
      setAttached((current) => {
        const held = current[key] ?? NO_ATTACHMENTS;
        const updated = typeof next === 'function' ? next(held) : next;
        return updated === held ? current : { ...current, [key]: updated };
      });
    },
    [key]
  );

  const forgetDraft = useCallback(
    (gone: string) => {
      const forgotten = draftKey(userId, gone);
      writeDraft(forgotten, '');
      setTyped((current) => without(current, forgotten));
      setAttached((current) => without(current, forgotten));
    },
    [userId]
  );

  return { draft, setDraft, attachments, setAttachments, forgetDraft };
}

function without<T>(held: Record<string, T>, key: string): Record<string, T> {
  if (!(key in held)) return held;
  return Object.fromEntries(Object.entries(held).filter(([kept]) => kept !== key));
}
