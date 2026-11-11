import { afterAll, afterEach, describe, expect, it, spyOn } from 'bun:test';
import type {
  CreateMcpServerRequest,
  McpServer,
  UpdateMcpServerRequest,
  WorkspaceMcpServer,
} from '../features/settings/mcp/types';
import { ApiError } from './ApiError';
import { mcpApi } from './mcp';
import { session } from './session';

const base = '/api/organizations/org-1/mcp-servers';
const workspaceBase = '/api/organizations/org-1/workspaces/ws-1/mcp-servers';

const server: McpServer = {
  id: 's-1',
  name: 'docs',
  source: 'organization',
  transport: 'http',
  command: null,
  arguments: [],
  url: 'https://docs.example.com/mcp',
  tools: null,
  enabled: true,
  environment: [],
  headers: [{ name: 'Authorization', has_value: true }],
  status: 'connected',
  error: null,
  available_tools: ['search_pages', 'fetch_page'],
  created_at: '2026-11-01T00:00:00Z',
  updated_at: '2026-11-01T00:00:00Z',
};

const workspaceServer: WorkspaceMcpServer = {
  ...server,
  workspace_enabled: false,
  effective: false,
};

const fetchSpy = spyOn(globalThis, 'fetch');

function respond(body: unknown, status = 200) {
  return fetchSpy.mockImplementation(async () =>
    body === null ? new Response(null, { status }) : Response.json(body, { status })
  );
}

function sent(request: ReturnType<typeof respond>) {
  const [url, init] = request.mock.calls[0];
  return { url, method: init?.method ?? 'GET', body: init?.body ?? undefined };
}

afterEach(() => {
  fetchSpy.mockReset();
  session.setAccessToken(null);
});

afterAll(() => {
  fetchSpy.mockRestore();
});

describe('mcpApi.list', () => {
  it('reads the servers of the organization and whether commands are allowed', async () => {
    const request = respond({ commands: true, servers: [server] });

    expect(await mcpApi.list('org-1')).toEqual({ commands: true, servers: [server] });
    expect(sent(request)).toEqual({ url: base, method: 'GET', body: undefined });
  });

  it('percent-encodes the organization id', async () => {
    const request = respond({ commands: false, servers: [] });

    await mcpApi.list('org 1/2');

    expect(sent(request).url).toBe('/api/organizations/org%201%2F2/mcp-servers');
  });

  it('sends the session token', async () => {
    session.setAccessToken('token-1');
    const request = respond({ commands: false, servers: [] });

    await mcpApi.list('org-1');

    const headers = new Headers(request.mock.calls[0][1]?.headers);
    expect(headers.get('Authorization')).toBe('Bearer token-1');
  });

  it('rejects a server with an unknown status', async () => {
    respond({ commands: false, servers: [{ ...server, status: 'weird' }] });

    await expect(mcpApi.list('org-1')).rejects.toThrow('Validation failed');
  });

  it('rejects a failure with the reason the server gave', async () => {
    respond({ error: 'forbidden' }, 403);

    const failure = await mcpApi.list('org-1').catch((error: unknown) => error);

    expect(failure).toBeInstanceOf(ApiError);
    expect(failure).toMatchObject({
      status: 403,
      message: 'Failed to load MCP servers: forbidden',
    });
  });
});

describe('mcpApi.create', () => {
  const request: CreateMcpServerRequest = {
    name: 'docs',
    transport: 'http',
    url: 'https://docs.example.com/mcp',
    headers: { Authorization: 'Bearer t' },
  };

  it('posts the request as JSON and resolves to the created server', async () => {
    const fetched = respond(server, 201);

    expect(await mcpApi.create('org-1', request)).toEqual(server);
    expect(sent(fetched)).toEqual({ url: base, method: 'POST', body: JSON.stringify(request) });
  });

  it('rejects a duplicate name with the reason and status', async () => {
    respond({ error: 'name taken' }, 409);

    const failure = await mcpApi.create('org-1', request).catch((error: unknown) => error);

    expect(failure).toBeInstanceOf(ApiError);
    expect(failure).toMatchObject({
      status: 409,
      message: 'Failed to add the MCP server: name taken',
    });
  });

  it('rejects a response that does not match the schema', async () => {
    respond({ ...server, status: 'weird' }, 201);

    await expect(mcpApi.create('org-1', request)).rejects.toThrow('Validation failed');
  });
});

describe('mcpApi.update', () => {
  it('puts only the changed fields', async () => {
    const changes: UpdateMcpServerRequest = { enabled: false, headers: { Authorization: null } };
    const fetched = respond({ ...server, enabled: false, status: 'disabled' });

    expect(await mcpApi.update('org-1', 's-1', changes)).toEqual({
      ...server,
      enabled: false,
      status: 'disabled',
    });
    expect(sent(fetched)).toEqual({
      url: `${base}/s-1`,
      method: 'PUT',
      body: '{"enabled":false,"headers":{"Authorization":null}}',
    });
  });

  it('percent-encodes the server id', async () => {
    const request = respond(server);

    await mcpApi.update('org-1', 's 1/2', { enabled: true });

    expect(sent(request).url).toBe(`${base}/s%201%2F2`);
  });

  it('names the action when saving fails', async () => {
    respond({ error: 'invalid url' }, 400);

    await expect(mcpApi.update('org-1', 's-1', { url: 'nope' })).rejects.toThrow(
      'Failed to save the MCP server: invalid url'
    );
  });
});

describe('mcpApi.remove', () => {
  it('resolves to undefined on a 204 without reading a body', async () => {
    const request = respond(null, 204);

    expect(await mcpApi.remove('org-1', 's-1')).toBeUndefined();
    expect(sent(request)).toEqual({ url: `${base}/s-1`, method: 'DELETE', body: undefined });
  });

  it('names the action when removing fails', async () => {
    respond({ error: 'not found' }, 404);

    await expect(mcpApi.remove('org-1', 's-1')).rejects.toThrow(
      'Failed to remove the MCP server: not found'
    );
  });
});

describe('mcpApi.reconnect', () => {
  it('posts to the connection with no body and resolves to the connecting server', async () => {
    const connecting = { ...server, status: 'connecting', available_tools: [] };
    const request = respond(connecting, 202);

    expect(await mcpApi.reconnect('org-1', 's-1')).toEqual({
      ...server,
      status: 'connecting',
      available_tools: [],
    });
    expect(sent(request)).toEqual({
      url: '/api/organizations/org-1/mcp-servers/s-1/connection',
      method: 'POST',
      body: undefined,
    });
  });

  it.each([
    [{ status: 'disabled', enabled: false, error: null }],
    [{ status: 'failed', error: 'Streamable HTTP servers are not connected by this build yet' }],
  ] satisfies [Partial<McpServer>][])(
    'resolves an accepted reconnect to the status the server reports: %o',
    async (reported) => {
      const answered = { ...server, ...reported, available_tools: [] };
      respond(answered, 202);

      expect(await mcpApi.reconnect('org-1', 's-1')).toEqual(answered);
    }
  );

  it('names the action when reconnecting fails', async () => {
    respond({ error: 'forbidden' }, 403);

    await expect(mcpApi.reconnect('org-1', 's-1')).rejects.toThrow(
      'Failed to reconnect the MCP server: forbidden'
    );
  });
});

describe('mcpApi.workspace', () => {
  it('reads the servers with their workspace overrides', async () => {
    const request = respond({ servers: [workspaceServer] });

    expect(await mcpApi.workspace('org-1', 'ws-1')).toEqual({ servers: [workspaceServer] });
    expect(sent(request)).toEqual({ url: workspaceBase, method: 'GET', body: undefined });
  });

  it('percent-encodes both ids', async () => {
    const request = respond({ servers: [] });

    await mcpApi.workspace('org/1', 'ws 1');

    expect(sent(request).url).toBe('/api/organizations/org%2F1/workspaces/ws%201/mcp-servers');
  });

  it('names the action when loading fails', async () => {
    respond({ error: 'not found' }, 404);

    await expect(mcpApi.workspace('org-1', 'ws-1')).rejects.toThrow(
      'Failed to load workspace MCP servers: not found'
    );
  });

  it('reports a caller with no role in the workspace as not found', async () => {
    respond({ error: 'Workspace not found' }, 404);

    const failure = await mcpApi.workspace('org-1', 'ws-1').catch((error: unknown) => error);

    expect(failure).toBeInstanceOf(ApiError);
    expect(failure).toMatchObject({
      status: 404,
      message: 'Failed to load workspace MCP servers: Workspace not found',
    });
  });
});

describe('mcpApi.updateWorkspace', () => {
  it('puts the overrides and resolves to the new list', async () => {
    const request = respond({ servers: [workspaceServer] });

    expect(
      await mcpApi.updateWorkspace('org-1', 'ws-1', { servers: { 's-1': false, 's-2': null } })
    ).toEqual({ servers: [workspaceServer] });
    expect(sent(request)).toEqual({
      url: workspaceBase,
      method: 'PUT',
      body: '{"servers":{"s-1":false,"s-2":null}}',
    });
  });

  it('names the action when saving fails', async () => {
    respond({ error: 'forbidden' }, 403);

    await expect(
      mcpApi.updateWorkspace('org-1', 'ws-1', { servers: { 's-1': true } })
    ).rejects.toThrow('Failed to save workspace MCP servers: forbidden');
  });
});
