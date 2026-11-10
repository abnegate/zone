import { describe, expect, it } from 'bun:test';
import { join } from 'node:path';
import { read, rule } from '../../../test/css';

const css = read(join(import.meta.dir, 'McpServerRow.css'));

describe('MCP server row layout', () => {
  it('lines the tools toggle label up with the server name', () => {
    expect(rule(css, '.mcp-server-tools-toggle')).toContain(
      'margin-inline-start: calc(-1 * var(--ui-space-2-5))'
    );
  });

  it('drops the empty actions column so a row without controls ends at the row edge', () => {
    expect(rule(css, '.mcp-server:not(:has(> .mcp-server-actions))')).toContain(
      'grid-template-columns: minmax(0, 1fr) auto'
    );
  });
});
