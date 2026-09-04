import {
  TaskResponseSchema,
  TaskRunLogsResponseSchema,
  TaskRunResponseSchema,
  TaskRunsResponseSchema,
  TasksResponseSchema,
} from '../features/tasks/schemas';
import type {
  CreateTaskRequest,
  Task,
  TaskRun,
  TaskRunLog,
  UpdateTaskRequest,
} from '../features/tasks/types';
import { parse } from '../validation';
import { API_BASE } from './client';

// Helper to parse error responses
async function parseErrorResponse(response: Response): Promise<{ message?: string }> {
  try {
    const data = await response.json();
    return { message: data.message || data.error || data.detail };
  } catch {
    return {};
  }
}

class TasksApi {
  private getAccessToken: (() => string | null) | null = null;

  setGetAccessToken(fn: () => string | null): void {
    this.getAccessToken = fn;
  }

  private getHeaders(): HeadersInit {
    const headers: HeadersInit = {
      'Content-Type': 'application/json',
    };
    const token = this.getAccessToken?.();
    if (token) {
      headers.Authorization = `Bearer ${token}`;
    }
    return headers;
  }

  async getTasks(workspaceId: string, projectId?: string, status?: string): Promise<Task[]> {
    const params = new URLSearchParams();
    if (projectId) params.set('project_id', projectId);
    if (status) params.set('status', status);
    const query = params.toString() ? `?${params}` : '';

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
