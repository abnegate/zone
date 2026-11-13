import {
  ImportedMcpServersSchema,
  McpServerListSchema,
  McpServerSchema,
  ProviderMcpServerListSchema,
  WorkspaceMcpServerListSchema,
} from '../features/settings/mcp/schemas';
import type {
  CreateMcpServerRequest,
  ImportedMcpServers,
  ImportMcpServersRequest,
  McpServer,
  McpServerList,
  ProviderMcpServerList,
  UpdateMcpServerRequest,
  UpdateWorkspaceMcpServersRequest,
  WorkspaceMcpServerList,
} from '../features/settings/mcp/types';
import { parse } from '../validation';
import { ApiError } from './ApiError';
import { API_BASE, client } from './client';

function organizationUrl(organizationId: string): string {
  return `${API_BASE}/api/organizations/${encodeURIComponent(organizationId)}`;
}

function serversUrl(organizationId: string): string {
  return `${organizationUrl(organizationId)}/mcp-servers`;
}

function serverUrl(organizationId: string, serverId: string): string {
  return `${serversUrl(organizationId)}/${encodeURIComponent(serverId)}`;
}

function workspaceUrl(organizationId: string, workspaceId: string): string {
  return `${organizationUrl(organizationId)}/workspaces/${encodeURIComponent(workspaceId)}/mcp-servers`;
}

async function send(url: string, init: RequestInit, action: string): Promise<Response> {
  const response = await fetch(url, { ...init, headers: client.getHeaders() });
  if (!response.ok) {
    throw await ApiError.from(response, action);
  }
  return response;
}

export const mcpApi = {
  async list(organizationId: string): Promise<McpServerList> {
    const response = await send(
      serversUrl(organizationId),
      { method: 'GET' },
      'Failed to load MCP servers'
    );
    return parse(McpServerListSchema, await response.json());
  },

  async create(organizationId: string, request: CreateMcpServerRequest): Promise<McpServer> {
    const response = await send(
      serversUrl(organizationId),
      { method: 'POST', body: JSON.stringify(request) },
      'Failed to add the MCP server'
    );
    return parse(McpServerSchema, await response.json());
  },

  async update(
    organizationId: string,
    serverId: string,
    request: UpdateMcpServerRequest
  ): Promise<McpServer> {
    const response = await send(
      serverUrl(organizationId, serverId),
      { method: 'PUT', body: JSON.stringify(request) },
      'Failed to save the MCP server'
    );
    return parse(McpServerSchema, await response.json());
  },

  async remove(organizationId: string, serverId: string): Promise<void> {
    await send(
      serverUrl(organizationId, serverId),
      { method: 'DELETE' },
      'Failed to remove the MCP server'
    );
  },

  async reconnect(organizationId: string, serverId: string): Promise<McpServer> {
    const response = await send(
      `${serverUrl(organizationId, serverId)}/connection`,
      { method: 'POST' },
      'Failed to reconnect the MCP server'
    );
    return parse(McpServerSchema, await response.json());
  },

  async providers(organizationId: string): Promise<ProviderMcpServerList> {
    const response = await send(
      `${serversUrl(organizationId)}/providers`,
      { method: 'GET' },
      'Failed to load the MCP servers your providers have configured'
    );
    return parse(ProviderMcpServerListSchema, await response.json());
  },

  async import(
    organizationId: string,
    request: ImportMcpServersRequest
  ): Promise<ImportedMcpServers> {
    const response = await send(
      `${serversUrl(organizationId)}/imports`,
      { method: 'POST', body: JSON.stringify(request) },
      'Failed to import the MCP servers'
    );
    return parse(ImportedMcpServersSchema, await response.json());
  },

  async workspace(organizationId: string, workspaceId: string): Promise<WorkspaceMcpServerList> {
    const response = await send(
      workspaceUrl(organizationId, workspaceId),
      { method: 'GET' },
      'Failed to load workspace MCP servers'
    );
    return parse(WorkspaceMcpServerListSchema, await response.json());
  },

  async updateWorkspace(
    organizationId: string,
    workspaceId: string,
    request: UpdateWorkspaceMcpServersRequest
  ): Promise<WorkspaceMcpServerList> {
    const response = await send(
      workspaceUrl(organizationId, workspaceId),
      { method: 'PUT', body: JSON.stringify(request) },
      'Failed to save workspace MCP servers'
    );
    return parse(WorkspaceMcpServerListSchema, await response.json());
  },
};
