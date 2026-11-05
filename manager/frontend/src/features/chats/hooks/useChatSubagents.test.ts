import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { act, renderHook, waitFor } from '@testing-library/react';
import type { ChatSubagent } from '../subagentTypes';
import { publishFrame } from '../utils/frames';

const mockGetChatSubagents = mock();

mock.module('../../../api/chats', () => ({
  chatsApi: {
    getChatSubagents: mockGetChatSubagents,
  },
}));

let useChatSubagents: typeof import('./useChatSubagents').useChatSubagents;
let RECORDING_SETTLE_DELAY: number;

beforeAll(async () => {
  ({ useChatSubagents } = await import('./useChatSubagents'));
  ({ RECORDING_SETTLE_DELAY } = await import('./useChatPullRequests'));
});

afterAll(() => {
  mock.restore();
});

const RUNNING: ChatSubagent = {
  id: 'subagent-1',
  chat_id: 'chat-1',
  message_id: 'message-1',
  external_id: 'toolu_01Agent',
  description: 'Ask for a review',
  kind: 'general-purpose',
  model: 'sonnet',
  background: true,
  status: 'running',
  tool_uses: 2,
  duration_ms: 4_000,
  tokens: 900,
  last_tool: 'Bash',
  summary: null,
  error: null,
  steps: [{ id: 'toolu_x', name: 'Bash', subject: 'cargo test', status: 'running' }],
  started_at: '2026-11-05T10:00:00Z',
  finished_at: null,
  updated_at: '2026-11-05T10:00:04Z',
};

const COMPLETED: ChatSubagent = {
  ...RUNNING,
  status: 'completed',
  summary: 'Looks good',
  steps: [{ ...RUNNING.steps[0], status: 'succeeded' }],
  finished_at: '2026-11-05T10:00:53Z',
  updated_at: '2026-11-05T10:00:53Z',
};

const OTHER: ChatSubagent = {
  ...RUNNING,
  id: 'subagent-2',
  chat_id: 'chat-2',
  message_id: 'message-9',
  external_id: 'toolu_02Agent',
  description: 'Run the tests',
};

function deferred(): { promise: Promise<ChatSubagent[]>; answer: (rows: ChatSubagent[]) => void } {
  let answer: (rows: ChatSubagent[]) => void = () => undefined;
  const promise = new Promise<ChatSubagent[]>((resolve) => (answer = resolve));
  return { promise, answer };
}

function send(chatId: string, frame: unknown) {
  act(() => {
    publishFrame(chatId, frame);
  });
}

describe('useChatSubagents', () => {
  beforeEach(() => {
    mockGetChatSubagents.mockReset();
  });

  it("reads the chat's agents, and nothing for no chat", async () => {
    mockGetChatSubagents.mockResolvedValue([RUNNING]);
    const { result, rerender } = renderHook(({ chat }) => useChatSubagents(chat, false), {
      initialProps: { chat: 'chat-1' as string | null },
    });
    await waitFor(() => expect(result.current.subagents).toEqual([RUNNING]));
    expect(mockGetChatSubagents).toHaveBeenCalledWith('chat-1');

    rerender({ chat: null });
    await waitFor(() => expect(result.current.subagents).toEqual([]));
    expect(mockGetChatSubagents).toHaveBeenCalledTimes(1);
  });

  it('merges a subagent frame for the chat', async () => {
    mockGetChatSubagents.mockResolvedValue([RUNNING]);
    const { result } = renderHook(() => useChatSubagents('chat-1', true));
    await waitFor(() => expect(result.current.subagents).toEqual([RUNNING]));

    send('chat-1', { type: 'subagent', chat_id: 'chat-1', subagent: COMPLETED });

    expect(result.current.subagents).toEqual([COMPLETED]);
    expect(mockGetChatSubagents).toHaveBeenCalledTimes(1);
  });

  it('adds an agent a frame announces before the first read answers', async () => {
    const read = deferred();
    mockGetChatSubagents.mockReturnValueOnce(read.promise);
    const { result } = renderHook(() => useChatSubagents('chat-1', true));

    send('chat-1', { type: 'subagent', chat_id: 'chat-1', subagent: COMPLETED });
    expect(result.current.subagents).toEqual([COMPLETED]);

    await act(async () => read.answer([RUNNING]));
    expect(result.current.subagents).toEqual([COMPLETED]);
  });

  it('keeps an agent a frame announced while the read was in flight', async () => {
    mockGetChatSubagents.mockResolvedValueOnce([]);
    const { result } = renderHook(() => useChatSubagents('chat-1', true));
    await waitFor(() => expect(mockGetChatSubagents).toHaveBeenCalledTimes(1));

    const read = deferred();
    mockGetChatSubagents.mockReturnValueOnce(read.promise);
    act(() => void result.current.refresh());
    send('chat-1', { type: 'subagent', chat_id: 'chat-1', subagent: RUNNING });

    const later: ChatSubagent = { ...OTHER, chat_id: 'chat-1', started_at: '2026-11-05T10:02:00Z' };
    await act(async () => read.answer([later]));
    expect(result.current.subagents).toEqual([RUNNING, later]);
  });

  it('ignores frames about another chat, malformed frames, and other frame types', async () => {
    mockGetChatSubagents.mockResolvedValue([RUNNING]);
    const { result } = renderHook(() => useChatSubagents('chat-1', true));
    await waitFor(() => expect(result.current.subagents).toEqual([RUNNING]));
    const shown = result.current.subagents;

    send('chat-2', { type: 'subagent', chat_id: 'chat-2', subagent: OTHER });
    send('chat-1', { type: 'subagent', chat_id: 'chat-2', subagent: OTHER });
    send('chat-1', { type: 'subagent', chat_id: 'chat-1', subagent: { id: 'broken' } });
    send('chat-1', { type: 'token', content: 'hello' });
    send('chat-1', { type: 'message_end', message_id: 'message-1' });

    expect(result.current.subagents).toBe(shown);
    expect(mockGetChatSubagents).toHaveBeenCalledTimes(1);
  });

  it('reads again when the socket says init', async () => {
    mockGetChatSubagents.mockResolvedValueOnce([RUNNING]);
    const { result } = renderHook(() => useChatSubagents('chat-1', false));
    await waitFor(() => expect(result.current.subagents).toEqual([RUNNING]));

    mockGetChatSubagents.mockResolvedValueOnce([COMPLETED]);
    send('chat-1', { type: 'init', chat_id: 'chat-1' });

    await waitFor(() => expect(result.current.subagents).toEqual([COMPLETED]));
    expect(mockGetChatSubagents).toHaveBeenCalledTimes(2);
  });

  it('reads again when a dropped reply resumes, and not when a new one starts', async () => {
    mockGetChatSubagents.mockResolvedValue([RUNNING]);
    const { result } = renderHook(() => useChatSubagents('chat-1', true));
    await waitFor(() => expect(result.current.subagents).toEqual([RUNNING]));

    send('chat-1', { type: 'message_start', message_id: 'message-2', role: 'assistant' });
    send('chat-1', {
      type: 'message_start',
      message_id: 'message-2',
      role: 'assistant',
      resumed: false,
    });
    expect(mockGetChatSubagents).toHaveBeenCalledTimes(1);

    mockGetChatSubagents.mockResolvedValue([COMPLETED]);
    send('chat-1', {
      type: 'message_start',
      message_id: 'message-1',
      role: 'assistant',
      resumed: true,
    });
    await waitFor(() => expect(result.current.subagents).toEqual([COMPLETED]));
    expect(mockGetChatSubagents).toHaveBeenCalledTimes(2);
  });

  it('never lets a read older than a frame undo it', async () => {
    mockGetChatSubagents.mockResolvedValueOnce([RUNNING]);
    const { result } = renderHook(() => useChatSubagents('chat-1', true));
    await waitFor(() => expect(result.current.subagents).toEqual([RUNNING]));

    const read = deferred();
    mockGetChatSubagents.mockReturnValueOnce(read.promise);
    act(() => void result.current.refresh());
    send('chat-1', { type: 'subagent', chat_id: 'chat-1', subagent: COMPLETED });

    await act(async () => read.answer([RUNNING]));
    expect(result.current.subagents).toEqual([COMPLETED]);
  });

  it('takes a read newer than the frame it has', async () => {
    mockGetChatSubagents.mockResolvedValueOnce([RUNNING]);
    const { result } = renderHook(() => useChatSubagents('chat-1', false));
    await waitFor(() => expect(result.current.subagents).toEqual([RUNNING]));

    mockGetChatSubagents.mockResolvedValueOnce([COMPLETED]);
    await act(() => result.current.refresh());
    expect(result.current.subagents).toEqual([COMPLETED]);
  });

  it("never shows one chat's agents while the next chat's are being read", async () => {
    mockGetChatSubagents.mockResolvedValueOnce([RUNNING]);
    const { result, rerender } = renderHook(({ chat }) => useChatSubagents(chat, false), {
      initialProps: { chat: 'chat-1' },
    });
    await waitFor(() => expect(result.current.subagents).toEqual([RUNNING]));

    const read = deferred();
    mockGetChatSubagents.mockReturnValueOnce(read.promise);
    rerender({ chat: 'chat-2' });

    expect(result.current.subagents).toEqual([]);
    await act(async () => read.answer([OTHER]));
    expect(result.current.subagents).toEqual([OTHER]);
  });

  it("drops the previous chat's read when it answers after a switch", async () => {
    const stale = deferred();
    mockGetChatSubagents.mockReturnValueOnce(stale.promise);
    const { result, rerender } = renderHook(({ chat }) => useChatSubagents(chat, false), {
      initialProps: { chat: 'chat-1' },
    });

    mockGetChatSubagents.mockResolvedValueOnce([OTHER]);
    rerender({ chat: 'chat-2' });
    await waitFor(() => expect(result.current.subagents).toEqual([OTHER]));

    await act(async () => stale.answer([RUNNING]));
    expect(result.current.subagents).toEqual([OTHER]);
  });

  it('stops listening to a chat once it is left', async () => {
    mockGetChatSubagents.mockResolvedValueOnce([RUNNING]);
    const { result, rerender } = renderHook(({ chat }) => useChatSubagents(chat, false), {
      initialProps: { chat: 'chat-1' },
    });
    await waitFor(() => expect(result.current.subagents).toEqual([RUNNING]));

    mockGetChatSubagents.mockResolvedValueOnce([OTHER]);
    rerender({ chat: 'chat-2' });
    await waitFor(() => expect(result.current.subagents).toEqual([OTHER]));

    send('chat-1', { type: 'subagent', chat_id: 'chat-1', subagent: COMPLETED });
    send('chat-1', { type: 'init' });
    expect(result.current.subagents).toEqual([OTHER]);
    expect(mockGetChatSubagents).toHaveBeenCalledTimes(2);
  });

  it('reads again as a turn ends, and once more after it settles', async () => {
    mockGetChatSubagents.mockResolvedValue([]);
    const { rerender } = renderHook(({ streaming }) => useChatSubagents('chat-1', streaming), {
      initialProps: { streaming: true },
    });
    await waitFor(() => expect(mockGetChatSubagents).toHaveBeenCalledTimes(1));

    rerender({ streaming: false });
    await waitFor(() => expect(mockGetChatSubagents).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(mockGetChatSubagents).toHaveBeenCalledTimes(3), {
      timeout: RECORDING_SETTLE_DELAY + 2_000,
    });
  });

  it('says why the first read failed, with nothing to list', async () => {
    mockGetChatSubagents.mockRejectedValueOnce(new Error('Failed to load agents'));
    const { result } = renderHook(() => useChatSubagents('chat-1', false));

    await waitFor(() => expect(result.current.error).toBe('Failed to load agents'));
    expect(result.current.subagents).toEqual([]);
  });

  it('keeps the agents it has and says why a read failed', async () => {
    mockGetChatSubagents.mockResolvedValueOnce([RUNNING]);
    const { result } = renderHook(() => useChatSubagents('chat-1', false));
    await waitFor(() => expect(result.current.subagents).toEqual([RUNNING]));

    mockGetChatSubagents.mockRejectedValueOnce(new Error('Failed to load agents'));
    await act(() => result.current.refresh());
    await waitFor(() => expect(result.current.error).toBe('Failed to load agents'));
    expect(result.current.subagents).toEqual([RUNNING]);
  });
});
