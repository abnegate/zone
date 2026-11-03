import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { act, renderHook, waitFor } from '@testing-library/react';
import type { ChatProviderOffer, ChatProviders } from '../types';

const mockGetChatProviders = mock();
const mockSetChatProvider = mock();

mock.module('../../../api/chats', () => ({
  chatsApi: {
    getChatProviders: mockGetChatProviders,
    setChatProvider: mockSetChatProvider,
  },
}));

let useChatProviders: typeof import('./useChatProviders').useChatProviders;

beforeAll(async () => {
  ({ useChatProviders } = await import('./useChatProviders'));
});

afterAll(() => {
  mock.restore();
});

const codex: ChatProviderOffer = {
  provider: 'codex',
  label: 'Codex',
  usable: true,
  reason: null,
  models: [{ name: 'gpt-5.5', label: 'gpt-5.5', efforts: ['low', 'medium', 'high', 'xhigh'] }],
  efforts: ['low', 'medium', 'high', 'xhigh'],
  requires_model: false,
};

const claude: ChatProviderOffer = {
  ...codex,
  provider: 'claude_code',
  label: 'Claude Code',
  models: [],
  efforts: ['low', 'medium', 'high', 'xhigh', 'max'],
};

const following = (offer: ChatProviderOffer): ChatProviders => ({
  default: offer,
  selected: null,
  providers: [codex, claude],
});

function deferred<T>() {
  let resolve: (value: T) => void = () => {};
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

describe('useChatProviders', () => {
  beforeEach(() => {
    mockGetChatProviders.mockReset();
    mockSetChatProvider.mockReset();
  });

  it('loads the providers of the selected chat and reloads when the chat changes', async () => {
    mockGetChatProviders.mockImplementation(async (chatId: string) =>
      following(chatId === 'chat-1' ? codex : claude)
    );

    const { result, rerender } = renderHook(({ chatId }) => useChatProviders(chatId), {
      initialProps: { chatId: 'chat-1' as string | null },
    });

    await waitFor(() => {
      expect(result.current.providers?.default.label).toBe('Codex');
    });
    expect(result.current.loading).toBe(false);

    rerender({ chatId: 'chat-2' });
    await waitFor(() => {
      expect(result.current.providers?.default.label).toBe('Claude Code');
    });

    rerender({ chatId: null });
    await waitFor(() => {
      expect(result.current.providers).toBeNull();
    });
    expect(mockGetChatProviders).toHaveBeenCalledTimes(2);
  });

  it('drops a response that arrives after the chat changed', async () => {
    const first = deferred<ChatProviders>();
    mockGetChatProviders.mockImplementation((chatId: string) =>
      chatId === 'chat-1' ? first.promise : Promise.resolve(following(claude))
    );

    const { result, rerender } = renderHook(({ chatId }) => useChatProviders(chatId), {
      initialProps: { chatId: 'chat-1' as string | null },
    });
    rerender({ chatId: 'chat-2' });
    await waitFor(() => {
      expect(result.current.providers?.default.label).toBe('Claude Code');
    });

    await act(async () => {
      first.resolve(following(codex));
      await first.promise;
    });

    expect(result.current.providers?.default.label).toBe('Claude Code');
    expect(result.current.loading).toBe(false);
  });

  it('leaves the providers empty and names the reason when a load fails', async () => {
    mockGetChatProviders.mockRejectedValue(new Error('Failed to load AI providers: Forbidden'));

    const { result } = renderHook(() => useChatProviders('chat-1'));

    await waitFor(() => {
      expect(result.current.error).toBe('Failed to load AI providers: Forbidden');
    });
    expect(result.current.providers).toBeNull();
    expect(result.current.loading).toBe(false);
  });

  it('keeps the last providers and names the reason when a refresh fails', async () => {
    mockGetChatProviders.mockResolvedValueOnce(following(codex));
    mockGetChatProviders.mockRejectedValueOnce(new Error('Failed to load AI providers: 502'));
    mockGetChatProviders.mockResolvedValueOnce(following(claude));

    const { result } = renderHook(() => useChatProviders('chat-1'));
    await waitFor(() => {
      expect(result.current.providers).not.toBeNull();
    });

    await act(async () => {
      await result.current.refresh();
    });

    expect(result.current.providers).toEqual(following(codex));
    expect(result.current.error).toBe('Failed to load AI providers: 502');

    await act(async () => {
      await result.current.refresh();
    });

    expect(result.current.providers).toEqual(following(claude));
    expect(result.current.error).toBeNull();
  });

  it('clears the last providers when the chat changes', async () => {
    const next = deferred<ChatProviders>();
    mockGetChatProviders.mockResolvedValueOnce(following(codex));
    mockGetChatProviders.mockReturnValueOnce(next.promise);

    const { result, rerender } = renderHook(({ id }) => useChatProviders(id), {
      initialProps: { id: 'chat-1' },
    });
    await waitFor(() => {
      expect(result.current.providers).not.toBeNull();
    });

    rerender({ id: 'chat-2' });

    expect(result.current.providers).toBeNull();
    await act(async () => {
      next.resolve(following(claude));
      await next.promise;
    });
    expect(result.current.providers).toEqual(following(claude));
  });

  it('puts the choice and then reloads the providers', async () => {
    const chosen: ChatProviders = {
      ...following(codex),
      selected: { ...claude, model: null },
    };
    mockGetChatProviders.mockResolvedValueOnce(following(codex));
    mockGetChatProviders.mockResolvedValueOnce(chosen);
    mockSetChatProvider.mockResolvedValue({ id: 'chat-1', provider: 'claude_code' });

    const { result } = renderHook(() => useChatProviders('chat-1'));
    await waitFor(() => {
      expect(result.current.providers?.selected).toBeNull();
    });

    let updated: unknown;
    await act(async () => {
      updated = await result.current.choose('claude_code');
    });

    expect(mockSetChatProvider).toHaveBeenCalledWith('chat-1', {
      provider: 'claude_code',
      model: null,
    });
    expect(mockSetChatProvider.mock.invocationCallOrder[0]).toBeLessThan(
      mockGetChatProviders.mock.invocationCallOrder[1]
    );
    expect(mockGetChatProviders).toHaveBeenCalledTimes(2);
    expect(result.current.providers?.selected?.label).toBe('Claude Code');
    expect(updated).toEqual({ id: 'chat-1', provider: 'claude_code' });
  });

  it('passes the chosen model and clears the choice with null', async () => {
    mockGetChatProviders.mockResolvedValue(following(codex));
    mockSetChatProvider.mockResolvedValue({ id: 'chat-1' });

    const { result } = renderHook(() => useChatProviders('chat-1'));
    await waitFor(() => {
      expect(result.current.providers).not.toBeNull();
    });

    await act(async () => {
      await result.current.choose('codex', 'gpt-5.5');
      await result.current.choose(null);
    });

    expect(mockSetChatProvider.mock.calls).toEqual([
      ['chat-1', { provider: 'codex', model: 'gpt-5.5' }],
      ['chat-1', { provider: null, model: null }],
    ]);
  });

  it('rejects a refused choice and keeps the loaded providers', async () => {
    mockGetChatProviders.mockResolvedValue(following(codex));
    mockSetChatProvider.mockRejectedValue(
      new Error('Failed to change the AI provider: Claude Code is not signed in.')
    );

    const { result } = renderHook(() => useChatProviders('chat-1'));
    await waitFor(() => {
      expect(result.current.providers).not.toBeNull();
    });

    let failure: unknown;
    await act(async () => {
      failure = await result.current.choose('claude_code').catch((error: unknown) => error);
    });

    expect((failure as Error).message).toContain('not signed in');
    expect(mockGetChatProviders).toHaveBeenCalledTimes(1);
    expect(result.current.providers?.default.label).toBe('Codex');
  });

  it('does not reload another chat after a choice lands late', async () => {
    const put = deferred<unknown>();
    mockGetChatProviders.mockImplementation(async (chatId: string) =>
      following(chatId === 'chat-1' ? codex : claude)
    );
    mockSetChatProvider.mockReturnValue(put.promise);

    const { result, rerender } = renderHook(({ chatId }) => useChatProviders(chatId), {
      initialProps: { chatId: 'chat-1' as string | null },
    });
    await waitFor(() => {
      expect(result.current.providers).not.toBeNull();
    });

    let choice: Promise<unknown> = Promise.resolve();
    act(() => {
      choice = result.current.choose('claude_code');
    });
    rerender({ chatId: 'chat-2' });
    await waitFor(() => {
      expect(result.current.providers?.default.label).toBe('Claude Code');
    });

    await act(async () => {
      put.resolve({ id: 'chat-1' });
      await choice;
    });

    expect(mockGetChatProviders.mock.calls).toEqual([['chat-1'], ['chat-2']]);
    expect(result.current.providers?.default.label).toBe('Claude Code');
  });

  it('does not call the API without a chat', async () => {
    const { result } = renderHook(() => useChatProviders(null));

    await expect(result.current.choose('codex')).rejects.toThrow('No chat selected');
    expect(mockGetChatProviders).not.toHaveBeenCalled();
    expect(mockSetChatProvider).not.toHaveBeenCalled();
    expect(result.current.providers).toBeNull();
  });
});
