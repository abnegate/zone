import type { McpServerSource, McpServerStatus, McpTransport } from './types';

export const Transport = {
  Stdio: 'stdio',
  Http: 'http',
  Sse: 'sse',
} as const satisfies Record<string, McpTransport>;

export const Status = {
  Connected: 'connected',
  Failed: 'failed',
  Disabled: 'disabled',
  Connecting: 'connecting',
} as const satisfies Record<string, McpServerStatus>;

export const Source = {
  Organization: 'organization',
  Instance: 'instance',
} as const satisfies Record<string, McpServerSource>;

export const SERVER_NAME_LIMIT = 32;

export const POLL_INTERVAL = 2000;
