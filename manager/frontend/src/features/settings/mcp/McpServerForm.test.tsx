import { describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { chooseSelect, selectOptionValues } from '../../../test/select';
import { Source, Status, Transport } from './constants';
import { COMMANDS_OFF_NOTICE, McpServerForm } from './McpServerForm';
import { SAVED_PLACEHOLDER } from './McpSecretRows';
import { type McpServerSubmission, SubmissionKind } from './submission';
import type { McpServer } from './types';
import { NAME_LENGTH } from './validation';

function server(overrides: Partial<McpServer> = {}): McpServer {
  return {
    id: 's-1',
    name: 'docs',
    source: Source.Organization,
    transport: Transport.Http,
    command: null,
    arguments: [],
    url: 'https://docs.example/mcp',
    tools: null,
    enabled: true,
    environment: [],
    headers: [{ name: 'Authorization', has_value: true }],
    status: Status.Connected,
    error: null,
    available_tools: [],
    created_at: '2026-11-01T00:00:00Z',
    updated_at: '2026-11-01T00:00:00Z',
    ...overrides,
  };
}

function setup(props: { server?: McpServer; commands?: boolean } = {}) {
  const onSubmit = mock((_submission: McpServerSubmission) => Promise.resolve());
  const onClose = mock(() => {});
  render(
    <McpServerForm
      server={props.server}
      commands={props.commands ?? true}
      onSubmit={onSubmit}
      onClose={onClose}
    />
  );
  return { onSubmit, onClose };
}

function type(label: string, value: string) {
  fireEvent.change(screen.getByLabelText(label), { target: { value } });
}

describe('McpServerForm', () => {
  it('offers command servers only when the instance allows them', () => {
    setup({ commands: false });
    expect(selectOptionValues('Transport')).toEqual(['http']);
  });

  it('offers http and command servers when commands are on', () => {
    setup({ commands: true });
    expect(selectOptionValues('Transport')).toEqual(['http', 'stdio']);
  });

  it('submits an http server with a header', async () => {
    const { onSubmit } = setup();
    expect(screen.getByRole('dialog', { name: 'Add MCP server' })).toBeTruthy();
    type('Name', 'wiki');
    type('URL', 'https://wiki.example/mcp');
    fireEvent.click(screen.getByRole('button', { name: 'Add header' }));
    type('Header name 1', 'Authorization');
    type('Value for Authorization', 'Bearer t');
    fireEvent.click(screen.getByRole('button', { name: 'Add server' }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(onSubmit.mock.calls[0]?.[0]).toEqual({
      kind: SubmissionKind.Create,
      request: {
        name: 'wiki',
        transport: Transport.Http,
        url: 'https://wiki.example/mcp',
        headers: { Authorization: 'Bearer t' },
      },
    });
  });

  it('submits a command server with arguments and a variable', async () => {
    const { onSubmit } = setup();
    chooseSelect('Transport', 'Command (stdio)');
    type('Name', 'github');
    type('Command', 'npx');
    type('Arguments', '-y\n@modelcontextprotocol/server-github\n');
    fireEvent.click(screen.getByRole('button', { name: 'Add variable' }));
    type('Variable name 1', 'GITHUB_TOKEN');
    type('Value for GITHUB_TOKEN', 'ghp_x');
    fireEvent.click(screen.getByRole('button', { name: 'Add server' }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(onSubmit.mock.calls[0]?.[0]).toEqual({
      kind: SubmissionKind.Create,
      request: {
        name: 'github',
        transport: Transport.Stdio,
        command: 'npx',
        arguments: ['-y', '@modelcontextprotocol/server-github'],
        environment: { GITHUB_TOKEN: 'ghp_x' },
      },
    });
  });

  it('shows a saved header as Saved and keeps it with null', async () => {
    const { onSubmit } = setup({ server: server() });
    expect(screen.getByRole('dialog', { name: 'Edit docs' })).toBeTruthy();
    const saved = screen.getByLabelText('Value for Authorization');
    expect(saved.getAttribute('placeholder')).toBe(SAVED_PLACEHOLDER);
    expect(saved.getAttribute('value')).toBe('');
    expect(saved.getAttribute('type')).toBe('password');
    const save = screen.getByRole('button', { name: 'Save' });
    expect(save.hasAttribute('disabled')).toBe(true);
    fireEvent.click(screen.getByRole('button', { name: 'Add header' }));
    type('Header name 2', 'X-Team');
    type('Value for X-Team', 'platform');
    fireEvent.click(save);
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(onSubmit.mock.calls[0]?.[0]).toEqual({
      kind: SubmissionKind.Update,
      request: { headers: { Authorization: null, 'X-Team': 'platform' } },
    });
  });

  it('sends only the changed URL when editing', async () => {
    const { onSubmit } = setup({ server: server() });
    expect(screen.getByRole('combobox', { name: 'Transport' }).hasAttribute('disabled')).toBe(true);
    type('URL', 'https://docs.example/v2/mcp');
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(onSubmit.mock.calls[0]?.[0]).toEqual({
      kind: SubmissionKind.Update,
      request: { url: 'https://docs.example/v2/mcp' },
    });
  });

  it('asks for the value again when a saved header is renamed', () => {
    const { onSubmit } = setup({ server: server() });
    type('Header name 1', 'X-Api-Key');
    const value = screen.getByLabelText('Value for X-Api-Key');
    expect(value.getAttribute('placeholder')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(screen.getByRole('alert').textContent).toBe('Enter a value');
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it('keeps Save disabled for a command server with an empty stored argument', () => {
    setup({
      server: server({
        transport: Transport.Stdio,
        command: 'npx',
        arguments: ['--flag', ''],
        url: null,
        headers: [],
      }),
    });
    expect(screen.getByRole('button', { name: 'Save' }).hasAttribute('disabled')).toBe(true);
  });

  it('keeps a saved header when its name is changed and then restored', () => {
    setup({ server: server() });
    type('Header name 1', 'X-Api-Key');
    type('Header name 1', 'Authorization');
    const value = screen.getByLabelText('Value for Authorization');
    expect(value.getAttribute('placeholder')).toBe(SAVED_PLACEHOLDER);
    expect(screen.getByRole('button', { name: 'Save' }).hasAttribute('disabled')).toBe(true);
  });

  it('refuses a name over 32 characters without submitting', () => {
    const { onSubmit } = setup();
    type('Name', 'a'.repeat(33));
    type('URL', 'https://wiki.example/mcp');
    fireEvent.click(screen.getByRole('button', { name: 'Add server' }));
    expect(screen.getByRole('alert').textContent).toBe(NAME_LENGTH);
    expect(screen.getByLabelText('Name').getAttribute('aria-invalid')).toBe('true');
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it('shows a rejected submission and stays open', async () => {
    const onSubmit = mock((_submission: McpServerSubmission) =>
      Promise.reject(new Error('Failed to add the MCP server: name taken'))
    );
    render(<McpServerForm commands onSubmit={onSubmit} onClose={() => {}} />);
    type('Name', 'wiki');
    type('URL', 'https://wiki.example/mcp');
    fireEvent.click(screen.getByRole('button', { name: 'Add server' }));
    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toBe('Failed to add the MCP server: name taken');
    expect(alert.className).toBe('modal-error');
    expect(screen.getByRole('dialog', { name: 'Add MCP server' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Add server' }).hasAttribute('disabled')).toBe(false);
  });

  it('blocks saving a stored command server while commands are off', () => {
    const { onSubmit } = setup({
      commands: false,
      server: server({
        transport: Transport.Stdio,
        command: 'npx',
        arguments: ['-y', 'pkg'],
        url: null,
        headers: [],
      }),
    });
    expect(screen.getByRole('status').textContent).toBe(COMMANDS_OFF_NOTICE);
    expect(screen.getByRole('combobox', { name: 'Transport' }).textContent).toContain(
      'Command (stdio)'
    );
    type('Command', 'bunx');
    const save = screen.getByRole('button', { name: 'Save' });
    expect(save.hasAttribute('disabled')).toBe(true);
    fireEvent.click(save);
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it('calls onClose from Cancel', () => {
    const { onClose } = setup();
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
