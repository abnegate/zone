import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import type { ComponentType } from 'react';
import type { McpProviderSelection } from './hooks';
import type { ProviderMcpServer, ProviderMcpServerList } from './types';

const mcpApi = {
  providers: mock(),
  import: mock(),
};

mock.module('../../../api/mcp', () => ({ mcpApi }));

let McpProviderImportDialog: ComponentType<{
  organizationId: string;
  onImport: (selections: McpProviderSelection[]) => Promise<void>;
  onClose: () => void;
}>;

beforeAll(async () => {
  ({ McpProviderImportDialog } = await import('./McpProviderImportDialog'));
});

afterAll(() => {
  mock.restore();
});

const ORGANIZATION_ID = 'org-1';

function configured(name: string, overrides: Partial<ProviderMcpServer> = {}): ProviderMcpServer {
  return {
    name,
    transport: 'http',
    command: null,
    arguments: [],
    url: `https://${name}.example.com/mcp`,
    tools: null,
    enabled: true,
    environment: [],
    headers: [],
    ignored: [],
    refusal: null,
    ...overrides,
  };
}

const notes = configured('notes', {
  transport: 'stdio',
  command: 'notes-server',
  arguments: ['mcp'],
  url: null,
  environment: [{ name: 'NOTES_TOKEN', has_value: true }],
  ignored: ['cwd'],
});

const docs = configured('docs');

function listing(
  providers: ProviderMcpServerList['providers'],
  commands = true
): ProviderMcpServerList {
  return { commands, providers };
}

function claude(servers: ProviderMcpServer[], errors: string[] = []) {
  return { provider: 'claude_code', label: 'Claude Code', servers, errors };
}

function codex(servers: ProviderMcpServer[], errors: string[] = []) {
  return { provider: 'codex', label: 'Codex', servers, errors };
}

function renderDialog(onImport = () => Promise.resolve(), onClose = () => {}) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });
  render(
    <QueryClientProvider client={queryClient}>
      <McpProviderImportDialog
        organizationId={ORGANIZATION_ID}
        onImport={onImport}
        onClose={onClose}
      />
    </QueryClientProvider>
  );
}

function row(name: string): Promise<HTMLElement> {
  return screen.findByRole('listitem', { name });
}

beforeEach(() => {
  mcpApi.providers.mockReset();
  mcpApi.import.mockReset();
  mcpApi.providers.mockResolvedValue(listing([claude([notes, docs])]));
});

describe('McpProviderImportDialog', () => {
  it('reads what the providers configured once the dialog opens', async () => {
    renderDialog();
    expect(screen.getByRole('status').textContent).toBe(
      'Reading what your providers have configured…'
    );
    await row('notes');
    expect(mcpApi.providers).toHaveBeenCalledWith(ORGANIZATION_ID);
  });

  it('lists the servers of each provider with how they are reached', async () => {
    renderDialog();
    const list = await screen.findByRole('list', { name: 'Claude Code' });
    expect(
      within(list)
        .getAllByRole('listitem')
        .map((item) => item.getAttribute('aria-label'))
    ).toEqual(['notes', 'docs']);
    expect(within(await row('notes')).getByText('notes-server mcp')).toBeTruthy();
    expect(within(await row('notes')).getByText('Command')).toBeTruthy();
    expect(within(await row('docs')).getByText('https://docs.example.com/mcp')).toBeTruthy();
    expect(within(await row('docs')).getByText('HTTP')).toBeTruthy();
  });

  it('offers every importable server, chosen to begin with', async () => {
    renderDialog();
    await row('notes');
    expect(screen.getByRole('button', { name: 'Import 2 servers' })).toBeTruthy();
    expect(
      screen
        .getAllByRole('checkbox')
        .every((checkbox) => checkbox.getAttribute('aria-checked') === 'true')
    ).toBe(true);
  });

  it('says what an import will not carry', async () => {
    renderDialog();
    expect(within(await row('notes')).getByText('Not imported: cwd')).toBeTruthy();
  });

  it('imports only what is still chosen, provider by provider', async () => {
    mcpApi.providers.mockResolvedValue(
      listing([claude([notes, docs]), codex([configured('grafana')])])
    );
    const onImport = mock((_selections: McpProviderSelection[]) => Promise.resolve());
    renderDialog(onImport);
    await row('notes');
    fireEvent.click(within(await row('docs')).getByRole('checkbox'));
    fireEvent.click(screen.getByRole('button', { name: 'Import 2 servers' }));
    await waitFor(() => expect(onImport).toHaveBeenCalledTimes(1));
    expect(onImport.mock.calls[0][0]).toEqual([
      { provider: 'claude_code', label: 'Claude Code', servers: ['notes'] },
      { provider: 'codex', label: 'Codex', servers: ['grafana'] },
    ]);
  });

  it('refuses a server the create route would refuse, and says why', async () => {
    const taken = configured('docs', {
      refusal: 'A server with this name is already configured on this instance',
    });
    mcpApi.providers.mockResolvedValue(listing([claude([taken, notes])]));
    renderDialog();
    const refused = await row('docs');
    expect(within(refused).getByRole('checkbox').hasAttribute('disabled')).toBe(true);
    expect(
      within(refused).getByText('A server with this name is already configured on this instance')
    ).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Import 1 server' })).toBeTruthy();
  });

  it('marks a server its provider has turned off', async () => {
    mcpApi.providers.mockResolvedValue(listing([claude([configured('old', { enabled: false })])]));
    renderDialog();
    expect(within(await row('old')).getByText('Disabled')).toBeTruthy();
  });

  it('shows what could not be read, without importing it', async () => {
    mcpApi.providers.mockResolvedValue(
      listing([claude([docs], ['broken: the entry could not be read'])])
    );
    renderDialog();
    await row('docs');
    const skipped = screen.getByRole('list', { name: 'Skipped' });
    expect(
      within(skipped)
        .getAllByRole('listitem')
        .map((item) => item.textContent)
    ).toEqual(['broken: the entry could not be read']);
  });

  it('says when a connected provider configures nothing', async () => {
    mcpApi.providers.mockResolvedValue(listing([codex([])]));
    renderDialog();
    expect(await screen.findByText('Codex configures no MCP servers.')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Import 0 servers' }).hasAttribute('disabled')).toBe(
      true
    );
  });

  it('says when no provider is connected', async () => {
    mcpApi.providers.mockResolvedValue(listing([]));
    renderDialog();
    expect(await screen.findByText('No connected provider')).toBeTruthy();
    expect(
      screen.getByText(
        'Zone holds no coding agent sign-in for this organization, so there is nothing to read.'
      )
    ).toBeTruthy();
  });

  it('reports a listing that failed', async () => {
    mcpApi.providers.mockRejectedValue(new Error('Failed to load the MCP servers: 500'));
    renderDialog();
    expect((await screen.findByRole('alert')).textContent).toBe(
      'Failed to load the MCP servers: 500'
    );
  });

  it('reports an import that failed and keeps the dialog open', async () => {
    renderDialog(() => Promise.reject(new Error('Failed to import the MCP servers: 503')));
    await row('notes');
    fireEvent.click(screen.getByRole('button', { name: 'Import 2 servers' }));
    expect((await screen.findByRole('alert')).textContent).toBe(
      'Failed to import the MCP servers: 503'
    );
    expect(screen.getByRole('dialog', { name: 'Import from a provider' })).toBeTruthy();
  });
});
