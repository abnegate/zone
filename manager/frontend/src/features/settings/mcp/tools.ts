import { Status } from './constants';
import type { McpServer } from './types';

function counted(total: number): string {
  return total === 1 ? '1 tool' : `${total} tools`;
}

export function toolSummary(server: McpServer): string | null {
  if (server.status !== Status.Connected) return null;
  const total = server.available_tools.length;
  if (server.tools === null) return counted(total);
  const allowed = new Set(server.tools);
  const enabled = server.available_tools.filter((tool) => allowed.has(tool)).length;
  return `${enabled} of ${counted(total)}`;
}

export function toggledTools(server: McpServer, tool: string, checked: boolean): string[] | null {
  const selected = new Set(server.tools ?? server.available_tools);
  if (checked) selected.add(tool);
  else selected.delete(tool);
  const available = new Set(server.available_tools);
  const ordered = server.available_tools.filter((name) => selected.has(name));
  if (ordered.length === available.size) return null;
  const extras = [...selected].filter((name) => !available.has(name));
  return [...ordered, ...extras];
}
