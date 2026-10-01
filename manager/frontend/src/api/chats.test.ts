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
});
