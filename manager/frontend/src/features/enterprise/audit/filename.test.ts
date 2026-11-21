import { describe, expect, it } from 'bun:test';
import { dispositionFilename, exportExtension, exportFilename } from './filename';
import {
  EXPORT_FILENAME,
  EXPORT_QUERY,
  ORGANIZATION_SCOPE,
  SERVER_EXPORT_FILENAME,
} from './fixtures';
import { EXPORT_FORMATS } from './schemas';
import { SERVER_SCOPE } from './scope';

const FALLBACK = 'audit.ndjson';

describe('exportFilename', () => {
  it('names the file after the organization or the server and the range', () => {
    expect(exportFilename(ORGANIZATION_SCOPE, EXPORT_QUERY)).toBe(EXPORT_FILENAME);
    expect(exportFilename(SERVER_SCOPE, EXPORT_QUERY)).toBe(SERVER_EXPORT_FILENAME);
  });

  it('uses the extension of each format', () => {
    expect(EXPORT_FORMATS.map(exportExtension)).toEqual(['ndjson', 'csv']);
  });
});

describe('dispositionFilename', () => {
  it('reads a quoted filename', () => {
    expect(dispositionFilename('attachment; filename="audit-a.csv"', FALLBACK)).toBe('audit-a.csv');
  });

  it('reads a bare filename', () => {
    expect(dispositionFilename('attachment; filename=audit-a.csv', FALLBACK)).toBe('audit-a.csv');
  });

  it('prefers the encoded filename', () => {
    expect(
      dispositionFilename(
        `attachment; filename="audit.csv"; filename*=UTF-8''audit-%C3%A9t%C3%A9.csv`,
        FALLBACK
      )
    ).toBe('audit-été.csv');
  });

  it('unescapes quoted pairs', () => {
    expect(dispositionFilename('attachment; filename="a\\"b.csv"', FALLBACK)).toBe('a"b.csv');
  });

  it('keeps only the last path segment and drops control characters', () => {
    expect(dispositionFilename('attachment; filename="../../etc/passwd"', FALLBACK)).toBe('passwd');
    expect(dispositionFilename('attachment; filename="C:\\\\temp\\\\audit.csv"', FALLBACK)).toBe(
      'audit.csv'
    );
    expect(dispositionFilename('attachment; filename="a\u0007b.csv"', FALLBACK)).toBe('ab.csv');
  });

  it('falls back when the header is missing, empty or a bare directory', () => {
    expect(dispositionFilename(null, FALLBACK)).toBe(FALLBACK);
    expect(dispositionFilename('attachment', FALLBACK)).toBe(FALLBACK);
    expect(dispositionFilename('attachment; filename=""', FALLBACK)).toBe(FALLBACK);
    expect(dispositionFilename('attachment; filename=".."', FALLBACK)).toBe(FALLBACK);
    expect(dispositionFilename("attachment; filename*=UTF-8''%E0%A4%A", FALLBACK)).toBe(FALLBACK);
  });
});
