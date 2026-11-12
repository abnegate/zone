/**
 * The settings pages and the server agree on MCP servers only by convention:
 * the response structs live in `runner/zone_server/src/routes/mcp_servers.rs`
 * and the rules a request must pass in
 * `runner/zone_server/src/services/mcp_servers/`. A field, a status or a
 * reserved header changed on one side alone either fails every read of the
 * list or lets the form post a request the server refuses.
 *
 * This reads the server's source and asserts the console mirrors it.
 */

import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

import { ApiError } from '../../../api/ApiError';
import { rustVariants, structFields } from '../../../test/rust';
import { SERVER_NAME_LIMIT, Source, Status, Transport } from './constants';
import { urlRefusal } from './failure';
import { COMMANDS_OFF_NOTICE } from './McpServerForm';
import {
  McpSecretFieldSchema,
  McpServerListSchema,
  McpServerSchema,
  WorkspaceMcpServerListSchema,
  WorkspaceMcpServerSchema,
} from './schemas';
import { RESERVED_HEADER_PREFIX, RESERVED_HEADERS, URL_METADATA, urlError } from './validation';

const RUNNER = join(import.meta.dir, '../../../../../../runner/zone_server/src');
const ROUTES_RS = readFileSync(join(RUNNER, 'routes/mcp_servers.rs'), 'utf8');
const VALIDATION_RS = readFileSync(join(RUNNER, 'services/mcp_servers/validation.rs'), 'utf8');
const STATUS_RS = readFileSync(join(RUNNER, 'services/mcp_servers/status.rs'), 'utf8');
const SOURCE_RS = readFileSync(join(RUNNER, 'services/mcp_servers/source.rs'), 'utf8');
const TRANSPORT_RS = readFileSync(join(RUNNER, 'services/mcp_servers/transport.rs'), 'utf8');
const CARGO_LOCK = readFileSync(join(RUNNER, '../../Cargo.lock'), 'utf8');

const METADATA_CRATE = 'abnegate-http';
const METADATA_MIRRORED = '0.1.5';

const FLATTENED = 'server';

function names(source: string, structName: string): string[] {
  return structFields(source, structName)
    .map((field) => field.name)
    .sort();
}

function keys(schema: { shape: Record<string, unknown> }): string[] {
  return Object.keys(schema.shape).sort();
}

function constant(source: string, name: string): string {
  const value = source.match(new RegExp(`const ${name}: &str =\\s*"([^"]*)";`))?.[1];
  if (value === undefined) throw new Error(`${name} is not a string constant in the Rust source`);
  return value;
}

function list(source: string, name: string): string[] {
  const body = source.match(new RegExp(`const ${name}: \\[&str; \\d+\\] = \\[([^\\]]*)\\];`))?.[1];
  if (body === undefined) throw new Error(`${name} is not a string list in the Rust source`);
  return [...body.matchAll(/"([^"]*)"/g)].map((match) => match[1]);
}

function rustTest(source: string, name: string): string {
  const start = source.indexOf(`fn ${name}()`);
  const end = source.indexOf('\n    }\n', start);
  if (start === -1 || end === -1) throw new Error(`${name} is not a test in the Rust source`);
  return source.slice(start, end);
}

function locked(lock: string, crate: string): string {
  const version = lock.match(new RegExp(`\\nname = "${crate}"\\nversion = "([^"]+)"\\n`))?.[1];
  if (version === undefined) throw new Error(`${crate} is not in runner/Cargo.lock`);
  return version;
}

function urls(text: string): string[] {
  return [...text.matchAll(/"(https?:\/\/[^"]*)"/g)].map((match) => match[1]);
}

describe('the servers the console parses are the ones routes/mcp_servers.rs sends', () => {
  test('a server carries exactly the fields McpServerResponse declares', () => {
    expect(keys(McpServerSchema)).toEqual(names(ROUTES_RS, 'McpServerResponse'));
  });

  test('a secret entry carries exactly the fields McpSecretField declares', () => {
    expect(keys(McpSecretFieldSchema)).toEqual(names(ROUTES_RS, 'McpSecretField'));
  });

  test('the organization list carries exactly the fields McpServerListResponse declares', () => {
    expect(keys(McpServerListSchema)).toEqual(names(ROUTES_RS, 'McpServerListResponse'));
  });

  test('a workspace server is the flattened server plus its override', () => {
    expect(ROUTES_RS).toContain(`#[serde(flatten)]\n    pub ${FLATTENED}: McpServerResponse,`);
    const own = names(ROUTES_RS, 'WorkspaceMcpServerResponse').filter((name) => name !== FLATTENED);

    expect(own).toEqual(['effective', 'workspace_enabled']);
    expect(keys(WorkspaceMcpServerSchema)).toEqual(
      [...names(ROUTES_RS, 'McpServerResponse'), ...own].sort()
    );
    expect(keys(WorkspaceMcpServerListSchema)).toEqual(
      names(ROUTES_RS, 'WorkspaceMcpServerListResponse')
    );
  });

  test('every status, source and transport the server names is one the console knows', () => {
    expect(Object.values<string>(Status).sort()).toEqual(rustVariants(STATUS_RS, 'Status').sort());
    expect(Object.values<string>(Source).sort()).toEqual(rustVariants(SOURCE_RS, 'Source').sort());
    expect(Object.values<string>(Transport).sort()).toEqual(
      rustVariants(TRANSPORT_RS, 'Transport').sort()
    );
  });
});

describe('the form refuses what services/mcp_servers/validation.rs refuses', () => {
  test('RESERVED_HEADERS mirrors RESERVED in services/mcp_servers/validation.rs', () => {
    expect([...RESERVED_HEADERS]).toEqual(list(VALIDATION_RS, 'RESERVED'));
    expect(RESERVED_HEADER_PREFIX).toBe(constant(VALIDATION_RS, 'RESERVED_PREFIX'));
  });

  test('a name is as long as NAME_LIMIT allows', () => {
    const limit = VALIDATION_RS.match(/pub const NAME_LIMIT: usize = (\d+);/)?.[1];

    expect(Number(limit)).toBe(SERVER_NAME_LIMIT);
  });

  test('the notice on a locked command server is the reason the server gives', () => {
    expect(COMMANDS_OFF_NOTICE).toBe(constant(STATUS_RS, 'COMMANDS_DISABLED'));
  });
});

/**
 * The hosts and addresses themselves live in the `abnegate-http` crate, outside
 * this repository, so the form is held to the URLs the server's own tests name,
 * and to the release its lists were copied from: a new lock fails here until
 * validation.ts is checked against that release's `src/metadata.rs`.
 */
describe('the form refuses the addresses services/mcp_servers/validation.rs refuses', () => {
  const refused = [
    ...[...VALIDATION_RS.matchAll(/"([^"]+)",\s*ValidationError::Metadata \{/g)].map(
      (match) => match[1]
    ),
    ...urls(rustTest(VALIDATION_RS, 'url_is_normalised_before_the_metadata_check')),
  ];
  const accepted = urls(rustTest(VALIDATION_RS, 'url_allows_private_loopback_and_public_hosts'));

  test(`validation.ts mirrors src/metadata.rs of the ${METADATA_CRATE} release the server locks`, () => {
    expect(locked(CARGO_LOCK, METADATA_CRATE)).toBe(METADATA_MIRRORED);
  });

  test('every URL the server refuses as a metadata address is refused before it is sent', () => {
    for (const anchor of [
      'http://169.254.169.254/',
      'http://[fe80::1]/',
      'http://metadata.google.internal/',
      'http://2852039166/',
      'http://[::ffff:169.254.169.254]/',
    ]) {
      expect(refused).toContain(anchor);
    }
    expect(refused.map((url) => [url, urlError(url)])).toEqual(
      refused.map((url) => [url, URL_METADATA])
    );
  });

  test('every private, loopback and public URL the server accepts is accepted', () => {
    expect(accepted).toContain('http://127.0.0.1:8080/mcp');
    expect(accepted.map((url) => [url, urlError(url)])).toEqual(accepted.map((url) => [url, null]));
  });

  test('a refusal of the url is recognised in every sentence the server writes about a field', () => {
    const field = constant(VALIDATION_RS, 'URL');
    const sentences = [...VALIDATION_RS.matchAll(/#\[error\(\s*"(`\{field\}`[^"]*)"\s*\)\]/g)].map(
      (match) => match[1].replace('{field}', field)
    );

    expect(sentences).toContain('`url` must not point at a link-local or cloud metadata address');
    for (const sentence of sentences) {
      expect(urlRefusal(new ApiError(`Failed to add the MCP server: ${sentence}`, 400))).toBe(
        sentence
      );
    }
  });
});
