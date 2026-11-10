import { Source } from './constants';
import type { McpServer } from './types';

export function keyOf(server: McpServer): string {
  return server.id ?? `${Source.Instance}:${server.name}`;
}
