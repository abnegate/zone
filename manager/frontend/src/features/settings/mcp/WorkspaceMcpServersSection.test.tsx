import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import type { WorkspaceMcpServer, WorkspaceMcpServerList } from './types';

const list = mock();
const create = mock();
const update = mock();
const remove = mock();
const reconnect = mock();
const workspace = mock();
const updateWorkspace = mock();

mock.module('../../../api/mcp', () => ({
  mcpApi: { list, create, update, remove, reconnect, workspace, updateWorkspace },
}));

let WorkspaceMcpServersSection: typeof import('./WorkspaceMcpServersSection').WorkspaceMcpServersSection;

beforeAll(async () => {
  ({ WorkspaceMcpServersSection } = await import('./WorkspaceMcpServersSection'));
});

afterAll(() => {
  mock.restore();
});

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

const docs = server();
const notes = server({
  id: 's-2',
  name: 'notes',
  enabled: false,
  status: 'disabled',
  available_tools: [],
  workspace_enabled: false,
  effective: false,
});
const github = server({
  id: 's-3',
  name: 'github',
  transport: 'stdio',
  command: 'npx',
  url: null,
  status: 'connecting',
  available_tools: [],
  workspace_enabled: true,
});
const magents = server({
  id: null,
  name: 'magents',
  source: 'instance',
  transport: 'stdio',
  command: 'magents',
  url: null,
  created_at: null,
  updated_at: null,
});

function listOf(...servers: WorkspaceMcpServer[]): WorkspaceMcpServerList {
  return { servers };
}

function renderSection(editable = true) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 }, mutations: { retry: false } },
  });
  render(
    <QueryClientProvider client={queryClient}>
      <WorkspaceMcpServersSection organizationId="org-1" workspaceId="ws-1" editable={editable} />
    </QueryClientProvider>
  );
  return queryClient;
}

async function row(name: string): Promise<HTMLElement> {
  return screen.findByRole('group', { name });
}

function group(name: string): HTMLElement {
  return screen.getByRole('radiogroup', { name: `${name} in this workspace` });
}

function radios(name: string): HTMLInputElement[] {
  return within(group(name))
    .getAllByRole('radio')
    .filter((element) => element instanceof HTMLInputElement);
}

function radio(name: string, option: string): HTMLInputElement {
  const element = within(group(name)).getByRole('radio', { name: option });
  if (!(element instanceof HTMLInputElement)) {
    throw new Error(`${name} ${option} is not a radio input`);
  }
  return element;
}

describe('WorkspaceMcpServersSection', () => {
  beforeEach(() => {
    for (const method of [list, create, update, remove, reconnect, workspace, updateWorkspace]) {
      method.mockReset();
    }
    workspace.mockResolvedValue(listOf(docs, notes, github, magents));
  });

  it('describes the section and lists each server with its status, effect and source', async () => {
    renderSection();

    expect(screen.getByRole('heading', { level: 2, name: 'MCP servers' })).toBeInTheDocument();
    expect(
      screen.getByText(
        'Each server follows the organization unless this workspace turns it on or off.'
      )
    ).toBeInTheDocument();
    expect(screen.getByRole('status')).toHaveTextContent('Loading MCP servers…');

    const texts = await Promise.all(
      ['docs', 'notes', 'github', 'magents'].map(async (name) => (await row(name)).textContent)
    );
    expect(texts).toEqual([
      'docsHTTPConnectedOnInherited from organizationInheritOnOff',
      'notesHTTPDisabledOffSet for this workspaceInheritOnOffOff for the organization',
      'githubCommandConnectingOnSet for this workspaceInheritOnOff',
      'magentsCommandConnectedOnInstanceSet by the instance',
    ]);
    expect(workspace).toHaveBeenCalledWith('org-1', 'ws-1');
    expect(screen.queryByText('Only workspace admins can change MCP servers.')).toBeNull();
  });

  it('checks the radio matching each workspace override', async () => {
    renderSection();
    await row('docs');

    const checked = (name: string) =>
      radios(name)
        .filter((input) => input.checked)
        .map((input) => input.value);
    expect(checked('docs')).toEqual(['inherit']);
    expect(checked('notes')).toEqual(['off']);
    expect(checked('github')).toEqual(['on']);
  });

  it('turns a server off for the workspace with a single sparse entry', async () => {
    updateWorkspace.mockResolvedValue(
      listOf({ ...docs, workspace_enabled: false, effective: false }, notes, github, magents)
    );
    renderSection();
    await row('docs');

    fireEvent.click(radio('docs', 'Off'));

    await waitFor(() =>
      expect(updateWorkspace).toHaveBeenCalledWith('org-1', 'ws-1', { servers: { 's-1': false } })
    );
    expect(updateWorkspace).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(radio('docs', 'Off').checked).toBe(true));
    expect(within(await row('docs')).getByText('Set for this workspace')).toBeInTheDocument();
  });

  it('turns a server on for the workspace', async () => {
    updateWorkspace.mockResolvedValue(listOf(docs, notes, github, magents));
    renderSection();
    await row('docs');

    fireEvent.click(radio('docs', 'On'));

    await waitFor(() =>
      expect(updateWorkspace).toHaveBeenCalledWith('org-1', 'ws-1', { servers: { 's-1': true } })
    );
  });

  it('clears an override with null when the workspace inherits again', async () => {
    updateWorkspace.mockResolvedValue(listOf(docs, notes, github, magents));
    renderSection();
    await row('github');

    fireEvent.click(radio('github', 'Inherit'));

    await waitFor(() =>
      expect(updateWorkspace).toHaveBeenCalledWith('org-1', 'ws-1', { servers: { 's-3': null } })
    );
  });

  it('keeps a server the organization turned off from being turned on, but lets its override clear', async () => {
    updateWorkspace.mockResolvedValue(listOf(docs, notes, github, magents));
    renderSection();
    const notesRow = await row('notes');

    expect(within(notesRow).getByText('Off for the organization')).toBeInTheDocument();
    expect(radios('notes').map((input) => [input.value, input.disabled])).toEqual([
      ['inherit', false],
      ['on', true],
      ['off', false],
    ]);

    fireEvent.click(radio('notes', 'Inherit'));

    await waitFor(() =>
      expect(updateWorkspace).toHaveBeenCalledWith('org-1', 'ws-1', { servers: { 's-2': null } })
    );
  });

  it('gives an instance server no control', async () => {
    renderSection();
    const magentsRow = await row('magents');

    expect(
      screen.queryByRole('radiogroup', { name: 'magents in this workspace' })
    ).not.toBeInTheDocument();
    expect(within(magentsRow).queryAllByRole('radio')).toEqual([]);
    expect(within(magentsRow).getByText('Set by the instance')).toBeInTheDocument();
    expect(
      screen.getAllByRole('radiogroup').map((group) => group.getAttribute('aria-label'))
    ).toEqual(['docs in this workspace', 'notes in this workspace', 'github in this workspace']);
  });

  it('disables every radio and explains why when the workspace role cannot edit', async () => {
    renderSection(false);
    await row('docs');

    const hint = screen.getByText('Only workspace admins can change MCP servers.');
    expect(hint).toHaveClass('form-hint');
    expect(hint).toHaveAttribute('role', 'status');
    const inputs = ['docs', 'notes', 'github'].flatMap(radios);
    expect(inputs).toHaveLength(9);
    expect(inputs.every((input) => input.disabled)).toBe(true);
  });

  it('locks only the row whose change is saving', async () => {
    let resolve: (value: WorkspaceMcpServerList) => void = () => {};
    updateWorkspace.mockImplementation(
      () =>
        new Promise<WorkspaceMcpServerList>((done) => {
          resolve = done;
        })
    );
    renderSection();
    await row('docs');

    fireEvent.click(radio('docs', 'Off'));

    await waitFor(() => expect(radios('docs').every((input) => input.disabled)).toBe(true));
    expect(radios('github').map((input) => input.disabled)).toEqual([false, false, false]);

    resolve(
      listOf({ ...docs, workspace_enabled: false, effective: false }, notes, github, magents)
    );

    await waitFor(() =>
      expect(radios('docs').map((input) => input.disabled)).toEqual([false, false, false])
    );
  });

  it('shows why a change failed and keeps the saved override', async () => {
    updateWorkspace.mockRejectedValue(
      new Error('Failed to update workspace MCP servers: Forbidden')
    );
    renderSection();
    await row('docs');

    fireEvent.click(radio('docs', 'Off'));

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Failed to update workspace MCP servers: Forbidden'
    );
    expect(radio('docs', 'Inherit').checked).toBe(true);
    expect(radios('docs').map((input) => input.disabled)).toEqual([false, false, false]);
  });

  it('offers a retry when the servers fail to load', async () => {
    workspace.mockRejectedValueOnce(new Error('Failed to load workspace MCP servers: Not found'));
    renderSection();

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Failed to load workspace MCP servers: Not found'
    );

    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));

    expect(await row('docs')).toBeInTheDocument();
    expect(workspace).toHaveBeenCalledTimes(2);
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('keeps the servers and their controls when a later read fails, and retries', async () => {
    const queryClient = renderSection();
    await row('docs');
    workspace.mockRejectedValueOnce(new Error('Failed to load workspace MCP servers: Bad gateway'));

    await act(async () => {
      await queryClient.refetchQueries();
    });

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Failed to load workspace MCP servers: Bad gateway'
    );
    expect(radio('docs', 'Inherit').checked).toBe(true);
    expect(screen.getAllByRole('radiogroup')).toHaveLength(3);
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    await waitFor(() => expect(screen.queryByRole('alert')).toBeNull());
    expect(workspace).toHaveBeenCalledTimes(3);
    expect(await row('docs')).toBeInTheDocument();
  });

  it('labels the section by its own heading', async () => {
    renderSection();
    await row('docs');

    const heading = screen.getByRole('heading', { level: 2, name: 'MCP servers' });
    expect(heading.id).not.toBe('workspace-mcp-title');
    expect(screen.getByRole('region', { name: 'MCP servers' })).toHaveAttribute(
      'aria-labelledby',
      heading.id
    );
  });

  it('says so when the organization has no servers', async () => {
    workspace.mockResolvedValue(listOf());
    renderSection();

    expect(await screen.findByText('No MCP servers')).toBeInTheDocument();
    expect(screen.getByText('This organization has no MCP servers yet.')).toBeInTheDocument();
    expect(screen.queryByRole('radiogroup')).toBeNull();
  });
});
