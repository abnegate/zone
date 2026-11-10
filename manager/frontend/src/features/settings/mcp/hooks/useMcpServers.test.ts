import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
  mock,
  vi,
} from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { createElement } from 'react';
import { POLL_INTERVAL } from '../constants';
import type { McpServer } from '../types';

const list = mock();
const create = mock();
const update = mock();
const remove = mock();
const reconnect = mock();
const workspace = mock();
const updateWorkspace = mock();

mock.module('../../../../api/mcp', () => ({
  mcpApi: { list, create, update, remove, reconnect, workspace, updateWorkspace },
}));

let useMcpServers: typeof import('./useMcpServers').useMcpServers;

beforeAll(async () => {
  ({ useMcpServers } = await import('./useMcpServers'));
});

afterAll(() => {
  mock.restore();
});

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 }, mutations: { retry: false } },
  });
  return ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client: queryClient }, children);
}

function server(name: string, overrides: Partial<McpServer> = {}): McpServer {
  return {
    id: `s-${name}`,
    name,
    source: 'organization',
    transport: 'http',
    command: null,
    arguments: [],
    url: `https://${name}.example.com/mcp`,
    tools: null,
    enabled: true,
    environment: [],
    headers: [],
    status: 'connected',
    error: null,
    available_tools: ['search'],
    created_at: '2026-11-01T00:00:00Z',
    updated_at: '2026-11-01T00:00:00Z',
    ...overrides,
  };
}

const docs = server('docs');

async function advance(milliseconds: number): Promise<void> {
  await act(async () => {
    vi.advanceTimersByTime(milliseconds);
  });
  await act(async () => {
    vi.advanceTimersByTime(0);
  });
}

async function renderLoaded() {
  list.mockResolvedValue({ commands: true, servers: [docs] });
  const rendered = renderHook(() => useMcpServers('org-1'), { wrapper: createWrapper() });
  await waitFor(() => {
    expect(rendered.result.current.loading).toBe(false);
  });
  return rendered;
}

describe('useMcpServers', () => {
  beforeEach(() => {
    for (const method of [list, create, update, remove, reconnect, workspace, updateWorkspace]) {
      method.mockReset();
    }
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('starts empty with commands off while loading', () => {
    list.mockImplementation(() => new Promise(() => {}));

    const { result } = renderHook(() => useMcpServers('org-1'), { wrapper: createWrapper() });

    expect(result.current.servers).toEqual([]);
    expect(result.current.commands).toBe(false);
    expect(result.current.loading).toBe(true);
    expect(result.current.loaded).toBe(false);
    expect(result.current.error).toBeNull();
  });

  it('loads the servers and whether commands are allowed', async () => {
    const { result } = await renderLoaded();

    expect(list).toHaveBeenCalledWith('org-1');
    expect(result.current.servers).toEqual([docs]);
    expect(result.current.commands).toBe(true);
    expect(result.current.fetching).toBe(false);
    expect(result.current.error).toBeNull();
  });

  it('reports why the list failed to load', async () => {
    list.mockRejectedValue(new Error('Failed to load MCP servers: 500'));

    const { result } = renderHook(() => useMcpServers('org-1'), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.error).toBe('Failed to load MCP servers: 500');
    });
    expect(result.current.loading).toBe(false);
    expect(result.current.loaded).toBe(false);
    expect(result.current.servers).toEqual([]);
  });

  it('keeps the loaded list when a later read fails', async () => {
    const { result } = await renderLoaded();
    list.mockRejectedValue(new Error('Failed to load MCP servers: 502'));

    await result.current.refresh();

    await waitFor(() => {
      expect(result.current.error).toBe('Failed to load MCP servers: 502');
    });
    expect(result.current.loaded).toBe(true);
    expect(result.current.servers).toEqual([docs]);
    expect(result.current.commands).toBe(true);
  });

  it('reloads the list on refresh', async () => {
    const { result } = await renderLoaded();

    await result.current.refresh();

    expect(list).toHaveBeenCalledTimes(2);
  });

  it('creates a server and rereads the list', async () => {
    const request = { name: 'wiki', transport: 'http' as const, url: 'https://wiki.example.com' };
    const wiki = server('wiki');
    create.mockResolvedValue(wiki);
    const { result } = await renderLoaded();

    expect(await result.current.create(request)).toEqual(wiki);

    expect(create).toHaveBeenCalledWith('org-1', request);
    expect(list).toHaveBeenCalledTimes(2);
  });

  it('updates a server and rereads the list', async () => {
    const disabled = server('docs', { enabled: false, status: 'disabled' });
    update.mockResolvedValue(disabled);
    const { result } = await renderLoaded();

    expect(await result.current.update('s-docs', { enabled: false })).toEqual(disabled);

    expect(update).toHaveBeenCalledWith('org-1', 's-docs', { enabled: false });
    expect(list).toHaveBeenCalledTimes(2);
  });

  it('shows the saved server while the list is reread', async () => {
    const disabled = server('docs', { enabled: false, status: 'disabled' });
    update.mockResolvedValue(disabled);
    const { result } = await renderLoaded();
    list.mockImplementation(() => new Promise(() => {}));

    void result.current.update('s-docs', { enabled: false });

    await waitFor(() => {
      expect(result.current.servers).toEqual([disabled]);
    });
    expect(list).toHaveBeenCalledTimes(2);
  });

  it('shows the reconnecting server while the list is reread', async () => {
    const connecting = server('docs', { status: 'connecting', available_tools: [] });
    reconnect.mockResolvedValue(connecting);
    const { result } = await renderLoaded();
    list.mockImplementation(() => new Promise(() => {}));

    void result.current.reconnect('s-docs');

    await waitFor(() => {
      expect(result.current.servers).toEqual([connecting]);
    });
    expect(list).toHaveBeenCalledTimes(2);
  });

  it('removes a server and rereads the list', async () => {
    remove.mockResolvedValue(undefined);
    const { result } = await renderLoaded();

    expect(await result.current.remove('s-docs')).toBeUndefined();

    expect(remove).toHaveBeenCalledWith('org-1', 's-docs');
    expect(list).toHaveBeenCalledTimes(2);
  });

  it('reconnects a server and rereads the list', async () => {
    const connecting = server('docs', { status: 'connecting', available_tools: [] });
    reconnect.mockResolvedValue(connecting);
    const { result } = await renderLoaded();

    expect(await result.current.reconnect('s-docs')).toEqual(connecting);

    expect(reconnect).toHaveBeenCalledWith('org-1', 's-docs');
    expect(list).toHaveBeenCalledTimes(2);
  });

  it('does not reread the list when a mutation fails', async () => {
    remove.mockRejectedValue(new Error('Failed to remove the MCP server: not found'));
    const { result } = await renderLoaded();

    await expect(result.current.remove('s-docs')).rejects.toThrow(
      'Failed to remove the MCP server: not found'
    );

    expect(list).toHaveBeenCalledTimes(1);
  });

  it('imports every server in order, collects failures and rereads the list once', async () => {
    const a = server('a');
    create
      .mockResolvedValueOnce(a)
      .mockRejectedValueOnce(new Error('Failed to add the MCP server: name taken'));
    const { result } = await renderLoaded();
    const requests = [
      { name: 'a', transport: 'http' as const, url: 'https://a.example.com' },
      { name: 'b', transport: 'http' as const, url: 'https://b.example.com' },
    ];

    expect(await result.current.importAll(requests)).toEqual({
      created: [a],
      failures: [{ name: 'b', message: 'Failed to add the MCP server: name taken' }],
    });

    expect(create).toHaveBeenNthCalledWith(1, 'org-1', requests[0]);
    expect(create).toHaveBeenNthCalledWith(2, 'org-1', requests[1]);
    expect(list).toHaveBeenCalledTimes(2);
  });

  it('polls every interval while a server is connecting and stops once it connects', async () => {
    vi.useFakeTimers();
    list
      .mockResolvedValueOnce({
        commands: false,
        servers: [server('docs', { status: 'connecting', available_tools: [] })],
      })
      .mockResolvedValue({ commands: false, servers: [docs] });

    const { result } = renderHook(() => useMcpServers('org-1'), { wrapper: createWrapper() });
    await advance(0);
    expect(list).toHaveBeenCalledTimes(1);

    await advance(POLL_INTERVAL / 2);
    expect(list).toHaveBeenCalledTimes(1);

    await advance(POLL_INTERVAL / 2);
    expect(list).toHaveBeenCalledTimes(2);
    expect(result.current.servers).toEqual([docs]);

    await advance(POLL_INTERVAL);
    expect(list).toHaveBeenCalledTimes(2);
  });
});
