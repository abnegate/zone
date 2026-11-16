import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import type { z } from 'zod';

import { rustVariants, structFields } from '../../test/rust';
import {
  CreatedResponseSchema,
  CreatedSchema,
  CreateReleaseRequestSchema,
  IncludedPullSchema,
  LOOKUPS,
  OptionsResponseSchema,
  PIPELINE_BLOCKER_CODES,
  PIPELINE_STATES,
  PipelineBlockerSchema,
  PipelineResponseSchema,
  PipelineRunSchema,
  PreviewRequestSchema,
  PreviewResponseSchema,
  PreviewSchema,
  RECONCILIATION_CODE,
  REFUSAL_CODES,
  RefusalSchema,
  ReleaseIdentitySchema,
  ReleasePipelineSchema,
  ReleaseSchema,
  ReleasesResponseSchema,
  SourceOptionsSchema,
  TAG_REFUSAL_CODES,
  TagLineSchema,
  TagRefusalCode,
} from './schemas';

const SERVER = join(import.meta.dir, '../../../../../runner/zone_server/src');
const GITHUB = join(SERVER, 'services/github');
const PIPELINE = join(GITHUB, 'release.rs');
const RELEASES = join(GITHUB, 'releases');
const ROUTES = join(SERVER, 'routes/releases');

const ENVELOPE = ['code', 'error'];
const FLATTENED = 'detail';
const SUCCESS = 'success';

const read = (path: string): string => readFileSync(path, 'utf8');

type Shaped = z.ZodObject<z.ZodRawShape>;

function rustShape(path: string, structName: string): Record<string, boolean> {
  return Object.fromEntries(
    structFields(read(path), structName).map((field) => [field.name, field.optional])
  );
}

/// Whether the parsed value may lack the key. `isOptional()` would also count a
/// `.default()` or `.catch()`, which only make the console tolerant of a field
/// the server always sends; serde omits a key only under `skip_serializing_if`,
/// and that is what the output optionality must mirror.
function zodShape(schema: Shaped): Record<string, boolean> {
  return Object.fromEntries(
    Object.entries(schema.shape).map(([key, value]) => [key, value._zod.optout === 'optional'])
  );
}

const keys = (shape: Record<string, unknown>): string[] => Object.keys(shape).sort();

const sorted = (values: readonly string[]): string[] => [...values].sort();

function withoutEnvelope(shape: Record<string, boolean>): Record<string, boolean> {
  return Object.fromEntries(Object.entries(shape).filter(([key]) => !ENVELOPE.includes(key)));
}

function refusalFor(code: string): Shaped {
  const option = RefusalSchema.options.find(
    (candidate) => candidate.shape.code.safeParse(code).success
  );
  if (!option) throw new Error(`no refusal models ${code}`);
  return option as unknown as Shaped;
}

describe('a release reads as the server serialises it', () => {
  test.each<[string, string, Shaped]>([
    ['Release', join(RELEASES, 'release.rs'), ReleaseSchema],
    ['IncludedPull', join(RELEASES, 'included_pull.rs'), IncludedPullSchema],
    ['ReleasePipeline', PIPELINE, ReleasePipelineSchema],
    ['ReleaseIdentity', PIPELINE, ReleaseIdentitySchema],
    ['PipelineRun', PIPELINE, PipelineRunSchema],
    ['PipelineBlockerReport', PIPELINE, PipelineBlockerSchema],
    ['SourceOptions', join(RELEASES, 'source_options.rs'), SourceOptionsSchema],
    ['TagLine', join(RELEASES, 'tag_line.rs'), TagLineSchema],
    ['Preview', join(RELEASES, 'preview.rs'), PreviewSchema],
    ['Created', join(RELEASES, 'created.rs'), CreatedSchema],
  ])('%s carries the same fields on both sides', (structName, path, schema) => {
    expect(zodShape(schema)).toEqual(rustShape(path, structName));
  });
});

describe('the release routes answer in the envelopes the console reads', () => {
  test.each<[string, string, Shaped]>([
    ['ListingResponse', 'listing_response.rs', ReleasesResponseSchema],
    ['OptionsResponse', 'options_response.rs', OptionsResponseSchema],
    ['PipelineResponse', 'pipeline_response.rs', PipelineResponseSchema],
    ['PreviewResponse', 'preview_response.rs', PreviewResponseSchema],
    ['CreatedResponse', 'created_response.rs', CreatedResponseSchema],
  ])('%s carries the same fields on both sides', (structName, file, schema) => {
    expect(zodShape(schema)).toEqual(rustShape(join(ROUTES, file), structName));
  });

  test.each<[string, string, Shaped]>([
    ['PreviewRequest', 'preview_request.rs', PreviewRequestSchema],
    ['CreateRequest', 'create_request.rs', CreateReleaseRequestSchema],
  ])('%s is sent with the fields the server reads', (structName, file, schema) => {
    expect(keys(schema.shape)).toEqual(keys(rustShape(join(ROUTES, file), structName)));
  });
});

describe('a release refusal reads as the server flattens it', () => {
  const envelope = rustShape(join(ROUTES, 'refusal_response.rs'), 'RefusalResponse');
  const tag = rustShape(join(ROUTES, 'tag_detail.rs'), 'TagDetail');
  const reconciliation = rustShape(
    join(ROUTES, 'reconciliation_detail.rs'),
    'ReconciliationDetail'
  );

  test('every refusal carries the error and code, a flattened detail, and a success flag the console ignores', () => {
    expect(envelope).toEqual({ [SUCCESS]: false, error: false, code: false, [FLATTENED]: false });
    for (const option of RefusalSchema.options) {
      expect(keys(option.shape)).toEqual(expect.arrayContaining(ENVELOPE));
      expect(option.shape).not.toHaveProperty(SUCCESS);
    }
  });

  test('a refusal with no detail carries only the envelope', () => {
    for (const code of REFUSAL_CODES) {
      expect(keys(refusalFor(code).shape)).toEqual(ENVELOPE);
    }
  });

  test('the tag refusals between them carry every TagDetail field, and only those', () => {
    const details = TAG_REFUSAL_CODES.map((code) => withoutEnvelope(zodShape(refusalFor(code))));

    for (const detail of details) {
      expect(detail).toHaveProperty('tag', tag.tag);
      expect(keys(tag)).toEqual(expect.arrayContaining(keys(detail)));
    }
    expect(sorted([...new Set(details.flatMap(keys))])).toEqual(keys(tag));
    expect(withoutEnvelope(zodShape(refusalFor(TagRefusalCode.TagExists)))).toEqual({
      tag: tag.tag,
      tag_object: tag.tag_object,
    });
  });

  test('the reconciliation refusal carries every ReconciliationDetail field', () => {
    expect(withoutEnvelope(zodShape(refusalFor(RECONCILIATION_CODE)))).toEqual(reconciliation);
  });

  test('the console models every refusal code the server sends', () => {
    const codes = [
      ...read(join(RELEASES, 'refusal.rs')).matchAll(/pub const [A-Z_]+: &str = "([^"]+)";/g),
    ].map((match) => match[1]);

    expect(sorted([...REFUSAL_CODES, ...TAG_REFUSAL_CODES, RECONCILIATION_CODE])).toEqual(
      sorted(codes)
    );
  });
});

describe('the release vocabularies match the server', () => {
  test('pipeline states, which serde writes in kebab case', () => {
    expect(sorted(PIPELINE_STATES)).toEqual(
      sorted(
        rustVariants(read(PIPELINE), 'PipelineState').map((state) => state.replaceAll('_', '-'))
      )
    );
  });

  test('lookups', () => {
    expect(sorted(LOOKUPS)).toEqual(sorted(rustVariants(read(PIPELINE), 'Lookup')));
  });

  test('pipeline blocker codes', () => {
    expect(sorted(PIPELINE_BLOCKER_CODES)).toEqual(
      sorted(rustVariants(read(PIPELINE), 'PipelineBlockerCode'))
    );
  });
});
