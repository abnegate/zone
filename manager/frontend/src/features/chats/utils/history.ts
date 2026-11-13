import type { ChatWithMessages } from '../types';

export const CACHED_CHATS = 100;

/** The last-seen history of recently opened chats, so reopening one draws it at once. */
class History {
  private readonly chats = new Map<string, ChatWithMessages>();
  private cleared = 0;

  constructor(private readonly capacity: number) {}

  /** Changes whenever the cache is cleared, so a fetch begun before a logout can tell. */
  get epoch(): number {
    return this.cleared;
  }

  has(chatId: string): boolean {
    return this.chats.has(chatId);
  }

  recall(chatId: string): ChatWithMessages | null {
    const chat = this.chats.get(chatId);
    if (!chat) return null;
    this.chats.delete(chatId);
    this.chats.set(chatId, chat);
    return chat;
  }

  remember(chat: ChatWithMessages): void {
    this.chats.delete(chat.id);
    this.chats.set(chat.id, chat);
    for (const chatId of this.chats.keys()) {
      if (this.chats.size <= this.capacity) break;
      this.chats.delete(chatId);
    }
  }

  retitle(chatId: string, title: string): void {
    const chat = this.chats.get(chatId);
    if (chat) this.chats.set(chatId, { ...chat, title });
  }

  forget(chatId: string): void {
    this.chats.delete(chatId);
  }

  clear(): void {
    this.cleared += 1;
    this.chats.clear();
  }
}

export const histories = new History(CACHED_CHATS);
