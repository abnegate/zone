import { describe, expect, it } from 'bun:test';
import { Source, Status, Transport } from './constants';
import {
  changed,
  createRequest,
  DraftField,
  DUPLICATE_NAME,
  draftErrors,
  draftOf,
  emptyDraft,
  RowPart,
  renamedRow,
  rowField,
  updateRequest,
  VALUE_REQUIRED,
} from './draft';
import type { McpServer } from './types';
import { HEADER_RESERVED, LITERAL, NAME_LENGTH } from './validation';

function server(overrides: Partial<McpServer> = {}): McpServer {
  return {
    id: 's-1',
    name: 'docs',
    source: Source.Organization,
    transport: Transport.Http,
    command: null,
    arguments: [],
    url: 'https://docs.example/mcp',
    tools: null,
    enabled: true,
    environment: [],
    headers: [
      { name: 'Authorization', has_value: true },
      { name: 'X-Team', has_value: true },
    ],
    status: Status.Connected,
    error: null,
    available_tools: [],
    created_at: '2026-11-01T00:00:00Z',
    updated_at: '2026-11-01T00:00:00Z',
    ...overrides,
  };
}

function stdioServer(): McpServer {
  return server({
    transport: Transport.Stdio,
    command: 'npx',
    arguments: ['-y', 'pkg'],
    url: null,
    headers: [],
    environment: [
      { name: 'TOKEN', has_value: true },
      { name: 'REGION', has_value: true },
    ],
  });
}

describe('draftOf', () => {
  it('marks stored values as saved and leaves every value empty', () => {
    const stored = server({ headers: [{ name: 'Authorization', has_value: false }] });
    expect(draftOf(stored)).toEqual({
      name: 'docs',
      transport: Transport.Http,
      command: '',
      arguments: '',
      url: 'https://docs.example/mcp',
      environment: [],
      headers: [{ name: 'Authorization', value: '', saved: false }],
    });
  });

  it('remembers the stored name of a saved value', () => {
    expect(draftOf(server()).headers).toEqual([
      { name: 'Authorization', value: '', saved: true, original: 'Authorization' },
      { name: 'X-Team', value: '', saved: true, original: 'X-Team' },
    ]);
  });
});

describe('renamedRow', () => {
  it('keeps the saved value again once the stored name is restored', () => {
    const [stored] = draftOf(stdioServer()).environment;
    if (stored === undefined) {
      throw new Error('missing stored row');
    }
    const renamed = renamedRow(stored, 'TOKE');
    expect(renamed).toEqual({ name: 'TOKE', value: '', saved: false, original: 'TOKEN' });
    expect(renamedRow(renamed, 'TOKEN')).toEqual(stored);
  });

  it('never marks a new row as saved', () => {
    expect(renamedRow({ name: '', value: 'v', saved: false }, '')).toEqual({
      name: '',
      value: 'v',
      saved: false,
    });
  });
});

describe('updateRequest', () => {
  it('sends only the URL when a saved header is untouched', () => {
    const stored = server();
    const draft = { ...draftOf(stored), url: 'https://docs.example/v2/mcp' };
    expect(updateRequest(stored, draft)).toEqual({ url: 'https://docs.example/v2/mcp' });
  });

  it('sends a typed header value and keeps every other saved name with null', () => {
    const stored = server();
    const draft = draftOf(stored);
    const headers = draft.headers.map((row) =>
      row.name === 'Authorization' ? { ...row, value: 'Bearer x' } : row
    );
    expect(updateRequest(stored, { ...draft, headers })).toEqual({
      headers: { Authorization: 'Bearer x', 'X-Team': null },
    });
  });

  it('omits a removed saved variable from the map', () => {
    const stored = stdioServer();
    const draft = draftOf(stored);
    const environment = draft.environment.filter((row) => row.name !== 'REGION');
    expect(updateRequest(stored, { ...draft, environment })).toEqual({
      environment: { TOKEN: null },
    });
  });

  it('sends changed arguments and command, and nothing when the draft is untouched', () => {
    const stored = stdioServer();
    const draft = draftOf(stored);
    expect(updateRequest(stored, draft)).toEqual({});
    expect(changed(updateRequest(stored, draft))).toBe(false);
    expect(
      updateRequest(stored, { ...draft, command: 'bunx', arguments: '-y\npkg\n--verbose\n' })
    ).toEqual({ command: 'bunx', arguments: ['-y', 'pkg', '--verbose'] });
  });

  it('keeps stored empty and multi-line arguments when they are untouched', () => {
    const stored = server({
      transport: Transport.Stdio,
      command: 'npx',
      arguments: ['--flag', '', 'two\nlines'],
      url: null,
      headers: [],
    });
    expect(updateRequest(stored, draftOf(stored))).toEqual({});
    expect(updateRequest(stored, { ...draftOf(stored), arguments: '--flag\n--verbose' })).toEqual({
      arguments: ['--flag', '--verbose'],
    });
  });

  it('sends nothing once a renamed saved variable gets its stored name back', () => {
    const stored = stdioServer();
    const draft = draftOf(stored);
    const environment = draft.environment.map((row) =>
      row.name === 'TOKEN' ? renamedRow(renamedRow(row, 'API_TOKEN'), 'TOKEN') : row
    );
    expect(updateRequest(stored, { ...draft, environment })).toEqual({});
    expect(draftErrors({ ...draft, environment }, true)).toEqual({});
  });

  it('never sends the secrets of the other transport', () => {
    const stray = [{ name: 'X-Stray', value: 'v', saved: false }];
    const command = stdioServer();
    expect(updateRequest(command, { ...draftOf(command), headers: stray })).toEqual({});
    const remote = server();
    expect(
      updateRequest(remote, { ...draftOf(remote), environment: [{ ...stray[0], name: 'STRAY' }] })
    ).toEqual({});
  });

  it('never names the transport, which the server refuses to change', () => {
    const stored = stdioServer();
    const request = updateRequest(stored, {
      ...draftOf(stored),
      transport: Transport.Http,
      name: 'renamed',
      command: 'bunx',
    });
    expect(request).toEqual({ name: 'renamed', command: 'bunx' });
  });

  it('sends a new name', () => {
    const stored = server();
    expect(updateRequest(stored, { ...draftOf(stored), name: 'wiki' })).toEqual({ name: 'wiki' });
  });
});

describe('draftErrors', () => {
  it('asks for a value again when a saved variable is renamed', () => {
    const stored = stdioServer();
    const draft = draftOf(stored);
    const environment = draft.environment.map((row) =>
      row.name === 'TOKEN' ? renamedRow(row, 'API_TOKEN') : row
    );
    expect(draftErrors({ ...draft, environment }, true)).toEqual({
      [rowField(DraftField.Environment, 0, RowPart.Value)]: VALUE_REQUIRED,
    });
  });

  it('asks for a value for a stored name that has none', () => {
    const stored = server({ headers: [{ name: 'Authorization', has_value: false }] });
    expect(draftErrors(draftOf(stored), true)).toEqual({
      [rowField(DraftField.Headers, 0, RowPart.Value)]: VALUE_REQUIRED,
    });
  });

  it('accepts an untouched saved draft', () => {
    expect(draftErrors(draftOf(server()), true)).toEqual({});
  });

  it('refuses a long name, a missing command, references and duplicate names', () => {
    const draft = {
      ...emptyDraft(Transport.Stdio),
      name: 'a'.repeat(33),
      arguments: `--home\n\${HOME}`,
      environment: [
        { name: 'TOKEN', value: 't', saved: false },
        { name: 'TOKEN', value: `\${TOKEN}`, saved: false },
        { name: '1BAD', value: '', saved: false },
      ],
    };
    expect(draftErrors(draft, false)).toEqual({
      [DraftField.Name]: NAME_LENGTH,
      [DraftField.Command]: 'Enter a command',
      [DraftField.Arguments]: LITERAL,
      [rowField(DraftField.Environment, 1, RowPart.Name)]: DUPLICATE_NAME,
      [rowField(DraftField.Environment, 1, RowPart.Value)]: LITERAL,
      [rowField(DraftField.Environment, 2, RowPart.Name)]:
        'Use letters, digits and underscores, starting with a letter or underscore',
      [rowField(DraftField.Environment, 2, RowPart.Value)]: VALUE_REQUIRED,
    });
  });

  it('treats header names that differ only in case as duplicates', () => {
    const draft = {
      ...emptyDraft(Transport.Http),
      name: 'docs',
      url: 'https://docs.example/mcp',
      headers: [
        { name: 'Authorization', value: 'a', saved: false },
        { name: 'authorization', value: 'b', saved: false },
      ],
    };
    expect(draftErrors(draft, false)).toEqual({
      [rowField(DraftField.Headers, 1, RowPart.Name)]: DUPLICATE_NAME,
    });
  });
});

describe('draftErrors for headers', () => {
  it('refuses a header the connection sets itself before anything is sent', () => {
    const draft = {
      ...emptyDraft(Transport.Http),
      name: 'docs',
      url: 'https://docs.example/mcp',
      headers: [
        { name: 'Content-Type', value: 'application/json', saved: false },
        { name: 'Authorization', value: 'Bearer t', saved: false },
      ],
    };
    expect(draftErrors(draft, false)).toEqual({
      [rowField(DraftField.Headers, 0, RowPart.Name)]: HEADER_RESERVED,
    });
  });
});

describe('createRequest', () => {
  it('builds a stdio request from the draft', () => {
    const draft = {
      ...emptyDraft(Transport.Stdio),
      name: 'pkg',
      command: 'npx',
      arguments: '-y\npkg',
      environment: [{ name: 'TOKEN', value: 't', saved: false }],
    };
    expect(createRequest(draft)).toEqual({
      name: 'pkg',
      transport: Transport.Stdio,
      command: 'npx',
      arguments: ['-y', 'pkg'],
      environment: { TOKEN: 't' },
    });
  });

  it('ignores headers and a URL on a stdio server', () => {
    const draft = {
      ...emptyDraft(Transport.Stdio),
      name: 'pkg',
      command: ' npx ',
      url: 'https://docs.example/mcp',
      headers: [{ name: 'Authorization', value: 'Bearer t', saved: false }],
    };
    expect(createRequest(draft)).toEqual({
      name: 'pkg',
      transport: Transport.Stdio,
      command: 'npx',
      arguments: [],
    });
  });

  it('ignores a command and arguments on an http server', () => {
    const draft = {
      ...emptyDraft(Transport.Http),
      name: 'docs',
      url: 'https://docs.example/mcp',
      command: 'npx',
      arguments: '-y\npkg',
    };
    expect(createRequest(draft)).toEqual({
      name: 'docs',
      transport: Transport.Http,
      url: 'https://docs.example/mcp',
    });
  });

  it('omits an empty header map and ignores variables on an http server', () => {
    const draft = {
      ...emptyDraft(Transport.Http),
      name: 'docs',
      url: ' https://docs.example/mcp ',
      environment: [{ name: 'TOKEN', value: 't', saved: false }],
    };
    expect(createRequest(draft)).toEqual({
      name: 'docs',
      transport: Transport.Http,
      url: 'https://docs.example/mcp',
    });
  });
});
