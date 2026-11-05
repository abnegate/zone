import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { act, renderHook, waitFor } from '@testing-library/react';
import type { ChatTask } from '../types';

const mockGetChatTasks = mock();

mock.module('../../../api/chats', () => ({
  chatsApi: {
    getChatTasks: mockGetChatTasks,
  },
}));

let useChatTasks: typeof import('./useChatTasks').useChatTasks;
let TASK_REFRESH_INTERVAL: number;
let RECORDING_SETTLE_DELAY: number;

beforeAll(async () => {
  ({ useChatTasks, TASK_REFRESH_INTERVAL } = await import('./useChatTasks'));
  ({ RECORDING_SETTLE_DELAY } = await import('./useChatPullRequests'));
});

afterAll(() => {
  mock.restore();
});

const RUNNING: ChatTask = {
  id: 'task-1',
  title: 'Fix the build',
  title_pending: false,
  status: 'in_progress',
  is_agentic: true,
  href: '/tasks?id=task-1',
  created_at: '2026-11-05T10:00:00Z',
  updated_at: '2026-11-05T10:00:00Z',
  run: {
    id: 'run-1',
    status: 'running',
    phase: 'acting',
    progress: 10,
    started_at: '2026-11-05T10:00:00Z',
    completed_at: null,
  },
  pull_request: null,
};

const DONE: ChatTask = {
  ...RUNNING,
  status: 'review',
  run: { ...(RUNNING.run as NonNullable<ChatTask['run']>), status: 'completed', progress: 100 },
};

const intervals: Array<{ delay: number; tick: () => void; cleared: boolean }> = [];
const realSetInterval = globalThis.setInterval;
const realClearInterval = globalThis.clearInterval;

function tick(delay: number) {
  for (const interval of intervals)
    if (interval.delay === delay && !interval.cleared) interval.tick();
}

function watching(delay: number): boolean {
  return intervals.some((interval) => interval.delay === delay && !interval.cleared);
}

describe('useChatTasks', () => {
  beforeEach(() => {
    mockGetChatTasks.mockReset();
    intervals.length = 0;
    globalThis.setInterval = ((tick: () => void, delay: number) => {
      if (delay !== TASK_REFRESH_INTERVAL) return realSetInterval(tick, delay);
      const interval = { delay, tick, cleared: false };
      intervals.push(interval);
      return interval as unknown as ReturnType<typeof setInterval>;
    }) as typeof setInterval;
    globalThis.clearInterval = ((handle: ReturnType<typeof setInterval> | undefined) => {
      const faked = intervals.find((interval) => (interval as unknown) === handle);
      if (faked) faked.cleared = true;
      else realClearInterval(handle);
    }) as typeof clearInterval;
  });

  afterEach(() => {
    globalThis.setInterval = realSetInterval;
    globalThis.clearInterval = realClearInterval;
  });

  it("reads the chat's tasks, and nothing for no chat", async () => {
    mockGetChatTasks.mockResolvedValue([RUNNING]);
    const { result, rerender } = renderHook(({ chat }) => useChatTasks(chat, false), {
      initialProps: { chat: 'chat-1' as string | null },
    });
    await waitFor(() => expect(result.current.tasks).toEqual([RUNNING]));
    expect(mockGetChatTasks).toHaveBeenCalledWith('chat-1');

    rerender({ chat: null });
    await waitFor(() => expect(result.current.tasks).toEqual([]));
  });

  it('keeps reading while a run is going and stops once it ends', async () => {
    mockGetChatTasks.mockResolvedValue([RUNNING]);
    const { result } = renderHook(() => useChatTasks('chat-1', false));
    await waitFor(() => expect(result.current.tasks).toEqual([RUNNING]));
    expect(watching(TASK_REFRESH_INTERVAL)).toBe(true);

    mockGetChatTasks.mockResolvedValue([DONE]);
    tick(TASK_REFRESH_INTERVAL);
    await waitFor(() => expect(result.current.tasks).toEqual([DONE]));
    expect(watching(TASK_REFRESH_INTERVAL)).toBe(false);
  });

  it('reads while the agent replies, since it may start a task mid-turn', async () => {
    mockGetChatTasks.mockResolvedValue([]);
    renderHook(() => useChatTasks('chat-1', true));
    await waitFor(() => expect(mockGetChatTasks).toHaveBeenCalledTimes(1));
    expect(watching(TASK_REFRESH_INTERVAL)).toBe(true);

    mockGetChatTasks.mockResolvedValue([RUNNING]);
    tick(TASK_REFRESH_INTERVAL);
    await waitFor(() => expect(mockGetChatTasks).toHaveBeenCalledTimes(2));
  });

  it('reads nothing on a timer for a chat whose tasks have all settled', async () => {
    mockGetChatTasks.mockResolvedValue([DONE]);
    const { result } = renderHook(() => useChatTasks('chat-1', false));
    await waitFor(() => expect(result.current.tasks).toEqual([DONE]));
    expect(watching(TASK_REFRESH_INTERVAL)).toBe(false);
  });

  it('reads again as a turn ends, and once more after it settles', async () => {
    mockGetChatTasks.mockResolvedValue([]);
    const { rerender } = renderHook(({ streaming }) => useChatTasks('chat-1', streaming), {
      initialProps: { streaming: true },
    });
    await waitFor(() => expect(mockGetChatTasks).toHaveBeenCalledTimes(1));

    rerender({ streaming: false });
    await waitFor(() => expect(mockGetChatTasks).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(mockGetChatTasks).toHaveBeenCalledTimes(3), {
      timeout: RECORDING_SETTLE_DELAY + 2_000,
    });
  });

  it("never shows one chat's tasks while the next chat's are being read", async () => {
    mockGetChatTasks.mockResolvedValueOnce([RUNNING]);
    const { result, rerender } = renderHook(({ chat }) => useChatTasks(chat, false), {
      initialProps: { chat: 'chat-1' },
    });
    await waitFor(() => expect(result.current.tasks).toEqual([RUNNING]));

    let answer: (rows: ChatTask[]) => void = () => undefined;
    mockGetChatTasks.mockImplementationOnce(() => new Promise((resolve) => (answer = resolve)));
    rerender({ chat: 'chat-2' });

    expect(result.current.tasks).toEqual([]);
    await act(async () => answer([DONE]));
    expect(result.current.tasks).toEqual([DONE]);
  });

  it('says why the first read failed, with nothing to list', async () => {
    mockGetChatTasks.mockRejectedValueOnce(new Error('Failed to load tasks'));
    const { result } = renderHook(() => useChatTasks('chat-1', false));

    await waitFor(() => expect(result.current.error).toBe('Failed to load tasks'));
    expect(result.current.tasks).toEqual([]);
  });

  it('keeps the tasks it has and says why a read failed', async () => {
    mockGetChatTasks.mockResolvedValueOnce([RUNNING]);
    const { result } = renderHook(() => useChatTasks('chat-1', false));
    await waitFor(() => expect(result.current.tasks).toEqual([RUNNING]));

    mockGetChatTasks.mockRejectedValueOnce(new Error('Failed to load tasks'));
    await act(() => result.current.refresh());
    await waitFor(() => expect(result.current.error).toBe('Failed to load tasks'));
    expect(result.current.tasks).toEqual([RUNNING]);
  });
});
