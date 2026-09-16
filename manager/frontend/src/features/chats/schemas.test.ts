import { describe, expect, it } from 'bun:test';
import { z } from 'zod';
import {
  ActionReceiptSchema,
  ActionTargetSchema,
  MessageMetadataSchema,
  MessageSchema,
} from './schemas';
import { ACTION_TARGETS } from './types';

const reply = {
  id: 'msg-2',
  chat_id: 'chat-1',
  role: 'assistant',
  content: 'Hi there!',
  created_at: '2026-09-13T09:45:00.000Z',
};

describe('declaring the memory-read flag costs no message', () => {
  it('keeps the flag on a reply the server set it on', () => {
    const parsed = MessageSchema.parse({ ...reply, metadata: { memory_used: true } });

    expect(parsed.metadata?.memory_used).toBe(true);
  });

  it('parses a reply stored before the flag existed, with the flag absent', () => {
    const parsed = MessageSchema.parse({ ...reply, metadata: { reasoning: 'Paris.' } });

    expect(parsed.metadata?.memory_used).toBeUndefined();
    expect(parsed.metadata?.reasoning).toBe('Paris.');
  });

  it('drops a flag the server wrote as something other than a boolean, keeping the reply', () => {
    const parsed = MessageSchema.parse({
      ...reply,
      metadata: { memory_used: 'yes', reasoning: 'Paris.' },
    });

    expect(parsed.metadata?.memory_used).toBeUndefined();
    expect(parsed.metadata?.reasoning).toBe('Paris.');
    expect(parsed.content).toBe('Hi there!');
  });

  it('a bare optional would have failed that reply instead of dropping the flag', () => {
    const bare = z.object({ ...MessageMetadataSchema.shape, memory_used: z.boolean().optional() });

    expect(bare.safeParse({ memory_used: 'yes' }).success).toBe(false);
    expect(MessageMetadataSchema.safeParse({ memory_used: 'yes' }).success).toBe(true);
  });
});

describe('a memory receipt', () => {
  const stored = {
    id: 'call_1',
    action: 'memory_write',
    target_type: 'memory',
    target_id: 'preference/Preferences',
    target_label: 'Preferences',
    actor_id: 'user-1',
    actor_name: 'Alice',
    occurred_at: '2026-09-13T09:45:00.000Z',
    success: true,
    outcome: 'Memory written',
    href: '',
  };

  it('parses with the memory target and the empty href the server gives it', () => {
    const parsed = ActionReceiptSchema.parse(stored);

    expect(parsed.target_type).toBe('memory');
    expect(parsed.href).toBe('');
  });

  it('is kept on the reply it arrived on', () => {
    const parsed = MessageSchema.parse({ ...reply, metadata: { action_receipts: [stored] } });

    expect(parsed.metadata?.action_receipts).toHaveLength(1);
    expect(parsed.metadata?.action_receipts?.[0].target_type).toBe('memory');
  });

  it('the schema names every target the console lists, memory included', () => {
    expect(ActionTargetSchema.options).toEqual([...ACTION_TARGETS]);
    expect(ACTION_TARGETS).toContain('memory');
  });
});
