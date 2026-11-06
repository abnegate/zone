import { afterAll, beforeAll, beforeEach, describe, expect, it, mock, spyOn } from 'bun:test';
import { act, renderHook, waitFor } from '@testing-library/react';
import { SubagentFrameSchema } from '../subagentSchemas';
import type { ChatSubagent, SubagentList, SubagentStep } from '../subagentTypes';
import { publishFrame } from '../utils/frames';

const mockGetChatSubagents = mock();
const mockGetChatSubagentSteps = mock();

function listed(answer: ChatSubagent[] | SubagentList): SubagentList {
  return Array.isArray(answer) ? { subagents: answer, complete: true } : answer;
}

mock.module('../../../api/chats', () => ({
  chatsApi: {
    getChatSubagents: async (chatId: string) => listed(await mockGetChatSubagents(chatId)),
    getChatSubagentSteps: mockGetChatSubagentSteps,
  },
}));

let useChatSubagents: typeof import('./useChatSubagents').useChatSubagents;
let SETTLE_DELAY: number;

beforeAll(async () => {
  ({ useChatSubagents, SETTLE_DELAY } = await import('./useChatSubagents'));
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
  step_count: 1,
  revision: 1,
  started_at: '2026-11-05T10:00:00Z',
  finished_at: null,
  updated_at: '2026-11-05T10:00:04Z',
};

const COMPLETED: ChatSubagent = {
  ...RUNNING,
  status: 'completed',
  summary: 'Looks good',
  revision: 2,
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

const BASH: SubagentStep = {
  id: 'toolu_x',
  name: 'Bash',
  subject: 'cargo test',
  status: 'running',
};
const PASSED: SubagentStep = { ...BASH, status: 'succeeded' };
const READ: SubagentStep = {
  id: 'toolu_y',
  name: 'Read',
  subject: 'src/lib.rs',
  status: 'succeeded',
};

interface Deferred<T> {
  promise: Promise<T>;
  answer: (value: T) => void;
}

function deferred<T = ChatSubagent[]>(): Deferred<T> {
  let answer: (value: T) => void = () => undefined;
  const promise = new Promise<T>((resolve) => (answer = resolve));
  return { promise, answer };
}

function update(subagent: ChatSubagent, steps: SubagentStep[] = []) {
  return { type: 'subagent', chat_id: subagent.chat_id, subagent, steps };
}

function send(chatId: string, frame: unknown) {
  act(() => {
    publishFrame(chatId, frame);
  });
}

describe('useChatSubagents', () => {
  beforeEach(() => {
    mockGetChatSubagents.mockReset();
    mockGetChatSubagentSteps.mockReset();
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

    send('chat-1', update(COMPLETED));

    expect(result.current.subagents).toEqual([COMPLETED]);
    expect(mockGetChatSubagents).toHaveBeenCalledTimes(1);
  });

  it('adds an agent a frame announces before the first read answers', async () => {
    const read = deferred();
    mockGetChatSubagents.mockReturnValueOnce(read.promise);
    const { result } = renderHook(() => useChatSubagents('chat-1', true));

    send('chat-1', update(COMPLETED));
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
    send('chat-1', update(RUNNING));

    const later: ChatSubagent = { ...OTHER, chat_id: 'chat-1', started_at: '2026-11-05T10:02:00Z' };
    await act(async () => read.answer([later]));
    expect(result.current.subagents).toEqual([RUNNING, later]);
  });

  it('ignores frames about another chat, malformed frames, and other frame types', async () => {
    mockGetChatSubagents.mockResolvedValue([RUNNING]);
    const { result } = renderHook(() => useChatSubagents('chat-1', true));
    await waitFor(() => expect(result.current.subagents).toEqual([RUNNING]));
    const shown = result.current.subagents;

    send('chat-2', update(OTHER));
    send('chat-1', update(OTHER));
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
    send('chat-1', update(COMPLETED));

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

    send('chat-1', update(COMPLETED));
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
      timeout: SETTLE_DELAY + 2_000,
    });
  });

  it('reads once more after the first read when init raced it', async () => {
    const read = deferred();
    mockGetChatSubagents.mockReturnValueOnce(read.promise);
    const { result } = renderHook(() => useChatSubagents('chat-1', false));

    send('chat-1', { type: 'init', chat_id: 'chat-1' });
    expect(mockGetChatSubagents).toHaveBeenCalledTimes(1);

    mockGetChatSubagents.mockResolvedValueOnce([COMPLETED]);
    await act(async () => read.answer([RUNNING]));

    await waitFor(() => expect(result.current.subagents).toEqual([COMPLETED]));
    expect(mockGetChatSubagents).toHaveBeenCalledTimes(2);
  });

  it('parses only the frame types it handles', async () => {
    mockGetChatSubagents.mockResolvedValue([RUNNING]);
    const parse = spyOn(SubagentFrameSchema, 'safeParse');
    try {
      const { result } = renderHook(() => useChatSubagents('chat-1', true));
      await waitFor(() => expect(result.current.subagents).toEqual([RUNNING]));

      for (let index = 0; index < 50; index += 1) {
        send('chat-1', { type: 'chunk', content: 'x', index });
      }
      expect(parse).not.toHaveBeenCalled();

      send('chat-1', update(COMPLETED));
      expect(parse).toHaveBeenCalledTimes(1);
    } finally {
      parse.mockRestore();
    }
  });

  it('groups the agents by the message that started them', async () => {
    const elsewhere: ChatSubagent = { ...OTHER, chat_id: 'chat-1' };
    mockGetChatSubagents.mockResolvedValue([RUNNING, elsewhere]);
    const { result } = renderHook(() => useChatSubagents('chat-1', false));

    await waitFor(() => expect(result.current.byMessage.get('message-1')).toEqual([RUNNING]));
    expect(result.current.byMessage.get('message-9')).toEqual([elsewhere]);
    const grouped = result.current.byMessage;

    send('chat-1', { type: 'token', content: 'x' });
    expect(result.current.byMessage).toBe(grouped);
  });

  it('removes the agents a message deletion took, with their steps, for good', async () => {
    const elsewhere: ChatSubagent = { ...OTHER, chat_id: 'chat-1' };
    mockGetChatSubagents.mockResolvedValue([RUNNING, elsewhere]);
    mockGetChatSubagentSteps.mockResolvedValue([BASH]);
    const { result } = renderHook(() => useChatSubagents('chat-1', false));
    await waitFor(() => expect(result.current.subagents).toHaveLength(2));
    await act(() => result.current.loadSteps(RUNNING.id));
    expect(result.current.steps[RUNNING.id]).toEqual([BASH]);

    send('chat-2', { type: 'subagents_removed', chat_id: 'chat-2', ids: [elsewhere.id] });
    send('chat-1', { type: 'subagents_removed', chat_id: 'chat-1', ids: 'all' });
    expect(result.current.subagents).toHaveLength(2);

    send('chat-1', { type: 'subagents_removed', chat_id: 'chat-1', ids: [RUNNING.id] });
    expect(result.current.subagents).toEqual([elsewhere]);
    expect(result.current.steps[RUNNING.id]).toBeUndefined();

    await act(() => result.current.refresh());
    expect(result.current.subagents).toEqual([elsewhere]);

    send('chat-1', update({ ...COMPLETED, revision: 9 }, [PASSED]));
    expect(result.current.subagents).toEqual([elsewhere]);
    expect(result.current.steps[RUNNING.id]).toBeUndefined();
  });

  describe('reconnecting', () => {
    const SIBLING: ChatSubagent = { ...OTHER, chat_id: 'chat-1' };
    const INIT = { type: 'init', chat_id: 'chat-1' };

    it('drops an agent the read no longer lists, with its steps, for good', async () => {
      mockGetChatSubagents.mockResolvedValueOnce([RUNNING, SIBLING]);
      mockGetChatSubagentSteps.mockResolvedValueOnce([BASH]);
      const { result } = renderHook(() => useChatSubagents('chat-1', false));
      await waitFor(() => expect(result.current.subagents).toEqual([RUNNING, SIBLING]));
      await act(() => result.current.loadSteps(RUNNING.id));
      expect(result.current.steps[RUNNING.id]).toEqual([BASH]);

      const stepRead = deferred<SubagentStep[]>();
      mockGetChatSubagentSteps.mockReturnValueOnce(stepRead.promise);
      mockGetChatSubagents.mockResolvedValueOnce([SIBLING]);
      send('chat-1', INIT);

      await waitFor(() => expect(result.current.subagents).toEqual([SIBLING]));
      expect(result.current.steps[RUNNING.id]).toBeUndefined();

      await act(async () => stepRead.answer([PASSED]));
      expect(result.current.steps[RUNNING.id]).toBeUndefined();

      mockGetChatSubagents.mockResolvedValueOnce([RUNNING, SIBLING]);
      await act(() => result.current.refresh());
      expect(result.current.subagents).toEqual([SIBLING]);

      mockGetChatSubagents.mockResolvedValueOnce([SIBLING]);
      send('chat-1', INIT);
      await waitFor(() => expect(mockGetChatSubagents).toHaveBeenCalledTimes(4));
      expect(mockGetChatSubagentSteps).toHaveBeenCalledTimes(2);
    });

    it('keeps an agent a frame announced during the read, but only for that read', async () => {
      mockGetChatSubagents.mockResolvedValueOnce([RUNNING]);
      const { result } = renderHook(() => useChatSubagents('chat-1', false));
      await waitFor(() => expect(result.current.subagents).toEqual([RUNNING]));

      const read = deferred();
      mockGetChatSubagents.mockReturnValueOnce(read.promise);
      send('chat-1', INIT);
      send('chat-1', update(SIBLING));

      await act(async () => read.answer([RUNNING]));
      expect(result.current.subagents).toEqual([RUNNING, SIBLING]);

      mockGetChatSubagents.mockResolvedValueOnce([RUNNING]);
      await act(() => result.current.refresh());
      expect(result.current.subagents).toEqual([RUNNING]);
    });

    it('drops no agent a read listed only in part', async () => {
      mockGetChatSubagents.mockResolvedValueOnce([RUNNING, SIBLING]);
      const { result } = renderHook(() => useChatSubagents('chat-1', false));
      await waitFor(() => expect(result.current.subagents).toEqual([RUNNING, SIBLING]));

      mockGetChatSubagents.mockResolvedValueOnce({ subagents: [COMPLETED], complete: false });
      await act(() => result.current.refresh());
      expect(result.current.subagents).toEqual([COMPLETED, SIBLING]);

      mockGetChatSubagents.mockResolvedValueOnce([COMPLETED, SIBLING]);
      await act(() => result.current.refresh());
      expect(result.current.subagents).toEqual([COMPLETED, SIBLING]);
    });

    it('updates the agents the read still lists by revision', async () => {
      mockGetChatSubagents.mockResolvedValueOnce([RUNNING, SIBLING]);
      const { result } = renderHook(() => useChatSubagents('chat-1', false));
      await waitFor(() => expect(result.current.subagents).toEqual([RUNNING, SIBLING]));

      const newer: ChatSubagent = { ...SIBLING, tool_uses: 9, revision: 3 };
      send('chat-1', update(newer));

      const older: ChatSubagent = { ...SIBLING, tool_uses: 5, revision: 2 };
      mockGetChatSubagents.mockResolvedValueOnce([COMPLETED, older]);
      send('chat-1', INIT);

      await waitFor(() => expect(result.current.subagents).toEqual([COMPLETED, newer]));
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

  describe('steps', () => {
    async function opened() {
      mockGetChatSubagents.mockResolvedValue([RUNNING]);
      const view = renderHook(() => useChatSubagents('chat-1', true));
      await waitFor(() => expect(view.result.current.subagents).toEqual([RUNNING]));
      return view;
    }

    it('reads no steps until they are asked for, then reads that agent only', async () => {
      const { result } = await opened();
      expect(result.current.steps).toEqual({});
      expect(mockGetChatSubagentSteps).not.toHaveBeenCalled();

      mockGetChatSubagentSteps.mockResolvedValueOnce([BASH]);
      await act(() => result.current.loadSteps(RUNNING.id));

      expect(mockGetChatSubagentSteps).toHaveBeenCalledWith('chat-1', RUNNING.id);
      expect(result.current.steps).toEqual({ [RUNNING.id]: [BASH] });
    });

    it('keeps the steps it read current from the changed steps of each frame', async () => {
      const { result } = await opened();
      mockGetChatSubagentSteps.mockResolvedValueOnce([BASH]);
      await act(() => result.current.loadSteps(RUNNING.id));

      send('chat-1', update({ ...RUNNING, revision: 2 }, [PASSED, READ]));

      expect(result.current.steps[RUNNING.id]).toEqual([PASSED, READ]);
    });

    it('drops step changes for an agent whose steps nobody asked for', async () => {
      const { result } = await opened();

      send('chat-1', update(COMPLETED, [PASSED]));

      expect(result.current.subagents).toEqual([COMPLETED]);
      expect(result.current.steps).toEqual({});
    });

    it('keeps the step changes that land while its steps are being read', async () => {
      const { result } = await opened();
      const read = deferred<SubagentStep[]>();
      mockGetChatSubagentSteps.mockReturnValueOnce(read.promise);
      let loading: Promise<void> = Promise.resolve();
      act(() => {
        loading = result.current.loadSteps(RUNNING.id);
      });

      send('chat-1', update({ ...RUNNING, revision: 2 }, [PASSED, READ]));
      expect(result.current.steps[RUNNING.id]).toBeUndefined();

      await act(async () => {
        read.answer([BASH]);
        await loading;
      });
      expect(result.current.steps[RUNNING.id]).toEqual([PASSED, READ]);
    });

    it('takes the steps a read answers over the ones it held before it', async () => {
      const { result } = await opened();
      mockGetChatSubagentSteps.mockResolvedValueOnce([BASH]);
      await act(() => result.current.loadSteps(RUNNING.id));

      const widened: SubagentStep = { ...BASH, subject: 'cargo test --workspace' };
      mockGetChatSubagentSteps.mockResolvedValueOnce([widened]);
      send('chat-1', { type: 'init', chat_id: 'chat-1' });

      await waitFor(() => expect(result.current.steps[RUNNING.id]).toEqual([widened]));
    });

    it('reads the steps again when a frame older than one it applied arrives', async () => {
      const { result } = await opened();
      mockGetChatSubagentSteps.mockResolvedValueOnce([BASH, READ]);
      await act(() => result.current.loadSteps(RUNNING.id));

      const finished: ChatSubagent = { ...COMPLETED, revision: 3 };
      send('chat-1', update(finished, [PASSED]));
      expect(result.current.steps[RUNNING.id]).toEqual([PASSED, READ]);

      const abandoned: SubagentStep = { ...READ, status: 'failed' };
      const stopped: ChatSubagent = { ...RUNNING, status: 'stopped', revision: 2 };
      const read = deferred<SubagentStep[]>();
      mockGetChatSubagentSteps.mockReturnValueOnce(read.promise);
      send('chat-1', update(stopped, [{ ...BASH, status: 'failed' }, abandoned]));

      expect(result.current.steps[RUNNING.id]).toEqual([PASSED, READ]);
      expect(result.current.subagents).toEqual([finished]);
      expect(mockGetChatSubagentSteps).toHaveBeenCalledTimes(2);

      await act(async () => read.answer([PASSED, abandoned]));
      expect(result.current.steps[RUNNING.id]).toEqual([PASSED, abandoned]);
    });

    it('says when the steps could not be read', async () => {
      const { result } = await opened();
      mockGetChatSubagentSteps.mockRejectedValueOnce(new Error('Failed to load steps'));

      let failure: unknown = null;
      await act(async () => {
        await result.current.loadSteps(RUNNING.id).catch((caught: unknown) => {
          failure = caught;
        });
      });

      expect(failure).toBeInstanceOf(Error);
      expect(result.current.steps[RUNNING.id]).toBeUndefined();
    });

    it('reads the asked-for steps again when the socket says init', async () => {
      const { result } = await opened();
      mockGetChatSubagentSteps.mockResolvedValueOnce([BASH]);
      await act(() => result.current.loadSteps(RUNNING.id));

      mockGetChatSubagents.mockResolvedValue([COMPLETED]);
      mockGetChatSubagentSteps.mockResolvedValueOnce([PASSED, READ]);
      send('chat-1', { type: 'init', chat_id: 'chat-1' });

      await waitFor(() => expect(result.current.steps[RUNNING.id]).toEqual([PASSED, READ]));
      expect(result.current.subagents).toEqual([COMPLETED]);
      expect(mockGetChatSubagentSteps).toHaveBeenCalledTimes(2);
    });

    it('reads the asked-for steps again when a dropped reply resumes', async () => {
      const { result } = await opened();
      mockGetChatSubagentSteps.mockResolvedValueOnce([BASH]);
      await act(() => result.current.loadSteps(RUNNING.id));

      mockGetChatSubagentSteps.mockResolvedValueOnce([PASSED]);
      send('chat-1', { type: 'message_start', message_id: 'message-1', resumed: true });

      await waitFor(() => expect(result.current.steps[RUNNING.id]).toEqual([PASSED]));
    });

    it('forgets the steps of the chat it left, and drops their late answer', async () => {
      mockGetChatSubagents.mockResolvedValue([RUNNING]);
      const { result, rerender } = renderHook(({ chat }) => useChatSubagents(chat, false), {
        initialProps: { chat: 'chat-1' },
      });
      await waitFor(() => expect(result.current.subagents).toEqual([RUNNING]));
      const read = deferred<SubagentStep[]>();
      mockGetChatSubagentSteps.mockReturnValueOnce(read.promise);
      let loading: Promise<void> = Promise.resolve();
      act(() => {
        loading = result.current.loadSteps(RUNNING.id);
      });

      rerender({ chat: 'chat-2' });
      await act(async () => {
        read.answer([BASH]);
        await loading;
      });

      expect(result.current.steps).toEqual({});
    });
  });
});
