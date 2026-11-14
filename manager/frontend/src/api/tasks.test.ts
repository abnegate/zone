import { afterEach, beforeEach, describe, expect, it, spyOn } from 'bun:test';
import created from '../../../../runner/zone_server/tests/fixtures/task-created.json';
import { ActiveRunError } from '../features/tasks/ActiveRunError';
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

const receipt = {
  id: 'receipt-1',
  action: 'create_task',
  target_type: 'task',
  target_id: 'task-2',
  target_label: 'Created task',
  actor_id: 'user-1',
  actor_name: 'Task actor',
  occurred_at: '2026-09-08T12:00:00Z',
  success: true,
  outcome: 'Created',
  href: '/tasks/task-2',
};
const log = {
  id: 'log-1',
  phase: 'acting',
  agent_type: 'task',
  log_level: 'info',
  message: 'Created task',
  created_at: '2026-09-08T12:00:00Z',
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
  it('preserves durable action receipts and other metadata in HTTP logs', async () => {
    const metadata = { action_receipt: receipt, tool_name: 'create_task' };
    fetch.mockResolvedValueOnce(
      Response.json({ logs: [{ ...log, metadata }, { ...log, metadata: null }, log] })
    );
    const logs = await tasksApi.getTaskRunLogs('task-1', 'run-1');
    expect(logs[0]).toMatchObject({ metadata });
    expect(logs[0].metadata?.action_receipt).toEqual(receipt);
    expect(logs[1]).toMatchObject({ metadata: null });
    expect(logs[2]).not.toHaveProperty('metadata');
  });
  it('preserves action receipts in streamed task logs', () => {
    const message = { type: 'log', ...log, metadata: { action_receipt: receipt } };
    expect(TaskProgressMessageSchema.parse(message)).toMatchObject({ metadata: message.metadata });
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
  it('sends the structured answers to the answering route and reads the confirmation back', async () => {
    const answers = [
      { header: 'Scope', labels: ['Backfill'] },
      { header: 'Timing', labels: ['Other'], other: 'after the release' },
    ];
    const accepted = { run_id: 'run/1', answered: 2 };
    fetch.mockResolvedValueOnce(Response.json(accepted, { status: 202 }));

    expect(await tasksApi.answerRun('run/1', answers)).toEqual(accepted);
    expect(fetch).toHaveBeenCalledWith(
      expect.stringContaining('/api/tasks/runs/run%2F1/answers'),
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ answers }),
        headers: expect.objectContaining({ Authorization: 'Bearer test-token' }),
      })
    );
  });

  it('keeps an accepted answer accepted when the confirmation is unreadable', async () => {
    fetch.mockResolvedValueOnce(new Response('<html>proxy ate it</html>', { status: 202 }));

    expect(
      await tasksApi.answerRun('run-1', [{ header: 'Scope', labels: ['Backfill'] }])
    ).toBeUndefined();
  });

  it('keeps an accepted answer accepted when the confirmation is a shape it cannot read', async () => {
    fetch.mockResolvedValueOnce(Response.json({ run_id: 'run-1' }, { status: 202 }));

    expect(
      await tasksApi.answerRun('run-1', [{ header: 'Scope', labels: ['Backfill'] }])
    ).toBeUndefined();
  });

  it('surfaces the server rejection of an answer rather than a bare status', async () => {
    fetch.mockResolvedValueOnce(
      Response.json({ message: 'Answer a required question' }, { status: 400 })
    );

    await expect(tasksApi.answerRun('run-1', [])).rejects.toThrow('Answer a required question');
  });

  it('stops a run at the registered route and reads the stopped run back', async () => {
    const stopped = { ...run, status: 'cancelled', error_message: 'Task run was stopped' };
    const controller = new AbortController();
    fetch.mockResolvedValueOnce(Response.json({ run: stopped }));

    expect(await tasksApi.stopRun('run/1', controller.signal)).toEqual(stopped);
    expect(fetch).toHaveBeenCalledWith(
      expect.stringContaining('/api/tasks/runs/run%2F1/stop'),
      expect.objectContaining({
        method: 'POST',
        signal: controller.signal,
        headers: expect.objectContaining({ Authorization: 'Bearer test-token' }),
      })
    );
    expect(fetch.mock.calls[0][1]?.body).toBeUndefined();
  });

  it('surfaces the server refusal to stop a run that is no longer active', async () => {
    fetch.mockResolvedValueOnce(
      Response.json({ error: 'Task run is not active' }, { status: 409 })
    );

    await expect(tasksApi.stopRun('run-1')).rejects.toThrow('Task run is not active');
  });

  it('reports a bare status when a stop is refused without a message', async () => {
    fetch.mockResolvedValueOnce(new Response('', { status: 502 }));

    await expect(tasksApi.stopRun('run-1')).rejects.toThrow('Failed to stop task run: 502');
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
      { type: 'cancelled', error: 'Task run was stopped' },
      { type: 'cancelled', message: 'Task run was stopped' },
      { type: 'error', message: 'Authentication failed' },
    ];
    for (const message of messages)
      expect(TaskProgressMessageSchema.safeParse(message).success).toBe(true);
  });
});

describe('editing many tasks at once', () => {
  const first = { ...created.task, id: 'task-1' };
  const second = { ...created.task, id: 'task-2' };
  const ids = ['task-1', 'task-2'];

  let fetch: ReturnType<typeof spyOn>;
  beforeEach(() => {
    fetch = spyOn(globalThis, 'fetch');
    tasksApi.setGetAccessToken(() => 'test-token');
  });
  afterEach(() => {
    fetch.mockRestore();
    tasksApi.setGetAccessToken(() => null);
  });

  it('patches the workspace task list with the ids and only the changes given', async () => {
    fetch.mockResolvedValueOnce(Response.json({ tasks: [first, second] }));

    await tasksApi.updateTasks('workspace/1', ids, { status: 'paused' });

    expect(fetch).toHaveBeenCalledTimes(1);
    const [url, init] = fetch.mock.calls[0];
    expect(url).toEndWith('/api/workspaces/workspace%2F1/tasks');
    expect(init).toMatchObject({
      method: 'PATCH',
      headers: expect.objectContaining({ Authorization: 'Bearer test-token' }),
    });
    expect(JSON.parse(init.body)).toEqual({ ids, status: 'paused' });
  });

  it('returns every task the server sent back, parsed', async () => {
    fetch.mockResolvedValueOnce(
      Response.json({
        tasks: [
          { ...first, priority: 5 },
          { ...second, priority: 5 },
        ],
      })
    );

    const tasks = await tasksApi.updateTasks('workspace-1', ids, { priority: 5 });

    expect(tasks.map((task) => [task.id, task.priority])).toEqual([
      ['task-1', 5],
      ['task-2', 5],
    ]);
  });

  it('sends the projects to add and remove, never a replacement list', async () => {
    fetch.mockResolvedValueOnce(Response.json({ tasks: [first, second] }));

    await tasksApi.updateTasks('workspace-1', ids, {
      add_project_ids: ['project-3'],
      remove_project_ids: ['project-1'],
    });

    const [, init] = fetch.mock.calls[0];
    expect(JSON.parse(init.body)).toEqual({
      ids,
      add_project_ids: ['project-3'],
      remove_project_ids: ['project-1'],
    });
  });

  it('names the tasks a live run holds when the server refuses for them', async () => {
    fetch.mockResolvedValueOnce(
      Response.json(
        { error: 'Task has an active run or is no longer available', ids: ['task-2'] },
        { status: 409 }
      )
    );

    const failure = await tasksApi
      .updateTasks('workspace-1', ids, { status: 'paused' })
      .catch((error: unknown) => error);

    expect(failure).toBeInstanceOf(ActiveRunError);
    expect((failure as ActiveRunError).name).toBe('ActiveRunError');
    expect((failure as ActiveRunError).message).toBe(
      'Task has an active run or is no longer available'
    );
    expect((failure as ActiveRunError).ids).toEqual(['task-2']);
  });

  it('surfaces a refusal without ids as a plain error', async () => {
    fetch.mockResolvedValueOnce(
      Response.json({ error: 'Task has an active run or is no longer available' }, { status: 409 })
    );

    const failure = await tasksApi
      .updateTasks('workspace-1', ids, { status: 'paused' })
      .catch((error: unknown) => error);

    expect(failure).toBeInstanceOf(Error);
    expect(failure).not.toBeInstanceOf(ActiveRunError);
    expect((failure as Error).message).toBe('Task has an active run or is no longer available');
  });

  it('surfaces any other refusal with its message', async () => {
    fetch.mockResolvedValueOnce(
      Response.json({ error: 'A project cannot be both added and removed' }, { status: 400 })
    );

    const failure = await tasksApi
      .updateTasks('workspace-1', ids, { add_project_ids: ['project-1'] })
      .catch((error: unknown) => error);

    expect(failure).not.toBeInstanceOf(ActiveRunError);
    expect((failure as Error).message).toBe('A project cannot be both added and removed');
  });

  it('names the status when the refusal has no message', async () => {
    fetch.mockResolvedValueOnce(new Response('<html>bad gateway</html>', { status: 502 }));

    await expect(tasksApi.updateTasks('workspace-1', ids, { priority: 1 })).rejects.toThrow(
      'Failed to update tasks: 502'
    );
  });

  it('rejects a reply that is not a task list', async () => {
    fetch.mockResolvedValueOnce(Response.json({ task: first }));

    await expect(tasksApi.updateTasks('workspace-1', ids, { priority: 1 })).rejects.toThrow();
  });
});
