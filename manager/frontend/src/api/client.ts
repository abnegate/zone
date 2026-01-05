import type {
  Chat,
  CreateChatRequest,
  CreateOrganizationRequest,
  CreateProjectRequest,
  CreateSourceRequest,
  CreateTaskRequest,
  CreateWorkspaceRequest,
  Message,
  ModelSource,
  Organization,
  Project,
  SendMessageRequest,
  Source,
  SourceType,
  Task,
  TaskRun,
  TaskRunLog,
  UpdateOrganizationRequest,
  UpdateProjectRequest,
  UpdateSourceRequest,
  UpdateTaskRequest,
  UpdateWorkspaceRequest,
  UpdateWorkspaceThemeRequest,
  Workspace,
  WorkspaceTheme,
} from '../types';
import type { SourceTypesResponse, SourceVerifyResponse } from '../types';
import { parse } from '../validation';
import {
  BrowseResponseSchema,
  ChatResponseSchema,
  ChatsResponseSchema,
  MessageResponseSchema,
  MessagesResponseSchema,
  ModelsResponseSchema,
  OrganizationResponseSchema,
  OrganizationsResponseSchema,
  ProjectResponseSchema,
  ProjectsResponseSchema,
  SourceResponseSchema,
  SourceTypesResponseSchema,
  SourceVerifyResponseSchema,
  SourcesResponseSchema,
  TaskResponseSchema,
  TaskRunLogsResponseSchema,
  TaskRunResponseSchema,
  TaskRunsResponseSchema,
  TasksResponseSchema,
  WorkspaceResponseSchema,
  WorkspaceThemeResponseSchema,
  WorkspacesResponseSchema,
} from '../validation/schemas';

// In development, set REACT_APP_API_URL=http://localhost:8000
// In production (served by backend), leave empty to use relative URLs
const API_BASE = process.env.REACT_APP_API_URL || '';

class Client {
  private accessToken: string | null = null;

  setAccessToken(token: string | null) {
    this.accessToken = token;
  }

  private getHeaders(): HeadersInit {
    const headers: HeadersInit = {
      'Content-Type': 'application/json',
    };
    if (this.accessToken) {
      headers.Authorization = `Bearer ${this.accessToken}`;
    }
    return headers;
  }

  async getModels(): Promise<{
    models: Array<{
      name: string;
      size: number;
      modified_at: string;
      details?: { description?: string; family?: string };
    }>;
  }> {
    const response = await fetch(`${API_BASE}/api/models`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to fetch models: ${response.status}`);
    }
    const text = await response.text();
    if (!text) {
      return { models: [] };
    }
    try {
      const data = JSON.parse(text);
      return parse(ModelsResponseSchema, data);
    } catch (e) {
      if (e instanceof SyntaxError) {
        throw new Error('Invalid response from server');
      }
      throw e;
    }
  }

  async deleteModel(name: string): Promise<void> {
    const response = await fetch(`${API_BASE}/api/models/${encodeURIComponent(name)}`, {
      method: 'DELETE',
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to delete model: ${response.status}`);
    }
  }

  async browseModels(
    source: ModelSource,
    query = '',
    offset = 0,
    limit = 20
  ): Promise<{
    source: ModelSource;
    models: Array<{
      id: string;
      name: string;
      description: string;
      downloads: number;
      tags: string[];
    }>;
    total?: number | null;
    has_more: boolean;
  }> {
    const params = new URLSearchParams({
      source,
      q: query,
      offset: offset.toString(),
      limit: limit.toString(),
    });
    const response = await fetch(`${API_BASE}/api/models?${params}`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to browse models: ${response.status}`);
    }
    const data = await response.json();
    return parse(BrowseResponseSchema, data);
  }

  async getModelInfo(
    modelId: string
  ): Promise<{ content: string | null; gguf_size: number | null }> {
    // modelId may contain slashes (e.g., "author/model"), don't encode them
    const response = await fetch(`${API_BASE}/api/models/${modelId}`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to fetch model info: ${response.status}`);
    }
    const data = await response.json();
    return { content: data.content, gguf_size: data.gguf_size };
  }

  createPullWebSocket(modelName: string): WebSocket {
    let wsUrl: string;
    if (API_BASE) {
      // Development: use configured API URL
      const wsBase = API_BASE.replace(/^http/, 'ws');
      wsUrl = `${wsBase}/ws/pull?model=${encodeURIComponent(modelName)}`;
    } else {
      // Production: use current host
      const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
      wsUrl = `${protocol}//${window.location.host}/ws/pull?model=${encodeURIComponent(modelName)}`;
    }
    return new WebSocket(wsUrl);
  }

  // =============================================================================
  // Chats API
  // =============================================================================

  async getChats(archived?: boolean): Promise<Chat[]> {
    const params = archived !== undefined ? `?archived=${archived}` : '';
    const response = await fetch(`${API_BASE}/api/chats${params}`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to fetch chats: ${response.status}`);
    }
    const data = parse(ChatsResponseSchema, await response.json());
    return data.chats;
  }

  async getChat(id: string): Promise<Chat & { messages: Message[] }> {
    const response = await fetch(`${API_BASE}/api/chats/${id}`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to fetch chat: ${response.status}`);
    }
    const data = parse(ChatResponseSchema, await response.json());
    return data.chat;
  }

  async createChat(request: CreateChatRequest): Promise<Chat> {
    const response = await fetch(`${API_BASE}/api/chats`, {
      method: 'POST',
      headers: this.getHeaders(),
      body: JSON.stringify(request),
    });
    if (!response.ok) {
      throw new Error(`Failed to create chat: ${response.status}`);
    }
    const data = parse(ChatResponseSchema, await response.json());
    return data.chat;
  }

  async updateChatTitle(id: string, title: string): Promise<Chat> {
    const response = await fetch(`${API_BASE}/api/chats/${id}`, {
      method: 'PATCH',
      headers: this.getHeaders(),
      body: JSON.stringify({ title }),
    });
    if (!response.ok) {
      throw new Error(`Failed to update chat: ${response.status}`);
    }
    const data = parse(ChatResponseSchema, await response.json());
    return data.chat;
  }

  async deleteChat(id: string): Promise<void> {
    const response = await fetch(`${API_BASE}/api/chats/${id}`, {
      method: 'DELETE',
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to delete chat: ${response.status}`);
    }
  }

  async archiveChat(id: string): Promise<Chat> {
    const response = await fetch(`${API_BASE}/api/chats/${id}/archive`, {
      method: 'POST',
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to archive chat: ${response.status}`);
    }
    const data = parse(ChatResponseSchema, await response.json());
    return data.chat;
  }

  async unarchiveChat(id: string): Promise<Chat> {
    const response = await fetch(`${API_BASE}/api/chats/${id}/unarchive`, {
      method: 'POST',
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to unarchive chat: ${response.status}`);
    }
    const data = parse(ChatResponseSchema, await response.json());
    return data.chat;
  }

  async getMessages(chatId: string): Promise<Message[]> {
    const response = await fetch(`${API_BASE}/api/chats/${chatId}/messages`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to fetch messages: ${response.status}`);
    }
    const data = parse(MessagesResponseSchema, await response.json());
    return data.messages;
  }

  async sendMessage(chatId: string, request: SendMessageRequest): Promise<Message> {
    const response = await fetch(`${API_BASE}/api/chats/${chatId}/messages`, {
      method: 'POST',
      headers: this.getHeaders(),
      body: JSON.stringify(request),
    });
    if (!response.ok) {
      throw new Error(`Failed to send message: ${response.status}`);
    }
    const data = parse(MessageResponseSchema, await response.json());
    return data.message;
  }

  async deleteMessage(chatId: string, messageId: string): Promise<void> {
    const response = await fetch(`${API_BASE}/api/chats/${chatId}/messages/${messageId}`, {
      method: 'DELETE',
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to delete message: ${response.status}`);
    }
  }

  // =============================================================================
  // Projects API
  // =============================================================================

  async getProjects(status?: string): Promise<Project[]> {
    const params = status ? `?status=${status}` : '';
    const response = await fetch(`${API_BASE}/api/projects${params}`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to fetch projects: ${response.status}`);
    }
    const data = parse(ProjectsResponseSchema, await response.json());
    return data.projects;
  }

  async getProject(id: string): Promise<Project> {
    const response = await fetch(`${API_BASE}/api/projects/${id}`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to fetch project: ${response.status}`);
    }
    const data = parse(ProjectResponseSchema, await response.json());
    return data.project;
  }

  async createProject(request: CreateProjectRequest): Promise<Project> {
    const response = await fetch(`${API_BASE}/api/projects`, {
      method: 'POST',
      headers: this.getHeaders(),
      body: JSON.stringify(request),
    });
    if (!response.ok) {
      throw new Error(`Failed to create project: ${response.status}`);
    }
    const data = parse(ProjectResponseSchema, await response.json());
    return data.project;
  }

  async updateProject(id: string, request: UpdateProjectRequest): Promise<Project> {
    const response = await fetch(`${API_BASE}/api/projects/${id}`, {
      method: 'PATCH',
      headers: this.getHeaders(),
      body: JSON.stringify(request),
    });
    if (!response.ok) {
      throw new Error(`Failed to update project: ${response.status}`);
    }
    const data = parse(ProjectResponseSchema, await response.json());
    return data.project;
  }

  async deleteProject(id: string): Promise<void> {
    const response = await fetch(`${API_BASE}/api/projects/${id}`, {
      method: 'DELETE',
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to delete project: ${response.status}`);
    }
  }

  async linkGitHub(projectId: string, repoUrl: string): Promise<Project> {
    const response = await fetch(`${API_BASE}/api/projects/${projectId}/github`, {
      method: 'PUT',
      headers: this.getHeaders(),
      body: JSON.stringify({ repo_url: repoUrl }),
    });
    if (!response.ok) {
      throw new Error(`Failed to link GitHub: ${response.status}`);
    }
    const data = parse(ProjectResponseSchema, await response.json());
    return data.project;
  }

  async unlinkGitHub(projectId: string): Promise<Project> {
    const response = await fetch(`${API_BASE}/api/projects/${projectId}/github`, {
      method: 'DELETE',
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to unlink GitHub: ${response.status}`);
    }
    const data = parse(ProjectResponseSchema, await response.json());
    return data.project;
  }

  // =============================================================================
  // Tasks API
  // =============================================================================

  async getTasks(projectId?: string, status?: string): Promise<Task[]> {
    const params = new URLSearchParams();
    if (projectId) params.set('project_id', projectId);
    if (status) params.set('status', status);
    const query = params.toString() ? `?${params}` : '';

    const response = await fetch(`${API_BASE}/api/tasks${query}`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to fetch tasks: ${response.status}`);
    }
    const data = parse(TasksResponseSchema, await response.json());
    return data.tasks;
  }

  async getTask(id: string): Promise<Task> {
    const response = await fetch(`${API_BASE}/api/tasks/${id}`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to fetch task: ${response.status}`);
    }
    const data = parse(TaskResponseSchema, await response.json());
    return data.task;
  }

  async createTask(request: CreateTaskRequest): Promise<Task> {
    const response = await fetch(`${API_BASE}/api/tasks`, {
      method: 'POST',
      headers: this.getHeaders(),
      body: JSON.stringify(request),
    });
    if (!response.ok) {
      throw new Error(`Failed to create task: ${response.status}`);
    }
    const data = parse(TaskResponseSchema, await response.json());
    return data.task;
  }

  async updateTask(id: string, request: UpdateTaskRequest): Promise<Task> {
    const response = await fetch(`${API_BASE}/api/tasks/${id}`, {
      method: 'PATCH',
      headers: this.getHeaders(),
      body: JSON.stringify(request),
    });
    if (!response.ok) {
      throw new Error(`Failed to update task: ${response.status}`);
    }
    const data = parse(TaskResponseSchema, await response.json());
    return data.task;
  }

  async deleteTask(id: string): Promise<void> {
    const response = await fetch(`${API_BASE}/api/tasks/${id}`, {
      method: 'DELETE',
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to delete task: ${response.status}`);
    }
  }

  async startTask(id: string): Promise<{ run_id: string }> {
    const response = await fetch(`${API_BASE}/api/tasks/${id}/start`, {
      method: 'POST',
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to start task: ${response.status}`);
    }
    return response.json();
  }

  async stopTask(id: string): Promise<{ run_id: string }> {
    const response = await fetch(`${API_BASE}/api/tasks/${id}/stop`, {
      method: 'POST',
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to stop task: ${response.status}`);
    }
    return response.json();
  }

  async getTaskRuns(taskId: string): Promise<TaskRun[]> {
    const response = await fetch(`${API_BASE}/api/tasks/${taskId}/runs`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to fetch task runs: ${response.status}`);
    }
    const data = parse(TaskRunsResponseSchema, await response.json());
    return data.runs;
  }

  async getTaskRun(taskId: string, runId: string): Promise<TaskRun> {
    const response = await fetch(`${API_BASE}/api/tasks/${taskId}/runs/${runId}`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to fetch task run: ${response.status}`);
    }
    const data = parse(TaskRunResponseSchema, await response.json());
    return data.run;
  }

  async getTaskRunLogs(taskId: string, runId: string): Promise<TaskRunLog[]> {
    const response = await fetch(`${API_BASE}/api/tasks/${taskId}/runs/${runId}/logs`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to fetch task run logs: ${response.status}`);
    }
    const data = parse(TaskRunLogsResponseSchema, await response.json());
    return data.logs;
  }

  createTaskWebSocket(runId: string): WebSocket {
    let wsUrl: string;
    if (API_BASE) {
      const wsBase = API_BASE.replace(/^http/, 'ws');
      wsUrl = `${wsBase}/ws/tasks/${runId}`;
    } else {
      const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
      wsUrl = `${protocol}//${window.location.host}/ws/tasks/${runId}`;
    }
    return new WebSocket(wsUrl);
  }

  // =============================================================================
  // Sources API
  // =============================================================================

  async getSourceTypes(): Promise<SourceTypesResponse['types']> {
    const response = await fetch(`${API_BASE}/api/sources/types`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to fetch source types: ${response.status}`);
    }
    const data = parse(SourceTypesResponseSchema, await response.json());
    return data.types;
  }

  async getSources(type?: SourceType, activeOnly = false): Promise<Source[]> {
    const params = new URLSearchParams();
    if (type) params.set('type', type);
    if (activeOnly) params.set('active', 'true');
    const query = params.toString() ? `?${params}` : '';

    const response = await fetch(`${API_BASE}/api/sources${query}`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to fetch sources: ${response.status}`);
    }
    const data = parse(SourcesResponseSchema, await response.json());
    return data.sources;
  }

  async getSource(id: string): Promise<Source> {
    const response = await fetch(`${API_BASE}/api/sources/${id}`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to fetch source: ${response.status}`);
    }
    const data = parse(SourceResponseSchema, await response.json());
    return data.source;
  }

  async createSource(request: CreateSourceRequest): Promise<Source> {
    const response = await fetch(`${API_BASE}/api/sources`, {
      method: 'POST',
      headers: this.getHeaders(),
      body: JSON.stringify(request),
    });
    if (!response.ok) {
      throw new Error(`Failed to create source: ${response.status}`);
    }
    const data = parse(SourceResponseSchema, await response.json());
    return data.source;
  }

  async updateSource(id: string, request: UpdateSourceRequest): Promise<Source> {
    const response = await fetch(`${API_BASE}/api/sources/${id}`, {
      method: 'PATCH',
      headers: this.getHeaders(),
      body: JSON.stringify(request),
    });
    if (!response.ok) {
      throw new Error(`Failed to update source: ${response.status}`);
    }
    const data = parse(SourceResponseSchema, await response.json());
    return data.source;
  }

  async deleteSource(id: string): Promise<void> {
    const response = await fetch(`${API_BASE}/api/sources/${id}`, {
      method: 'DELETE',
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to delete source: ${response.status}`);
    }
  }

  async verifySource(id: string): Promise<SourceVerifyResponse> {
    const response = await fetch(`${API_BASE}/api/sources/${id}/verify`, {
      method: 'POST',
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to verify source: ${response.status}`);
    }
    return parse(SourceVerifyResponseSchema, await response.json());
  }

  async linkSource(projectId: string, sourceId: string): Promise<Project> {
    const response = await fetch(`${API_BASE}/api/projects/${projectId}/source`, {
      method: 'PUT',
      headers: this.getHeaders(),
      body: JSON.stringify({ source_id: sourceId }),
    });
    if (!response.ok) {
      throw new Error(`Failed to link source: ${response.status}`);
    }
    const data = parse(ProjectResponseSchema, await response.json());
    return data.project;
  }

  async unlinkSource(projectId: string): Promise<Project> {
    const response = await fetch(`${API_BASE}/api/projects/${projectId}/source`, {
      method: 'DELETE',
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to unlink source: ${response.status}`);
    }
    const data = parse(ProjectResponseSchema, await response.json());
    return data.project;
  }

  // =============================================================================
  // Organizations API
  // =============================================================================

  async getOrganizations(activeOnly = false): Promise<Organization[]> {
    const params = activeOnly ? '?active=true' : '';
    const response = await fetch(`${API_BASE}/api/organizations${params}`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to fetch organizations: ${response.status}`);
    }
    const data = parse(OrganizationsResponseSchema, await response.json());
    return data.organizations;
  }

  async getOrganization(id: string): Promise<Organization> {
    const response = await fetch(`${API_BASE}/api/organizations/${id}`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to fetch organization: ${response.status}`);
    }
    const data = parse(OrganizationResponseSchema, await response.json());
    return data.organization;
  }

  async createOrganization(request: CreateOrganizationRequest): Promise<Organization> {
    const response = await fetch(`${API_BASE}/api/organizations`, {
      method: 'POST',
      headers: this.getHeaders(),
      body: JSON.stringify(request),
    });
    if (!response.ok) {
      throw new Error(`Failed to create organization: ${response.status}`);
    }
    const data = parse(OrganizationResponseSchema, await response.json());
    return data.organization;
  }

  async updateOrganization(id: string, request: UpdateOrganizationRequest): Promise<Organization> {
    const response = await fetch(`${API_BASE}/api/organizations/${id}`, {
      method: 'PATCH',
      headers: this.getHeaders(),
      body: JSON.stringify(request),
    });
    if (!response.ok) {
      throw new Error(`Failed to update organization: ${response.status}`);
    }
    const data = parse(OrganizationResponseSchema, await response.json());
    return data.organization;
  }

  async deleteOrganization(id: string): Promise<void> {
    const response = await fetch(`${API_BASE}/api/organizations/${id}`, {
      method: 'DELETE',
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to delete organization: ${response.status}`);
    }
  }

  // =============================================================================
  // Workspaces API (nested under organizations)
  // =============================================================================

  async getWorkspaces(orgId: string, activeOnly = false): Promise<Workspace[]> {
    const params = activeOnly ? '?active=true' : '';
    const response = await fetch(`${API_BASE}/api/organizations/${orgId}/workspaces${params}`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to fetch workspaces: ${response.status}`);
    }
    const data = parse(WorkspacesResponseSchema, await response.json());
    return data.workspaces;
  }

  async getWorkspace(orgId: string, wsId: string): Promise<Workspace> {
    const response = await fetch(`${API_BASE}/api/organizations/${orgId}/workspaces/${wsId}`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to fetch workspace: ${response.status}`);
    }
    const data = parse(WorkspaceResponseSchema, await response.json());
    return data.workspace;
  }

  async createWorkspace(orgId: string, request: CreateWorkspaceRequest): Promise<Workspace> {
    const response = await fetch(`${API_BASE}/api/organizations/${orgId}/workspaces`, {
      method: 'POST',
      headers: this.getHeaders(),
      body: JSON.stringify(request),
    });
    if (!response.ok) {
      throw new Error(`Failed to create workspace: ${response.status}`);
    }
    const data = parse(WorkspaceResponseSchema, await response.json());
    return data.workspace;
  }

  async updateWorkspace(
    orgId: string,
    wsId: string,
    request: UpdateWorkspaceRequest
  ): Promise<Workspace> {
    const response = await fetch(`${API_BASE}/api/organizations/${orgId}/workspaces/${wsId}`, {
      method: 'PATCH',
      headers: this.getHeaders(),
      body: JSON.stringify(request),
    });
    if (!response.ok) {
      throw new Error(`Failed to update workspace: ${response.status}`);
    }
    const data = parse(WorkspaceResponseSchema, await response.json());
    return data.workspace;
  }

  async deleteWorkspace(orgId: string, wsId: string): Promise<void> {
    const response = await fetch(`${API_BASE}/api/organizations/${orgId}/workspaces/${wsId}`, {
      method: 'DELETE',
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to delete workspace: ${response.status}`);
    }
  }

  // =============================================================================
  // Workspace Theme API (nested under organizations/workspaces)
  // =============================================================================

  async getWorkspaceTheme(orgId: string, wsId: string): Promise<WorkspaceTheme> {
    const response = await fetch(
      `${API_BASE}/api/organizations/${orgId}/workspaces/${wsId}/settings/theme`,
      { headers: this.getHeaders() }
    );
    if (!response.ok) {
      throw new Error(`Failed to fetch workspace theme: ${response.status}`);
    }
    const data = parse(WorkspaceThemeResponseSchema, await response.json());
    return data.theme;
  }

  async updateWorkspaceTheme(
    orgId: string,
    wsId: string,
    request: UpdateWorkspaceThemeRequest
  ): Promise<WorkspaceTheme> {
    const response = await fetch(
      `${API_BASE}/api/organizations/${orgId}/workspaces/${wsId}/settings/theme`,
      {
        method: 'PUT',
        headers: this.getHeaders(),
        body: JSON.stringify(request),
      }
    );
    if (!response.ok) {
      throw new Error(`Failed to update workspace theme: ${response.status}`);
    }
    const data = parse(WorkspaceThemeResponseSchema, await response.json());
    return data.theme;
  }

  async resetWorkspaceTheme(orgId: string, wsId: string): Promise<WorkspaceTheme> {
    const response = await fetch(
      `${API_BASE}/api/organizations/${orgId}/workspaces/${wsId}/settings/theme`,
      {
        method: 'DELETE',
        headers: this.getHeaders(),
      }
    );
    if (!response.ok) {
      throw new Error(`Failed to reset workspace theme: ${response.status}`);
    }
    const data = parse(WorkspaceThemeResponseSchema, await response.json());
    return data.theme;
  }
}

export const client = new Client();
