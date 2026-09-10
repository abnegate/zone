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
import { z } from 'zod';

import {
  ActionReceiptSchema,
  CitationSchema,
  MessageMetadataSchema,
  ToolCallRecordSchema,
} from './schemas';
import { REASONED_TOOLS } from './types';

const CITATIONS_RS = join(
  import.meta.dir,
  '../../../../../runner/zone_server/src/agent/citations.rs'
);

const PROVENANCE_RS = join(
  import.meta.dir,
  '../../../../../runner/zone_server/src/agent/verification/provenance.rs'
);

const TOOLS_RS = join(import.meta.dir, '../../../../../runner/zone_server/src/agent/tools.rs');

const REASONED_TOOLS_RS = 'REASONED_TOOLS';

function rustVariants(source: string, enumName: string): string[] {
  const body = source.match(new RegExp(`pub enum ${enumName} \\{([^}]*)\\}`))?.[1];
  if (!body) throw new Error(`${enumName} not found in the Rust source`);
  return body
    .split('\n')
    .map((line) => line.trim().replace(/,$/, ''))
    .filter((line) => line.length > 0 && !line.startsWith('//'))
    .map((variant) => variant.replace(/([a-z0-9])([A-Z])/g, '$1_$2').toLowerCase());
}

/// Reads a `const NAME: [&str; N] = [..]` literal. The declared length is part
/// of the match, so a list that grew without its count being updated — which
/// does not compile on the Rust side — is not silently accepted here either.
function rustStringArray(source: string, constantName: string): string[] {
  const body = source.match(
    new RegExp(`const ${constantName}: \\[&str; (\\d+)\\] = \\[([^\\]]*)\\];`)
  );
  if (!body) throw new Error(`${constantName} not found in the Rust source`);

  const names = [...body[2].matchAll(/"([^"]+)"/g)].map((match) => match[1]);
  if (names.length !== Number(body[1])) {
    throw new Error(`${constantName} declares ${body[1]} entries and lists ${names.length}`);
  }
  return names.sort();
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

/**
 * The console decides on its own which calls owe a reason, so a tool the server
 * asks and the console does not know about shows nothing at all where "No
 * reason given" should have been — the exact silence the feature exists to end.
 *
 * The Rust list this reads is not itself hand-written prose: the test beside it
 * asserts it equals the set of schemas in the assembled chat catalog that carry
 * the parameter. So an eighth reasoned tool fails there if the Rust list is not
 * updated, and fails here if the console's is not.
 */
describe('the console owes a reason for exactly the tools the server asks', () => {
  const server = () => rustStringArray(readFileSync(TOOLS_RS, 'utf8'), REASONED_TOOLS_RS);
  const trace = () => [...REASONED_TOOLS].sort();

  test('the two lists are the same set', () => {
    expect(trace()).toEqual(server());
  });

  test('the server list is the seven side-effecting tools, read not assumed', () => {
    expect(server()).toEqual([
      'apply_patch',
      'comment_on_issue',
      'create_pull_request',
      'run_command',
      'run_shell',
      'send_message',
      'write_file',
    ]);
  });

  test('an eighth reasoned tool the console has not caught up with fails, not passes', () => {
    expect(trace()).not.toEqual([...server(), 'delete_document'].sort());
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

/**
 * A `reason` is model-authored, added after these records were already being
 * persisted. Two things could quietly eat it or the history around it: these
 * objects are not passthrough, so an unlisted field is stripped on reload, and
 * `tolerantArray` drops elements that do not conform. Optional survives both.
 */
describe('a stated reason survives storage', () => {
  const storedCall = {
    id: 'call_1',
    name: 'run_shell',
    arguments: '{"command":"bun test"}',
    success: true,
    detail: 'ok',
    duration_ms: 3,
  };

  const storedReceipt = {
    id: 'call_1',
    action: 'send_message',
    target_type: 'message',
    target_id: 'msg-3',
    target_label: 'Standup is at ten',
    actor_id: 'user-1',
    actor_name: 'Alice',
    occurred_at: '2026-09-05T10:47:00.000Z',
    success: true,
    outcome: 'Message sent',
    href: '/chats?id=chat-9&message=msg-3',
  };

  test('an unlisted field is stripped, which is why reason had to be declared', () => {
    const parsed = ToolCallRecordSchema.parse({ ...storedCall, undeclared: 'gone' }) as Record<
      string,
      unknown
    >;

    expect(parsed.undeclared).toBeUndefined();
  });

  test('a reason on a tool call is kept rather than stripped', () => {
    expect(
      ToolCallRecordSchema.parse({ ...storedCall, reason: 'The user asked for the run.' })
    ).toHaveProperty('reason', 'The user asked for the run.');
  });

  test('a reason on a receipt is kept rather than stripped', () => {
    expect(
      ActionReceiptSchema.parse({ ...storedReceipt, reason: 'The user asked me to post it.' })
    ).toHaveProperty('reason', 'The user asked me to post it.');
  });

  test('a message stored before the reason existed still parses', () => {
    const parsed = MessageMetadataSchema.parse({
      tool_calls: [storedCall],
      action_receipts: [storedReceipt],
    });

    expect(parsed.tool_calls).toHaveLength(1);
    expect(parsed.tool_calls?.[0].reason).toBeUndefined();
    expect(parsed.action_receipts).toHaveLength(1);
    expect(parsed.action_receipts?.[0].reason).toBeUndefined();
  });

  test('a required reason would have deleted that history, which is why it is optional', () => {
    const required = z.object({ ...ToolCallRecordSchema.shape, reason: z.string() });

    expect(required.safeParse(storedCall).success).toBe(false);
    expect(MessageMetadataSchema.parse({ tool_calls: [storedCall] }).tool_calls).toHaveLength(1);
  });

  test('an unreadable reason costs the reason, never the row it sits on', () => {
    const parsed = MessageMetadataSchema.parse({
      tool_calls: [{ ...storedCall, reason: 42 }],
      action_receipts: [{ ...storedReceipt, reason: { why: 'not a string' } }],
    });

    expect(parsed.tool_calls).toHaveLength(1);
    expect(parsed.tool_calls?.[0].reason).toBeUndefined();
    expect(parsed.tool_calls?.[0].detail).toBe('ok');
    expect(parsed.action_receipts).toHaveLength(1);
    expect(parsed.action_receipts?.[0].reason).toBeUndefined();
    expect(parsed.action_receipts?.[0].outcome).toBe('Message sent');
  });
});
