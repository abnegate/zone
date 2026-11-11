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
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import type { ComponentType } from 'react';
import { chooseMenuItem } from '../../../test/menu';
import { fakeTimers } from '../../../test/timers';
import { POLL_INTERVAL } from './constants';
import type { McpServer, McpServerList, McpServerStatus } from './types';

const mcpApi = {
  list: mock(),
  create: mock(),
  update: mock(),
  remove: mock(),
  reconnect: mock(),
  workspace: mock(),
  updateWorkspace: mock(),
};

mock.module('../../../api/mcp', () => ({ mcpApi }));

let McpServersSection: ComponentType<{ organizationId: string; canManage: boolean }>;

beforeAll(async () => {
  ({ McpServersSection } = await import('./index'));
});

afterAll(() => {
  mock.restore();
});

const ORGANIZATION_ID = 'org-1';
const COMMANDS_OFF = 'Command (stdio) servers are turned off on this instance.';
const READ_ONLY = 'Only organization admins can change MCP servers.';

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
    available_tools: ['search', 'fetch'],
    created_at: '2026-11-01T00:00:00Z',
    updated_at: '2026-11-01T00:00:00Z',
    ...overrides,
  };
}

const docs = server('docs', { id: 's-1' });

function listing(servers: McpServer[], commands = true): McpServerList {
  return { commands, servers };
}

function renderSection(canManage = true) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 }, mutations: { retry: false } },
  });
  render(
    <QueryClientProvider client={queryClient}>
      <McpServersSection organizationId={ORGANIZATION_ID} canManage={canManage} />
    </QueryClientProvider>
  );
  return queryClient;
}

async function settleLoading(): Promise<void> {
  for (let attempt = 0; attempt < 10 && screen.queryByText('Loading MCP servers…'); attempt++) {
    await advance(0);
  }
}

async function advance(milliseconds: number): Promise<void> {
  await act(async () => {
    vi.advanceTimersByTime(milliseconds);
  });
  await act(async () => {
    vi.advanceTimersByTime(0);
  });
}

function findRow(name: string): Promise<HTMLElement> {
  return screen.findByRole('listitem', { name });
}

function statusOf(row: HTMLElement): string | null {
  return within(row).getByRole('status').textContent;
}

function statusTexts(): (string | null)[] {
  return screen.queryAllByRole('status').map((element) => element.textContent);
}

function type(label: string, value: string) {
  fireEvent.change(screen.getByLabelText(label), { target: { value } });
}

function click(button: HTMLElement) {
  button.focus();
  fireEvent.click(button);
}

describe('McpServersSection', () => {
  beforeEach(() => {
    for (const method of Object.values(mcpApi)) {
      method.mockReset();
    }
    mcpApi.list.mockResolvedValue(listing([docs]));
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('labels the section and describes it', async () => {
    renderSection();
    const section = screen.getByRole('region', { name: 'MCP servers' });
    expect(
      within(section).getByText(
        'Servers whose tools reach every chat and task in this organization.'
      )
    ).toBeTruthy();
    await findRow('docs');
    expect(mcpApi.list).toHaveBeenCalledWith(ORGANIZATION_ID);
  });

  it('shows loading until the list arrives, without actions or the commands notice', () => {
    mcpApi.list.mockImplementation(() => new Promise(() => {}));
    renderSection();
    expect(screen.getByRole('status').textContent).toBe('Loading MCP servers…');
    expect(screen.queryByRole('button', { name: 'Add server' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Import JSON' })).toBeNull();
    expect(statusTexts()).not.toContain(COMMANDS_OFF);
  });

  it('lists each server with its status', async () => {
    const statuses: McpServerStatus[] = ['connected', 'failed', 'connecting', 'disabled'];
    mcpApi.list.mockResolvedValue(
      listing(
        statuses.map((status) =>
          server(status, {
            status,
            enabled: status !== 'disabled',
            error: status === 'failed' ? 'connection refused' : null,
          })
        )
      )
    );
    renderSection();
    const list = await screen.findByRole('list', { name: 'MCP servers' });
    expect(within(list).getAllByRole('listitem', { name: /^[a-z]+$/ })).toHaveLength(4);
    expect(statusOf(await findRow('connected'))).toBe('Connected');
    expect(statusOf(await findRow('failed'))).toBe('Failed');
    expect(statusOf(await findRow('connecting'))).toBe('Connecting');
    expect(statusOf(await findRow('disabled'))).toBe('Disabled');
    expect(screen.getByText('connection refused')).toBeTruthy();
  });

  it('shows the empty state when there are no servers', async () => {
    mcpApi.list.mockResolvedValue(listing([]));
    renderSection();
    expect(await screen.findByRole('heading', { name: 'No MCP servers' })).toBeTruthy();
    expect(screen.getByText('Add a server or import an mcpServers JSON document.')).toBeTruthy();
    expect(screen.queryByRole('list', { name: 'MCP servers' })).toBeNull();
    expect(screen.getAllByRole('button', { name: 'Add server' })).toHaveLength(1);
  });

  it('reports a failed load and retries', async () => {
    mcpApi.list
      .mockRejectedValueOnce(new Error('Failed to load MCP servers: 500'))
      .mockResolvedValue(listing([docs]));
    renderSection();
    expect((await screen.findByRole('alert')).textContent).toBe('Failed to load MCP servers: 500');
    expect(screen.queryByRole('button', { name: 'Add server' })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    await findRow('docs');
    expect(mcpApi.list).toHaveBeenCalledTimes(2);
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('keeps the list and its actions when a later read fails, and retries', async () => {
    const queryClient = renderSection();
    await findRow('docs');
    mcpApi.list.mockRejectedValueOnce(new Error('Failed to load MCP servers: 502'));

    await act(async () => {
      await queryClient.refetchQueries();
    });

    expect((await screen.findByRole('alert')).textContent).toBe('Failed to load MCP servers: 502');
    expect(screen.getByRole('listitem', { name: 'docs' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Add server' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Import JSON' })).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    await waitFor(() => expect(screen.queryByRole('alert')).toBeNull());
    expect(mcpApi.list).toHaveBeenCalledTimes(3);
    expect(screen.getByRole('listitem', { name: 'docs' })).toBeTruthy();
  });

  it('says when command servers are turned off on the instance', async () => {
    mcpApi.list.mockResolvedValue(listing([docs], false));
    renderSection();
    await findRow('docs');
    expect(statusTexts()).toContain(COMMANDS_OFF);
  });

  it('locks the switch of a command server while commands are off, and no other', async () => {
    const files = server('files', {
      transport: 'stdio',
      command: 'npx',
      url: null,
      status: 'failed',
      error: 'Command servers are turned off on this instance (ZONE_MCP_COMMANDS)',
      available_tools: [],
    });
    mcpApi.list.mockResolvedValue(listing([docs, files], false));
    renderSection();
    await findRow('files');

    const locked = screen.getByRole('switch', { name: 'Enable files' });
    expect(locked).toHaveProperty('disabled', true);
    const hint = document.getElementById(locked.getAttribute('aria-describedby') ?? '');
    expect(hint?.textContent).toBe(COMMANDS_OFF);
    expect(hint?.getAttribute('role')).toBe('status');
    const open = screen.getByRole('switch', { name: 'Enable docs' });
    expect(open).toHaveProperty('disabled', false);
    expect(open.hasAttribute('aria-describedby')).toBe(false);
    fireEvent.click(locked);
    expect(mcpApi.update).not.toHaveBeenCalled();
  });

  it('does not show the commands notice when commands are on', async () => {
    renderSection();
    await findRow('docs');
    expect(statusTexts()).not.toContain(COMMANDS_OFF);
    expect(statusTexts()).not.toContain(READ_ONLY);
  });

  it('adds an http server with the exact request and returns focus to Add server', async () => {
    const wiki = server('wiki');
    mcpApi.create.mockResolvedValue(wiki);
    renderSection();
    await findRow('docs');
    mcpApi.list.mockResolvedValue(listing([docs, wiki]));
    const trigger = screen.getByRole('button', { name: 'Add server' });
    click(trigger);
    const dialog = screen.getByRole('dialog', { name: 'Add MCP server' });
    type('Name', 'wiki');
    type('URL', 'https://wiki.example.com/mcp');
    fireEvent.click(within(dialog).getByRole('button', { name: 'Add header' }));
    type('Header name 1', 'Authorization');
    type('Value for Authorization', 'Bearer t');
    fireEvent.click(within(dialog).getByRole('button', { name: 'Add server' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(mcpApi.create).toHaveBeenCalledTimes(1);
    expect(mcpApi.create).toHaveBeenCalledWith(ORGANIZATION_ID, {
      name: 'wiki',
      transport: 'http',
      url: 'https://wiki.example.com/mcp',
      headers: { Authorization: 'Bearer t' },
    });
    await findRow('wiki');
    await waitFor(() => expect(document.activeElement).toBe(trigger));
  });

  it('keeps the form open with the reason when adding fails', async () => {
    mcpApi.create.mockRejectedValue(new Error('Failed to add the MCP server: name taken'));
    renderSection();
    await findRow('docs');
    click(screen.getByRole('button', { name: 'Add server' }));
    const dialog = screen.getByRole('dialog', { name: 'Add MCP server' });
    type('Name', 'docs');
    type('URL', 'https://docs.example.com/mcp');
    fireEvent.click(within(dialog).getByRole('button', { name: 'Add server' }));
    expect((await within(dialog).findByRole('alert')).textContent).toBe(
      'Failed to add the MCP server: name taken'
    );
  });

  it('returns focus to Add server when the form is cancelled', async () => {
    renderSection();
    await findRow('docs');
    const trigger = screen.getByRole('button', { name: 'Add server' });
    click(trigger);
    const dialog = screen.getByRole('dialog', { name: 'Add MCP server' });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(mcpApi.create).not.toHaveBeenCalled();
    await waitFor(() => expect(document.activeElement).toBe(trigger));
  });

  it('edits a server with only the changed fields', async () => {
    mcpApi.update.mockResolvedValue(
      server('docs', { id: 's-1', url: 'https://docs.example.com/v2' })
    );
    renderSection();
    await findRow('docs');
    const trigger = await chooseMenuItem('Actions for docs', 'Edit');
    const dialog = screen.getByRole('dialog', { name: 'Edit docs' });
    type('URL', 'https://docs.example.com/v2');
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(mcpApi.update).toHaveBeenCalledTimes(1);
    expect(mcpApi.update).toHaveBeenCalledWith(ORGANIZATION_ID, 's-1', {
      url: 'https://docs.example.com/v2',
    });
    await waitFor(() => expect(document.activeElement).toBe(trigger));
  });

  it('removes a server after confirmation and focuses the heading', async () => {
    mcpApi.remove.mockResolvedValue(undefined);
    renderSection();
    await findRow('docs');
    mcpApi.list.mockResolvedValue(listing([]));
    await chooseMenuItem('Actions for docs', 'Remove');
    const dialog = screen.getByRole('dialog', { name: 'Remove docs?' });
    expect(within(dialog).getByText('Chats and tasks lose its tools.')).toBeTruthy();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Remove server' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(mcpApi.remove).toHaveBeenCalledTimes(1);
    expect(mcpApi.remove).toHaveBeenCalledWith(ORGANIZATION_ID, 's-1');
    expect(await screen.findByRole('heading', { name: 'No MCP servers' })).toBeTruthy();
    await waitFor(() =>
      expect(document.activeElement).toBe(screen.getByRole('heading', { name: 'MCP servers' }))
    );
  });

  it('keeps the remove dialog open with the reason when removing fails', async () => {
    mcpApi.remove.mockRejectedValue(new Error('Failed to remove the MCP server: not found'));
    renderSection();
    await findRow('docs');
    await chooseMenuItem('Actions for docs', 'Remove');
    const dialog = screen.getByRole('dialog', { name: 'Remove docs?' });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Remove server' }));
    expect((await within(dialog).findByRole('alert')).textContent).toBe(
      'Failed to remove the MCP server: not found'
    );
    expect(within(dialog).getByRole('button', { name: 'Remove server' })).toBeTruthy();
  });

  it('returns focus to the row menu when removal is cancelled', async () => {
    renderSection();
    await findRow('docs');
    const trigger = await chooseMenuItem('Actions for docs', 'Remove');
    const dialog = screen.getByRole('dialog', { name: 'Remove docs?' });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(mcpApi.remove).not.toHaveBeenCalled();
    await waitFor(() => expect(document.activeElement).toBe(trigger));
  });

  it('turns a server off with only enabled', async () => {
    mcpApi.update.mockResolvedValue(
      server('docs', { id: 's-1', enabled: false, status: 'disabled' })
    );
    renderSection();
    const row = await findRow('docs');
    fireEvent.click(within(row).getByRole('switch', { name: 'Enable docs' }));
    await waitFor(() => expect(mcpApi.update).toHaveBeenCalledTimes(1));
    expect(mcpApi.update).toHaveBeenCalledWith(ORGANIZATION_ID, 's-1', { enabled: false });
  });

  it('keeps the switch off once the server is saved while the list is reread', async () => {
    mcpApi.update.mockResolvedValue(
      server('docs', { id: 's-1', enabled: false, status: 'disabled' })
    );
    renderSection();
    const row = await findRow('docs');
    mcpApi.list.mockImplementation(() => new Promise(() => {}));
    const toggle = within(row).getByRole('switch', { name: 'Enable docs' });
    fireEvent.click(toggle);
    await waitFor(() => expect(toggle).toHaveProperty('checked', false));
    expect(statusOf(row)).toBe('Disabled');
    expect(mcpApi.list).toHaveBeenCalledTimes(2);
  });

  it('narrows the tools with only tools', async () => {
    mcpApi.update.mockResolvedValue(server('docs', { id: 's-1', tools: ['fetch'] }));
    renderSection();
    const row = await findRow('docs');
    fireEvent.click(within(row).getByRole('button', { name: 'Tools for docs' }));
    fireEvent.click(within(row).getByRole('checkbox', { name: 'search' }));
    await waitFor(() => expect(mcpApi.update).toHaveBeenCalledTimes(1));
    expect(mcpApi.update).toHaveBeenCalledWith(ORGANIZATION_ID, 's-1', { tools: ['fetch'] });
  });

  it('reconnects a server', async () => {
    mcpApi.reconnect.mockResolvedValue(server('docs', { id: 's-1', status: 'connecting' }));
    renderSection();
    await findRow('docs');
    await chooseMenuItem('Actions for docs', 'Reconnect');
    await waitFor(() => expect(mcpApi.reconnect).toHaveBeenCalledTimes(1));
    expect(mcpApi.reconnect).toHaveBeenCalledWith(ORGANIZATION_ID, 's-1');
  });

  it('reports a failed change and locks the row while it runs', async () => {
    let reject: (reason: Error) => void = () => {};
    mcpApi.update.mockImplementation(
      () =>
        new Promise((_resolve, rejectChange) => {
          reject = rejectChange;
        })
    );
    renderSection();
    const row = await findRow('docs');
    const toggle = within(row).getByRole('switch', { name: 'Enable docs' });
    fireEvent.click(toggle);
    await waitFor(() => expect(toggle.hasAttribute('disabled')).toBe(true));
    reject(new Error('Failed to save the MCP server: forbidden'));
    expect((await screen.findByRole('alert')).textContent).toBe(
      'Failed to save the MCP server: forbidden'
    );
    await waitFor(() => expect(toggle.hasAttribute('disabled')).toBe(false));
  });

  it('imports each server in order and reports the outcome', async () => {
    const alpha = server('alpha');
    mcpApi.create
      .mockResolvedValueOnce(alpha)
      .mockRejectedValueOnce(new Error('Failed to add the MCP server: name taken'));
    renderSection();
    await findRow('docs');
    mcpApi.list.mockResolvedValue(listing([docs, alpha]));
    const trigger = screen.getByRole('button', { name: 'Import JSON' });
    click(trigger);
    const dialog = screen.getByRole('dialog', { name: 'Import MCP servers' });
    type(
      'mcpServers JSON',
      JSON.stringify({
        mcpServers: {
          alpha: { url: 'https://alpha.example.com/mcp' },
          beta: { url: 'https://beta.example.com/mcp' },
        },
      })
    );
    fireEvent.click(within(dialog).getByRole('button', { name: 'Import 2 servers' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(mcpApi.create).toHaveBeenCalledTimes(2);
    expect(mcpApi.create).toHaveBeenNthCalledWith(1, ORGANIZATION_ID, {
      name: 'alpha',
      transport: 'http',
      url: 'https://alpha.example.com/mcp',
    });
    expect(mcpApi.create).toHaveBeenNthCalledWith(2, ORGANIZATION_ID, {
      name: 'beta',
      transport: 'http',
      url: 'https://beta.example.com/mcp',
    });
    expect(statusTexts()).toContain('Imported 1 server.');
    const failures = within(screen.getByRole('alert')).getByRole('list', { name: 'Not imported' });
    expect(
      within(failures)
        .getAllByRole('listitem')
        .map((item) => item.textContent)
    ).toEqual(['beta: Failed to add the MCP server: name taken']);
    await findRow('alpha');
    await waitFor(() => expect(document.activeElement).toBe(trigger));
  });

  it('counts every imported server in the notice', async () => {
    mcpApi.create.mockResolvedValueOnce(server('alpha')).mockResolvedValueOnce(server('beta'));
    renderSection();
    await findRow('docs');
    click(screen.getByRole('button', { name: 'Import JSON' }));
    type(
      'mcpServers JSON',
      JSON.stringify({
        alpha: { url: 'https://alpha.example.com/mcp' },
        beta: { url: 'https://beta.example.com/mcp' },
      })
    );
    fireEvent.click(screen.getByRole('button', { name: 'Import 2 servers' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(statusTexts()).toContain('Imported 2 servers.');
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('shows a member the servers read-only', async () => {
    renderSection(false);
    const row = await findRow('docs');
    expect(statusTexts()).toContain(READ_ONLY);
    expect(screen.queryByRole('button', { name: 'Add server' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Import JSON' })).toBeNull();
    expect(within(row).queryByRole('switch')).toBeNull();
    expect(within(row).queryByRole('button', { name: 'Actions for docs' })).toBeNull();
    fireEvent.click(within(row).getByRole('button', { name: 'Tools for docs' }));
    expect(
      within(row)
        .getAllByRole('checkbox')
        .every((checkbox) => checkbox.hasAttribute('disabled'))
    ).toBe(true);
  });

  it('shows instance servers without controls', async () => {
    mcpApi.list.mockResolvedValue(
      listing([docs, server('local', { id: null, source: 'instance' })])
    );
    renderSection();
    const row = await findRow('local');
    expect(within(row).getByText('Instance')).toBeTruthy();
    expect(within(row).queryByRole('switch')).toBeNull();
    expect(within(row).queryByRole('button', { name: 'Actions for local' })).toBeNull();
    expect(within(await findRow('docs')).getByRole('switch', { name: 'Enable docs' })).toBeTruthy();
  });

  it('polls every interval while a server is connecting and stops once it connects', async () => {
    await fakeTimers();
    mcpApi.list
      .mockResolvedValueOnce(
        listing([server('wiki', { status: 'connecting', available_tools: [] })])
      )
      .mockResolvedValue(listing([server('wiki')]));
    renderSection();
    await settleLoading();
    expect(statusOf(screen.getByRole('listitem', { name: 'wiki' }))).toBe('Connecting');

    await advance(POLL_INTERVAL / 2);
    expect(mcpApi.list).toHaveBeenCalledTimes(1);

    await advance(POLL_INTERVAL / 2);
    expect(mcpApi.list).toHaveBeenCalledTimes(2);
    expect(statusOf(screen.getByRole('listitem', { name: 'wiki' }))).toBe('Connected');

    await advance(POLL_INTERVAL);
    expect(mcpApi.list).toHaveBeenCalledTimes(2);
  });
});
