import type {
  AddOrgMemberRequest,
  AddWorkspaceMemberRequest,
  AiSettings,
  AuditLog,
  AuditLogFilters,
  AuditLogsResponse,
  Chat,
  ChatSearchOptions,
  ChatSearchResponse,
  ChatWithMessages,
  CreateChatRequest,
  CreateInvitationRequest,
  CreateOrganizationRequest,
  CreateWorkspaceRequest,
  Invitation,
  InvitationDetails,
  InvitationsResponse,
  Limits,
  Message,
  ModelSource,
  OrgMembersResponse,
  Organization,
  OrganizationMember,
  Plan,
  SendMessageRequest,
  SessionsResponse,
  Subscription,
  UpdateAiSettingsRequest,
  UpdateOrgMemberRequest,
  UpdateOrganizationRequest,
  UpdateWorkspaceMemberRequest,
  UpdateWorkspaceRequest,
  UpdateWorkspaceThemeRequest,
  Usage,
  Workspace,
  WorkspaceMember,
  WorkspaceMembersResponse,
  WorkspaceTheme,
} from '../types';
import type {
  CreateKnowledgeRequest,
  GatherContextRequest,
  SearchOptions,
} from '../features/knowledge/types';
import type {
  Project,
  CreateProjectRequest,
  UpdateProjectRequest,
  SyncConfig,
  CreateSyncConfigRequest,
} from '../features/projects/types';
import type {
  Source,
  SourceType,
  CreateSourceRequest,
  UpdateSourceRequest,
} from '../features/sources/types';
import type { SourceVerifyResponse, SourceTypesResponse } from '../features/sources/schemas';
import { modelsApi } from './models';
import { chatsApi } from './chats';
import { projectsApi } from './projects';
import { tasksApi } from './tasks';
import { sourcesApi } from './sources';
import { knowledgeApi } from './knowledge';
import { parse } from '../validation';
import {
  AiSettingsResponseSchema,
  AuditLogSchema,
  AuditLogsResponseSchema,
  ForgotPasswordResponseSchema,
  InvitationDetailsSchema,
  InvitationSchema,
  InvitationsResponseSchema,
  LimitsResponseSchema,
  OrgMembersResponseSchema,
  OrganizationMemberSchema,
  OrganizationResponseSchema,
  OrganizationsResponseSchema,
  PlanResponseSchema,
  PlansResponseSchema,
  ProjectResponseSchema,
  ResendVerificationResponseSchema,
  ResetPasswordResponseSchema,
  SessionsResponseSchema,
  SubscriptionResponseSchema,
  UsageResponseSchema,
  VerifyEmailResponseSchema,
  WorkspaceMemberSchema,
  WorkspaceMembersResponseSchema,
  WorkspaceResponseSchema,
  WorkspaceThemeResponseSchema,
  WorkspacesResponseSchema,
} from '../validation/schemas';

// In development, set REACT_APP_API_URL=http://localhost:8000
// In production (served by backend), leave empty to use relative URLs
export const API_BASE = import.meta.env.VITE_API_URL || '';

class Client {
  private accessToken: string | null = null;

  setAccessToken(token: string | null) {
    this.accessToken = token;
    // Update chatsApi with token getter
    chatsApi.setGetAccessToken(() => this.accessToken);
    // Update projectsApi with token getter
    projectsApi.setGetAccessToken(() => this.accessToken);
    // Update tasksApi with token getter
    tasksApi.setGetAccessToken(() => this.accessToken);
    // Update sourcesApi with token getter
    sourcesApi.setGetAccessToken(() => this.accessToken);
    // Update knowledgeApi with token getter
    knowledgeApi.setGetAccessToken(() => this.accessToken);
  }

  getAccessToken(): string | null {
    return this.accessToken;
  }

  getHeaders(): HeadersInit {
    const headers: HeadersInit = {
      'Content-Type': 'application/json',
    };
    if (this.accessToken) {
      headers.Authorization = `Bearer ${this.accessToken}`;
    }
    return headers;
  }

  // Model methods now delegate to modelsApi
  async getModels() {
    return modelsApi.getModels();
  }

  async deleteModel(name: string) {
    return modelsApi.deleteModel(name);
  }

  async browseModels(source: ModelSource, query = '', cursor?: string | null, limit = 20) {
    return modelsApi.browseModels(source, query, cursor, limit);
  }

  async getModelInfo(modelId: string) {
    return modelsApi.getModelInfo(modelId);
  }

  createPullWebSocket(modelName: string) {
    return modelsApi.createPullWebSocket(modelName);
  }

  // =============================================================================
  // Chats API (delegates to chatsApi)
  // =============================================================================

  async getChats(workspaceId: string, archived?: boolean): Promise<Chat[]> {
    return chatsApi.getChats(workspaceId, archived);
  }

  async getChat(id: string): Promise<ChatWithMessages> {
    return chatsApi.getChat(id);
  }

  async createChat(request: CreateChatRequest): Promise<Chat> {
    return chatsApi.createChat(request);
  }

  async updateChatTitle(id: string, title: string): Promise<Chat> {
    return chatsApi.updateChatTitle(id, title);
  }

  async deleteChat(id: string): Promise<void> {
    return chatsApi.deleteChat(id);
  }

  async archiveChat(id: string): Promise<Chat> {
    return chatsApi.archiveChat(id);
  }

  async unarchiveChat(id: string): Promise<Chat> {
    return chatsApi.unarchiveChat(id);
  }

  async getMessages(chatId: string): Promise<Message[]> {
    return chatsApi.getMessages(chatId);
  }

  async sendMessage(chatId: string, request: SendMessageRequest): Promise<Message> {
    return chatsApi.sendMessage(chatId, request);
  }

  async deleteMessage(chatId: string, messageId: string): Promise<void> {
    return chatsApi.deleteMessage(chatId, messageId);
  }

  async searchChatMessages(options: ChatSearchOptions): Promise<ChatSearchResponse> {
    return chatsApi.searchChatMessages(options);
  }

  // =============================================================================
  // Projects API (delegates to projectsApi)
  // =============================================================================

  async getProjects(workspaceId: string, status?: string): Promise<Project[]> {
    return projectsApi.getProjects(workspaceId, status);
  }

  async getProject(id: string): Promise<Project> {
    return projectsApi.getProject(id);
  }

  async createProject(request: CreateProjectRequest): Promise<Project> {
    return projectsApi.createProject(request);
  }

  async updateProject(id: string, request: UpdateProjectRequest): Promise<Project> {
    return projectsApi.updateProject(id, request);
  }

  async deleteProject(id: string): Promise<void> {
    return projectsApi.deleteProject(id);
  }

  async linkSource(projectId: string, sourceId: string): Promise<Project> {
    return projectsApi.linkSource(projectId, sourceId);
  }

  async unlinkSource(projectId: string): Promise<Project> {
    return projectsApi.unlinkSource(projectId);
  }

  // Deprecated GitHub-specific methods (kept for backward compatibility)
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
  // Tasks API (delegates to tasksApi)
  // =============================================================================

  async getTasks(workspaceId: string, projectId?: string, status?: string) {
    return tasksApi.getTasks(workspaceId, projectId, status);
  }

  async getTask(id: string) {
    return tasksApi.getTask(id);
  }

  async createTask(workspaceId: string, request: import('../features/tasks/types').CreateTaskRequest) {
    return tasksApi.createTask(workspaceId, request);
  }

  async updateTask(id: string, request: import('../features/tasks/types').UpdateTaskRequest) {
    return tasksApi.updateTask(id, request);
  }

  async deleteTask(id: string) {
    return tasksApi.deleteTask(id);
  }

  async startTask(id: string) {
    return tasksApi.runTask(id);
  }

  async stopTask(id: string) {
    return tasksApi.cancelTaskRun(id);
  }

  async getTaskRuns(taskId: string) {
    return tasksApi.getTaskRuns(taskId);
  }

  async getTaskRun(taskId: string, runId: string) {
    return tasksApi.getTaskRun(taskId, runId);
  }

  async getTaskRunLogs(taskId: string, runId: string) {
    return tasksApi.getTaskRunLogs(taskId, runId);
  }

  createTaskWebSocket(runId: string) {
    return tasksApi.createTaskWebSocket(runId);
  }

  // =============================================================================
  // Sources API (delegates to sourcesApi)
  // =============================================================================

  async getSourceTypes(): Promise<SourceTypesResponse['types']> {
    return sourcesApi.getSourceTypes();
  }

  async getSources(workspaceId: string, type?: SourceType, activeOnly = false): Promise<Source[]> {
    return sourcesApi.getSources(workspaceId, type, activeOnly);
  }

  async getSource(workspaceId: string, id: string): Promise<Source> {
    return sourcesApi.getSource(workspaceId, id);
  }

  async createSource(workspaceId: string, request: CreateSourceRequest): Promise<Source> {
    return sourcesApi.createSource(workspaceId, request);
  }

  async updateSource(workspaceId: string, id: string, request: UpdateSourceRequest): Promise<Source> {
    return sourcesApi.updateSource(workspaceId, id, request);
  }

  async deleteSource(workspaceId: string, id: string): Promise<void> {
    return sourcesApi.deleteSource(workspaceId, id);
  }

  async verifySource(workspaceId: string, id: string): Promise<SourceVerifyResponse> {
    return sourcesApi.verifySource(workspaceId, id);
  }

  async reindexSource(workspaceId: string, id: string): Promise<void> {
    return sourcesApi.reindexSource(workspaceId, id);
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

  async getWorkspaceTheme(_orgId: string, wsId: string): Promise<WorkspaceTheme | null> {
    const response = await fetch(
      `${API_BASE}/api/workspaces/${wsId}/theme`,
      { headers: this.getHeaders() }
    );
    // A workspace with no theme override has no stored row; that is the state a
    // new workspace starts in and the state a theme reset returns it to.
    if (response.status === 404) {
      return null;
    }
    if (!response.ok) {
      throw new Error(`Failed to fetch workspace theme: ${response.status}`);
    }
    const data = parse(WorkspaceThemeResponseSchema, await response.json());
    return data.theme;
  }

  async updateWorkspaceTheme(
    _orgId: string,
    wsId: string,
    request: UpdateWorkspaceThemeRequest
  ): Promise<WorkspaceTheme> {
    const response = await fetch(
      `${API_BASE}/api/workspaces/${wsId}/theme`,
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

  async resetWorkspaceTheme(_orgId: string, wsId: string): Promise<WorkspaceTheme> {
    const response = await fetch(
      `${API_BASE}/api/workspaces/${wsId}/theme`,
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

  // =============================================================================
  // Organization AI Settings API
  // =============================================================================

  async getOrgAiSettings(orgId: string): Promise<AiSettings> {
    const response = await fetch(`${API_BASE}/api/organizations/${orgId}/settings/ai`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to fetch org AI settings: ${response.status}`);
    }
    return parse(AiSettingsResponseSchema, await response.json());
  }

  async updateOrgAiSettings(orgId: string, request: UpdateAiSettingsRequest): Promise<AiSettings> {
    const response = await fetch(`${API_BASE}/api/organizations/${orgId}/settings/ai`, {
      method: 'PUT',
      headers: this.getHeaders(),
      body: JSON.stringify(request),
    });
    if (!response.ok) {
      throw new Error(`Failed to update org AI settings: ${response.status}`);
    }
    return parse(AiSettingsResponseSchema, await response.json());
  }

  async resetOrgAiSettings(orgId: string): Promise<AiSettings> {
    const response = await fetch(`${API_BASE}/api/organizations/${orgId}/settings/ai`, {
      method: 'DELETE',
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to reset org AI settings: ${response.status}`);
    }
    return parse(AiSettingsResponseSchema, await response.json());
  }

  // =============================================================================
  // Workspace AI Settings API
  // =============================================================================

  async getWorkspaceAiSettings(orgId: string, wsId: string): Promise<AiSettings> {
    const response = await fetch(
      `${API_BASE}/api/organizations/${orgId}/workspaces/${wsId}/settings/ai`,
      { headers: this.getHeaders() }
    );
    if (!response.ok) {
      throw new Error(`Failed to fetch workspace AI settings: ${response.status}`);
    }
    return parse(AiSettingsResponseSchema, await response.json());
  }

  async updateWorkspaceAiSettings(
    orgId: string,
    wsId: string,
    request: UpdateAiSettingsRequest
  ): Promise<AiSettings> {
    const response = await fetch(
      `${API_BASE}/api/organizations/${orgId}/workspaces/${wsId}/settings/ai`,
      {
        method: 'PUT',
        headers: this.getHeaders(),
        body: JSON.stringify(request),
      }
    );
    if (!response.ok) {
      throw new Error(`Failed to update workspace AI settings: ${response.status}`);
    }
    return parse(AiSettingsResponseSchema, await response.json());
  }

  async resetWorkspaceAiSettings(orgId: string, wsId: string): Promise<AiSettings> {
    const response = await fetch(
      `${API_BASE}/api/organizations/${orgId}/workspaces/${wsId}/settings/ai`,
      {
        method: 'DELETE',
        headers: this.getHeaders(),
      }
    );
    if (!response.ok) {
      throw new Error(`Failed to reset workspace AI settings: ${response.status}`);
    }
    return parse(AiSettingsResponseSchema, await response.json());
  }

  async getEffectiveAiSettings(orgId: string, wsId: string): Promise<AiSettings> {
    const response = await fetch(
      `${API_BASE}/api/organizations/${orgId}/workspaces/${wsId}/settings/ai/effective`,
      { headers: this.getHeaders() }
    );
    if (!response.ok) {
      throw new Error(`Failed to fetch effective AI settings: ${response.status}`);
    }
    return parse(AiSettingsResponseSchema, await response.json());
  }

  // =============================================================================
  // Email Verification & Password Reset API
  // =============================================================================

  async verifyEmail(token: string): Promise<{ success: boolean; message: string }> {
    const response = await fetch(`${API_BASE}/api/auth/verify-email`, {
      method: 'POST',
      headers: this.getHeaders(),
      body: JSON.stringify({ token }),
    });
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to verify email: ${response.status}`);
    }
    return parse(VerifyEmailResponseSchema, await response.json());
  }

  async resendVerification(email: string): Promise<{ success: boolean; message: string }> {
    const response = await fetch(`${API_BASE}/api/auth/resend-verification`, {
      method: 'POST',
      headers: this.getHeaders(),
      body: JSON.stringify({ email }),
    });
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(
        errorData.message || `Failed to resend verification email: ${response.status}`
      );
    }
    return parse(ResendVerificationResponseSchema, await response.json());
  }

  async forgotPassword(email: string): Promise<{ success: boolean; message: string }> {
    const response = await fetch(`${API_BASE}/api/auth/forgot-password`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({ email }),
    });
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to request password reset: ${response.status}`);
    }
    return parse(ForgotPasswordResponseSchema, await response.json());
  }

  async resetPassword(
    token: string,
    newPassword: string
  ): Promise<{ success: boolean; message: string }> {
    const response = await fetch(`${API_BASE}/api/auth/reset-password`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({ token, new_password: newPassword }),
    });
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to reset password: ${response.status}`);
    }
    return parse(ResetPasswordResponseSchema, await response.json());
  }

  private async parseErrorResponse(response: Response): Promise<{ message?: string }> {
    try {
      const data = await response.json();
      return { message: data.message || data.error || data.detail };
    } catch {
      return {};
    }
  }

  // =============================================================================
  // Session Management API
  // =============================================================================

  async getSessions(): Promise<SessionsResponse> {
    const response = await fetch(`${API_BASE}/api/auth/sessions`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to fetch sessions: ${response.status}`);
    }
    return parse(SessionsResponseSchema, await response.json());
  }

  async revokeSession(sessionId: string): Promise<void> {
    const response = await fetch(`${API_BASE}/api/auth/sessions/${encodeURIComponent(sessionId)}`, {
      method: 'DELETE',
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to revoke session: ${response.status}`);
    }
  }

  async revokeAllSessions(): Promise<void> {
    const response = await fetch(`${API_BASE}/api/auth/sessions`, {
      method: 'DELETE',
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`Failed to revoke all sessions: ${response.status}`);
    }
  }

  // =============================================================================
  // Organization Member Management API
  // =============================================================================

  async getOrgMembers(orgId: string): Promise<OrgMembersResponse> {
    const response = await fetch(
      `${API_BASE}/api/organizations/${encodeURIComponent(orgId)}/members`,
      {
        headers: this.getHeaders(),
      }
    );
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(
        errorData.message || `Failed to fetch organization members: ${response.status}`
      );
    }
    return parse(OrgMembersResponseSchema, await response.json());
  }

  async addOrgMember(orgId: string, request: AddOrgMemberRequest): Promise<OrganizationMember> {
    const response = await fetch(
      `${API_BASE}/api/organizations/${encodeURIComponent(orgId)}/members`,
      {
        method: 'POST',
        headers: this.getHeaders(),
        body: JSON.stringify(request),
      }
    );
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to add organization member: ${response.status}`);
    }
    return parse(OrganizationMemberSchema, await response.json());
  }

  async updateOrgMemberRole(
    orgId: string,
    userId: string,
    request: UpdateOrgMemberRequest
  ): Promise<OrganizationMember> {
    const response = await fetch(
      `${API_BASE}/api/organizations/${encodeURIComponent(orgId)}/members/${encodeURIComponent(userId)}`,
      {
        method: 'PATCH',
        headers: this.getHeaders(),
        body: JSON.stringify(request),
      }
    );
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(
        errorData.message || `Failed to update organization member role: ${response.status}`
      );
    }
    return parse(OrganizationMemberSchema, await response.json());
  }

  async removeOrgMember(orgId: string, userId: string): Promise<void> {
    const response = await fetch(
      `${API_BASE}/api/organizations/${encodeURIComponent(orgId)}/members/${encodeURIComponent(userId)}`,
      {
        method: 'DELETE',
        headers: this.getHeaders(),
      }
    );
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(
        errorData.message || `Failed to remove organization member: ${response.status}`
      );
    }
  }

  // =============================================================================
  // Workspace Member Management API
  // =============================================================================

  async getWorkspaceMembers(workspaceId: string): Promise<WorkspaceMembersResponse> {
    const response = await fetch(
      `${API_BASE}/api/workspaces/${encodeURIComponent(workspaceId)}/members`,
      {
        headers: this.getHeaders(),
      }
    );
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to fetch workspace members: ${response.status}`);
    }
    return parse(WorkspaceMembersResponseSchema, await response.json());
  }

  async addWorkspaceMember(
    workspaceId: string,
    request: AddWorkspaceMemberRequest
  ): Promise<WorkspaceMember> {
    const response = await fetch(
      `${API_BASE}/api/workspaces/${encodeURIComponent(workspaceId)}/members`,
      {
        method: 'POST',
        headers: this.getHeaders(),
        body: JSON.stringify(request),
      }
    );
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to add workspace member: ${response.status}`);
    }
    return parse(WorkspaceMemberSchema, await response.json());
  }

  async updateWorkspaceMemberRole(
    workspaceId: string,
    userId: string,
    request: UpdateWorkspaceMemberRequest
  ): Promise<WorkspaceMember> {
    const response = await fetch(
      `${API_BASE}/api/workspaces/${encodeURIComponent(workspaceId)}/members/${encodeURIComponent(userId)}`,
      {
        method: 'PATCH',
        headers: this.getHeaders(),
        body: JSON.stringify(request),
      }
    );
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(
        errorData.message || `Failed to update workspace member role: ${response.status}`
      );
    }
    return parse(WorkspaceMemberSchema, await response.json());
  }

  async removeWorkspaceMember(workspaceId: string, userId: string): Promise<void> {
    const response = await fetch(
      `${API_BASE}/api/workspaces/${encodeURIComponent(workspaceId)}/members/${encodeURIComponent(userId)}`,
      {
        method: 'DELETE',
        headers: this.getHeaders(),
      }
    );
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to remove workspace member: ${response.status}`);
    }
  }

  // =============================================================================
  // Invitation Management API
  // =============================================================================

  async createInvitation(orgId: string, request: CreateInvitationRequest): Promise<Invitation> {
    const response = await fetch(
      `${API_BASE}/api/organizations/${encodeURIComponent(orgId)}/invitations`,
      {
        method: 'POST',
        headers: this.getHeaders(),
        body: JSON.stringify(request),
      }
    );
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to create invitation: ${response.status}`);
    }
    return parse(InvitationSchema, await response.json());
  }

  async getInvitations(orgId: string): Promise<InvitationsResponse> {
    const response = await fetch(
      `${API_BASE}/api/organizations/${encodeURIComponent(orgId)}/invitations`,
      {
        headers: this.getHeaders(),
      }
    );
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to fetch invitations: ${response.status}`);
    }
    return parse(InvitationsResponseSchema, await response.json());
  }

  async revokeInvitation(orgId: string, invitationId: string): Promise<void> {
    const response = await fetch(
      `${API_BASE}/api/organizations/${encodeURIComponent(orgId)}/invitations/${encodeURIComponent(invitationId)}`,
      {
        method: 'DELETE',
        headers: this.getHeaders(),
      }
    );
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to revoke invitation: ${response.status}`);
    }
  }

  async getInvitationByToken(token: string): Promise<InvitationDetails> {
    const response = await fetch(`${API_BASE}/api/invitations/${encodeURIComponent(token)}`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(
        errorData.message || `Failed to fetch invitation details: ${response.status}`
      );
    }
    return parse(InvitationDetailsSchema, await response.json());
  }

  async acceptInvitation(token: string): Promise<void> {
    const response = await fetch(
      `${API_BASE}/api/invitations/${encodeURIComponent(token)}/accept`,
      {
        method: 'POST',
        headers: this.getHeaders(),
      }
    );
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to accept invitation: ${response.status}`);
    }
  }

  // =============================================================================
  // Billing & Usage API
  // =============================================================================

  async getPlans(): Promise<Plan[]> {
    const response = await fetch(`${API_BASE}/api/plans`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to fetch plans: ${response.status}`);
    }
    const data = parse(PlansResponseSchema, await response.json());
    return data.plans;
  }

  async getPlan(planId: string): Promise<Plan> {
    const response = await fetch(`${API_BASE}/api/plans/${encodeURIComponent(planId)}`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to fetch plan: ${response.status}`);
    }
    const data = parse(PlanResponseSchema, await response.json());
    return data.plan;
  }

  async getSubscription(orgId: string): Promise<Subscription> {
    const response = await fetch(
      `${API_BASE}/api/organizations/${encodeURIComponent(orgId)}/subscription`,
      {
        headers: this.getHeaders(),
      }
    );
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to fetch subscription: ${response.status}`);
    }
    const data = parse(SubscriptionResponseSchema, await response.json());
    return data.subscription;
  }

  async getUsage(orgId: string): Promise<Usage> {
    const response = await fetch(
      `${API_BASE}/api/organizations/${encodeURIComponent(orgId)}/usage`,
      {
        headers: this.getHeaders(),
      }
    );
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to fetch usage: ${response.status}`);
    }
    return parse(UsageResponseSchema, await response.json());
  }

  async getLimits(orgId: string): Promise<Limits> {
    const response = await fetch(
      `${API_BASE}/api/organizations/${encodeURIComponent(orgId)}/limits`,
      {
        headers: this.getHeaders(),
      }
    );
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to fetch limits: ${response.status}`);
    }
    return parse(LimitsResponseSchema, await response.json());
  }

  // =============================================================================
  // Audit Logs API
  // =============================================================================

  async getAuditLogs(orgId: string, filters?: AuditLogFilters): Promise<AuditLogsResponse> {
    const params = new URLSearchParams();
    if (filters?.action) params.set('action', filters.action);
    if (filters?.resource_type) params.set('resource_type', filters.resource_type);
    if (filters?.resource_id) params.set('resource_id', filters.resource_id);
    if (filters?.actor_id) params.set('actor_id', filters.actor_id);
    if (filters?.start_date) params.set('start_date', filters.start_date);
    if (filters?.end_date) params.set('end_date', filters.end_date);
    if (filters?.limit !== undefined) params.set('limit', filters.limit.toString());
    if (filters?.offset !== undefined) params.set('offset', filters.offset.toString());

    const query = params.toString() ? `?${params}` : '';
    const response = await fetch(
      `${API_BASE}/api/organizations/${encodeURIComponent(orgId)}/audit-logs${query}`,
      {
        headers: this.getHeaders(),
      }
    );
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to fetch audit logs: ${response.status}`);
    }
    return parse(AuditLogsResponseSchema, await response.json());
  }

  async getAuditLog(orgId: string, logId: string): Promise<AuditLog> {
    const response = await fetch(
      `${API_BASE}/api/organizations/${encodeURIComponent(orgId)}/audit-logs/${encodeURIComponent(logId)}`,
      {
        headers: this.getHeaders(),
      }
    );
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to fetch audit log: ${response.status}`);
    }
    return parse(AuditLogSchema, await response.json());
  }

  async exportAuditLogs(orgId: string, filters?: AuditLogFilters): Promise<Blob> {
    const params = new URLSearchParams();
    if (filters?.action) params.set('action', filters.action);
    if (filters?.resource_type) params.set('resource_type', filters.resource_type);
    if (filters?.resource_id) params.set('resource_id', filters.resource_id);
    if (filters?.actor_id) params.set('actor_id', filters.actor_id);
    if (filters?.start_date) params.set('start_date', filters.start_date);
    if (filters?.end_date) params.set('end_date', filters.end_date);

    const query = params.toString() ? `?${params}` : '';
    const response = await fetch(
      `${API_BASE}/api/organizations/${encodeURIComponent(orgId)}/audit-logs/export${query}`,
      {
        headers: this.getHeaders(),
      }
    );
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to export audit logs: ${response.status}`);
    }
    return response.blob();
  }

  // =============================================================================
  // Knowledge Base API (delegates to knowledgeApi)
  // =============================================================================

  async getKnowledge(workspaceId?: string) {
    if (!workspaceId) {
      throw new Error('Workspace ID is required');
    }
    return knowledgeApi.getKnowledge(workspaceId);
  }

  async createKnowledge(request: CreateKnowledgeRequest) {
    return knowledgeApi.createKnowledge(request);
  }

  async deleteKnowledge(id: string) {
    return knowledgeApi.deleteKnowledge(id);
  }

  async refreshKnowledge(id: string) {
    return knowledgeApi.refreshKnowledge(id);
  }

  // =============================================================================
  // Context Search API (delegates to knowledgeApi)
  // =============================================================================

  async searchContext(options: SearchOptions) {
    return knowledgeApi.searchContext(options);
  }

  async gatherContext(request: GatherContextRequest) {
    return knowledgeApi.gatherContext(request);
  }

  createContextGatheringWebSocket(gatheringId: string) {
    return knowledgeApi.createContextGatheringWebSocket(gatheringId);
  }

  // =============================================================================
  // Sync Configuration API (delegates to projectsApi)
  // =============================================================================

  async getSyncConfigs(projectId: string): Promise<SyncConfig[]> {
    return projectsApi.getSyncConfigs(projectId);
  }

  async createSyncConfig(projectId: string, request: CreateSyncConfigRequest): Promise<SyncConfig> {
    return projectsApi.createSyncConfig(projectId, request);
  }

  async deleteSyncConfig(projectId: string, configId: string): Promise<void> {
    return projectsApi.deleteSyncConfig(projectId, configId);
  }
}

export const client = new Client();
