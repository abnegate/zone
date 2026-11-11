import { describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { chooseMenuItem } from '../../../test/menu';
import { McpServerRow } from './McpServerRow';
import type { McpServer, McpServerStatus } from './types';

function server(overrides: Partial<McpServer> = {}): McpServer {
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
    available_tools: ['a', 'b', 'c'],
    created_at: '2026-11-01T00:00:00Z',
    updated_at: '2026-11-01T00:00:00Z',
    ...overrides,
  };
}

interface Options {
  canManage?: boolean;
  busy?: boolean;
}

function renderRow(value: McpServer, { canManage = true, busy = false }: Options = {}) {
  const handlers = {
    onToggle: mock((_enabled: boolean) => {}),
    onTools: mock((_tools: string[] | null) => {}),
    onEdit: mock(() => {}),
    onRemove: mock(() => {}),
    onReconnect: mock(() => {}),
  };
  const view = render(
    <ul>
      <McpServerRow server={value} canManage={canManage} busy={busy} {...handlers} />
    </ul>
  );
  return { ...view, ...handlers };
}

describe('McpServerRow', () => {
  it.each([
    ['connected', 'Connected'],
    ['failed', 'Failed'],
    ['connecting', 'Connecting'],
    ['disabled', 'Disabled'],
  ] satisfies [McpServerStatus, string][])('labels a %s server %s', (status, label) => {
    renderRow(server({ status }));
    expect(screen.getByRole('status').textContent).toBe(label);
  });

  it('names the row, its transport and its tool summary', () => {
    renderRow(server({ tools: ['a'] }));
    const row = screen.getByRole('listitem', { name: 'docs' });
    expect(within(row).getByText('HTTP')).toBeTruthy();
    expect(within(row).getByText('1 of 3 tools')).toBeTruthy();
  });

  it('labels a command server', () => {
    renderRow(server({ transport: 'stdio', command: 'npx', url: null }));
    expect(screen.getByText('Command')).toBeTruthy();
  });

  it('shows why a failed server failed', () => {
    renderRow(server({ status: 'failed', error: 'connection refused', available_tools: [] }));
    expect(screen.getByText('connection refused').className).toBe('mcp-server-error');
  });

  it('spins while connecting', () => {
    const { container } = renderRow(server({ status: 'connecting', available_tools: [] }));
    expect(container.querySelectorAll('[role="status"] .spinner')).toHaveLength(1);
  });

  it('does not spin once connected', () => {
    const { container } = renderRow(server());
    expect(container.querySelector('.spinner')).toBeNull();
  });

  it('turns the server off from its switch', () => {
    const { onToggle } = renderRow(server());
    const toggle = screen.getByRole('switch', { name: 'Enable docs' });
    expect(toggle).toHaveProperty('checked', true);
    fireEvent.click(toggle);
    expect(onToggle).toHaveBeenCalledTimes(1);
    expect(onToggle).toHaveBeenCalledWith(false);
  });

  it('disables the switch while busy', () => {
    renderRow(server(), { busy: true });
    expect(screen.getByRole('switch', { name: 'Enable docs' })).toHaveProperty('disabled', true);
  });

  it('marks the tools toggle so it lines up with the server name', () => {
    renderRow(server());
    const expand = screen.getByRole('button', { name: 'Tools for docs' });
    expect(expand.classList.contains('mcp-server-tools-toggle')).toBe(true);
    expect(expand.classList.contains('ui-btn-ghost')).toBe(true);
  });

  it('expands the tool list and narrows it when a tool is unchecked', () => {
    const { onTools } = renderRow(server());
    const expand = screen.getByRole('button', { name: 'Tools for docs' });
    expect(expand.getAttribute('aria-expanded')).toBe('false');
    expect(screen.queryByRole('checkbox', { name: 'b' })).toBeNull();

    fireEvent.click(expand);

    expect(expand.getAttribute('aria-expanded')).toBe('true');
    const panel = document.getElementById(expand.getAttribute('aria-controls') ?? '');
    expect(panel?.hidden).toBe(false);
    fireEvent.click(screen.getByRole('checkbox', { name: 'b' }));
    expect(onTools).toHaveBeenCalledTimes(1);
    expect(onTools).toHaveBeenCalledWith(['a', 'c']);
  });

  it('checks only the allowed tools', () => {
    renderRow(server({ tools: ['a'] }));
    fireEvent.click(screen.getByRole('button', { name: 'Tools for docs' }));
    expect(screen.getByRole('checkbox', { name: 'a' }).getAttribute('aria-checked')).toBe('true');
    expect(screen.getByRole('checkbox', { name: 'b' }).getAttribute('aria-checked')).toBe('false');
  });

  it('gives the same tool a distinct id in each row', () => {
    render(
      <ul>
        {['docs', 'wiki'].map((name) => (
          <McpServerRow
            key={name}
            server={server({ name })}
            canManage
            busy={false}
            onToggle={() => {}}
            onTools={() => {}}
            onEdit={() => {}}
            onRemove={() => {}}
            onReconnect={() => {}}
          />
        ))}
      </ul>
    );
    fireEvent.click(screen.getByRole('button', { name: 'Tools for docs' }));
    fireEvent.click(screen.getByRole('button', { name: 'Tools for wiki' }));
    const ids = screen.getAllByRole('checkbox', { name: 'a' }).map((checkbox) => checkbox.id);
    expect(ids).toHaveLength(2);
    expect(new Set(ids).size).toBe(2);
  });

  it('hides the tools control when the server advertises no tools', () => {
    renderRow(server({ status: 'disabled', enabled: false, available_tools: [] }));
    expect(screen.queryByRole('button', { name: 'Tools for docs' })).toBeNull();
  });

  it.each([
    ['Edit', 'onEdit'],
    ['Reconnect', 'onReconnect'],
    ['Remove', 'onRemove'],
  ] as const)('runs %s from the actions menu', async (item, handler) => {
    const handlers = renderRow(server());
    await chooseMenuItem('Actions for docs', item);
    expect(handlers[handler]).toHaveBeenCalledTimes(1);
  });

  it('disables Reconnect for a disabled server', async () => {
    const { onReconnect } = renderRow(
      server({ enabled: false, status: 'disabled', available_tools: [] })
    );
    await chooseMenuItem('Actions for docs', 'Reconnect');
    expect(onReconnect).not.toHaveBeenCalled();
  });

  it('marks an instance server and offers no controls', () => {
    renderRow(server({ id: null, source: 'instance', created_at: null, updated_at: null }));
    expect(screen.getByText('Instance')).toBeTruthy();
    expect(screen.queryByRole('switch')).toBeNull();
    expect(screen.queryByRole('button', { name: 'Actions for docs' })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Tools for docs' }));
    expect(screen.getByRole('checkbox', { name: 'a' })).toHaveProperty('disabled', true);
  });

  it('gives a member no switch and read-only tools', () => {
    renderRow(server(), { canManage: false });
    expect(screen.queryByRole('switch')).toBeNull();
    expect(screen.queryByRole('button', { name: 'Actions for docs' })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Tools for docs' }));
    for (const tool of ['a', 'b', 'c']) {
      expect(screen.getByRole('checkbox', { name: tool })).toHaveProperty('disabled', true);
    }
  });

  it('disables the tools while busy', () => {
    renderRow(server(), { busy: true });
    fireEvent.click(screen.getByRole('button', { name: 'Tools for docs' }));
    expect(screen.getByRole('checkbox', { name: 'a' })).toHaveProperty('disabled', true);
  });
});
