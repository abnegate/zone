import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import { createElement, type ReactNode, StrictMode } from 'react';
import {
  AWAITING_ANSWER_DETAIL,
  type ChatWithMessages,
  type ContextUsage,
  type Message,
  type Question,
} from '../types';
import { renderAnswers } from '../utils/answers';

const mockGetChat = mock();
const mockPreviewContext = mock();
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
    previewContext: mockPreviewContext,
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
    messages: mockMessages,
  };

  beforeEach(() => {
    lastSocket = null;
    mockGetChat.mockReset();
    mockSendMessage.mockReset();
    mockDeleteMessage.mockReset();
    mockUpdateChat.mockReset();
  });

  it('keeps a title event that arrives before the initial fetch resolves', async () => {
    let resolve: (chat: ChatWithMessages) => void = () => {};
    mockGetChat.mockImplementation(
      () =>
        new Promise<ChatWithMessages>((done) => {
          resolve = done;
        })
    );
    const updated = mock();
    const { result } = renderHook(() => useChat('1', updated), { wrapper: createWrapper() });
    act(() => lastSocket!.emit({ type: 'title_updated', chat_id: '1', title: 'Generated title' }));
    await act(async () => {
      resolve(mockChat);
    });
    expect(result.current.chat?.title).toBe('Generated title');
    expect(result.current.chat?.messages).toEqual(mockMessages);
    expect(updated).toHaveBeenCalledWith('1', 'Generated title');
  });

  it('ignores a stale socket callback after selection changes', async () => {
    mockGetChat.mockResolvedValue(mockChat);
    const updated = mock();
    const { result, rerender } = renderHook(({ id }) => useChat(id, updated), {
      initialProps: { id: '1' },
      wrapper: createWrapper(),
    });
    await waitFor(() => expect(result.current.chat?.id).toBe('1'));
    const stale = lastSocket!.onmessage!;
    mockGetChat.mockResolvedValue({ ...mockChat, id: '2', title: 'Second chat' });
    rerender({ id: '2' });
    await waitFor(() => expect(result.current.chat?.id).toBe('2'));
    updated.mockClear();
    act(() =>
      stale({ data: JSON.stringify({ type: 'title_updated', chat_id: '1', title: 'Old title' }) })
    );
    expect(result.current.chat?.title).toBe('Second chat');
    expect(updated).not.toHaveBeenCalled();
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

  it('accumulates streamed reasoning into assistant metadata', async () => {
    mockGetChat.mockResolvedValue(mockChat);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });
    await waitFor(() => {
      expect(lastSocket).not.toBeNull();
    });

    lastSocket?.emit({ type: 'message_start', message_id: 'm4', role: 'assistant' });
    lastSocket?.emit({ type: 'reasoning', content: 'Need the capital. ' });
    lastSocket?.emit({ type: 'reasoning', content: 'Paris is the capital.' });
    lastSocket?.emit({ type: 'chunk', content: 'Paris.', index: 0 });
    await waitFor(() => {
      expect(result.current.chat?.messages.at(-1)?.metadata?.reasoning).toBe(
        'Need the capital. Paris is the capital.'
      );
    });

    lastSocket?.emit({
      type: 'message_end',
      message_id: 'm4',
      content: 'Paris.',
      metadata: { reasoning: 'Need the capital. Paris is the capital.' },
    });
    await waitFor(() => {
      expect(result.current.streaming).toBe(false);
    });
    expect(result.current.chat?.messages.at(-1)?.metadata?.reasoning).toBe(
      'Need the capital. Paris is the capital.'
    );
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
  });

  it('keeps the stated reason on the call it arrived with, through approval', async () => {
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
      name: 'run_shell',
      arguments: '{"command":"bun test","reason":"The user asked which tests fail."}',
      reason: 'The user asked which tests fail.',
    });
    lastSocket?.emit({
      type: 'tool_approval_required',
      message_id: 'm4',
      tool_call_id: 'call_1',
      name: 'run_shell',
      arguments: '{"command":"bun test","reason":"The user asked which tests fail."}',
      reason: 'The user asked which tests fail.',
    });

    await waitFor(() => {
      expect(result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0]?.approval).toBe(
        'pending'
      );
    });
    expect(result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0]?.reason).toBe(
      'The user asked which tests fail.'
    );

    lastSocket?.emit({
      type: 'tool_result',
      message_id: 'm4',
      tool_call_id: 'call_1',
      name: 'run_shell',
      success: true,
      detail: '2 failing',
      duration_ms: 900,
    });

    // The result frame carries no reason of its own; the completed row must
    // still show the one the model gave when it asked.
    await waitFor(() => {
      expect(result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0]?.pending).toBe(false);
    });
    expect(result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0]?.reason).toBe(
      'The user asked which tests fail.'
    );
  });

  it('carries the observed preview from the approval frame onto the call', async () => {
    mockGetChat.mockResolvedValue(mockChat);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });
    await waitFor(() => {
      expect(lastSocket).not.toBeNull();
    });

    lastSocket?.emit({ type: 'message_start', message_id: 'm5', role: 'assistant' });
    lastSocket?.emit({
      type: 'tool_approval_required',
      message_id: 'm5',
      tool_call_id: 'call_write',
      name: 'write_file',
      arguments: '{"path":"config.toml","content":"port = 8080"}',
      reason: 'The user asked me to set the port.',
      preview: 'Write 11 characters to config.toml, replacing whatever is there.',
    });

    await waitFor(() => {
      expect(result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0]?.approval).toBe(
        'pending'
      );
    });
    expect(result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0]?.preview).toBe(
      'Write 11 characters to config.toml, replacing whatever is there.'
    );

    lastSocket?.emit({
      type: 'tool_result',
      message_id: 'm5',
      tool_call_id: 'call_write',
      name: 'write_file',
      success: true,
      detail: 'Wrote config.toml',
      duration_ms: 12,
    });

    // The result frame carries no preview of its own; the finished row must
    // still show the action the reader was shown when they allowed it.
    await waitFor(() => {
      expect(result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0]?.pending).toBe(false);
    });
    expect(result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0]?.preview).toBe(
      'Write 11 characters to config.toml, replacing whatever is there.'
    );
  });

  it('attaches streamed reasoning to the following tool call', async () => {
    mockGetChat.mockResolvedValue(mockChat);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
      expect(lastSocket).not.toBeNull();
    });

    lastSocket?.emit({ type: 'message_start', message_id: 'm4', role: 'assistant' });
    lastSocket?.emit({ type: 'reasoning', content: 'Search the workspace first.' });
    await waitFor(() => {
      expect(result.current.chat?.messages.at(-1)?.metadata?.reasoning).toBe(
        'Search the workspace first.'
      );
    });

    lastSocket?.emit({
      type: 'tool_call',
      message_id: 'm4',
      tool_call_id: 'call_1',
      name: 'search_knowledge',
      arguments: '{"query":"deploys"}',
    });

    await waitFor(() => {
      expect(result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0]?.reasoning).toBe(
        'Search the workspace first.'
      );
    });
    expect(result.current.chat?.messages.at(-1)?.metadata?.reasoning).toBeUndefined();

    lastSocket?.emit({ type: 'reasoning', content: 'Now answer from those hits.' });
    lastSocket?.emit({
      type: 'message_end',
      message_id: 'm4',
      content: 'We deploy on Fridays.',
      metadata: {
        tool_calls: [
          {
            id: 'call_1',
            name: 'search_knowledge',
            arguments: '{"query":"deploys"}',
            success: true,
            detail: '3 passages',
            duration_ms: 128,
            reasoning: 'Search the workspace first.',
          },
        ],
        reasoning: 'Now answer from those hits.',
      },
    });

    await waitFor(() => {
      expect(result.current.streaming).toBe(false);
    });
    expect(result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0]?.reasoning).toBe(
      'Search the workspace first.'
    );
    expect(result.current.chat?.messages.at(-1)?.metadata?.reasoning).toBe(
      'Now answer from those hits.'
    );
  });

  it('gives each tool the thinking that preceded it', async () => {
    mockGetChat.mockResolvedValue(mockChat);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
      expect(lastSocket).not.toBeNull();
    });

    lastSocket?.emit({ type: 'message_start', message_id: 'm4', role: 'assistant' });
    lastSocket?.emit({ type: 'reasoning', content: 'Search first.' });
    lastSocket?.emit({
      type: 'tool_call',
      message_id: 'm4',
      tool_call_id: 'call_1',
      name: 'search_knowledge',
      arguments: '{"query":"deploys"}',
    });
    lastSocket?.emit({
      type: 'tool_result',
      message_id: 'm4',
      tool_call_id: 'call_1',
      name: 'search_knowledge',
      success: true,
      detail: '3 passages',
      duration_ms: 128,
    });
    lastSocket?.emit({ type: 'reasoning', content: 'Now read that document.' });
    lastSocket?.emit({
      type: 'tool_call',
      message_id: 'm4',
      tool_call_id: 'call_2',
      name: 'read_document',
      arguments: '{"id":"doc-1"}',
    });

    await waitFor(() => {
      expect(result.current.chat?.messages.at(-1)?.metadata?.tool_calls).toHaveLength(2);
    });
    const calls = result.current.chat?.messages.at(-1)?.metadata?.tool_calls;
    expect(calls?.[0]?.reasoning).toBe('Search first.');
    expect(calls?.[1]?.reasoning).toBe('Now read that document.');
    expect(result.current.chat?.messages.at(-1)?.metadata?.reasoning).toBeUndefined();
  });

  it('keeps tool arguments when the result frame omits them', async () => {
    mockGetChat.mockResolvedValue(mockChat);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
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

  it('attaches streamed citations and keeps them through message_end', async () => {
    mockGetChat.mockResolvedValue(mockChat);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
      expect(lastSocket).not.toBeNull();
    });

    const citation = {
      kind: 'github_build' as const,
      title: 'repository main@aaaaaaa',
      url: 'https://github.com/owner/repository/commit/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
      revision: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
      observed_at: '2026-09-05T00:00:00+00:00',
      complete: false,
      outcome: 'incomplete' as const,
      note: 'Observed CI only',
    };

    lastSocket?.emit({ type: 'message_start', message_id: 'm-cite', role: 'assistant' });
    lastSocket?.emit({
      type: 'tool_result',
      message_id: 'm-cite',
      tool_call_id: 'call_1',
      name: 'get_build_status',
      success: true,
      detail: 'unknown',
      duration_ms: 12,
      citations: [citation],
    });

    await waitFor(() => {
      expect(result.current.chat?.messages.at(-1)?.metadata?.citations).toEqual([citation]);
    });

    lastSocket?.emit({
      type: 'message_end',
      message_id: 'm-cite',
      content: 'Checks are still incomplete.',
      metadata: { citations: [citation] },
    });

    await waitFor(() => {
      expect(result.current.chat?.messages.at(-1)?.content).toBe('Checks are still incomplete.');
    });
    expect(result.current.chat?.messages.at(-1)?.metadata?.citations).toEqual([citation]);
  });

  it('records a workspace write receipt from the live frame and keeps it on reload', async () => {
    const receipt = {
      id: 'call_1',
      action: 'create_task',
      target_type: 'task' as const,
      target_id: 'task-1',
      target_label: 'Ship the billing export',
      actor_id: 'user-1',
      actor_name: 'Alice',
      occurred_at: '2026-09-05T10:47:00.000Z',
      success: true,
      outcome: 'Task created',
      href: '/tasks?id=task-1',
    };
    mockGetChat.mockResolvedValue(mockChat);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
      expect(lastSocket).not.toBeNull();
    });

    lastSocket?.emit({ type: 'message_start', message_id: 'm4', role: 'assistant' });
    lastSocket?.emit({
      type: 'action_receipt',
      message_id: 'm4',
      receipt,
    });
    await waitFor(() => {
      expect(result.current.chat?.messages.at(-1)?.metadata?.action_receipts).toEqual([receipt]);
    });

    lastSocket?.emit({
      type: 'message_end',
      message_id: 'm4',
      content: 'Created the task.',
      metadata: { action_receipts: [receipt] },
    });
    await waitFor(() => {
      expect(result.current.chat?.messages.at(-1)?.content).toBe('Created the task.');
    });
    expect(result.current.chat?.messages.at(-1)?.metadata?.action_receipts).toEqual([receipt]);

    mockGetChat.mockResolvedValue({
      ...mockChat,
      messages: [
        ...mockMessages,
        {
          id: 'm4',
          chat_id: '1',
          role: 'assistant',
          content: 'Created the task.',
          created_at: '2024-01-01T00:02:00Z',
          metadata: { action_receipts: [receipt] },
        },
      ],
    });
    await result.current.refresh();
    await waitFor(() => {
      expect(result.current.chat?.messages.at(-1)?.metadata?.action_receipts).toEqual([receipt]);
    });
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

  it('persists auto-approve on the chat', async () => {
    mockGetChat.mockResolvedValue({ ...mockChat, agent_enabled: true });
    mockUpdateChat.mockResolvedValue({ ...mockChat, agent_enabled: true, auto_approve: true });

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await result.current.setAutoApprove(true);

    expect(mockUpdateChat).toHaveBeenCalledWith('1', { auto_approve: true });
    await waitFor(() => {
      expect(result.current.chat?.auto_approve).toBe(true);
    });
    expect(result.current.chat?.messages).toHaveLength(2);
  });

  it('keeps in-flight tool traces when auto-approve is toggled', async () => {
    mockGetChat.mockResolvedValue({ ...mockChat, agent_enabled: true });
    mockUpdateChat.mockResolvedValue({
      ...mockChat,
      agent_enabled: true,
      auto_approve: true,
      messages: mockMessages,
    });

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
      expect(lastSocket).not.toBeNull();
    });

    lastSocket?.emit({ type: 'message_start', message_id: 'm-live', role: 'assistant' });
    lastSocket?.emit({
      type: 'tool_approval_required',
      message_id: 'm-live',
      tool_call_id: 'call_write',
      name: 'write_file',
      arguments: '{"path":"x.txt"}',
    });

    await waitFor(() => {
      expect(result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0]?.approval).toBe(
        'pending'
      );
    });

    await act(async () => {
      await result.current.setAutoApprove(true);
    });

    expect(result.current.chat?.auto_approve).toBe(true);
    expect(result.current.chat?.messages.at(-1)?.id).toBe('m-live');
    expect(result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0]).toMatchObject({
      id: 'call_write',
      approval: 'approved',
      pending: true,
    });
  });

  it('marks a mutating tool as waiting and sends the decision', async () => {
    mockGetChat.mockResolvedValue(mockChat);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
      expect(lastSocket).not.toBeNull();
    });

    lastSocket?.emit({ type: 'message_start', message_id: 'm4', role: 'assistant' });
    lastSocket?.emit({
      type: 'tool_approval_required',
      message_id: 'm4',
      tool_call_id: 'call_write',
      name: 'write_file',
      arguments: '{"path":"x.txt"}',
    });

    await waitFor(() => {
      expect(result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0]?.approval).toBe(
        'pending'
      );
    });

    act(() => {
      result.current.approveTool('call_write', false);
    });

    expect(JSON.parse(lastSocket?.sent.at(-1) ?? '{}')).toEqual({
      type: 'approve_tool',
      tool_call_id: 'call_write',
      approved: false,
    });
    expect(result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0]?.approval).toBe(
      'denied'
    );
  });

  it('keeps the turn live when a decision finds nothing waiting', async () => {
    mockGetChat.mockResolvedValue(mockChat);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
      expect(lastSocket).not.toBeNull();
    });

    lastSocket?.emit({ type: 'message_start', message_id: 'm4', role: 'assistant' });
    lastSocket?.emit({
      type: 'tool_approval_required',
      message_id: 'm4',
      tool_call_id: 'call_write',
      name: 'write_file',
      arguments: '{"path":"x.txt"}',
    });
    await waitFor(() => {
      expect(result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0]?.approval).toBe(
        'pending'
      );
    });

    act(() => {
      result.current.approveTool('call_write', true);
    });
    act(() => {
      lastSocket?.emit({ type: 'tool_approval_closed', tool_call_id: 'call_write' });
    });

    // Another window decided first. This one never learns which way, so the
    // card stops claiming an outcome and stops offering one.
    await waitFor(() => {
      expect(
        result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0]?.approval
      ).toBeUndefined();
    });
    expect(result.current.streaming).toBe(true);
    expect(result.current.error).toBeNull();

    act(() => {
      lastSocket?.emit({
        type: 'tool_result',
        message_id: 'm4',
        tool_call_id: 'call_write',
        name: 'write_file',
        success: false,
        detail: 'Denied',
        duration_ms: 3,
      });
      lastSocket?.emit({
        type: 'message_end',
        message_id: 'm4',
        content: 'I left the file alone.',
      });
    });

    await waitFor(() => {
      expect(result.current.chat?.messages.at(-1)?.content).toBe('I left the file alone.');
    });
    const call = result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0];
    expect(call?.detail).toBe('Denied');
    expect(call?.success).toBe(false);
    expect(call?.pending).toBe(false);
    expect(result.current.streaming).toBe(false);
    expect(result.current.error).toBeNull();
  });

  it('stops offering a decision another window has already made', async () => {
    mockGetChat.mockResolvedValue(mockChat);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
      expect(lastSocket).not.toBeNull();
    });

    lastSocket?.emit({ type: 'message_start', message_id: 'm4', role: 'assistant' });
    lastSocket?.emit({
      type: 'tool_approval_required',
      message_id: 'm4',
      tool_call_id: 'call_write',
      name: 'write_file',
      arguments: '{"path":"x.txt"}',
    });
    await waitFor(() => {
      expect(result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0]?.approval).toBe(
        'pending'
      );
    });

    // Another window denied the call. This one only ever hears the result,
    // which settles the call for every window: there is nothing left to decide.
    act(() => {
      lastSocket?.emit({
        type: 'tool_result',
        message_id: 'm4',
        tool_call_id: 'call_write',
        name: 'write_file',
        success: false,
        detail: 'Error: The user denied this tool call.',
        duration_ms: 0,
      });
    });

    await waitFor(() => {
      expect(result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0]?.pending).toBe(false);
    });
    const call = result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0];
    expect(call?.approval).toBeUndefined();
    expect(call?.detail).toBe('Error: The user denied this tool call.');
    expect(call?.success).toBe(false);
    expect(result.current.streaming).toBe(true);
  });

  it('does not assume an outcome for a call that already has one', async () => {
    mockGetChat.mockResolvedValue(mockChat);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
      expect(lastSocket).not.toBeNull();
    });

    lastSocket?.emit({ type: 'message_start', message_id: 'm4', role: 'assistant' });
    lastSocket?.emit({
      type: 'tool_approval_required',
      message_id: 'm4',
      tool_call_id: 'call_write',
      name: 'write_file',
      arguments: '{"path":"x.txt"}',
    });
    await waitFor(() => {
      expect(result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0]?.approval).toBe(
        'pending'
      );
    });
    act(() => {
      lastSocket?.emit({
        type: 'tool_result',
        message_id: 'm4',
        tool_call_id: 'call_write',
        name: 'write_file',
        success: false,
        detail: 'Error: The user denied this tool call.',
        duration_ms: 0,
      });
    });
    await waitFor(() => {
      expect(result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0]?.pending).toBe(false);
    });

    // A decision sent after the result cannot change what happened, so the row
    // keeps saying what the server said rather than "Approved. Running…".
    act(() => {
      result.current.approveTool('call_write', true);
    });
    act(() => {
      lastSocket?.emit({ type: 'tool_approval_closed', tool_call_id: 'call_write' });
    });

    const call = result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0];
    expect(call?.detail).toBe('Error: The user denied this tool call.');
    expect(call?.pending).toBe(false);
    expect(call?.success).toBe(false);
    expect(call?.approval).toBeUndefined();
    expect(result.current.streaming).toBe(true);
    expect(result.current.error).toBeNull();
  });

  it('keeps a result that lands in the same tick as the decision', async () => {
    mockGetChat.mockResolvedValue(mockChat);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
      expect(lastSocket).not.toBeNull();
    });

    lastSocket?.emit({ type: 'message_start', message_id: 'm4', role: 'assistant' });
    lastSocket?.emit({
      type: 'tool_approval_required',
      message_id: 'm4',
      tool_call_id: 'call_write',
      name: 'write_file',
      arguments: '{"path":"x.txt"}',
    });
    await waitFor(() => {
      expect(result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0]?.approval).toBe(
        'pending'
      );
    });

    // The result and the click queue before React renders either: the card
    // this window drew still says pending, but the state the updaters see
    // does not, and the result must be what survives.
    act(() => {
      lastSocket?.emit({
        type: 'tool_result',
        message_id: 'm4',
        tool_call_id: 'call_write',
        name: 'write_file',
        success: false,
        detail: 'Error: The user denied this tool call.',
        duration_ms: 0,
      });
      result.current.approveTool('call_write', true);
    });

    const call = result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0];
    expect(call?.detail).toBe('Error: The user denied this tool call.');
    expect(call?.pending).toBe(false);
    expect(call?.success).toBe(false);
    expect(call?.approval).toBeUndefined();
    expect(JSON.parse(lastSocket?.sent.at(-1) ?? '{}')).toEqual({
      type: 'approve_tool',
      tool_call_id: 'call_write',
      approved: true,
    });
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

  it('keeps reminder messages separate from a pending user message and streaming reply', async () => {
    mockGetChat.mockResolvedValue(mockChat);
    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });
    await waitFor(() => expect(result.current.loading).toBe(false));
    await waitFor(() => expect(lastSocket).not.toBeNull());
    await act(async () => {
      await result.current.sendMessage({ content: 'Pending request' });
    });
    lastSocket?.emit({ type: 'message_start', message_id: 'streaming', role: 'assistant' });
    lastSocket?.emit({ type: 'chunk', content: 'Partial', index: 0 });
    const reminder = {
      type: 'message_saved',
      message_id: 'reminder',
      role: 'assistant',
      content: 'Follow up Friday',
      metadata: { source: 'reminder' },
    };
    lastSocket?.emit(reminder);
    lastSocket?.emit(reminder);
    lastSocket?.emit({ type: 'chunk', content: ' reply', index: 1 });
    await waitFor(() => {
      const messages = result.current.chat?.messages ?? [];
      expect(messages.filter((message) => message.id === 'reminder')).toHaveLength(1);
      expect(messages.find((message) => message.id === 'reminder')?.role).toBe('assistant');
      expect(messages.find((message) => message.id === 'streaming')?.content).toBe('Partial reply');
      expect(messages.some((message) => message.content === 'Pending request')).toBe(true);
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

  it('removes an unsaved user message when generation is cancelled', async () => {
    mockGetChat.mockResolvedValue(mockChat);
    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });
    await waitFor(() => expect(result.current.chat).toEqual(mockChat));

    await act(() => result.current.sendMessage({ content: 'Cancelled prompt' }));
    expect(result.current.chat?.messages).toHaveLength(3);
    act(() => lastSocket?.emit({ type: 'cancelled', message_id: 'cancelled-response' }));

    expect(result.current.chat?.messages).toEqual(mockMessages);
    expect(result.current.streaming).toBe(false);
    expect(result.current.status).toBeNull();
  });

  it('saves only the immediate retry when cancellation precedes message_saved under Strict Mode', async () => {
    mockGetChat.mockResolvedValue(mockChat);
    const QueryWrapper = createWrapper();
    const { result } = renderHook(() => useChat('1'), {
      wrapper: ({ children }: { children: ReactNode }) =>
        createElement(StrictMode, null, createElement(QueryWrapper, null, children)),
    });
    await waitFor(() => expect(result.current.chat).toEqual(mockChat));

    await act(async () => {
      await result.current.sendMessage({ content: 'Cancelled prompt' });
      lastSocket?.emit({ type: 'cancelled', message_id: 'cancelled-response' });
      await result.current.sendMessage({ content: 'Retry prompt' });
      lastSocket?.emit({
        type: 'message_saved',
        message_id: 'saved-retry',
        role: 'user',
        content: 'Retry prompt',
      });
    });

    expect(result.current.chat?.messages).toHaveLength(3);
    expect(result.current.chat?.messages.at(-1)?.id).toBe('saved-retry');
    expect(result.current.chat?.messages.at(-1)?.content).toBe('Retry prompt');
    expect(result.current.chat?.messages.some((message) => message.id.startsWith('pending-'))).toBe(
      false
    );
    expect(result.current.streaming).toBe(true);
  });

  it('retains a saved user message when cancellation follows message_saved', async () => {
    mockGetChat.mockResolvedValue(mockChat);
    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });
    await waitFor(() => expect(result.current.chat).toEqual(mockChat));

    await act(async () => {
      await result.current.sendMessage({ content: 'Saved prompt' });
      lastSocket?.emit({
        type: 'message_saved',
        message_id: 'saved-user',
        role: 'user',
        content: 'Saved prompt',
      });
      lastSocket?.emit({ type: 'cancelled', message_id: 'cancelled-response' });
    });

    expect(result.current.chat?.messages).toHaveLength(3);
    expect(result.current.chat?.messages.at(-1)?.id).toBe('saved-user');
    expect(result.current.chat?.messages.at(-1)?.content).toBe('Saved prompt');
    expect(result.current.streaming).toBe(false);
  });

  it('applies a failed terminal snapshot and preserves the interruption after retry', async () => {
    mockGetChat.mockResolvedValue(mockChat);
    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });
    await waitFor(() => expect(result.current.chat).toEqual(mockChat));
    const attachment = {
      name: 'generated.webp',
      mime: 'image/webp',
      url: 'data:image/webp;base64,generated',
    };
    const content = 'Partial response\n\n[Response interrupted]';

    await act(async () => {
      await result.current.sendMessage({ content: 'First prompt' });
      lastSocket?.emit({
        type: 'message_saved',
        message_id: 'saved-first',
        role: 'user',
        content: 'First prompt',
      });
      lastSocket?.emit({ type: 'message_start', message_id: 'partial', role: 'assistant' });
      lastSocket?.emit({ type: 'chunk', content: 'Partial response', index: 0 });
      lastSocket?.emit({
        type: 'tool_call',
        message_id: 'partial',
        tool_call_id: 'tool-first',
        name: 'search_knowledge',
        arguments: '{}',
      });
      lastSocket?.emit({
        type: 'message_end',
        message_id: 'partial',
        content,
        metadata: { attachments: [attachment] },
        error: 'The model connection was interrupted',
      });
    });

    expect(result.current.error).toBe('The model connection was interrupted');
    expect(result.current.streaming).toBe(false);
    expect(result.current.chat?.messages.at(-1)?.content).toBe(content);
    expect(result.current.chat?.messages.at(-1)?.metadata?.attachments).toEqual([attachment]);
    expect(result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0]?.id).toBe('tool-first');

    await act(() => result.current.sendMessage({ content: 'Retry prompt' }));
    expect(result.current.error).toBeNull();
    expect(result.current.streaming).toBe(true);
    expect(result.current.chat?.messages.find((message) => message.id === 'partial')?.content).toBe(
      content
    );
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

  it('adds streamed assistant videos and keeps final metadata', async () => {
    mockGetChat.mockResolvedValue(mockChat);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
      expect(lastSocket).not.toBeNull();
    });

    const attachment = {
      name: 'generated-video-1.webm',
      mime: 'video/webm',
      url: '/api/artifacts/ws/chat/msg/generated-video-1.webm',
    };
    lastSocket?.emit({ type: 'message_start', message_id: 'm-video', role: 'assistant' });
    lastSocket?.emit({
      type: 'video',
      message_id: 'm-video',
      attachment,
    });

    await waitFor(() => {
      expect(result.current.chat?.messages.at(-1)?.metadata?.attachments).toEqual([attachment]);
    });

    lastSocket?.emit({
      type: 'message_end',
      message_id: 'm-video',
      content: 'Generated video.',
      metadata: { attachments: [attachment] },
    });

    await waitFor(() => {
      expect(result.current.streaming).toBe(false);
      expect(result.current.chat?.messages.at(-1)?.content).toBe('Generated video.');
      expect(result.current.chat?.messages.at(-1)?.metadata?.attachments).toEqual([attachment]);
    });
  });

  it('adds streamed assistant audio and keeps final metadata', async () => {
    mockGetChat.mockResolvedValue(mockChat);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
      expect(lastSocket).not.toBeNull();
    });

    const attachment = {
      name: 'generated-audio-1.flac',
      mime: 'audio/flac',
      url: '/api/artifacts/ws/chat/msg/generated-audio-1.flac',
    };
    lastSocket?.emit({ type: 'message_start', message_id: 'm-audio', role: 'assistant' });
    lastSocket?.emit({
      type: 'audio',
      message_id: 'm-audio',
      attachment,
    });

    await waitFor(() => {
      expect(result.current.chat?.messages.at(-1)?.metadata?.attachments).toEqual([attachment]);
    });

    lastSocket?.emit({
      type: 'message_end',
      message_id: 'm-audio',
      content: 'Generated audio.',
      metadata: { attachments: [attachment] },
    });

    await waitFor(() => {
      expect(result.current.streaming).toBe(false);
      expect(result.current.chat?.messages.at(-1)?.content).toBe('Generated audio.');
      expect(result.current.chat?.messages.at(-1)?.metadata?.attachments).toEqual([attachment]);
    });
  });

  it('applies frames that arrived before the chat finished loading', async () => {
    let resolve: (chat: ChatWithMessages) => void = () => {};
    mockGetChat.mockImplementation(
      () =>
        new Promise<ChatWithMessages>((done) => {
          resolve = done;
        })
    );
    const { result, unmount } = renderHook(() => useChat('1'), { wrapper: createWrapper() });

    // The socket connects, and the server replays the turn in flight, while
    // the chat those frames belong to is still being fetched.
    act(() => {
      lastSocket?.emit({
        type: 'message_start',
        message_id: 'live',
        role: 'assistant',
        resumed: true,
      });
      lastSocket?.emit({ type: 'chunk', content: 'Half a reply', index: 0 });
      lastSocket?.emit({
        type: 'tool_call',
        message_id: 'live',
        tool_call_id: 'call-1',
        name: 'read_file',
        arguments: '{}',
      });
    });

    await act(async () => {
      resolve(mockChat);
    });

    await waitFor(() => {
      const live = result.current.chat?.messages.find((message) => message.id === 'live');
      expect(live?.content).toBe('Half a reply');
      expect(live?.metadata?.tool_calls?.[0]?.detail).toBe('Running…');
      expect(result.current.streaming).toBe(true);
    });
    unmount();
  });

  it('drops a held reply too long to reassemble and settles it at the end', async () => {
    let resolve: (chat: ChatWithMessages) => void = () => {};
    mockGetChat.mockImplementation(
      () =>
        new Promise<ChatWithMessages>((done) => {
          resolve = done;
        })
    );
    const { result, unmount } = renderHook(() => useChat('1'), { wrapper: createWrapper() });

    act(() => {
      lastSocket?.emit({ type: 'message_start', message_id: 'live', role: 'assistant' });
      for (let index = 0; index < 1100; index += 1) {
        lastSocket?.emit({ type: 'chunk', content: 'x', index });
      }
    });

    await act(async () => {
      resolve(mockChat);
    });
    expect(result.current.chat?.messages.find((message) => message.id === 'live')).toBeUndefined();

    act(() =>
      lastSocket?.emit({ type: 'message_end', message_id: 'live', content: 'The whole reply' })
    );
    await waitFor(() => {
      expect(result.current.chat?.messages.at(-1)?.content).toBe('The whole reply');
    });
    unmount();
  });

  it('takes the reply back up when a dropped socket replays the turn', async () => {
    mockGetChat.mockResolvedValue(mockChat);
    const { result, unmount } = renderHook(() => useChat('1'), { wrapper: createWrapper() });
    await waitFor(() => expect(result.current.loading).toBe(false));
    act(() => {
      lastSocket?.emit({ type: 'message_start', message_id: 'live', role: 'assistant' });
      lastSocket?.emit({ type: 'chunk', content: 'Half a', index: 0 });
      lastSocket?.emit({
        type: 'tool_call',
        message_id: 'live',
        tool_call_id: 'call-1',
        name: 'read_file',
        arguments: '{}',
      });
    });
    const dropped = lastSocket;
    act(() => dropped?.onclose?.());
    await waitFor(() => expect(lastSocket).not.toBe(dropped));

    act(() => {
      lastSocket?.emit({
        type: 'message_start',
        message_id: 'live',
        role: 'assistant',
        resumed: true,
      });
      lastSocket?.emit({ type: 'chunk', content: 'Half a reply', index: 0 });
      lastSocket?.emit({
        type: 'tool_call',
        message_id: 'live',
        tool_call_id: 'call-1',
        name: 'read_file',
        arguments: '{}',
      });
    });

    await waitFor(() => {
      const revived = result.current.chat?.messages.filter((message) => message.id === 'live');
      expect(revived).toHaveLength(1);
      expect(revived?.[0]?.content).toBe('Half a reply');
      expect(revived?.[0]?.metadata?.tool_calls?.[0]?.detail).toBe('Running…');
      expect(result.current.streaming).toBe(true);
    });

    act(() =>
      lastSocket?.emit({ type: 'message_end', message_id: 'live', content: 'Half a reply, done' })
    );
    await waitFor(() => {
      expect(result.current.streaming).toBe(false);
      expect(result.current.chat?.messages.at(-1)?.content).toBe('Half a reply, done');
    });
    unmount();
  });

  it('keeps the saved partial when a reload joins the turn that wrote it', async () => {
    mockGetChat.mockResolvedValue({
      ...mockChat,
      messages: [
        ...mockMessages,
        {
          id: 'live',
          chat_id: '1',
          role: 'assistant',
          content: 'Half a',
          created_at: '2024-01-01T00:02:00Z',
        },
      ],
    });
    const { result, unmount } = renderHook(() => useChat('1'), { wrapper: createWrapper() });
    await waitFor(() => expect(result.current.loading).toBe(false));

    act(() =>
      lastSocket?.emit({
        type: 'message_start',
        message_id: 'live',
        role: 'assistant',
        resumed: true,
      })
    );
    expect(
      result.current.chat?.messages.find((message) => message.id === 'live')?.content,
      'a resumed start never blanks what was already saved'
    ).toBe('Half a');

    act(() => lastSocket?.emit({ type: 'chunk', content: 'Half a reply', index: 0 }));
    await waitFor(() => {
      const rows = result.current.chat?.messages.filter((message) => message.id === 'live');
      expect(rows).toHaveLength(1);
      expect(rows?.[0]?.content).toBe('Half a reply');
    });
    unmount();
  });

  it('reconnects after the socket drops so a later send still works', async () => {
    mockGetChat.mockResolvedValue(mockChat);
    const { result, unmount } = renderHook(() => useChat('1'), { wrapper: createWrapper() });
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
      expect(lastSocket).not.toBeNull();
    });
    const dropped = lastSocket;
    act(() => dropped?.onclose?.());
    await waitFor(() => {
      expect(lastSocket).not.toBe(dropped);
    });
    await act(async () => {
      await result.current.sendMessage({ content: 'After reconnect' });
    });
    expect(lastSocket?.sent.some((frame) => frame.includes('After reconnect'))).toBe(true);
    unmount();
  });

  it('a socket the server refuses before init is not reopened', async () => {
    mockGetChat.mockResolvedValue(mockChat);
    const { result, unmount } = renderHook(() => useChat('1'), { wrapper: createWrapper() });
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
      expect(lastSocket).not.toBeNull();
    });
    const refused = lastSocket;
    act(() => refused?.onopen?.());
    act(() => refused?.emit({ type: 'error', message: 'Access denied' }));
    act(() => refused?.onclose?.());
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(lastSocket, 'a refused connection must not be retried').toBe(refused);
    expect(result.current.error).toBe('Access denied');
    unmount();
  });

  it('only a connection the server announced resets the reconnect backoff', async () => {
    mockGetChat.mockResolvedValue(mockChat);
    const { result, unmount } = renderHook(() => useChat('1'), { wrapper: createWrapper() });
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
      expect(lastSocket).not.toBeNull();
    });
    const first = lastSocket;
    act(() => first?.onopen?.());
    act(() => first?.onclose?.());
    await waitFor(() => expect(lastSocket).not.toBe(first));
    const second = lastSocket;
    act(() => second?.onopen?.());
    act(() => second?.onclose?.());
    await new Promise((resolve) => setTimeout(resolve, 100));
    expect(lastSocket, 'a second drop before init waits out the backoff').toBe(second);
    await waitFor(() => expect(lastSocket).not.toBe(second), { timeout: 2_000 });
    unmount();
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

  const scope: Question = {
    header: 'Scope',
    question: 'How far should this go?',
    choices: [
      {
        label: 'Backfill',
        description: 'Rewrite every existing row.',
        recommended: true,
        free_text: false,
      },
      {
        label: 'Forward only',
        description: 'Leave the existing rows alone.',
        recommended: false,
        free_text: false,
      },
      {
        label: 'Other',
        description: 'Something else — type it below.',
        recommended: false,
        free_text: true,
      },
    ],
    multi_select: false,
    required: true,
  };

  it('puts the questions the agent asked on the call that asked them', async () => {
    mockGetChat.mockResolvedValue(mockChat);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
      expect(lastSocket).not.toBeNull();
    });

    lastSocket?.emit({ type: 'message_start', message_id: 'm6', role: 'assistant' });
    lastSocket?.emit({
      type: 'tool_call',
      message_id: 'm6',
      tool_call_id: 'call_ask',
      name: 'ask_user',
      arguments: '{"questions":[{"header":"Scope"}]}',
    });
    lastSocket?.emit({
      type: 'question_required',
      message_id: 'm6',
      tool_call_id: 'call_ask',
      questions: [scope],
    });

    await waitFor(() => {
      expect(result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0]?.questions).toEqual([
        scope,
      ]);
    });
    const asked = result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0];
    expect(asked?.name).toBe('ask_user');
    expect(asked?.detail).toBe(AWAITING_ANSWER_DETAIL);
  });

  it('keeps the questions it can read from a live frame, as a reload would', async () => {
    mockGetChat.mockResolvedValue(mockChat);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
      expect(lastSocket).not.toBeNull();
    });

    lastSocket?.emit({ type: 'message_start', message_id: 'm6', role: 'assistant' });
    lastSocket?.emit({
      type: 'question_required',
      message_id: 'm6',
      tool_call_id: 'call_ask',
      questions: [
        { header: 'Rollout', question: 'How fast?', multi_select: false, required: true },
        scope,
      ],
    });

    await waitFor(() => {
      expect(result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0]?.questions).toEqual([
        scope,
      ]);
    });
    expect(result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0]?.detail).toBe(
      AWAITING_ANSWER_DETAIL
    );
  });

  it('leaves the call as the tool result left it when a live frame cannot be read at all', async () => {
    mockGetChat.mockResolvedValue(mockChat);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
      expect(lastSocket).not.toBeNull();
    });

    act(() => {
      lastSocket?.emit({ type: 'message_start', message_id: 'm6', role: 'assistant' });
      lastSocket?.emit({
        type: 'tool_result',
        message_id: 'm6',
        tool_call_id: 'call_ask',
        name: 'ask_user',
        success: true,
        detail: 'Asked',
        duration_ms: 0,
      });
      lastSocket?.emit({
        type: 'question_required',
        message_id: 'm6',
        tool_call_id: 'call_ask',
        questions: { nope: 1 },
      });
    });

    const asked = result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0];
    expect(asked?.questions).toBeUndefined();
    expect(asked?.detail).toBe('Asked');
  });

  it('leaves the call as the tool result left it when every question on a live frame is unreadable', async () => {
    mockGetChat.mockResolvedValue(mockChat);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
      expect(lastSocket).not.toBeNull();
    });

    act(() => {
      lastSocket?.emit({ type: 'message_start', message_id: 'm6', role: 'assistant' });
      lastSocket?.emit({
        type: 'tool_result',
        message_id: 'm6',
        tool_call_id: 'call_ask',
        name: 'ask_user',
        success: true,
        detail: 'Asked',
        duration_ms: 0,
      });
      lastSocket?.emit({
        type: 'question_required',
        message_id: 'm6',
        tool_call_id: 'call_ask',
        questions: [
          { header: 'Rollout', question: 'How fast?', multi_select: false, required: true },
        ],
      });
    });

    const asked = result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0];
    expect(asked?.questions).toBeUndefined();
    expect(asked?.detail).toBe('Asked');
  });

  it('leaves the call that asked finished, since the card is what is waiting', async () => {
    mockGetChat.mockResolvedValue(mockChat);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
      expect(lastSocket).not.toBeNull();
    });

    lastSocket?.emit({ type: 'message_start', message_id: 'm6', role: 'assistant' });
    lastSocket?.emit({
      type: 'tool_call',
      message_id: 'm6',
      tool_call_id: 'call_ask',
      name: 'ask_user',
      arguments: '{"questions":[{"header":"Scope"}]}',
    });
    lastSocket?.emit({
      type: 'tool_result',
      message_id: 'm6',
      tool_call_id: 'call_ask',
      name: 'ask_user',
      success: true,
      detail: 'Asked',
      duration_ms: 0,
    });
    lastSocket?.emit({
      type: 'question_required',
      message_id: 'm6',
      tool_call_id: 'call_ask',
      questions: [scope],
    });

    await waitFor(() => {
      expect(result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0]?.questions).toEqual([
        scope,
      ]);
    });
    const asked = result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0];
    expect(asked?.pending).toBe(false);
    expect(asked?.success).toBe(true);
    expect(asked?.detail).toBe(AWAITING_ANSWER_DETAIL);
  });

  it('sends the rendered answer as an ordinary message rather than a frame of its own', async () => {
    mockGetChat.mockResolvedValue(mockChat);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
      expect(lastSocket).not.toBeNull();
    });

    lastSocket?.emit({ type: 'message_start', message_id: 'm6', role: 'assistant' });
    lastSocket?.emit({
      type: 'question_required',
      message_id: 'm6',
      tool_call_id: 'call_ask',
      questions: [scope],
    });
    lastSocket?.emit({ type: 'message_end', message_id: 'm6', content: '' });

    const content = renderAnswers(
      [scope],
      [{ header: 'Scope', labels: ['Other'], other: 'Only the backlog' }]
    );
    await act(async () => {
      await result.current.sendMessage({ content });
    });

    expect(lastSocket?.sent).toContain(
      JSON.stringify({ type: 'send', content: 'Scope: Other: Only the backlog' })
    );
  });

  it('leaves an unanswered question standing when the reader stops the reply', async () => {
    mockGetChat.mockResolvedValue(mockChat);

    const { result } = renderHook(() => useChat('1'), { wrapper: createWrapper() });
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
      expect(lastSocket).not.toBeNull();
    });

    act(() => {
      lastSocket?.emit({ type: 'message_start', message_id: 'm6', role: 'assistant' });
      lastSocket?.emit({ type: 'chunk', content: 'Before I start…', index: 0 });
      lastSocket?.emit({
        type: 'question_required',
        message_id: 'm6',
        tool_call_id: 'call_ask',
        questions: [scope],
      });
    });

    await waitFor(() => {
      expect(result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0]?.questions).toEqual([
        scope,
      ]);
    });

    act(() => {
      lastSocket?.emit({ type: 'cancelled', message_id: 'm6' });
    });

    const stopped = result.current.chat?.messages.at(-1)?.metadata?.tool_calls?.[0];
    expect(stopped?.questions).toEqual([scope]);
    expect(stopped?.approval).toBeUndefined();
    expect(stopped?.detail).toBe(AWAITING_ANSWER_DETAIL);
  });
});

const usage: ContextUsage = {
  model: 'gpt-4',
  used: 100,
  limit: 1000,
  reserved: 100,
  threshold: 800,
  remaining: 700,
  estimated: true,
  incomplete: false,
  source: 'configured',
  status: 'ready',
  revision: 0,
  compacted_messages: 0,
  updated_at: '2026-09-06T00:00:00Z',
  breakdown: {
    instructions: 20,
    conversation: 60,
    tools: 0,
    results: 0,
    summary: 0,
    attachments: 0,
    overhead: 20,
  },
};
const contextChat: ChatWithMessages = {
  id: 'context',
  title: 'Context',
  model_name: 'gpt-4',
  created_at: '',
  updated_at: '',
  archived: false,
  agent_enabled: false,
  messages: [],
  context: usage,
};

describe('context freshness', () => {
  beforeEach(() => {
    mockGetChat.mockResolvedValue(contextChat);
    mockPreviewContext.mockReset();
  });
  it('restores context and previews the draft', async () => {
    mockPreviewContext.mockResolvedValue({ ...usage, used: 120 });
    const { result, unmount } = renderHook(() =>
      useChat('context', undefined, { content: 'draft' })
    );
    await waitFor(() => expect(result.current.context?.used).toBe(100));
    await waitFor(() => expect(result.current.context?.used).toBe(120));
    expect(mockPreviewContext.mock.calls[0][1]).toEqual({ content: 'draft' });
    unmount();
  });
  it('does not replace live usage with an older HTTP preview at the same revision', async () => {
    let resolve!: (value: ContextUsage) => void;
    mockPreviewContext.mockImplementation(
      () =>
        new Promise<ContextUsage>((done) => {
          resolve = done;
        })
    );
    const { result, unmount } = renderHook(() =>
      useChat('context', undefined, { content: 'draft' })
    );
    await waitFor(() => expect(mockPreviewContext).toHaveBeenCalled());
    act(() => {
      lastSocket?.emit({ type: 'message_start', message_id: 'generation', role: 'assistant' });
      lastSocket?.emit({
        type: 'context',
        chat_id: 'context',
        message_id: 'generation',
        usage: { ...usage, used: 200 },
      });
    });
    await act(async () => resolve({ ...usage, used: 150 }));
    expect(result.current.context?.used).toBe(200);
    unmount();
  });
  it('rejects wrong chat, old generation and post-terminal context frames', async () => {
    const { result, unmount } = renderHook(() => useChat('context'));
    await waitFor(() => expect(result.current.chat).not.toBeNull());
    act(() => {
      lastSocket?.emit({ type: 'message_start', message_id: 'current', role: 'assistant' });
      lastSocket?.emit({
        type: 'context',
        chat_id: 'wrong',
        message_id: 'current',
        usage: { ...usage, used: 900 },
      });
      lastSocket?.emit({
        type: 'context',
        chat_id: 'context',
        message_id: 'old',
        usage: { ...usage, used: 900 },
      });
    });
    expect(result.current.context?.used).toBe(100);
    act(() => {
      lastSocket?.emit({
        type: 'context',
        chat_id: 'context',
        message_id: 'current',
        usage: { ...usage, used: 200 },
      });
      lastSocket?.emit({ type: 'message_end', message_id: 'current', content: 'Done' });
      lastSocket?.emit({
        type: 'context',
        chat_id: 'context',
        message_id: 'current',
        usage: { ...usage, used: 900 },
      });
    });
    expect(result.current.context?.used).toBe(200);
    unmount();
  });
  it('ignores wrong-model live usage and invalidates capacity when the model changes', async () => {
    const { result, unmount } = renderHook(() => useChat('context'));
    await waitFor(() => expect(result.current.context?.limit).toBe(1000));
    act(() => {
      lastSocket?.emit({ type: 'message_start', message_id: 'generation', role: 'assistant' });
      lastSocket?.emit({
        type: 'context',
        chat_id: 'context',
        message_id: 'generation',
        usage: { ...usage, model: 'wrong-model', used: 900 },
      });
    });
    expect(result.current.context?.used).toBe(100);
    mockGetChat.mockResolvedValue({ ...contextChat, model_name: 'new-model' });
    await act(async () => result.current.refresh());
    expect(result.current.context).toBeNull();
    unmount();
  });
  it('rejects an old chat preview even when the server ignores abort', async () => {
    let resolve!: (value: ContextUsage) => void;
    mockPreviewContext.mockImplementationOnce(
      () =>
        new Promise<ContextUsage>((done) => {
          resolve = done;
        })
    );
    const { result, rerender, unmount } = renderHook(
      ({ id }) => useChat(id, undefined, { content: 'draft' }),
      { initialProps: { id: 'context' } }
    );
    await waitFor(() => expect(mockPreviewContext).toHaveBeenCalled());
    mockGetChat.mockResolvedValue({ ...contextChat, id: 'other', context: null });
    mockPreviewContext.mockResolvedValue({ ...usage, used: 300 });
    rerender({ id: 'other' });
    await waitFor(() => expect(result.current.chat?.id).toBe('other'));
    await act(async () => resolve({ ...usage, used: 900 }));
    expect(result.current.context?.used).not.toBe(900);
    unmount();
  });
  it('rejects malformed preview values and invalidates attachment changes', async () => {
    mockPreviewContext.mockResolvedValue({ ...usage, used: -100 });
    const { result, rerender, unmount } = renderHook(
      ({ url }) =>
        useChat('context', undefined, {
          content: '',
          metadata: { attachments: [{ name: 'image', mime: 'image/png', url }] },
        }),
      { initialProps: { url: '/first.png' } }
    );
    await waitFor(() => expect(result.current.contextError).toContain('unavailable'));
    mockPreviewContext.mockResolvedValue(usage);
    rerender({ url: '/second.png' });
    await waitFor(() => expect(result.current.context?.used).toBe(100));
    expect(mockPreviewContext.mock.calls.at(-1)?.[1].metadata.attachments[0].url).toBe(
      '/second.png'
    );
    unmount();
  });
  it('does not let stale terminal frames close the current generation', async () => {
    const { result, unmount } = renderHook(() => useChat('context'));
    await waitFor(() => expect(result.current.chat).not.toBeNull());
    act(() => {
      lastSocket?.emit({ type: 'message_start', message_id: 'old', role: 'assistant' });
      lastSocket?.emit({ type: 'message_end', message_id: 'old', content: 'Old response' });
      lastSocket?.emit({ type: 'message_start', message_id: 'current', role: 'assistant' });
      lastSocket?.emit({ type: 'message_end', message_id: 'old', content: 'Late response' });
      lastSocket?.emit({ type: 'cancelled', message_id: 'old' });
      lastSocket?.emit({ type: 'cancelled', message_id: null });
      lastSocket?.emit({
        type: 'context',
        chat_id: 'context',
        message_id: 'current',
        usage: { ...usage, used: 200 },
      });
    });
    expect(result.current.streaming).toBe(true);
    expect(result.current.context?.used).toBe(200);
    expect(
      result.current.chat?.messages.some((message) => message.content === 'Late response')
    ).toBe(false);
    act(() => lastSocket?.emit({ type: 'cancelled', message_id: 'current' }));
    expect(result.current.streaming).toBe(false);
    unmount();
  });
  it('marks an interrupted generation estimate stale until a fresh preview succeeds', async () => {
    mockPreviewContext.mockResolvedValue({ ...usage, used: 150 });
    const { result, unmount } = renderHook(() =>
      useChat('context', undefined, { content: 'draft' })
    );
    await waitFor(() => expect(result.current.context?.used).toBe(150));
    act(() => {
      lastSocket?.emit({ type: 'message_start', message_id: 'generation', role: 'assistant' });
      lastSocket?.emit({
        type: 'context',
        chat_id: 'context',
        message_id: 'generation',
        usage: { ...usage, status: 'compacting' },
      });
      lastSocket?.onclose?.();
    });
    expect(result.current.context?.status).toBe('unavailable');
    expect(result.current.context?.incomplete).toBe(true);
    expect(result.current.context?.reason).toContain('last observation');
    await waitFor(() => expect(result.current.context?.status).toBe('ready'));
    expect(result.current.context?.used).toBe(150);
    unmount();
  });
  it('does not replace an idle live usage update with a pending preview', async () => {
    let resolve!: (value: ContextUsage) => void;
    mockPreviewContext.mockImplementation(
      () =>
        new Promise<ContextUsage>((done) => {
          resolve = done;
        })
    );
    const { result, unmount } = renderHook(() =>
      useChat('context', undefined, { content: 'draft' })
    );
    await waitFor(() => expect(mockPreviewContext).toHaveBeenCalled());
    act(() =>
      lastSocket?.emit({
        type: 'context',
        chat_id: 'context',
        message_id: null,
        usage: { ...usage, used: 200 },
      })
    );
    await act(async () => resolve({ ...usage, used: 150 }));
    expect(result.current.context?.used).toBe(200);
    unmount();
  });
  it('removes cancelled empty assistant placeholders but preserves tool evidence', async () => {
    const { result, unmount } = renderHook(() => useChat('context'));
    await waitFor(() => expect(result.current.chat).not.toBeNull());
    act(() => {
      lastSocket?.emit({ type: 'message_start', message_id: 'empty', role: 'assistant' });
      lastSocket?.emit({ type: 'cancelled', message_id: 'empty' });
    });
    expect(result.current.chat?.messages.find((message) => message.id === 'empty')).toBeUndefined();
    act(() => {
      lastSocket?.emit({ type: 'message_start', message_id: 'evidence', role: 'assistant' });
      lastSocket?.emit({
        type: 'tool_call',
        message_id: 'evidence',
        tool_call_id: 'tool',
        name: 'read_file',
        arguments: '{}',
      });
      lastSocket?.emit({ type: 'cancelled', message_id: 'evidence' });
    });
    const evidence = result.current.chat?.messages.find((message) => message.id === 'evidence');
    expect(evidence?.metadata?.tool_calls).toHaveLength(1);
    expect(evidence?.metadata?.tool_calls?.[0]?.pending).toBe(false);
    expect(evidence?.metadata?.tool_calls?.[0]?.detail).toBe('Did not finish');
    expect(evidence?.content).toBe('[Stopped before answering]');
    unmount();
  });

  it('reuses the pending user row when a send fails before it is saved', async () => {
    const { result, unmount } = renderHook(() => useChat('context'), { wrapper: createWrapper() });
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
      expect(lastSocket).not.toBeNull();
    });

    await act(async () => {
      await result.current.sendMessage({ content: 'First try' });
    });
    act(() => lastSocket?.emit({ type: 'error', message: 'Rate limit exceeded' }));
    await waitFor(() => {
      expect(result.current.error).toBe('Rate limit exceeded');
    });
    expect(result.current.chat?.messages.filter((message) => message.role === 'user')).toHaveLength(
      1
    );

    await act(async () => {
      await result.current.sendMessage({ content: 'Second try' });
    });
    const users = result.current.chat?.messages.filter((message) => message.role === 'user') ?? [];
    expect(users).toHaveLength(1);
    expect(users.at(-1)?.content).toBe('Second try');
    unmount();
  });
  it('removes disconnected empty placeholders while flushing partial text and preserving images', async () => {
    const { result, unmount } = renderHook(() => useChat('context'));
    await waitFor(() => expect(result.current.chat).not.toBeNull());
    act(() => {
      lastSocket?.emit({ type: 'message_start', message_id: 'empty', role: 'assistant' });
      lastSocket?.onclose?.();
    });
    expect(result.current.chat?.messages.find((message) => message.id === 'empty')).toBeUndefined();
    act(() => {
      lastSocket?.emit({ type: 'message_start', message_id: 'partial', role: 'assistant' });
      lastSocket?.emit({ type: 'chunk', content: 'Partial answer', index: 0 });
      lastSocket?.onclose?.();
    });
    expect(result.current.chat?.messages.find((message) => message.id === 'partial')?.content).toBe(
      'Partial answer'
    );
    act(() => {
      lastSocket?.emit({ type: 'message_start', message_id: 'image', role: 'assistant' });
      lastSocket?.emit({
        type: 'image',
        message_id: 'image',
        attachment: { name: 'image', mime: 'image/png', url: '/image.png' },
      });
      lastSocket?.emit({ type: 'cancelled', message_id: 'image' });
    });
    expect(
      result.current.chat?.messages.find((message) => message.id === 'image')?.metadata?.attachments
    ).toHaveLength(1);
    unmount();
  });
  it('aborts obsolete drafts, ignores their late result, and reports preview failure', async () => {
    let resolve!: (value: ContextUsage) => void;
    mockPreviewContext.mockImplementationOnce(
      () =>
        new Promise<ContextUsage>((done) => {
          resolve = done;
        })
    );
    mockPreviewContext.mockRejectedValueOnce(new Error('offline'));
    const { result, rerender, unmount } = renderHook(
      ({ content }) => useChat('context', undefined, { content }),
      { initialProps: { content: 'old' } }
    );
    await waitFor(() => expect(mockPreviewContext).toHaveBeenCalled());
    const signal = mockPreviewContext.mock.calls[0][2] as AbortSignal;
    rerender({ content: 'new' });
    expect(signal.aborted).toBe(true);
    await act(async () => resolve({ ...usage, used: 900 }));
    expect(result.current.context?.used).not.toBe(900);
    await waitFor(() => expect(result.current.contextError).toContain('unavailable'));
    expect(result.current.context).toBeNull();
    unmount();
  });
});
