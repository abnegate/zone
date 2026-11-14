import { describe, expect, it } from 'bun:test';
import { toggledTools, toolSummary } from './tools';
import type { McpServer } from './types';

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

describe('toolSummary', () => {
  it('returns null unless the server is connected', () => {
    expect(toolSummary(server({ status: 'failed', available_tools: [] }))).toBeNull();
    expect(toolSummary(server({ status: 'connecting' }))).toBeNull();
    expect(toolSummary(server({ status: 'disabled' }))).toBeNull();
  });

  it('counts every tool when all are allowed', () => {
    expect(toolSummary(server())).toBe('3 tools');
  });

  it('uses the singular for one tool', () => {
    expect(toolSummary(server({ available_tools: ['a'] }))).toBe('1 tool');
  });

  it('counts the allowed tools the server advertises', () => {
    expect(toolSummary(server({ tools: ['a', 'c', 'gone'] }))).toBe('2 of 3 tools');
  });
});

describe('toggledTools', () => {
  it('removes an unchecked tool from an allow-all list', () => {
    expect(toggledTools(server(), 'b', false)).toEqual(['a', 'c']);
  });

  it('returns null once every available tool is checked again', () => {
    expect(toggledTools(server({ tools: ['a', 'c'] }), 'b', true)).toBeNull();
  });

  it('returns an empty list when the last tool is unchecked', () => {
    expect(toggledTools(server({ tools: ['b'] }), 'b', false)).toEqual([]);
  });

  it('keeps the advertised order and appends tools the server no longer advertises', () => {
    expect(toggledTools(server({ tools: ['gone', 'c'] }), 'a', true)).toEqual(['a', 'c', 'gone']);
  });
});
