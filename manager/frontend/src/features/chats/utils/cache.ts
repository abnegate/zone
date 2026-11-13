export const CACHED_CHATS = 100;

const caches = new Set<{ clear: () => void }>();
let cleared = 0;

/** Empties every chat cache, as a logout must, and marks reads begun before it as stale. */
export function clearChatCaches(): void {
  cleared += 1;
  for (const cache of caches) cache.clear();
}

/** Changes whenever the caches are cleared, so a fetch begun before a logout can tell. */
export function cacheEpoch(): number {
  return cleared;
}

/** What was last read for each recently opened chat, so reopening one draws it at once. */
export class ChatCache<T> {
  private readonly entries = new Map<string, T>();

  constructor(private readonly capacity: number = CACHED_CHATS) {
    caches.add(this);
  }

  has(chatId: string): boolean {
    return this.entries.has(chatId);
  }

  /** Reads without marking the chat as recently used, so a render can call it. */
  peek(chatId: string): T | undefined {
    return this.entries.get(chatId);
  }

  recall(chatId: string): T | undefined {
    const value = this.entries.get(chatId);
    if (value === undefined) return undefined;
    this.entries.delete(chatId);
    this.entries.set(chatId, value);
    return value;
  }

  remember(chatId: string, value: T): void {
    this.entries.delete(chatId);
    this.entries.set(chatId, value);
    for (const key of this.entries.keys()) {
      if (this.entries.size <= this.capacity) break;
      this.entries.delete(key);
    }
  }

  update(chatId: string, change: (value: T) => T): void {
    const value = this.entries.get(chatId);
    if (value !== undefined) this.entries.set(chatId, change(value));
  }

  forget(chatId: string): void {
    this.entries.delete(chatId);
  }

  clear(): void {
    this.entries.clear();
  }
}
