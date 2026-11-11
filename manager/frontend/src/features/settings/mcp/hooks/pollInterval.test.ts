import { describe, expect, it } from 'bun:test';
import type { McpServer, McpServerStatus } from '../types';
import { pollInterval } from './pollInterval';

function server(status: McpServerStatus): McpServer {
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
    status,
    error: null,
    available_tools: [],
    created_at: null,
    updated_at: null,
  };
}

describe('pollInterval', () => {
  it('does not poll before the list has loaded', () => {
    expect(pollInterval(undefined)).toBe(false);
  });

  it('does not poll an empty list', () => {
    expect(pollInterval([])).toBe(false);
  });

  it('does not poll when no server is connecting', () => {
    expect(pollInterval([server('connected'), server('failed'), server('disabled')])).toBe(false);
  });

  it('polls every two seconds while a server is connecting', () => {
    expect(pollInterval([server('connected'), server('connecting')])).toBe(2000);
  });
});
