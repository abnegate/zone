import type { Answer } from '../features/chats/types';
import { ActiveRunError } from '../features/tasks/ActiveRunError';
import {
  ActiveRunRefusalSchema,
  AnswersResponseSchema,
  TaskResponseSchema,
  TaskRunLogsResponseSchema,
  TaskRunResponseSchema,
  TaskRunsResponseSchema,
  TasksResponseSchema,
} from '../features/tasks/schemas';
import type {
  AnswersResponse,
  CreateTaskRequest,
  Task,
  TaskChanges,
  TaskRun,
  TaskRunLog,
  UpdateTaskRequest,
  UpdateTasksRequest,
} from '../features/tasks/types';
import { parse } from '../validation';
import { API_BASE } from './client';
import { deviceHeaders } from './device';
import { session } from './session';

const CONFLICT = 409;

async function parseErrorResponse(response: Response): Promise<{ message?: string }> {
  try {
    return { message: messageOf(await response.json()) };
  } catch {
    return {};
  }
}

function messageOf(data: unknown): string | undefined {
  if (typeof data !== 'object' || data === null) return undefined;
  const { message, error, detail } = data as Record<string, unknown>;
  const text = message || error || detail;
  return typeof text === 'string' ? text : undefined;
}

class TasksApi {
  private getAccessToken: () => string | null = () => session.getAccessToken();

  setGetAccessToken(provider: () => string | null): void {
    this.getAccessToken = provider;
  }

  private getHeaders(): HeadersInit {
    const headers: HeadersInit = {
      'Content-Type': 'application/json',
      ...deviceHeaders(),
    };
    const token = this.getAccessToken?.();
    if (token) {
      headers.Authorization = `Bearer ${token}`;
    }
    return headers;
  }

  async getTasks(workspaceId: string, projectId?: string): Promise<Task[]> {
    const query = projectId ? `?${new URLSearchParams({ project_id: projectId })}` : '';

    const response = await fetch(
      `${API_BASE}/api/workspaces/${encodeURIComponent(workspaceId)}/tasks${query}`,
      {
        headers: this.getHeaders(),
      }
    );
    if (!response.ok) {
      const errorData = await parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to fetch tasks: ${response.status}`);
    }
    const data = parse(TasksResponseSchema, await response.json());
    return data.tasks;
  }

  async getTask(id: string): Promise<Task> {
    const response = await fetch(`${API_BASE}/api/tasks/${encodeURIComponent(id)}`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      const errorData = await parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to fetch task: ${response.status}`);
    }
    const data = parse(TaskResponseSchema, await response.json());
    return data.task;
  }

  async createTask(workspaceId: string, request: CreateTaskRequest): Promise<Task> {
    const response = await fetch(
      `${API_BASE}/api/workspaces/${encodeURIComponent(workspaceId)}/tasks`,
      {
        method: 'POST',
        headers: this.getHeaders(),
        body: JSON.stringify(request),
      }
    );
    if (!response.ok) {
      const errorData = await parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to create task: ${response.status}`);
    }
    const data = parse(TaskResponseSchema, await response.json());
    return data.task;
  }

  async updateTask(id: string, request: UpdateTaskRequest): Promise<Task> {
    const response = await fetch(`${API_BASE}/api/tasks/${encodeURIComponent(id)}`, {
      method: 'PUT',
      headers: this.getHeaders(),
      body: JSON.stringify(request),
    });
    if (!response.ok) {
      const errorData = await parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to update task: ${response.status}`);
    }
    const data = parse(TaskResponseSchema, await response.json());
    return data.task;
  }

  async updateTasks(
    workspaceId: string,
    ids: readonly string[],
    changes: TaskChanges
  ): Promise<Task[]> {
    const request: UpdateTasksRequest = { ids: [...ids], ...changes };
    const response = await fetch(
      `${API_BASE}/api/workspaces/${encodeURIComponent(workspaceId)}/tasks`,
      {
        method: 'PATCH',
        headers: this.getHeaders(),
        body: JSON.stringify(request),
      }
    );
    if (!response.ok) {
      const body: unknown = await response.json().catch(() => undefined);
      const refusal = ActiveRunRefusalSchema.safeParse(body);
      if (response.status === CONFLICT && refusal.success) {
        throw new ActiveRunError(refusal.data.error, refusal.data.ids);
      }
      throw new Error(messageOf(body) || `Failed to update tasks: ${response.status}`);
    }
    const data = parse(TasksResponseSchema, await response.json());
    return data.tasks;
  }

  async deleteTask(id: string): Promise<void> {
    const response = await fetch(`${API_BASE}/api/tasks/${encodeURIComponent(id)}`, {
      method: 'DELETE',
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      const errorData = await parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to delete task: ${response.status}`);
    }
  }

  async runTask(id: string, signal?: AbortSignal): Promise<TaskRun> {
    const response = await fetch(`${API_BASE}/api/tasks/${encodeURIComponent(id)}/runs`, {
      method: 'POST',
      headers: this.getHeaders(),
      signal,
    });
    if (!response.ok) {
      const errorData = await parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to run task: ${response.status}`);
    }
    const data = parse(TaskRunResponseSchema, await response.json());
    return data.run;
  }

  async getTaskRuns(taskId: string, signal?: AbortSignal): Promise<TaskRun[]> {
    const response = await fetch(`${API_BASE}/api/tasks/${encodeURIComponent(taskId)}/runs`, {
      headers: this.getHeaders(),
      signal,
    });
    if (!response.ok) {
      const errorData = await parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to fetch task runs: ${response.status}`);
    }
    const data = parse(TaskRunsResponseSchema, await response.json());
    return data.runs;
  }

  async getTaskRun(_taskId: string, runId: string, signal?: AbortSignal): Promise<TaskRun> {
    const response = await fetch(`${API_BASE}/api/tasks/runs/${encodeURIComponent(runId)}`, {
      headers: this.getHeaders(),
      signal,
    });
    if (!response.ok) {
      const errorData = await parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to fetch task run: ${response.status}`);
    }
    const data = parse(TaskRunResponseSchema, await response.json());
    return data.run;
  }

  /**
   * Answers the question a waiting run parked on.
   *
   * The structured answers travel, not the string the reader sees: the server
   * renders the message the agent reads back, so one renderer decides what the
   * agent was told and the console cannot disagree with it.
   *
   * The reply confirms the submission rather than returning the run: resuming
   * the parked worker happens out of band, so the moved run is read back by
   * refetching it.
   *
   * A 2xx means the waiter has the answers and the run has already moved, so
   * the confirmation on top of it is a receipt and nothing more. Refusing to
   * read one would report a delivered answer as a rejected one and invite a
   * second send of a question that is no longer being asked, so an unreadable
   * body costs the count, never the acceptance.
   */
  async answerRun(
    runId: string,
    answers: Answer[],
    signal?: AbortSignal
  ): Promise<AnswersResponse | undefined> {
    const response = await fetch(
      `${API_BASE}/api/tasks/runs/${encodeURIComponent(runId)}/answers`,
      {
        method: 'POST',
        headers: this.getHeaders(),
        body: JSON.stringify({ answers }),
        signal,
      }
    );
    if (!response.ok) {
      const errorData = await parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to answer task run: ${response.status}`);
    }
    const receipt = AnswersResponseSchema.safeParse(await response.json().catch(() => undefined));
    return receipt.success ? receipt.data : undefined;
  }

  async getTaskRunLogs(
    _taskId: string,
    runId: string,
    signal?: AbortSignal
  ): Promise<TaskRunLog[]> {
    const response = await fetch(`${API_BASE}/api/tasks/runs/${encodeURIComponent(runId)}/logs`, {
      headers: this.getHeaders(),
      signal,
    });
    if (!response.ok) {
      const errorData = await parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to fetch task run logs: ${response.status}`);
    }
    const data = parse(TaskRunLogsResponseSchema, await response.json());
    return data.logs;
  }

  createTaskWebSocket(runId: string): WebSocket {
    let wsUrl: string;
    if (API_BASE) {
      const wsBase = API_BASE.replace(/^http/, 'ws');
      wsUrl = `${wsBase}/ws/tasks/runs/${encodeURIComponent(runId)}`;
    } else {
      const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
      wsUrl = `${protocol}//${window.location.host}/ws/tasks/runs/${encodeURIComponent(runId)}`;
    }
    const socket = new WebSocket(wsUrl);
    socket.addEventListener(
      'open',
      () => {
        socket.send(JSON.stringify({ type: 'auth', token: this.getAccessToken?.() ?? '' }));
      },
      { once: true }
    );
    return socket;
  }
}

export const tasksApi = new TasksApi();
