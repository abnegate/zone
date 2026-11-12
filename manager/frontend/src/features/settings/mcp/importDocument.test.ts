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
      ignored: [],
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
      ignored: [],
    });
  });

  it('reports JSON it cannot read', () => {
    expect(parseMcpDocument('{ "mcpServers": ', true)).toEqual({
      requests: [],
      errors: [UNREADABLE],
      ignored: [],
    });
  });

  it('reports a root that is not an object', () => {
    expect(parseMcpDocument('[1, 2]', true)).toEqual({
      requests: [],
      errors: ['The JSON must be an object of servers'],
      ignored: [],
    });
  });

  it('returns nothing for blank text', () => {
    expect(parseMcpDocument('  \n', true)).toEqual({ requests: [], errors: [], ignored: [] });
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
      ignored: [],
    });
  });

  it('refuses environment references', () => {
    const json = JSON.stringify({
      mcpServers: { github: { command: 'npx', env: { GITHUB_TOKEN: `\${API_KEY}` } } },
    });
    expect(parseMcpDocument(json, true)).toEqual({
      requests: [],
      errors: [`github: replace \${…} references with the value`],
      ignored: [],
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
      ignored: [],
    });
  });

  it('refuses arguments the form cannot edit line by line', () => {
    const json = JSON.stringify({
      blank: { command: 'npx', args: ['--flag', ''] },
      split: { command: 'npx', args: ['two\nlines'] },
      carriage: { command: 'npx', args: ['two\rlines'] },
    });
    expect(parseMcpDocument(json, true)).toEqual({
      requests: [],
      errors: [
        'blank: args must not be empty or span several lines',
        'split: args must not be empty or span several lines',
        'carriage: args must not be empty or span several lines',
      ],
      ignored: [],
    });
  });

  it('refuses empty values and header names that differ only in case', () => {
    const json = JSON.stringify({
      variable: { command: 'npx', env: { TOKEN: '' } },
      header: { url: 'https://x.example/mcp', headers: { Authorization: '' } },
      repeated: {
        url: 'https://x.example/mcp',
        headers: { Authorization: 'a', authorization: 'b' },
      },
    });
    expect(parseMcpDocument(json, true)).toEqual({
      requests: [],
      errors: [
        'variable: env values must not be empty',
        'header: header values must not be empty',
        'repeated: header names must differ by more than case (authorization)',
      ],
      ignored: [],
    });
  });

  it('refuses a link-local or cloud metadata address', () => {
    const json = JSON.stringify({
      metadata: { url: 'http://169.254.169.254/latest' },
      local: { url: 'http://[fe80::1]/mcp', disabled: true },
      named: { url: 'http://metadata.google.internal/mcp' },
    });
    expect(parseMcpDocument(json, true)).toEqual({
      requests: [],
      errors: [
        'metadata: do not use a link-local or cloud metadata address',
        'local: do not use a link-local or cloud metadata address',
        'named: do not use a link-local or cloud metadata address',
      ],
      ignored: [],
    });
  });

  it('skips a server whose header the connection sets itself', () => {
    const json = JSON.stringify({
      typed: {
        url: 'https://x.example/mcp',
        headers: { 'Content-Type': 'application/json', Authorization: 'Bearer x' },
      },
      scoped: { url: 'https://x.example/mcp', headers: { 'Mcp-Param-Region': 'eu' } },
    });
    expect(parseMcpDocument(json, true)).toEqual({
      requests: [],
      errors: [
        'typed: the connection sets this header itself (Content-Type)',
        'scoped: the connection sets this header itself (Mcp-Param-Region)',
      ],
      ignored: [],
    });
  });

  it('keeps only the fields the transport of each server uses and names the rest', () => {
    const json = JSON.stringify({
      local: {
        command: 'npx',
        url: 'https://x.example/mcp',
        headers: { Authorization: 'Bearer x' },
      },
      remote: { url: 'https://x.example/mcp', args: ['-y'], env: { TOKEN: 't' } },
    });
    expect(parseMcpDocument(json, true)).toEqual({
      requests: [
        { name: 'local', transport: Transport.Stdio, command: 'npx', arguments: [] },
        { name: 'remote', transport: Transport.Http, url: 'https://x.example/mcp' },
      ],
      errors: [],
      ignored: [
        { name: 'local', keys: ['url', 'headers'] },
        { name: 'remote', keys: ['args', 'env'] },
      ],
    });
  });

  it('adds a disabled server turned off, whatever its transport', () => {
    const json = JSON.stringify({
      local: { command: 'npx', args: ['-y'], disabled: true },
      remote: {
        type: 'http',
        url: 'https://x.example/mcp',
        headers: { Authorization: 'Bearer x' },
        disabled: true,
      },
      running: { url: 'https://y.example/mcp', disabled: false },
    });
    expect(parseMcpDocument(json, true)).toEqual({
      requests: [
        {
          name: 'local',
          transport: Transport.Stdio,
          command: 'npx',
          arguments: ['-y'],
          enabled: false,
        },
        {
          name: 'remote',
          transport: Transport.Http,
          url: 'https://x.example/mcp',
          headers: { Authorization: 'Bearer x' },
          enabled: false,
        },
        { name: 'running', transport: Transport.Http, url: 'https://y.example/mcp' },
      ],
      errors: [],
      ignored: [],
    });
  });

  it('refuses a disabled flag that is not a boolean rather than guessing', () => {
    const json = JSON.stringify({
      quoted: { url: 'https://x.example/mcp', disabled: 'true' },
      numbered: { command: 'npx', disabled: 1 },
      empty: { command: 'npx', disabled: null },
    });
    expect(parseMcpDocument(json, true)).toEqual({
      requests: [],
      errors: [
        'quoted: disabled must be true or false',
        'numbered: disabled must be true or false',
        'empty: disabled must be true or false',
      ],
      ignored: [],
    });
  });

  it('names every key of a server it does not carry, in document order', () => {
    const json = JSON.stringify({
      files: {
        command: 'npx',
        cwd: '/srv/files',
        args: ['-y'],
        timeout: 30,
        autoApprove: ['read_file'],
      },
      wiki: { url: 'https://wiki.example/mcp', alwaysAllow: [], disabled: true },
      plain: { type: 'stdio', command: 'zone-mcp', env: { TOKEN: 't' } },
      broken: { url: 'ftp://x.example', cwd: '/tmp' },
    });
    expect(parseMcpDocument(json, true)).toEqual({
      requests: [
        { name: 'files', transport: Transport.Stdio, command: 'npx', arguments: ['-y'] },
        {
          name: 'wiki',
          transport: Transport.Http,
          url: 'https://wiki.example/mcp',
          enabled: false,
        },
        {
          name: 'plain',
          transport: Transport.Stdio,
          command: 'zone-mcp',
          arguments: [],
          environment: { TOKEN: 't' },
        },
      ],
      errors: ['broken: use an http or https URL'],
      ignored: [
        { name: 'files', keys: ['cwd', 'timeout', 'autoApprove'] },
        { name: 'wiki', keys: ['alwaysAllow'] },
      ],
    });
  });

  it('trims the command', () => {
    const json = JSON.stringify({ local: { command: '  zone-mcp  ' } });
    expect(parseMcpDocument(json, true)).toEqual({
      requests: [{ name: 'local', transport: Transport.Stdio, command: 'zone-mcp', arguments: [] }],
      errors: [],
      ignored: [],
    });
  });
});
