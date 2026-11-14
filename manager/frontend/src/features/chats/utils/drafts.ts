/** Prefix of every stored draft, so one viewer's unsent text can be found or swept. */
export const CHAT_DRAFT_PREFIX = 'zone:chat-draft';

/** Where one viewer's unsent message for one chat is kept, so people sharing a browser keep their own. */
export function draftKey(userId: string | null | undefined, chatId: string): string {
  return `${CHAT_DRAFT_PREFIX}:${userId || 'signed-out'}:${chatId}`;
}

export function readDraft(key: string | null): string {
  if (!key) return '';
  try {
    return localStorage.getItem(key) ?? '';
  } catch {
    return '';
  }
}

/** Keeps the text under its key, and forgets the key once nothing is left to send. */
export function writeDraft(key: string | null, text: string): void {
  if (!key) return;
  try {
    if (text) {
      localStorage.setItem(key, text);
    } else {
      localStorage.removeItem(key);
    }
  } catch {
    // A browser refusing storage still lets this message be typed and sent.
  }
}
