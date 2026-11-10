import { Transport } from './constants';
import type { McpTransport } from './types';

export const TRANSPORT_LABELS: Readonly<Record<McpTransport, string>> = {
  [Transport.Http]: 'HTTP',
  [Transport.Stdio]: 'Command',
  [Transport.Sse]: 'SSE',
};
