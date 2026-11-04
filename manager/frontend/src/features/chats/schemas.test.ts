import { describe, expect, it } from 'bun:test';
import { z } from 'zod';
import {
  ActionReceiptSchema,
  ActionTargetSchema,
  ChatProvidersSchema,
  ChatSchema,
  CreateChatRequestSchema,
  HandoverSchema,
  MessageMetadataSchema,
  MessageSchema,
  WorkspaceChatProvidersSchema,
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

const codexOffer = {
  provider: 'codex',
  label: 'Codex',
  usable: true,
  reason: null,
  models: [
    {
      name: 'gpt-6-sol',
      label: 'gpt-6-sol',
      efforts: ['low', 'medium', 'high', 'xhigh', 'max', 'ultra'],
    },
    { name: 'gpt-5.5', label: 'gpt-5.5', efforts: ['low', 'medium', 'high', 'xhigh'] },
  ],
  efforts: ['low', 'medium', 'high', 'xhigh'],
  requires_model: false,
};

const openaiOffer = {
  provider: 'openai',
  label: 'OpenAI',
  usable: true,
  reason: null,
  models: [],
  efforts: ['off', 'low', 'medium', 'high'],
  requires_model: true,
};

describe('a chat provider choice', () => {
  const chat = {
    id: 'chat-1',
    title: 'Chat 1',
    model_name: 'auto',
    created_at: '2026-11-03T00:00:00Z',
    updated_at: '2026-11-03T00:00:00Z',
    archived: false,
  };

  it('keeps the chosen provider and model', () => {
    const parsed = ChatSchema.parse({ ...chat, provider: 'codex', provider_model: 'gpt-5.5' });
    expect(parsed.provider).toBe('codex');
    expect(parsed.provider_model).toBe('gpt-5.5');
  });

  it('reads a provider this console does not know as following the defaults', () => {
    const parsed = ChatSchema.parse({ ...chat, provider: 'gemini_cli', provider_model: 'x' });
    expect(parsed.provider).toBeNull();
  });

  it('leaves a listed chat without provider fields untouched', () => {
    const parsed = ChatSchema.parse(chat);
    expect(parsed.provider).toBeUndefined();
    expect(parsed.provider_model).toBeUndefined();
  });

  it.each(['xhigh', 'max', 'ultra'])('accepts the %s reasoning effort', (effort) => {
    expect(ChatSchema.parse({ ...chat, reasoning_effort: effort }).reasoning_effort).toBe(effort);
  });

  it('sends the provider fields when creating a chat', () => {
    const request = {
      workspace_id: 'ws-1',
      title: 'New chat',
      model_name: 'auto',
      provider: 'claude_code',
      provider_model: null,
      reasoning_effort: 'max',
    };
    expect(CreateChatRequestSchema.parse(request)).toEqual(request);
    expect(CreateChatRequestSchema.safeParse({ ...request, provider: 'gemini_cli' }).success).toBe(
      false
    );
  });
});

describe('the providers a chat can run on', () => {
  it('reads whether the turns run on a CLI, and anything else as not knowing', () => {
    const listing = { default: openaiOffer, selected: null, providers: [] };

    expect(ChatProvidersSchema.parse({ ...listing, cli: true }).cli).toBe(true);
    expect(ChatProvidersSchema.parse({ ...listing, cli: false }).cli).toBe(false);
    expect(ChatProvidersSchema.parse(listing).cli).toBeUndefined();
    expect(ChatProvidersSchema.parse({ ...listing, cli: 'codex' }).cli).toBeUndefined();
  });

  it('parses the default, the selection and the offers with their labels and efforts', () => {
    const parsed = ChatProvidersSchema.parse({
      default: { ...openaiOffer, requires_model: false },
      selected: { ...codexOffer, model: 'gpt-6-sol' },
      providers: [openaiOffer, codexOffer],
    });

    expect(parsed.default.label).toBe('OpenAI');
    expect(parsed.selected?.label).toBe('Codex');
    expect(parsed.selected?.model).toBe('gpt-6-sol');
    expect(parsed.selected?.models[0].efforts).toEqual([
      'low',
      'medium',
      'high',
      'xhigh',
      'max',
      'ultra',
    ]);
    expect(parsed.providers.map((offer) => offer.label)).toEqual(['OpenAI', 'Codex']);
    expect(parsed.providers[0].requires_model).toBe(true);
  });

  it('keeps an unusable selection and its reason', () => {
    const reason =
      'Claude Code is not signed in for this organization. An organization admin can sign in under Organization Settings > AI Settings.';
    const parsed = ChatProvidersSchema.parse({
      default: openaiOffer,
      selected: {
        ...codexOffer,
        provider: 'claude_code',
        label: 'Claude Code',
        usable: false,
        reason,
        model: null,
      },
      providers: [openaiOffer],
    });
    expect(parsed.selected?.usable).toBe(false);
    expect(parsed.selected?.reason).toBe(reason);
    expect(parsed.selected?.model).toBeNull();
  });

  it('parses a chat that follows the defaults', () => {
    const parsed = ChatProvidersSchema.parse({
      default: codexOffer,
      selected: null,
      providers: [codexOffer],
    });
    expect(parsed.selected).toBeNull();
  });

  it('drops offers and efforts this console does not know instead of failing', () => {
    const parsed = ChatProvidersSchema.parse({
      default: { ...codexOffer, efforts: ['low', 'turbo'] },
      selected: null,
      providers: [{ ...codexOffer, provider: 'gemini_cli', label: 'Gemini CLI' }, openaiOffer],
    });
    expect(parsed.default.efforts).toEqual(['low']);
    expect(parsed.providers.map((offer) => offer.provider)).toEqual(['openai']);
  });

  it('parses the offers for a new chat in a workspace', () => {
    const parsed = WorkspaceChatProvidersSchema.parse({
      default: codexOffer,
      providers: [openaiOffer, codexOffer],
    });
    expect(parsed.default.models.map((model) => model.name)).toEqual(['gpt-6-sol', 'gpt-5.5']);
    expect(parsed.providers).toHaveLength(2);
  });
});
