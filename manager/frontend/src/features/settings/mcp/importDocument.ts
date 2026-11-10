import { Transport } from './constants';
import type { CreateMcpServerRequest } from './types';
import {
  environmentNameError,
  headerNameError,
  LITERAL,
  literalError,
  nameError,
  urlError,
} from './validation';

export const UNREADABLE = 'The JSON could not be read';
export const NOT_AN_OBJECT = 'The JSON must be an object of servers';
export const SERVERS_NOT_AN_OBJECT = 'mcpServers must be an object of servers';
export const COMMANDS_OFF = 'command servers are turned off on this instance';
export const SSE_UNSUPPORTED = 'SSE servers are not supported; use streamable HTTP (http)';
export const ENTRY_NOT_AN_OBJECT = 'each server must be an object';
export const TARGET_MISSING = 'give a command or a url';
export const COMMAND_NOT_A_STRING = 'command must be a non-empty string';
export const URL_NOT_A_STRING = 'url must be a string';
export const ARGUMENTS_NOT_STRINGS = 'args must be a list of strings';
export const ARGUMENTS_NOT_LINES = 'args must not be empty or span several lines';
export const ENVIRONMENT_NOT_STRINGS = 'env must map names to string values';
export const ENVIRONMENT_EMPTY = 'env values must not be empty';
export const HEADERS_NOT_STRINGS = 'headers must map names to string values';
export const HEADERS_EMPTY = 'header values must not be empty';
export const HEADERS_REPEATED = 'header names must differ by more than case';

const SERVERS_KEY = 'mcpServers';
const SSE_TYPE = 'sse';
const HTTP_TYPES: readonly unknown[] = [undefined, 'http', 'streamable-http', 'streamableHttp'];

export interface McpDocument {
  requests: CreateMcpServerRequest[];
  errors: string[];
}

type Entry = Record<string, unknown>;

interface MapErrors {
  type: string;
  empty: string;
}

const ENVIRONMENT_ERRORS: MapErrors = { type: ENVIRONMENT_NOT_STRINGS, empty: ENVIRONMENT_EMPTY };
const HEADER_ERRORS: MapErrors = { type: HEADERS_NOT_STRINGS, empty: HEADERS_EMPTY };
const LINE_BREAK = /[\r\n]/;

function isRecord(value: unknown): value is Entry {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function isStrings(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((item) => typeof item === 'string');
}

function isLine(value: string): boolean {
  return value !== '' && !LINE_BREAK.test(value);
}

function stringMap(value: unknown, errors: MapErrors): Record<string, string> | string {
  if (value === undefined) {
    return {};
  }
  if (!isRecord(value)) {
    return errors.type;
  }
  const map: Record<string, string> = {};
  for (const [key, item] of Object.entries(value)) {
    if (typeof item !== 'string') {
      return errors.type;
    }
    if (item === '') {
      return errors.empty;
    }
    map[key] = item;
  }
  return map;
}

function lowered(message: string): string {
  return message.charAt(0).toLowerCase() + message.slice(1);
}

function referenced(values: readonly string[]): boolean {
  return values.some((value) => literalError(value) !== null);
}

function namesError(
  map: Record<string, string>,
  check: (name: string) => string | null
): string | null {
  for (const key of Object.keys(map)) {
    const error = check(key);
    if (error !== null) {
      return `${lowered(error)} (${key})`;
    }
  }
  return null;
}

function repeatedHeader(headers: Record<string, string>): string | null {
  const seen = new Set<string>();
  for (const key of Object.keys(headers)) {
    const folded = key.toLowerCase();
    if (seen.has(folded)) {
      return `${HEADERS_REPEATED} (${key})`;
    }
    seen.add(folded);
  }
  return null;
}

function mapEntries(map: Record<string, string>): string[] {
  return Object.entries(map).flat();
}

function commandEntry(
  name: string,
  entry: Entry,
  commands: boolean
): CreateMcpServerRequest | string {
  if (!commands) {
    return COMMANDS_OFF;
  }
  const { command } = entry;
  if (typeof command !== 'string' || command.trim() === '') {
    return COMMAND_NOT_A_STRING;
  }
  const parameters = entry.args ?? [];
  if (!isStrings(parameters)) {
    return ARGUMENTS_NOT_STRINGS;
  }
  if (!parameters.every(isLine)) {
    return ARGUMENTS_NOT_LINES;
  }
  const environment = stringMap(entry.env, ENVIRONMENT_ERRORS);
  if (typeof environment === 'string') {
    return environment;
  }
  if (referenced([command, ...parameters, ...mapEntries(environment)])) {
    return lowered(LITERAL);
  }
  const invalid = namesError(environment, environmentNameError);
  if (invalid !== null) {
    return invalid;
  }
  return {
    name,
    transport: Transport.Stdio,
    command: command.trim(),
    arguments: parameters,
    ...(Object.keys(environment).length > 0 && { environment }),
  };
}

function urlEntry(name: string, entry: Entry): CreateMcpServerRequest | string {
  const { url, type } = entry;
  if (type === SSE_TYPE) {
    return SSE_UNSUPPORTED;
  }
  if (!HTTP_TYPES.includes(type)) {
    return `unsupported type ${String(type)}`;
  }
  if (typeof url !== 'string') {
    return URL_NOT_A_STRING;
  }
  const headers = stringMap(entry.headers, HEADER_ERRORS);
  if (typeof headers === 'string') {
    return headers;
  }
  if (referenced([url, ...mapEntries(headers)])) {
    return lowered(LITERAL);
  }
  const invalid =
    urlError(url.trim()) ?? namesError(headers, headerNameError) ?? repeatedHeader(headers);
  if (invalid !== null) {
    return lowered(invalid);
  }
  return {
    name,
    transport: Transport.Http,
    url: url.trim(),
    ...(Object.keys(headers).length > 0 && { headers }),
  };
}

function entryRequest(
  name: string,
  entry: unknown,
  commands: boolean
): CreateMcpServerRequest | string {
  if (!isRecord(entry)) {
    return ENTRY_NOT_AN_OBJECT;
  }
  const invalid = nameError(name);
  if (invalid !== null) {
    return lowered(invalid);
  }
  if ('command' in entry) {
    return commandEntry(name, entry, commands);
  }
  if ('url' in entry) {
    return urlEntry(name, entry);
  }
  return entry.type === SSE_TYPE ? SSE_UNSUPPORTED : TARGET_MISSING;
}

function serversOf(root: unknown): Entry | string {
  if (!isRecord(root)) {
    return NOT_AN_OBJECT;
  }
  if (!(SERVERS_KEY in root)) {
    return root;
  }
  const servers = root[SERVERS_KEY];
  return isRecord(servers) ? servers : SERVERS_NOT_AN_OBJECT;
}

export function parseMcpDocument(json: string, commands: boolean): McpDocument {
  if (json.trim() === '') {
    return { requests: [], errors: [] };
  }
  let root: unknown;
  try {
    root = JSON.parse(json);
  } catch {
    return { requests: [], errors: [UNREADABLE] };
  }
  const servers = serversOf(root);
  if (typeof servers === 'string') {
    return { requests: [], errors: [servers] };
  }
  const document: McpDocument = { requests: [], errors: [] };
  for (const [name, entry] of Object.entries(servers)) {
    const result = entryRequest(name, entry, commands);
    if (typeof result === 'string') {
      document.errors.push(`${name}: ${result}`);
    } else {
      document.requests.push(result);
    }
  }
  return document;
}
