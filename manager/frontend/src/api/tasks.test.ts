import { afterEach, beforeEach, describe, expect, it, spyOn } from 'bun:test';
import { TaskProgressMessageSchema } from '../features/tasks/schemas';
import { tasksApi } from './tasks';

const run = {
  id: 'run-1',
  task_id: 'task-1',
  status: 'pending',
  current_phase: null,
  progress_percent: null,
  error_message: null,
};

describe('task runner contract', () => {
  let fetch: ReturnType<typeof spyOn>;
  beforeEach(() => {
    fetch = spyOn(globalThis, 'fetch');
    tasksApi.setGetAccessToken(() => 'test-token');
  });
  afterEach(() => {
    fetch.mockRestore();
    tasksApi.setGetAccessToken(() => null);
  });
  it('creates a run at the registered route and accepts the pending runner DTO', async () => {
    fetch.mockResolvedValueOnce(Response.json({ run }));
    expect(await tasksApi.runTask('task/1')).toEqual(run);
    expect(fetch).toHaveBeenCalledWith(
      expect.stringContaining('/api/tasks/task%2F1/runs'),
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ Authorization: 'Bearer test-token' }),
      })
    );
  });
  it('loads a run by global ID without requiring timestamps', async () => {
    fetch.mockResolvedValueOnce(
      Response.json({ run: { ...run, status: 'running', progress_percent: 20 } })
    );
    expect((await tasksApi.getTaskRun('task-1', 'run/1')).progress_percent).toBe(20);
    expect(fetch).toHaveBeenCalledWith(
      expect.stringContaining('/api/tasks/runs/run%2F1'),
      expect.anything()
    );
  });
  it('normalizes runner log levels without requiring run IDs', async () => {
    const log = {
      id: 'log-1',
      phase: 'implementing',
      agent_type: 'developer',
      log_level: 'warning',
      message: 'Retrying',
      created_at: '',
    };
    fetch.mockResolvedValueOnce(Response.json({ logs: [log] }));
    const logs = await tasksApi.getTaskRunLogs('task-1', 'run/1');
    expect(logs[0].level).toBe('warning');
    expect(logs[0].message).toBe('Retrying');
    expect(fetch).toHaveBeenCalledWith(
      expect.stringContaining('/api/tasks/runs/run%2F1/logs'),
      expect.anything()
    );
  });
  it('forwards abort signals for run creation, history, progress, and logs', async () => {
    const controller = new AbortController();
    fetch.mockResolvedValueOnce(Response.json({ run }));
    await tasksApi.runTask('task-1', controller.signal);
    fetch.mockResolvedValueOnce(Response.json({ runs: [run] }));
    await tasksApi.getTaskRuns('task-1', controller.signal);
    fetch.mockResolvedValueOnce(Response.json({ run }));
    await tasksApi.getTaskRun('task-1', 'run-1', controller.signal);
    fetch.mockResolvedValueOnce(Response.json({ logs: [] }));
    await tasksApi.getTaskRunLogs('task-1', 'run-1', controller.signal);
    expect(fetch).toHaveBeenCalledTimes(4);
    for (const call of fetch.mock.calls) expect(call[1]?.signal).toBe(controller.signal);
  });
  it('preserves runner rejection messages', async () => {
    fetch.mockResolvedValueOnce(
      Response.json({ error: 'Task already has an active run' }, { status: 409 })
    );
    await expect(tasksApi.runTask('task-1')).rejects.toThrow('Task already has an active run');
  });
  it('connects to the registered websocket route and authenticates on open', () => {
    class Socket extends window.EventTarget {
      send = spyOn({ send: (_message: string): void => {} }, 'send');
      constructor(readonly url: string) {
        super();
      }
    }
    const original = globalThis.WebSocket;
    globalThis.WebSocket = Socket as unknown as typeof WebSocket;
    try {
      const socket = tasksApi.createTaskWebSocket('run/1') as unknown as Socket;
      expect(socket.url).toEndWith('/ws/tasks/runs/run%2F1');
      expect(socket.send).not.toHaveBeenCalled();
      socket.dispatchEvent(new Event('open'));
      expect(socket.send).toHaveBeenCalledWith(
        JSON.stringify({ type: 'auth', token: 'test-token' })
      );
    } finally {
      globalThis.WebSocket = original;
    }
  });
  it('accepts every actual runner websocket message', () => {
    const messages = [
      { type: 'init', run_id: 'run-1', task_id: 'task-1', status: 'pending' },
      { type: 'status_update', status: 'running', current_phase: null, progress_percent: null },
      {
        type: 'log',
        id: 'log-1',
        phase: 'planning',
        agent_type: 'architect',
        log_level: 'info',
        message: 'Planning',
      },
      { type: 'completed', status: 'completed' },
      { type: 'failed', error: 'Model unavailable' },
      { type: 'error', message: 'Authentication failed' },
    ];
    for (const message of messages)
      expect(TaskProgressMessageSchema.safeParse(message).success).toBe(true);
  });
});
