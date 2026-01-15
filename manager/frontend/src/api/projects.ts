import { API_BASE } from './client';
import { parse } from '../validation';
import {
  ProjectResponseSchema,
  ProjectsResponseSchema,
  SyncConfigResponseSchema,
  SyncConfigsResponseSchema,
} from '../validation/schemas';
import type {
  Project,
  CreateProjectRequest,
  UpdateProjectRequest,
  SyncConfig,
  CreateSyncConfigRequest,
} from '../features/projects/types';

/**
 * Projects API client
 * Handles all project-related API calls
 */
class ProjectsApi {
  private getAccessToken: (() => string | null) | null = null;

  setGetAccessToken(fn: () => string | null) {
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

  // =============================================================================
  // Projects
  // =============================================================================

  async getProjects(workspaceId: string, status?: string): Promise<Project[]> {
    const params = new URLSearchParams({ workspace_id: workspaceId });
    if (status) {
      params.set('status', status);
    }
    const response = await fetch(`${API_BASE}/api/projects?${params}`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to fetch projects: ${response.status}`);
    }
    const data = parse(ProjectsResponseSchema, await response.json());
    return data.projects;
  }

  async getProject(id: string): Promise<Project> {
    const response = await fetch(`${API_BASE}/api/projects/${encodeURIComponent(id)}`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to fetch project: ${response.status}`);
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
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to create project: ${response.status}`);
    }
    const data = parse(ProjectResponseSchema, await response.json());
    return data.project;
  }

  async updateProject(id: string, request: UpdateProjectRequest): Promise<Project> {
    const response = await fetch(`${API_BASE}/api/projects/${encodeURIComponent(id)}`, {
      method: 'PATCH',
      headers: this.getHeaders(),
      body: JSON.stringify(request),
    });
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to update project: ${response.status}`);
    }
    const data = parse(ProjectResponseSchema, await response.json());
    return data.project;
  }

  async deleteProject(id: string): Promise<void> {
    const response = await fetch(`${API_BASE}/api/projects/${encodeURIComponent(id)}`, {
      method: 'DELETE',
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to delete project: ${response.status}`);
    }
  }

  async linkSource(projectId: string, sourceId: string): Promise<Project> {
    const response = await fetch(
      `${API_BASE}/api/projects/${encodeURIComponent(projectId)}/source`,
      {
        method: 'PUT',
        headers: this.getHeaders(),
        body: JSON.stringify({ source_id: sourceId }),
      }
    );
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to link source: ${response.status}`);
    }
    const data = parse(ProjectResponseSchema, await response.json());
    return data.project;
  }

  async unlinkSource(projectId: string): Promise<Project> {
    const response = await fetch(
      `${API_BASE}/api/projects/${encodeURIComponent(projectId)}/source`,
      {
        method: 'DELETE',
        headers: this.getHeaders(),
      }
    );
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to unlink source: ${response.status}`);
    }
    const data = parse(ProjectResponseSchema, await response.json());
    return data.project;
  }

  // =============================================================================
  // Sync Configurations
  // =============================================================================

  async getSyncConfigs(projectId: string): Promise<SyncConfig[]> {
    const response = await fetch(`${API_BASE}/api/projects/${encodeURIComponent(projectId)}/sync`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to fetch sync configs: ${response.status}`);
    }
    const data = parse(SyncConfigsResponseSchema, await response.json());
    return data.configs;
  }

  async createSyncConfig(projectId: string, request: CreateSyncConfigRequest): Promise<SyncConfig> {
    const response = await fetch(`${API_BASE}/api/projects/${encodeURIComponent(projectId)}/sync`, {
      method: 'POST',
      headers: this.getHeaders(),
      body: JSON.stringify(request),
    });
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to create sync config: ${response.status}`);
    }
    const data = parse(SyncConfigResponseSchema, await response.json());
    return data.config;
  }

  async deleteSyncConfig(projectId: string, configId: string): Promise<void> {
    const response = await fetch(
      `${API_BASE}/api/projects/${encodeURIComponent(projectId)}/sync/${encodeURIComponent(configId)}`,
      {
        method: 'DELETE',
        headers: this.getHeaders(),
      }
    );
    if (!response.ok) {
      const errorData = await this.parseErrorResponse(response);
      throw new Error(errorData.message || `Failed to delete sync config: ${response.status}`);
    }
  }

  private async parseErrorResponse(response: Response): Promise<{ message?: string }> {
    try {
      const data = await response.json();
      return data;
    } catch {
      return {};
    }
  }
}

export const projectsApi = new ProjectsApi();
