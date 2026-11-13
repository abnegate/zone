import { describe, expect, it } from 'bun:test';
import type { ChatWithMessages } from '../types';
import { CACHED_CHATS, histories } from './history';

const chat = (id: string): ChatWithMessages => ({
  id,
  title: `Chat ${id}`,
  model_name: 'gpt-4',
  created_at: '2024-01-01T00:00:00Z',
  updated_at: '2024-01-01T00:00:00Z',
  archived: false,
  agent_enabled: false,
  messages: [],
});

describe('histories', () => {
  it('forgets the chat looked at longest ago once it holds more than its capacity', () => {
    for (let index = 0; index <= CACHED_CHATS; index += 1) {
      histories.remember(chat(String(index)));
    }
    expect(histories.recall('0')).toBeNull();
    expect(histories.recall('1')?.id).toBe('1');
    expect(histories.recall(String(CACHED_CHATS))?.id).toBe(String(CACHED_CHATS));
  });

  it('keeps a chat that was opened again over one left untouched', () => {
    for (let index = 0; index < CACHED_CHATS; index += 1) {
      histories.remember(chat(String(index)));
    }
    histories.recall('0');
    histories.remember(chat('new'));
    expect(histories.recall('0')?.id).toBe('0');
    expect(histories.recall('1')).toBeNull();
  });

  it('renames a remembered chat in place', () => {
    histories.remember(chat('1'));
    histories.retitle('1', 'Renamed');
    histories.retitle('missing', 'Ignored');
    expect(histories.recall('1')?.title).toBe('Renamed');
    expect(histories.recall('missing')).toBeNull();
  });
});
