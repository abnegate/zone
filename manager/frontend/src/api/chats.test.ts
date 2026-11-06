import { afterEach, describe, expect, it, mock } from 'bun:test';
import { ApiError } from './ApiError';
import { chatsApi } from './chats';

const fetch = globalThis.fetch;

afterEach(() => {
  globalThis.fetch = fetch;
});

const searchParameters = (request: ReturnType<typeof mock>): URLSearchParams => {
  const [url] = request.mock.calls[0] as [string];
  return new URL(url, 'http://console.test').searchParams;
};

describe('chatsApi.searchChatMessages', () => {
  it('sends the workspace the server requires beside the query', async () => {
    const request = mock(async () => Response.json({ results: [], total: 0 }));
    globalThis.fetch = request as unknown as typeof globalThis.fetch;

    await chatsApi.searchChatMessages({ query: 'zebra', workspace_id: 'ws-1', limit: 20 });

    const parameters = searchParameters(request);
    expect(parameters.get('query')).toBe('zebra');
    expect(parameters.get('workspace_id')).toBe('ws-1');
    expect(parameters.get('limit')).toBe('20');
    expect(parameters.has('chat_id')).toBe(false);
  });

  it('names the server reason when a search is refused', async () => {
    globalThis.fetch = mock(
      async () =>
        new Response(
          JSON.stringify({
            error: 'Failed to deserialize query string: missing field workspace_id',
          }),
          { status: 400 }
        )
    ) as unknown as typeof globalThis.fetch;

    await expect(
      chatsApi.searchChatMessages({ query: 'zebra', workspace_id: 'ws-1' })
    ).rejects.toThrow(
      'Failed to search chat messages: Failed to deserialize query string: missing field workspace_id'
    );
  });
});

describe('chatsApi.createChat', () => {
  const request = { workspace_id: 'ws-1', title: 'New chat', model_name: 'auto' };

  it("names the reason the server refuses a workspace's endpoint", async () => {
    const reason =
      "This workspace's AI endpoint can't be used: its AI settings couldn't be read. Check AI Settings.";
    globalThis.fetch = mock(async () =>
      Response.json({ success: false, error: reason }, { status: 409 })
    ) as unknown as typeof globalThis.fetch;

    const failure = await chatsApi.createChat(request).catch((error: unknown) => error);

    expect(failure).toBeInstanceOf(ApiError);
    expect((failure as ApiError).status).toBe(409);
    expect((failure as ApiError).message).toBe(`Failed to create chat: ${reason}`);
  });

  it('falls back to the status when the refusal carries no reason', async () => {
    globalThis.fetch = mock(
      async () => new Response('', { status: 502 })
    ) as unknown as typeof globalThis.fetch;

    await expect(chatsApi.createChat(request)).rejects.toThrow('Failed to create chat: 502');
  });

  it('sends the chosen provider and model with the new chat', async () => {
    const request = mock(async () => Response.json({ chat: chat({ provider: 'codex' }) }));
    globalThis.fetch = request as unknown as typeof globalThis.fetch;

    await chatsApi.createChat({
      workspace_id: 'ws-1',
      title: 'New chat',
      model_name: 'auto',
      provider: 'codex',
      provider_model: 'gpt-5.5',
    });

    expect(sent(request).body).toEqual({
      workspace_id: 'ws-1',
      title: 'New chat',
      model_name: 'auto',
      provider: 'codex',
      provider_model: 'gpt-5.5',
    });
  });
});

const codex = {
  provider: 'codex',
  label: 'Codex',
  usable: true,
  reason: null,
  models: [{ name: 'gpt-5.5', label: 'gpt-5.5', efforts: ['low', 'medium', 'high', 'xhigh'] }],
  efforts: ['low', 'medium', 'high', 'xhigh'],
  requires_model: false,
};

function chat(fields: Record<string, unknown> = {}) {
  return {
    id: 'chat 1',
    title: 'Chat',
    model_name: 'auto',
    created_at: '2026-11-03T00:00:00Z',
    updated_at: '2026-11-03T00:00:00Z',
    archived: false,
    messages: [],
    ...fields,
  };
}

function sent(request: ReturnType<typeof mock>): { url: string; method: string; body: unknown } {
  const [url, init] = request.mock.calls[0] as [string, RequestInit | undefined];
  return {
    url,
    method: init?.method ?? 'GET',
    body: init?.body === undefined ? undefined : JSON.parse(init.body as string),
  };
}

describe('chatsApi.getChatProviders', () => {
  it("reads the chat's providers", async () => {
    const request = mock(async () =>
      Response.json({
        default: codex,
        selected: { ...codex, model: 'gpt-5.5' },
        providers: [codex],
      })
    );
    globalThis.fetch = request as unknown as typeof globalThis.fetch;

    const providers = await chatsApi.getChatProviders('chat 1');

    expect(sent(request)).toEqual({
      url: '/api/chats/chat%201/providers',
      method: 'GET',
      body: undefined,
    });
    expect(providers.selected?.model).toBe('gpt-5.5');
    expect(providers.providers.map((offer) => offer.label)).toEqual(['Codex']);
  });

  it('names the reason a load is refused', async () => {
    globalThis.fetch = mock(async () =>
      Response.json({ error: 'Chat not found' }, { status: 404 })
    ) as unknown as typeof globalThis.fetch;

    await expect(chatsApi.getChatProviders('chat 1')).rejects.toThrow(
      'Failed to load AI providers: Chat not found'
    );
  });
});

describe('chatsApi.setChatProvider', () => {
  it('puts the provider and model and returns the updated chat', async () => {
    const request = mock(async () =>
      Response.json({ chat: chat({ provider: 'codex', provider_model: 'gpt-5.5' }) })
    );
    globalThis.fetch = request as unknown as typeof globalThis.fetch;

    const updated = await chatsApi.setChatProvider('chat 1', {
      provider: 'codex',
      model: 'gpt-5.5',
    });

    expect(sent(request)).toEqual({
      url: '/api/chats/chat%201/provider',
      method: 'PUT',
      body: { provider: 'codex', model: 'gpt-5.5' },
    });
    expect(updated.provider).toBe('codex');
    expect(updated.provider_model).toBe('gpt-5.5');
  });

  it('clears the choice with nulls', async () => {
    const request = mock(async () => Response.json({ chat: chat({ provider: null }) }));
    globalThis.fetch = request as unknown as typeof globalThis.fetch;

    await chatsApi.setChatProvider('chat 1', { provider: null });

    expect(sent(request).body).toEqual({ provider: null, model: null });
  });

  it('names the reason a provider is unavailable', async () => {
    const reason = 'Claude Code is not signed in for this organization.';
    globalThis.fetch = mock(async () =>
      Response.json({ error: reason, code: 'provider_unavailable' }, { status: 409 })
    ) as unknown as typeof globalThis.fetch;

    const failure = await chatsApi
      .setChatProvider('chat 1', { provider: 'claude_code' })
      .catch((error: unknown) => error);

    expect(failure).toBeInstanceOf(ApiError);
    expect((failure as ApiError).status).toBe(409);
    expect((failure as ApiError).message).toBe(`Failed to change the AI provider: ${reason}`);
  });
});

describe('chatsApi.getWorkspaceChatProviders', () => {
  it("reads the providers a workspace's new chats can use", async () => {
    const request = mock(async () => Response.json({ default: codex, providers: [codex] }));
    globalThis.fetch = request as unknown as typeof globalThis.fetch;

    const providers = await chatsApi.getWorkspaceChatProviders('ws 1');

    expect(sent(request)).toEqual({
      url: '/api/workspaces/ws%201/chat-providers',
      method: 'GET',
      body: undefined,
    });
    expect(providers.default.label).toBe('Codex');
  });

  it('names the reason a load is refused', async () => {
    globalThis.fetch = mock(async () =>
      Response.json({ error: 'Forbidden' }, { status: 403 })
    ) as unknown as typeof globalThis.fetch;

    await expect(chatsApi.getWorkspaceChatProviders('ws-1')).rejects.toThrow(
      'Failed to load AI providers: Forbidden'
    );
  });
});

const SUBAGENT = {
  id: 'subagent-1',
  chat_id: 'chat 1',
  message_id: 'message-1',
  external_id: 'toolu_01Agent',
  description: 'Ask for a review',
  kind: 'general-purpose',
  model: 'sonnet',
  background: true,
  status: 'running',
  tool_uses: 12,
  duration_ms: 53_000,
  tokens: 18_000,
  last_tool: 'Bash',
  summary: null,
  error: null,
  step_count: 1,
  revision: 4,
  started_at: '2026-11-05T10:00:00Z',
  finished_at: null,
  updated_at: '2026-11-05T10:00:53Z',
};

describe('chatsApi.getChatSubagents', () => {
  it("reads the chat's agents", async () => {
    const request = mock(async () => Response.json({ subagents: [SUBAGENT] }));
    globalThis.fetch = request as unknown as typeof globalThis.fetch;

    const list = await chatsApi.getChatSubagents('chat 1');

    expect(sent(request)).toEqual({
      url: '/api/chats/chat%201/subagents',
      method: 'GET',
      body: undefined,
    });
    expect(list).toEqual({ subagents: [SUBAGENT], complete: true });
  });

  it('drops an unreadable agent, keeps the rest, and says the list is incomplete', async () => {
    globalThis.fetch = mock(async () =>
      Response.json({ subagents: [{ id: 'broken' }, SUBAGENT] })
    ) as unknown as typeof globalThis.fetch;

    expect(await chatsApi.getChatSubagents('chat 1')).toEqual({
      subagents: [SUBAGENT],
      complete: false,
    });
  });

  it('reads an unknown status as settled, and an unreadable count as none', async () => {
    globalThis.fetch = mock(async () =>
      Response.json({
        subagents: [{ ...SUBAGENT, status: 'paused', tokens: 'many', step_count: -1 }],
      })
    ) as unknown as typeof globalThis.fetch;

    const { subagents, complete } = await chatsApi.getChatSubagents('chat 1');
    const [subagent] = subagents;

    expect(complete).toBe(true);

    expect(subagent.status).toBe('stopped');
    expect(subagent.tokens).toBeNull();
    expect(subagent.step_count).toBe(0);
    expect(subagent.revision).toBe(4);
  });

  it('reads an answer without a list as an incomplete one', async () => {
    globalThis.fetch = mock(async () => Response.json({})) as unknown as typeof globalThis.fetch;

    expect(await chatsApi.getChatSubagents('chat 1')).toEqual({ subagents: [], complete: false });
  });

  it('names the reason a load is refused', async () => {
    globalThis.fetch = mock(async () =>
      Response.json({ error: 'Chat not found' }, { status: 404 })
    ) as unknown as typeof globalThis.fetch;

    await expect(chatsApi.getChatSubagents('chat 1')).rejects.toThrow(
      'Failed to load agents: Chat not found'
    );
  });
});

const STEP = { id: 'toolu_x', name: 'Bash', subject: 'cargo test', status: 'succeeded' };

describe('chatsApi.getChatSubagentSteps', () => {
  it("reads one agent's steps", async () => {
    const request = mock(async () => Response.json({ steps: [STEP] }));
    globalThis.fetch = request as unknown as typeof globalThis.fetch;

    const steps = await chatsApi.getChatSubagentSteps('chat 1', 'agent/1');

    expect(sent(request)).toEqual({
      url: '/api/chats/chat%201/subagents/agent%2F1/steps',
      method: 'GET',
      body: undefined,
    });
    expect(steps).toEqual([STEP]);
  });

  it('reads an unknown step status as running and drops an unreadable step', async () => {
    globalThis.fetch = mock(async () =>
      Response.json({ steps: [{ ...STEP, status: 'retrying' }, { name: 'no id' }] })
    ) as unknown as typeof globalThis.fetch;

    expect(await chatsApi.getChatSubagentSteps('chat 1', 'agent-1')).toEqual([
      { ...STEP, status: 'running' },
    ]);
  });

  it('reads no steps from an answer without a list', async () => {
    globalThis.fetch = mock(async () => Response.json({})) as unknown as typeof globalThis.fetch;

    expect(await chatsApi.getChatSubagentSteps('chat 1', 'agent-1')).toEqual([]);
  });

  it('names the reason a load is refused', async () => {
    globalThis.fetch = mock(async () =>
      Response.json({ error: 'Subagent not found' }, { status: 404 })
    ) as unknown as typeof globalThis.fetch;

    await expect(chatsApi.getChatSubagentSteps('chat 1', 'agent-1')).rejects.toThrow(
      'Failed to load steps: Subagent not found'
    );
  });
});
