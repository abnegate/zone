import { describe, expect, it } from 'bun:test';
import { z } from 'zod';
import {
  ActionReceiptSchema,
  ActionTargetSchema,
  ChatSchema,
  HandoverSchema,
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

describe('ChatSchema', () => {
  const listed = {
    id: 'chat-1',
    title: 'Chat 1',
    model_name: 'llama2',
    created_at: '2024-01-01T00:00:00Z',
    updated_at: '2024-01-01T00:00:00Z',
    archived: false,
    agent_enabled: false,
  };

  it('treats a chat from an older server as online', () => {
    expect(ChatSchema.parse(listed).offline).toBe(false);
  });

  it('keeps offline when the server set it', () => {
    expect(ChatSchema.parse({ ...listed, offline: true }).offline).toBe(true);
  });
});

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

describe('a handover', () => {
  const frame = {
    type: 'handover',
    message_id: 'msg-2',
    from: 'a@example.com',
    to: 'b@example.com',
    from_agent: 'claude',
    agent: 'codex',
    reason: 'limit',
    resets_at: '2026-09-23T06:10:00Z',
    carried: false,
    at: 12,
  };
  const { type: _type, message_id: _message, ...stored } = frame;

  it('reads the live frame, which carries type rather than kind', () => {
    expect(HandoverSchema.parse(frame)).toEqual({
      kind: 'handover',
      from: 'a@example.com',
      to: 'b@example.com',
      from_agent: 'claude',
      agent: 'codex',
      reason: 'limit',
      resets_at: '2026-09-23T06:10:00Z',
      carried: false,
      at: 12,
    });
  });

  it('refuses a switch that does not say which agent it left', () => {
    const { from_agent: _left, ...unsaid } = stored;

    expect(HandoverSchema.safeParse(unsaid).success).toBe(false);
  });

  it('reads the stored record the same way the frame was read', () => {
    expect(HandoverSchema.parse({ kind: 'handover', ...stored })).toEqual(
      HandoverSchema.parse(frame)
    );
  });

  it('reads an unknown reason as a usage limit, keeping the switch', () => {
    expect(HandoverSchema.parse({ ...stored, reason: 'quota' }).reason).toBe('limit');
  });

  it('treats an empty or null from and a null reset as absent', () => {
    const parsed = HandoverSchema.parse({ ...stored, from: '', resets_at: null });

    expect(parsed.from).toBeUndefined();
    expect(parsed.resets_at).toBeUndefined();
  });

  it('is kept on the reply it arrived on', () => {
    const parsed = MessageSchema.parse({ ...reply, metadata: { handovers: [stored] } });

    expect(parsed.metadata?.handovers).toHaveLength(1);
    expect(parsed.metadata?.handovers?.[0].to).toBe('b@example.com');
  });

  it('drops an unreadable handover without costing the reply or the readable ones', () => {
    const parsed = MessageMetadataSchema.parse({
      handovers: [stored, { ...stored, at: -1 }, { ...stored, agent: 'gemini' }],
      reasoning: 'kept',
    });

    expect(parsed.handovers).toHaveLength(1);
    expect(parsed.reasoning).toBe('kept');
  });

  it('drops handovers that are not a list, keeping the reply', () => {
    const parsed = MessageSchema.parse({ ...reply, metadata: { handovers: 'oops' } });

    expect(parsed.metadata?.handovers).toBeUndefined();
    expect(parsed.content).toBe('Hi there!');
  });
});
