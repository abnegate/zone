import { describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { Transport } from './constants';
import { McpImportDialog } from './McpImportDialog';
import type { CreateMcpServerRequest } from './types';

const DOCUMENT = JSON.stringify({
  mcpServers: {
    github: { command: 'npx', args: ['-y', 'server-github'], env: { GITHUB_TOKEN: 'ghp_x' } },
    docs: { url: 'https://docs.example/mcp', headers: { Authorization: 'Bearer x' } },
    legacy: { type: 'sse', url: 'https://legacy.example/sse' },
  },
});

const REQUESTS: CreateMcpServerRequest[] = [
  {
    name: 'github',
    transport: Transport.Stdio,
    command: 'npx',
    arguments: ['-y', 'server-github'],
    environment: { GITHUB_TOKEN: 'ghp_x' },
  },
  {
    name: 'docs',
    transport: Transport.Http,
    url: 'https://docs.example/mcp',
    headers: { Authorization: 'Bearer x' },
  },
];

function paste(json: string) {
  fireEvent.change(screen.getByLabelText('mcpServers JSON'), { target: { value: json } });
}

function items(name: string): string[] {
  return within(screen.getByRole('list', { name }))
    .queryAllByRole('listitem')
    .map((item) => item.textContent ?? '');
}

describe('McpImportDialog', () => {
  it('disables import until a server can be read', () => {
    render(<McpImportDialog commands onImport={() => Promise.resolve()} onClose={() => {}} />);
    expect(screen.getByRole('dialog', { name: 'Import MCP servers' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Import 0 servers' }).hasAttribute('disabled')).toBe(
      true
    );
    expect(screen.queryByRole('list', { name: 'Servers to add' })).toBeNull();
  });

  it('previews the servers to add and the skipped entries', () => {
    render(<McpImportDialog commands onImport={() => Promise.resolve()} onClose={() => {}} />);
    paste(DOCUMENT);
    expect(items('Servers to add')).toEqual(['github', 'docs']);
    expect(items('Skipped')).toEqual([
      'legacy: SSE servers are not supported; use streamable HTTP (http)',
    ]);
    expect(within(screen.getByRole('status')).getByRole('list', { name: 'Skipped' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Import 2 servers' })).toBeTruthy();
  });

  it('skips command servers when the instance turns them off', () => {
    render(
      <McpImportDialog commands={false} onImport={() => Promise.resolve()} onClose={() => {}} />
    );
    paste(DOCUMENT);
    expect(items('Servers to add')).toEqual(['docs']);
    expect(screen.getByRole('button', { name: 'Import 1 server' })).toBeTruthy();
  });

  it('reports unreadable JSON', () => {
    render(<McpImportDialog commands onImport={() => Promise.resolve()} onClose={() => {}} />);
    paste('{ "mcpServers": ');
    expect(items('Skipped')).toEqual(['The JSON could not be read']);
  });

  it('imports the exact requests', async () => {
    const onImport = mock((_requests: CreateMcpServerRequest[]) => Promise.resolve());
    render(<McpImportDialog commands onImport={onImport} onClose={() => {}} />);
    paste(DOCUMENT);
    fireEvent.click(screen.getByRole('button', { name: 'Import 2 servers' }));
    await waitFor(() => expect(onImport).toHaveBeenCalledTimes(1));
    expect(onImport.mock.calls[0]?.[0]).toEqual(REQUESTS);
  });

  it('shows a rejected import and stays open', async () => {
    const onImport = mock((_requests: CreateMcpServerRequest[]) =>
      Promise.reject(new Error('Failed to add the MCP server: name taken'))
    );
    render(<McpImportDialog commands onImport={onImport} onClose={() => {}} />);
    paste(DOCUMENT);
    fireEvent.click(screen.getByRole('button', { name: 'Import 2 servers' }));
    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toBe('Failed to add the MCP server: name taken');
    expect(alert.className).toBe('modal-error');
    expect(screen.getByRole('dialog', { name: 'Import MCP servers' })).toBeTruthy();
  });

  it('calls onClose from Cancel', () => {
    const onClose = mock(() => {});
    render(<McpImportDialog commands onImport={() => Promise.resolve()} onClose={onClose} />);
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
