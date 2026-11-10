import { describe, expect, it } from 'bun:test';
import { Transport } from './constants';
import { parseMcpDocument, UNREADABLE } from './importDocument';

const DOCUMENT = JSON.stringify({
  mcpServers: {
    github: {
      command: 'npx',
      args: ['-y', '@modelcontextprotocol/server-github'],
      env: { GITHUB_TOKEN: 'ghp_x' },
    },
    docs: {
      type: 'streamable-http',
      url: 'https://docs.example/mcp',
      headers: { Authorization: 'Bearer x' },
    },
    legacy: { type: 'sse', url: 'https://legacy.example/sse' },
  },
});

describe('parseMcpDocument', () => {
  it('reads stdio and http servers from an mcpServers document and skips sse', () => {
    expect(parseMcpDocument(DOCUMENT, true)).toEqual({
      requests: [
        {
          name: 'github',
          transport: Transport.Stdio,
          command: 'npx',
          arguments: ['-y', '@modelcontextprotocol/server-github'],
          environment: { GITHUB_TOKEN: 'ghp_x' },
        },
        {
          name: 'docs',
          transport: Transport.Http,
          url: 'https://docs.example/mcp',
          headers: { Authorization: 'Bearer x' },
        },
      ],
      errors: ['legacy: SSE servers are not supported; use streamable HTTP (http)'],
    });
  });

  it('reads a bare map and drops empty maps', () => {
    const json = JSON.stringify({
      search: { url: 'https://search.example/mcp', headers: {} },
      local: { command: 'zone-mcp', env: {} },
    });
    expect(parseMcpDocument(json, true)).toEqual({
      requests: [
        { name: 'search', transport: Transport.Http, url: 'https://search.example/mcp' },
        { name: 'local', transport: Transport.Stdio, command: 'zone-mcp', arguments: [] },
      ],
      errors: [],
    });
  });

  it('reports JSON it cannot read', () => {
    expect(parseMcpDocument('{ "mcpServers": ', true)).toEqual({
      requests: [],
      errors: [UNREADABLE],
    });
  });

  it('reports a root that is not an object', () => {
    expect(parseMcpDocument('[1, 2]', true)).toEqual({
      requests: [],
      errors: ['The JSON must be an object of servers'],
    });
  });

  it('returns nothing for blank text', () => {
    expect(parseMcpDocument('  \n', true)).toEqual({ requests: [], errors: [] });
  });

  it('skips command servers when the instance turns them off', () => {
    expect(parseMcpDocument(DOCUMENT, false)).toEqual({
      requests: [
        {
          name: 'docs',
          transport: Transport.Http,
          url: 'https://docs.example/mcp',
          headers: { Authorization: 'Bearer x' },
        },
      ],
      errors: [
        'github: command servers are turned off on this instance',
        'legacy: SSE servers are not supported; use streamable HTTP (http)',
      ],
    });
  });

  it('refuses environment references', () => {
    const json = JSON.stringify({
      mcpServers: { github: { command: 'npx', env: { GITHUB_TOKEN: `\${API_KEY}` } } },
    });
    expect(parseMcpDocument(json, true)).toEqual({
      requests: [],
      errors: [`github: replace \${…} references with the value`],
    });
  });

  it('refuses invalid names, urls, header names and non-string values', () => {
    const json = JSON.stringify({
      'a.b': { url: 'https://x.example/mcp' },
      creds: { url: 'https://u:p@x.example/mcp' },
      header: { url: 'https://x.example/mcp', headers: { 'X Bad': 'v' } },
      numbers: { command: 'npx', args: ['-y', 1] },
      environment: { command: 'npx', env: { TOKEN: 1 } },
      typed: { type: 'websocket', url: 'wss://x.example' },
      empty: {},
    });
    expect(parseMcpDocument(json, true)).toEqual({
      requests: [],
      errors: [
        'a.b: use only letters, digits, hyphens and underscores',
        'creds: move the credentials out of the URL and into a header',
        "header: use a valid header name: letters, digits and !#$%&'*+-.^_`|~ (X Bad)",
        'numbers: args must be a list of strings',
        'environment: env must map names to string values',
        'typed: unsupported type websocket',
        'empty: give a command or a url',
      ],
    });
  });
});
