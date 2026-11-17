import { AuditScopeKind, ExportFormat } from './schemas';
import type { AuditScope, ExportFormat as ExportFormatValue, ExportQuery } from './types';

const EXTENSIONS: Readonly<Record<ExportFormatValue, string>> = {
  [ExportFormat.Ndjson]: 'ndjson',
  [ExportFormat.Csv]: 'csv',
};

const FILENAME_PREFIX = 'audit';

const ENCODED_FILENAME = /filename\*\s*=\s*([^']*)'[^']*'([^;]+)/i;

const QUOTED_FILENAME = /filename\s*=\s*"((?:[^"\\]|\\.)*)"/i;

const BARE_FILENAME = /filename\s*=\s*([^;\s]+)/i;

const QUOTED_PAIR = /\\(.)/g;

const PATH_SEPARATOR = /[/\\]/;

const CONTROL_CHARACTERS = /\p{Cc}/gu;

export function exportExtension(format: ExportFormatValue): string {
  return EXTENSIONS[format];
}

export function exportFilename(scope: AuditScope, query: ExportQuery): string {
  const owner = scope.kind === AuditScopeKind.Organization ? scope.organizationId : scope.kind;
  return `${FILENAME_PREFIX}-${owner}-${query.start_date}-${query.end_date}.${exportExtension(query.format)}`;
}

function decoded(value: string): string | null {
  try {
    return decodeURIComponent(value.trim());
  } catch {
    return null;
  }
}

function declared(disposition: string): string | null {
  const encoded = ENCODED_FILENAME.exec(disposition);
  if (encoded) {
    const value = decoded(encoded[2]);
    if (value !== null) return value;
  }
  const quoted = QUOTED_FILENAME.exec(disposition);
  if (quoted) return quoted[1].replace(QUOTED_PAIR, '$1');
  return BARE_FILENAME.exec(disposition)?.[1] ?? null;
}

function safe(name: string): string {
  const base = name.split(PATH_SEPARATOR).pop() ?? '';
  return base.replace(CONTROL_CHARACTERS, '').trim();
}

export function dispositionFilename(disposition: string | null, fallback: string): string {
  if (disposition === null) return fallback;
  const name = declared(disposition);
  if (name === null) return fallback;
  const cleaned = safe(name);
  return cleaned === '' || cleaned === '.' || cleaned === '..' ? fallback : cleaned;
}
