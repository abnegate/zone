import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import { createElement, type ReactNode, StrictMode } from 'react';
import type { ChatWithMessages, Message } from '../types';

const mockGetChat = mock();
const mockSendMessage = mock();
const mockDeleteMessage = mock();
const mockUpdateChat = mock();

// Minimal stand-in for the chat socket: records what the hook sends and lets a
// test push server frames back through onmessage.
class FakeSocket {
  static OPEN = 1;
  readyState = 1;
  sent: string[] = [];
  onopen: (() => void) | null = null;
  onmessage: ((event: { data: string }) => void) | null = null;
  onerror: (() => void) | null = null;
  onclose: (() => void) | null = null;
  send(data: string) {
    this.sent.push(data);
  }
  close() {
    this.readyState = 3;
  }
  addEventListener() {}
  emit(payload: unknown) {
    this.onmessage?.({ data: JSON.stringify(payload) });
  }
}

let lastSocket: FakeSocket | null = null;

mock.module('../../../api/chats', () => ({
  chatsApi: {
    getChat: mockGetChat,
    sendMessage: mockSendMessage,
    deleteMessage: mockDeleteMessage,
    updateChat: mockUpdateChat,
    createChatWebSocket: () => {
      lastSocket = new FakeSocket();
      return lastSocket;
    },
    chatAccessToken: () => 'test-token',
  },
}));

let useChat: typeof import('./useChat').useChat;

beforeAll(async () => {
  ({ useChat } = await import('./useChat'));
});

afterAll(() => {
  mock.restore();
});

const createWrapper = () => {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
      mutations: { retry: false, gcTime: 0 },
    },
  });
  return ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client: queryClient }, children);
};

describe('useChat', () => {
  const mockMessages: Message[] = [
    {
      id: 'm1',
      chat_id: '1',
      role: 'user',
      content: 'Hello',
      created_at: '2024-01-01T00:00:00Z',
    },
    {
      id: 'm2',
      chat_id: '1',
      role: 'assistant',
      content: 'Hi there!',
      created_at: '2024-01-01T00:01:00Z',
    },
  ];

  const mockChat: ChatWithMessages = {
    id: '1',
    title: 'Test Chat',
    model_name: 'gpt-4',
    created_at: '2024-01-01T00:00:00Z',
    updated_at: '2024-01-01T00:01:00Z',
    archived: false,
    agent_enabled: false,
    agent_sandboxed: true,
    messages: mockMessages,
  };

  beforeEach(() => {
    lastSocket = null;
    mockGetChat.mockReset();
    mockSendMessage.mockReset();
    mockDeleteMessage.mockReset();
    mockUpdateChat.mockReset();
  });

  it('should fetch chat on mount', async () => {
    mockGetChat.mockResolvedValue(mockChat);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });

    expect(result.current.loading).toBe(true);

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.chat).toEqual(mockChat);
    expect(result.current.error).toBeNull();
    expect(mockGetChat).toHaveBeenCalledWith('1');
  });

  it('should handle errors when fetching chat', async () => {
    const error = new Error('Failed to fetch chat');
    mockGetChat.mockRejectedValue(error);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.chat).toBeNull();
    expect(result.current.error).toBe('Failed to fetch chat');
  });

  it('should send a message', async () => {
    const newMessage: Message = {
      id: 'm3',
      chat_id: '1',
      role: 'user',
      content: 'New message',
      created_at: '2024-01-01T00:02:00Z',
    };
    mockGetChat.mockResolvedValue(mockChat);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });
    await waitFor(() => {
      expect(lastSocket).not.toBeNull();
    });

    await result.current.sendMessage({ content: 'New message' });

    // The user message goes over the socket, not to POST /messages: the socket
    // is what makes the server generate a reply.
    expect(mockSendMessage).not.toHaveBeenCalled();
    expect(JSON.parse(lastSocket?.sent.at(-1) ?? '{}')).toEqual({
      type: 'send',
      content: 'New message',
    });

    lastSocket?.emit({
      type: 'message_saved',
      message_id: 'm3',
      role: 'user',
      content: 'New message',
    });
    await waitFor(() => {
      expect(result.current.chat?.messages).toHaveLength(3);
    });

    lastSocket?.emit({ type: 'message_start', message_id: 'm4', role: 'assistant' });
    lastSocket?.emit({ type: 'chunk', content: 'Hel', index: 0 });
    lastSocket?.emit({ type: 'chunk', content: 'lo', index: 1 });
    await waitFor(() => {
      expect(result.current.chat?.messages.at(-1)?.content).toBe('Hello');
    });

    lastSocket?.emit({ type: 'message_end', message_id: 'm4', content: 'Hello' });
    await waitFor(() => {
      expect(result.current.streaming).toBe(false);
    });
    expect(result.current.chat?.messages).toHaveLength(4);
  });

  it('builds the tool trace from the agent frames', async () => {
    mockGetChat.mockResolvedValue(mockChat);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });
    await waitFor(() => {
      expect(lastSocket).not.toBeNull();
    });

    lastSocket?.emit({ type: 'message_start', message_id: 'm4', role: 'assistant' });
    lastSocket?.emit({
      type: 'tool_call',
      message_id: 'm4',
      tool_call_id: 'call_1',
      name: 'search_knowledge',
      arguments: '{"query":"deploys"}',
    });

    // While the tool runs the reader should see it as in flight, with the
    // arguments already available.
    await waitFor(() => {
      expect(result.current.chat?.messages.at(-1)?.metadata?.tool_calls).toHaveLength(1);
    });
    const running = result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0];
    expect(running?.pending).toBe(true);
    expect(running?.name).toBe('search_knowledge');
    expect(running?.arguments).toBe('{"query":"deploys"}');

    lastSocket?.emit({
      type: 'tool_result',
      message_id: 'm4',
      tool_call_id: 'call_1',
      name: 'search_knowledge',
      success: true,
      detail: '3 passages',
      duration_ms: 128,
    });

    await waitFor(() => {
      expect(result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0]?.pending).toBe(false);
    });

    // The result frame carries no arguments, so completing a call must not
    // wipe what the start frame recorded.
    const finished = result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0];
    expect(finished?.arguments).toBe('{"query":"deploys"}');
    expect(finished?.success).toBe(true);
    expect(finished?.detail).toBe('3 passages');
    expect(finished?.duration_ms).toBe(128);

    // The reply text arrives after the tool work and must not disturb it.
    lastSocket?.emit({ type: 'chunk', content: 'We deploy on Fridays.', index: 0 });
    lastSocket?.emit({
      type: 'message_end',
      message_id: 'm4',
      content: 'We deploy on Fridays.',
    });

    await waitFor(() => {
      expect(result.current.chat?.messages.at(-1)?.content).toBe('We deploy on Fridays.');
    });
    expect(result.current.chat?.messages.at(-1)?.metadata?.tool_calls).toHaveLength(1);
  });

  it('keeps the tool trace when an image arrives on the same message', async () => {
    mockGetChat.mockResolvedValue(mockChat);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
      expect(lastSocket).not.toBeNull();
    });

    const attachment = {
      name: 'generated-image-1.webp',
      mime: 'image/webp',
      url: 'data:image/webp;base64,generated',
    };
    lastSocket?.emit({ type: 'message_start', message_id: 'm-both', role: 'assistant' });
    lastSocket?.emit({
      type: 'tool_call',
      message_id: 'm-both',
      tool_call_id: 'call_1',
      name: 'run_shell',
      arguments: '{"command":"ls"}',
    });
    lastSocket?.emit({
      type: 'image',
      message_id: 'm-both',
      attachment,
    });

    await waitFor(() => {
      const metadata = result.current.chat?.messages.at(-1)?.metadata;
      expect(metadata?.tool_calls).toHaveLength(1);
      expect(metadata?.attachments).toEqual([attachment]);
    });
  });

  it('persists the agent toggle on the chat', async () => {
    mockGetChat.mockResolvedValue(mockChat);
    mockUpdateChat.mockResolvedValue({ ...mockChat, agent_enabled: true });

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await result.current.setAgentEnabled(true);

    expect(mockUpdateChat).toHaveBeenCalledWith('1', { agent_enabled: true });
    await waitFor(() => {
      expect(result.current.chat?.agent_enabled).toBe(true);
    });
    // Toggling must not disturb the loaded conversation.
    expect(result.current.chat?.messages).toHaveLength(2);
  });

  it('persists the sandbox toggle separately from agent mode', async () => {
    mockGetChat.mockResolvedValue({ ...mockChat, agent_enabled: true });
    mockUpdateChat.mockResolvedValue({
      ...mockChat,
      agent_enabled: true,
      agent_sandboxed: false,
    });

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await result.current.setAgentSandboxed(false);

    // Only the field being changed is sent, so a concurrent change to agent
    // mode from another window is not clobbered.
    expect(mockUpdateChat).toHaveBeenCalledWith('1', { agent_sandboxed: false });
    await waitFor(() => {
      expect(result.current.chat?.agent_sandboxed).toBe(false);
    });
    expect(result.current.chat?.agent_enabled).toBe(true);
  });

  it('should delete a message', async () => {
    mockGetChat.mockResolvedValue(mockChat);
    mockDeleteMessage.mockResolvedValue(undefined);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await result.current.deleteMessage('m1');

    expect(mockDeleteMessage).toHaveBeenCalledWith('1', 'm1');
    await waitFor(() => {
      expect(result.current.chat?.messages).toHaveLength(1);
    });
    expect(result.current.chat?.messages).not.toContainEqual(mockMessages[0]);
  });

  it('should refresh chat', async () => {
    mockGetChat.mockResolvedValue(mockChat);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    const updatedChat: ChatWithMessages = {
      ...mockChat,
      messages: [
        ...mockMessages,
        {
          id: 'm3',
          chat_id: '1',
          role: 'user',
          content: 'Updated message',
          created_at: '2024-01-01T00:02:00Z',
        },
      ],
    };
    mockGetChat.mockResolvedValue(updatedChat);

    await result.current.refresh();

    await waitFor(() => {
      expect(result.current.chat).toEqual(updatedChat);
    });
  });

  it('renders image metadata on the saved user message', async () => {
    mockGetChat.mockResolvedValue(mockChat);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });
    await waitFor(() => {
      expect(lastSocket).not.toBeNull();
    });

    const metadata = {
      attachments: [{ name: 'shot.png', mime: 'image/png', url: 'data:image/png;base64,xx' }],
    };
    await result.current.sendMessage({ content: 'see this', metadata });

    expect(JSON.parse(lastSocket?.sent.at(-1) ?? '{}')).toEqual({
      type: 'send',
      content: 'see this',
      metadata,
    });
    await waitFor(() => {
      expect(result.current.chat?.messages.at(-1)?.metadata).toEqual(metadata);
    });

    lastSocket?.emit({
      type: 'message_saved',
      message_id: 'm-real',
      role: 'user',
      content: 'see this',
      metadata,
    });

    await waitFor(() => {
      const saved = result.current.chat?.messages.find((m) => m.id === 'm-real');
      expect(saved?.metadata).toEqual(metadata);
      expect(result.current.chat?.messages.filter((m) => m.content === 'see this')).toHaveLength(1);
    });
  });

  it('keeps a single user message when message_saved arrives under Strict Mode', async () => {
    mockGetChat.mockResolvedValue(mockChat);
    const QueryWrapper = createWrapper();

    const { result } = renderHook(() => useChat('1'), {
      wrapper: ({ children }: { children: ReactNode }) =>
        createElement(StrictMode, null, createElement(QueryWrapper, null, children)),
    });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });
    await waitFor(() => {
      expect(lastSocket).not.toBeNull();
    });

    await result.current.sendMessage({ content: 'Test message' });
    lastSocket?.emit({
      type: 'message_saved',
      message_id: 'saved-sync',
      role: 'user',
      content: 'Test message',
    });

    await waitFor(() => {
      const matches = result.current.chat?.messages.filter((m) => m.content === 'Test message');
      expect(matches).toHaveLength(1);
      expect(matches?.[0].id).toBe('saved-sync');
    });
  });

  it('serializes sends until the current response completes', async () => {
    mockGetChat.mockResolvedValue(mockChat);
    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
      expect(lastSocket).not.toBeNull();
    });

    await result.current.sendMessage({ content: 'first' });
    await expect(result.current.sendMessage({ content: 'second' })).rejects.toThrow(
      'Wait for the current response to finish'
    );

    lastSocket?.emit({ type: 'message_start', message_id: 'a-first', role: 'assistant' });
    lastSocket?.emit({ type: 'message_end', message_id: 'a-first', content: 'done' });
    await waitFor(() => expect(result.current.streaming).toBe(false));
    await expect(result.current.sendMessage({ content: 'second' })).resolves.toBeUndefined();
  });

  it('adds streamed assistant images and keeps final metadata', async () => {
    mockGetChat.mockResolvedValue(mockChat);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
      expect(lastSocket).not.toBeNull();
    });

    const attachment = {
      name: 'generated-image-1.webp',
      mime: 'image/webp',
      url: 'data:image/webp;base64,generated',
    };
    lastSocket?.emit({ type: 'message_start', message_id: 'm-image', role: 'assistant' });
    lastSocket?.emit({
      type: 'image',
      message_id: 'm-image',
      attachment,
    });

    await waitFor(() => {
      expect(result.current.chat?.messages.at(-1)?.metadata?.attachments).toEqual([attachment]);
    });

    lastSocket?.emit({
      type: 'message_end',
      message_id: 'm-image',
      content: '',
      metadata: { attachments: [attachment] },
    });

    await waitFor(() => {
      expect(result.current.streaming).toBe(false);
      expect(result.current.chat?.messages.at(-1)?.metadata?.attachments).toEqual([attachment]);
    });
  });

  it('should handle sending message with error', async () => {
    mockGetChat.mockResolvedValue(mockChat);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });
    await waitFor(() => {
      expect(lastSocket).not.toBeNull();
    });

    lastSocket?.close();

    await expect(result.current.sendMessage({ content: 'Test' })).rejects.toThrow(
      'Chat connection is not open'
    );
    expect(result.current.chat?.messages).toHaveLength(2); // No new message added
  });

  it('surfaces a status frame while searching the web', async () => {
    mockGetChat.mockResolvedValue(mockChat);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });
    await waitFor(() => {
      expect(lastSocket).not.toBeNull();
    });

    lastSocket?.emit({ type: 'status', message: 'Searching the web...' });

    await waitFor(() => {
      expect(result.current.status).toBe('Searching the web...');
    });

    lastSocket?.emit({ type: 'message_start', message_id: 'a1', role: 'assistant' });

    await waitFor(() => {
      expect(result.current.status).toBeNull();
    });
  });

  it('should not fetch when chatId is null', async () => {
    mockGetChat.mockResolvedValue(mockChat);

    const { result } = renderHook(() => useChat(null), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    expect(result.current.chat).toBeNull();
    expect(mockGetChat).not.toHaveBeenCalled();
  });

  it('should clear chat when chatId becomes null', async () => {
    mockGetChat.mockResolvedValue(mockChat);

    const { result, rerender } = renderHook(({ id }) => useChat(id), {
      wrapper: createWrapper(),
      initialProps: { id: '1' as string | null },
    });

    await waitFor(() => {
      expect(result.current.chat).toEqual(mockChat);
    });

    rerender({ id: null });

    await waitFor(() => {
      expect(result.current.chat).toBeNull();
      expect(result.current.loading).toBe(false);
    });
  });

  it('does not send to a previous chat after waiting for the connection', async () => {
    mockGetChat.mockImplementation((id: string) => Promise.resolve({ ...mockChat, id }));
    const { result, rerender } = renderHook(({ id }) => useChat(id), {
      wrapper: createWrapper(),
      initialProps: { id: '1' },
    });
    await waitFor(() => expect(result.current.chat?.id).toBe('1'));
    const previous = lastSocket;
    const sending = result.current.sendMessage({ content: 'old chat prompt' });
    rerender({ id: '2' });
    await expect(sending).rejects.toThrow('Chat selection changed before the message was sent');
    await waitFor(() => expect(result.current.chat?.id).toBe('2'));
    expect(previous?.sent).toHaveLength(0);
    expect(result.current.streaming).toBe(false);
  });

  it('resets generation state when selecting another chat and ignores the old socket', async () => {
    mockGetChat.mockImplementation((id: string) => Promise.resolve({ ...mockChat, id }));
    const { result, rerender } = renderHook(({ id }) => useChat(id), {
      wrapper: createWrapper(),
      initialProps: { id: '1' as string | null },
    });
    await waitFor(() => expect(result.current.chat?.id).toBe('1'));
    await act(() => result.current.sendMessage({ content: 'first chat' }));
    const previous = lastSocket;
    act(() => previous?.emit({ type: 'status', message: 'Generating image…' }));
    expect(result.current.streaming).toBe(true);

    rerender({ id: '2' });
    await waitFor(() => expect(result.current.chat?.id).toBe('2'));
    expect(result.current.streaming).toBe(false);
    expect(result.current.status).toBeNull();
    await act(() => result.current.sendMessage({ content: 'second chat' }));
    act(() => {
      previous?.onclose?.();
      previous?.emit({ type: 'error', message: 'Old generation failed' });
      previous?.emit({ type: 'status', message: 'Old generation status' });
    });
    expect(result.current.streaming).toBe(true);
    expect(result.current.error).toBeNull();
    expect(result.current.status).toBeNull();

    rerender({ id: null });
    await waitFor(() => expect(result.current.chat).toBeNull());
    expect(result.current.streaming).toBe(false);
    expect(result.current.status).toBeNull();
  });

  it('should not apply a stale response after chatId changes', async () => {
    const otherChat: ChatWithMessages = {
      ...mockChat,
      id: '2',
      title: 'Other Chat',
    };
    let resolveFirst: (value: ChatWithMessages) => void = () => {};
    const first = new Promise<ChatWithMessages>((resolve) => {
      resolveFirst = resolve;
    });
    mockGetChat.mockImplementation((id: string) =>
      id === '1' ? first : Promise.resolve(otherChat)
    );

    const { result, rerender } = renderHook(({ id }) => useChat(id), {
      wrapper: createWrapper(),
      initialProps: { id: '1' as string | null },
    });

    rerender({ id: '2' });

    await waitFor(() => {
      expect(mockGetChat).toHaveBeenCalledWith('2');
    });

    resolveFirst(mockChat);

    await waitFor(() => {
      expect(result.current.chat?.id).toBe('2');
    });
    expect(result.current.chat?.id).not.toBe('1');
  });
});
