import { Transport } from './constants';
import type {
  CreateMcpServerRequest,
  McpSecretField,
  McpServer,
  McpTransport,
  UpdateMcpServerRequest,
} from './types';
import {
  environmentNameError,
  headerNameError,
  literalError,
  nameError,
  urlError,
} from './validation';

export const COMMAND_REQUIRED = 'Enter a command';
export const DUPLICATE_NAME = 'This name is already used';
export const VALUE_REQUIRED = 'Enter a value';

export interface SecretDraft {
  name: string;
  value: string;
  saved: boolean;
}

export interface McpServerDraft {
  name: string;
  transport: McpTransport;
  command: string;
  arguments: string;
  url: string;
  environment: SecretDraft[];
  headers: SecretDraft[];
}

export const DraftField = {
  Name: 'name',
  Command: 'command',
  Arguments: 'arguments',
  Url: 'url',
  Environment: 'environment',
  Headers: 'headers',
} as const;

export type DraftField = (typeof DraftField)[keyof typeof DraftField];

export type SecretList = typeof DraftField.Environment | typeof DraftField.Headers;

export const RowPart = {
  Name: 'name',
  Value: 'value',
} as const;

export type RowPart = (typeof RowPart)[keyof typeof RowPart];

export type RowField = `${SecretList}.${number}.${RowPart}`;

export type DraftErrors = Partial<Record<DraftField | RowField, string>>;

export function rowField(list: SecretList, index: number, part: RowPart): RowField {
  return `${list}.${index}.${part}`;
}

const NAME_ERRORS: Readonly<Record<SecretList, (name: string) => string | null>> = {
  [DraftField.Environment]: environmentNameError,
  [DraftField.Headers]: headerNameError,
};

const NAME_KEYS: Readonly<Record<SecretList, (name: string) => string>> = {
  [DraftField.Environment]: (name) => name,
  [DraftField.Headers]: (name) => name.toLowerCase(),
};

export function emptyDraft(transport: McpTransport): McpServerDraft {
  return {
    name: '',
    transport,
    command: '',
    arguments: '',
    url: '',
    environment: [],
    headers: [],
  };
}

function secretDrafts(fields: readonly McpSecretField[]): SecretDraft[] {
  return fields.map((field) => ({ name: field.name, value: '', saved: field.has_value }));
}

export function draftOf(server: McpServer): McpServerDraft {
  return {
    name: server.name,
    transport: server.transport,
    command: server.command ?? '',
    arguments: server.arguments.join('\n'),
    url: server.url ?? '',
    environment: secretDrafts(server.environment),
    headers: secretDrafts(server.headers),
  };
}

export function emptyRow(): SecretDraft {
  return { name: '', value: '', saved: false };
}

export function renamedRow(row: SecretDraft, name: string): SecretDraft {
  return { name, value: row.value, saved: false };
}

export function argumentsOf(text: string): string[] {
  return text.split(/\r?\n/).filter((line) => line.trim() !== '');
}

export function usesCommand(transport: McpTransport): boolean {
  return transport === Transport.Stdio;
}

function secretListOf(transport: McpTransport): SecretList {
  return usesCommand(transport) ? DraftField.Environment : DraftField.Headers;
}

function valueError(row: SecretDraft, editing: boolean): string | null {
  if (row.value !== '') {
    return literalError(row.value);
  }
  return editing && row.saved ? null : VALUE_REQUIRED;
}

function rowErrors(
  list: SecretList,
  rows: readonly SecretDraft[],
  editing: boolean,
  errors: DraftErrors
): void {
  const seen = new Set<string>();
  rows.forEach((row, index) => {
    const key = NAME_KEYS[list](row.name);
    const name =
      NAME_ERRORS[list](row.name) ??
      literalError(row.name) ??
      (seen.has(key) ? DUPLICATE_NAME : null);
    seen.add(key);
    if (name !== null) {
      errors[rowField(list, index, RowPart.Name)] = name;
    }
    const value = valueError(row, editing);
    if (value !== null) {
      errors[rowField(list, index, RowPart.Value)] = value;
    }
  });
}

export function draftErrors(draft: McpServerDraft, editing: boolean): DraftErrors {
  const errors: DraftErrors = {};
  const name = nameError(draft.name.trim());
  if (name !== null) {
    errors[DraftField.Name] = name;
  }
  if (usesCommand(draft.transport)) {
    const command = draft.command.trim();
    const commandError = command === '' ? COMMAND_REQUIRED : literalError(command);
    if (commandError !== null) {
      errors[DraftField.Command] = commandError;
    }
    const argumentError = literalError(draft.arguments);
    if (argumentError !== null) {
      errors[DraftField.Arguments] = argumentError;
    }
  } else {
    const url = draft.url.trim();
    const error = urlError(url) ?? literalError(url);
    if (error !== null) {
      errors[DraftField.Url] = error;
    }
  }
  const list = secretListOf(draft.transport);
  rowErrors(list, draft[list], editing, errors);
  return errors;
}

function createMap(rows: readonly SecretDraft[]): Record<string, string> | undefined {
  if (rows.length === 0) {
    return undefined;
  }
  return Object.fromEntries(rows.map((row) => [row.name, row.value]));
}

export function createRequest(draft: McpServerDraft): CreateMcpServerRequest {
  const name = draft.name.trim();
  if (usesCommand(draft.transport)) {
    const environment = createMap(draft.environment);
    return {
      name,
      transport: draft.transport,
      command: draft.command.trim(),
      arguments: argumentsOf(draft.arguments),
      ...(environment && { environment }),
    };
  }
  const headers = createMap(draft.headers);
  return {
    name,
    transport: draft.transport,
    url: draft.url.trim(),
    ...(headers && { headers }),
  };
}

function sameNames(left: readonly string[], right: readonly string[]): boolean {
  if (left.length !== right.length) {
    return false;
  }
  const sorted = [...right].sort();
  return [...left].sort().every((name, index) => name === sorted[index]);
}

function sameArguments(left: readonly string[], right: readonly string[]): boolean {
  return left.length === right.length && left.every((argument, index) => argument === right[index]);
}

function updateMap(
  stored: readonly McpSecretField[],
  rows: readonly SecretDraft[]
): Record<string, string | null> | undefined {
  const renamed = !sameNames(
    stored.map((field) => field.name),
    rows.map((row) => row.name)
  );
  const typed = rows.some((row) => row.value !== '');
  if (!renamed && !typed) {
    return undefined;
  }
  return Object.fromEntries(
    rows.map((row) => [row.name, row.value === '' && row.saved ? null : row.value])
  );
}

export function updateRequest(server: McpServer, draft: McpServerDraft): UpdateMcpServerRequest {
  const request: UpdateMcpServerRequest = {};
  const name = draft.name.trim();
  if (name !== server.name) {
    request.name = name;
  }
  if (usesCommand(server.transport)) {
    const command = draft.command.trim();
    if (command !== (server.command ?? '')) {
      request.command = command;
    }
    const parsed = argumentsOf(draft.arguments);
    if (!sameArguments(parsed, server.arguments)) {
      request.arguments = parsed;
    }
  } else {
    const url = draft.url.trim();
    if (url !== (server.url ?? '')) {
      request.url = url;
    }
  }
  const environment = updateMap(server.environment, draft.environment);
  if (environment) {
    request.environment = environment;
  }
  const headers = updateMap(server.headers, draft.headers);
  if (headers) {
    request.headers = headers;
  }
  return request;
}

export function changed(request: UpdateMcpServerRequest): boolean {
  return Object.keys(request).length > 0;
}
