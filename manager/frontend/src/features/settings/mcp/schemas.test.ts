import { describe, expect, it } from 'bun:test';
import { parse } from '../../../validation';
import {
  McpSecretFieldSchema,
  McpServerListSchema,
  McpServerSchema,
  WorkspaceMcpServerSchema,
} from './schemas';
import type { McpServer } from './types';

const instance: McpServer = {
  id: null,
  name: 'magents',
  source: 'instance',
  transport: 'stdio',
  command: 'magents',
  arguments: [],
  url: null,
  tools: null,
  enabled: true,
  environment: [{ name: 'MAGENTS_HOME', has_value: true }],
  headers: [],
  status: 'connected',
  error: null,
  available_tools: ['spawn_session'],
  created_at: null,
  updated_at: null,
};

describe('McpServerSchema', () => {
  it('parses an instance server with no id and no dates', () => {
    expect(parse(McpServerSchema, instance)).toEqual(instance);
  });

  it.each([['https://mcp.internal:8443'], [null]])(
    'parses a redacted remote instance server whose url is %p',
    (url) => {
      const remote = { ...instance, transport: 'http', command: null, url, environment: [] };

      expect(parse(McpServerSchema, remote)).toEqual(remote);
    }
  );

  it('refuses an unknown status', () => {
    expect(() => parse(McpServerSchema, { ...instance, status: 'weird' })).toThrow(
      'Validation failed: status:'
    );
  });

  it('refuses an unknown transport', () => {
    expect(() => parse(McpServerSchema, { ...instance, transport: 'websocket' })).toThrow(
      'Validation failed: transport:'
    );
  });

  it('strips a secret value that strays into an environment entry', () => {
    const parsed = parse(McpServerSchema, {
      ...instance,
      environment: [{ name: 'MAGENTS_HOME', has_value: true, value: 'secret' }],
    });

    expect(parsed.environment[0]).toEqual({ name: 'MAGENTS_HOME', has_value: true });
  });
});

describe('McpSecretFieldSchema', () => {
  it('strips unknown keys from a header entry', () => {
    expect(
      parse(McpSecretFieldSchema, { name: 'Authorization', has_value: false, value: 'Bearer t' })
    ).toEqual({ name: 'Authorization', has_value: false });
  });
});

describe('McpServerListSchema', () => {
  it('parses the list with the commands flag', () => {
    expect(parse(McpServerListSchema, { commands: false, servers: [instance] })).toEqual({
      commands: false,
      servers: [instance],
    });
  });
});

describe('WorkspaceMcpServerSchema', () => {
  it('parses a server that inherits the organization setting', () => {
    const server = { ...instance, workspace_enabled: null, effective: true };

    expect(parse(WorkspaceMcpServerSchema, server)).toEqual(server);
  });

  it('refuses a server without its effective state', () => {
    expect(() => parse(WorkspaceMcpServerSchema, { ...instance, workspace_enabled: true })).toThrow(
      'Validation failed: effective:'
    );
  });
});
