/**
 * The console's enums are a copy of the server's. Nothing in the type system
 * links them, so widening one without the other is silent until a user opens a
 * chat containing the new value.
 *
 * This reads the Rust definitions and asserts the sets match. It is the only
 * thing standing between the next added variant and a chat that cannot be
 * opened, because the value is persisted in messages.metadata and does not
 * clear on reload.
 */

import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

import { CitationSchema, MessageMetadataSchema } from './schemas';

const CITATIONS_RS = join(
  import.meta.dir,
  '../../../../../runner/zone_server/src/agent/citations.rs'
);

const PROVENANCE_RS = join(
  import.meta.dir,
  '../../../../../runner/zone_server/src/agent/verification/provenance.rs'
);

function rustVariants(source: string, enumName: string): string[] {
  const body = source.match(new RegExp(`pub enum ${enumName} \\{([^}]*)\\}`))?.[1];
  if (!body) throw new Error(`${enumName} not found in citations.rs`);
  return body
    .split('\n')
    .map((line) => line.trim().replace(/,$/, ''))
    .filter((line) => line.length > 0 && !line.startsWith('//'))
    .map((variant) => variant.replace(/([a-z0-9])([A-Z])/g, '$1_$2').toLowerCase());
}

function zodOptions(schema: unknown, key: string): string[] {
  const shape = (schema as { shape: Record<string, { options?: string[] }> }).shape;
  const options = shape[key]?.options;
  if (!options) throw new Error(`${key} is not an enum in the zod schema`);
  return [...options].sort();
}

const sample = {
  kind: 'github_build',
  title: 'main@aaaaaaa',
  url: 'https://github.com/owner/repository/commit/aaa',
  observed_at: '2026-09-08T00:00:00Z',
  complete: true,
  outcome: 'success',
};

describe('the console mirrors the server enums', () => {
  const source = readFileSync(CITATIONS_RS, 'utf8');

  test('CitationKind variants match', () => {
    expect(zodOptions(CitationSchema, 'kind')).toEqual(rustVariants(source, 'CitationKind').sort());
  });

  test('CitationOutcome variants match', () => {
    expect(zodOptions(CitationSchema, 'outcome')).toEqual(
      rustVariants(source, 'CitationOutcome').sort()
    );
  });

  test('Provenance variants match', () => {
    const rust = rustVariants(readFileSync(PROVENANCE_RS, 'utf8'), 'Provenance')
      .filter((variant) => !variant.startsWith('#['))
      .sort();

    expect(rust).toEqual(['model_asserted', 'server_execution']);
    expect(CitationSchema.parse({ ...sample, provenance: 'model_asserted' }).provenance).toBe(
      'model_asserted'
    );
  });
});

describe('an unreadable provenance is never shown as proof', () => {
  test('a citation stored before the field existed stays server-proven', () => {
    expect(CitationSchema.parse(sample).provenance).toBe('server_execution');
  });

  test('a value this client does not recognise is demoted to a claim', () => {
    expect(CitationSchema.parse({ ...sample, provenance: 'attested_by_vibes' }).provenance).toBe(
      'model_asserted'
    );
  });
});

describe('unrecognised metadata costs one entry, never the message', () => {
  const valid = sample;

  test('a citation kind this client has never heard of is dropped, not thrown', () => {
    const parsed = MessageMetadataSchema.parse({
      citations: [valid, { ...valid, kind: 'a_kind_from_a_later_release' }],
    });

    expect(parsed.citations).toHaveLength(1);
    expect(parsed.citations?.[0].kind).toBe('github_build');
  });

  test('a malformed entry does not discard its valid siblings', () => {
    const parsed = MessageMetadataSchema.parse({
      citations: [valid, { kind: 'github_build' }, valid],
    });

    expect(parsed.citations).toHaveLength(2);
  });
});
