import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { createElement } from 'react';
import { mcpQueryKeys } from '../mcpQueryKeys';
import type { WorkspaceMcpServer } from '../types';

const list = mock();
const get = mock();
const create = mock();
const update = mock();
const remove = mock();
const reconnect = mock();
const workspace = mock();
const updateWorkspace = mock();

mock.module('../../../../api/mcp', () => ({
  mcpApi: { list, get, create, update, remove, reconnect, workspace, updateWorkspace },
}));

let useWorkspaceMcpServers: typeof import('./useWorkspaceMcpServers').useWorkspaceMcpServers;

beforeAll(async () => {
  ({ useWorkspaceMcpServers } = await import('./useWorkspaceMcpServers'));
});

afterAll(() => {
  mock.restore();
});

function createQueryClient(gcTime = 0) {
  return new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime }, mutations: { retry: false } },
  });
}

function createWrapper(queryClient: QueryClient) {
  return ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client: queryClient }, children);
}

function server(overrides: Partial<WorkspaceMcpServer> = {}): WorkspaceMcpServer {
  return {
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
    headers: [],
    status: 'connected',
    error: null,
    available_tools: ['search'],
    created_at: '2026-11-01T00:00:00Z',
    updated_at: '2026-11-01T00:00:00Z',
    workspace_enabled: null,
    effective: true,
    ...overrides,
  };
}

const inherited = server();
const off = server({ workspace_enabled: false, effective: false });

async function renderLoaded(queryClient = createQueryClient()) {
  workspace.mockResolvedValue({ servers: [inherited] });
  const rendered = renderHook(() => useWorkspaceMcpServers('org-1', 'ws-1'), {
    wrapper: createWrapper(queryClient),
  });
  await waitFor(() => {
    expect(rendered.result.current.loading).toBe(false);
  });
  return rendered;
}

describe('useWorkspaceMcpServers', () => {
  beforeEach(() => {
    for (const method of [
      list,
      get,
      create,
      update,
      remove,
      reconnect,
      workspace,
      updateWorkspace,
    ]) {
      method.mockReset();
    }
  });

  it('loads the servers with their workspace overrides', async () => {
    const { result } = await renderLoaded();

    expect(workspace).toHaveBeenCalledWith('org-1', 'ws-1');
    expect(result.current.servers).toEqual([inherited]);
    expect(result.current.fetching).toBe(false);
    expect(result.current.error).toBeNull();
  });

  it('reports why the list failed to load', async () => {
    workspace.mockRejectedValue(new Error('Failed to load workspace MCP servers: 404'));

    const { result } = renderHook(() => useWorkspaceMcpServers('org-1', 'ws-1'), {
      wrapper: createWrapper(createQueryClient()),
    });

    await waitFor(() => {
      expect(result.current.error).toBe('Failed to load workspace MCP servers: 404');
    });
    expect(result.current.servers).toEqual([]);
  });

  it('reloads the list on refresh', async () => {
    const { result } = await renderLoaded();

    await result.current.refresh();

    expect(workspace).toHaveBeenCalledTimes(2);
  });

  it('sends the overrides and shows the returned list without reading it again', async () => {
    updateWorkspace.mockResolvedValue({ servers: [off] });
    const { result } = await renderLoaded();

    await result.current.update({ 's-1': false });

    expect(updateWorkspace).toHaveBeenCalledWith('org-1', 'ws-1', { servers: { 's-1': false } });
    await waitFor(() => {
      expect(result.current.servers).toEqual([off]);
    });
    expect(workspace).toHaveBeenCalledTimes(1);
  });

  it('marks the organization list stale after saving overrides', async () => {
    const queryClient = createQueryClient(Number.POSITIVE_INFINITY);
    queryClient.setQueryData(mcpQueryKeys.servers('org-1'), { commands: false, servers: [] });
    updateWorkspace.mockResolvedValue({ servers: [off] });
    const { result } = await renderLoaded(queryClient);

    await result.current.update({ 's-1': null });

    expect(updateWorkspace).toHaveBeenCalledWith('org-1', 'ws-1', { servers: { 's-1': null } });
    expect(queryClient.getQueryState(mcpQueryKeys.servers('org-1'))?.isInvalidated).toBe(true);
  });

  it('keeps the list when saving fails', async () => {
    updateWorkspace.mockRejectedValue(new Error('Failed to save workspace MCP servers: forbidden'));
    const { result } = await renderLoaded();

    await expect(result.current.update({ 's-1': true })).rejects.toThrow(
      'Failed to save workspace MCP servers: forbidden'
    );

    expect(result.current.servers).toEqual([inherited]);
  });

  it('polls while a server is connecting', async () => {
    workspace
      .mockResolvedValueOnce({ servers: [server({ status: 'connecting', available_tools: [] })] })
      .mockResolvedValue({ servers: [inherited] });

    const { result } = renderHook(() => useWorkspaceMcpServers('org-1', 'ws-1'), {
      wrapper: createWrapper(createQueryClient()),
    });

    await waitFor(
      () => {
        expect(workspace).toHaveBeenCalledTimes(2);
      },
      { timeout: 3000 }
    );
    await waitFor(() => {
      expect(result.current.servers).toEqual([inherited]);
    });
  });
});
