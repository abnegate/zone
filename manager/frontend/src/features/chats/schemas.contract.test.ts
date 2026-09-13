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

import { structFields } from '../../test/rust';
import {
  ActionReceiptSchema,
  ChoiceSchema,
  CitationSchema,
  JobExitedSchema,
  JobStartedSchema,
  MessageMetadataSchema,
  QuestionSchema,
  ToolCallRecordSchema,
  WaitingSchema,
  WaitSettledSchema,
} from './schemas';
import { AWAITING_ANSWER_DETAIL, REASONED_TOOLS } from './types';

const CITATIONS_RS = join(
  import.meta.dir,
  '../../../../../runner/zone_server/src/agent/citations.rs'
);

const PROVENANCE_RS = join(
  import.meta.dir,
  '../../../../../runner/zone_server/src/agent/verification/provenance.rs'
);

const TOOLS_RS = join(import.meta.dir, '../../../../../runner/zone_server/src/agent/tools.rs');

const QUESTION_RS = join(
  import.meta.dir,
  '../../../../../runner/zone_server/src/agent/question.rs'
);

const CHAT_WS_RS = join(import.meta.dir, '../../../../../runner/zone_server/src/ws/chat.rs');

const JOB_RS = join(import.meta.dir, '../../../../../runner/zone_core/src/tools/job.rs');

const WAIT_RS = join(import.meta.dir, '../../../../../runner/zone_server/src/agent/wait.rs');

const AGENT_RS = join(import.meta.dir, '../../../../../runner/zone_server/src/agent/mod.rs');

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

/// Reads the value of a `const NAME: &str = ".."` literal.
function rustStringConstant(source: string, constantName: string): string {
  const value = source.match(new RegExp(`const ${constantName}: &str = "([^"]*)";`))?.[1];
  if (value === undefined) throw new Error(`${constantName} not found in the Rust source`);
  return value;
}

/// The struct's serialised shape as the zod shapes below state it: the name
/// each field travels under, against whether serde may omit it. A field
/// carrying `skip_serializing_if` is absent from the wire when empty, which is
/// exactly what the console must model as optional.
function rustStructFields(source: string, structName: string): Record<string, boolean> {
  return Object.fromEntries(
    structFields(source, structName).map((field) => [field.name, field.optional])
  );
}

/// The same shape read off the zod side: every declared key, against whether
/// the schema accepts the key being absent.
const zodFields = (schema: unknown): Record<string, boolean> => {
  const shape = (schema as { shape: Record<string, { isOptional(): boolean }> }).shape;
  return Object.fromEntries(Object.entries(shape).map(([key, value]) => [key, value.isOptional()]));
};

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

/**
 * The live frame and the stored record label the same call. The console writes
 * the label as the frame arrives, the server writes it onto the record it
 * persists, and the reader sees one then the other across a reload. Two copies
 * of one string, so a changed ellipsis relabels the call on reload.
 */
describe('a question call is labelled the same live as it is after a reload', () => {
  test('the console writes the detail the server persists, ellipsis included', () => {
    expect(AWAITING_ANSWER_DETAIL).toBe(
      rustStringConstant(readFileSync(CHAT_WS_RS, 'utf8'), 'AWAITING_ANSWER_DETAIL')
    );
  });
});

/**
 * How a wait ended is a value the server sends, not something to be read back
 * out of the outcome's prose. The card draws on this alone, so a variant added
 * or respelled on the server that this console cannot parse must not be able to
 * reach it as anything — least of all as a pass.
 */
describe('the console reads which way a wait ended rather than the words it ended with', () => {
  test('the verdicts the console accepts are the ones the server declares', () => {
    const rust = rustVariants(readFileSync(WAIT_RS, 'utf8'), 'Verdict')
      .filter((variant) => !variant.startsWith('#['))
      .sort();

    expect(rust).toEqual(['settled', 'silent', 'timed_out', 'unreadable']);
    expect(zodOptions(WaitSettledSchema, 'verdict')).toEqual(rust);
  });

  /**
   * Nothing here falls back to a value, unlike every other tolerant read in
   * these schemas: there is no safe default for whether something passed. A
   * verdict this console cannot read fails the whole parse, the frame is
   * dropped where it is read, and the card stays drawn as the wait it still
   * was — which is not a claim that anything finished.
   */
  test('an unreadable verdict fails the parse rather than defaulting to one', () => {
    const settled = {
      tool_call_id: 'call_1',
      outcome: 'The checks on main could not be read after 120s, so this is not a pass: offline.',
    };

    for (const verdict of ['', 'ok', 'passed', 'unknown', 'timed-out', null, undefined, 7]) {
      const parsed = WaitSettledSchema.safeParse({ ...settled, verdict });
      expect(parsed.success).toBe(false);
    }
    expect(WaitSettledSchema.safeParse({ ...settled, verdict: 'unreadable' }).success).toBe(true);
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
 * The identifier is the handle a reply cites a source by, added after citations
 * were already being persisted. `CitationSchema` is not passthrough and
 * `tolerantArray` drops what does not conform, so declaring it optional is what
 * keeps both the new field and the rows stored without one.
 */
describe('a cited identifier survives storage', () => {
  const web = { ...sample, kind: 'web', identifier: 'web-1' };

  test('a web source parses with the identifier it was cited by', () => {
    const parsed = MessageMetadataSchema.parse({ citations: [web] });

    expect(parsed.citations).toHaveLength(1);
    expect(parsed.citations?.[0].kind).toBe('web');
    expect(parsed.citations?.[0].identifier).toBe('web-1');
  });

  test('a citation stored before identifiers existed still parses', () => {
    const parsed = MessageMetadataSchema.parse({ citations: [sample] });

    expect(parsed.citations).toHaveLength(1);
    expect(parsed.citations?.[0].identifier).toBeUndefined();
  });

  test('a required identifier would have dropped that history', () => {
    const required = z.object({ ...CitationSchema.shape, identifier: z.string() });

    expect(required.safeParse(sample).success).toBe(false);
    expect(CitationSchema.safeParse(sample).success).toBe(true);
  });
});

/**
 * A `reason` is model-authored, added after these records were already being
 * persisted. Two things could quietly eat it or the history around it: these
 * objects are not passthrough, so an unlisted field is stripped on reload, and
 * `tolerantArray` drops elements that do not conform. Optional survives both.
 */
describe('a stated reason and an observed preview survive storage', () => {
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

  /**
   * `exited` and `settled` are live-frame-only by design. `useChat` patches
   * them onto the record as a `job_exited` or `wait_settled` frame arrives, and
   * neither the schema above nor Rust's `ToolCallRecord` declares either, so a
   * reload strips both and re-derives them from the frames it replays. They are
   * the two card fields it is correct to leave undeclared, and this is the test
   * that says so rather than leaving the omission looking like the oversight
   * the strip test above exists to catch.
   */
  test('the live-frame-only exited and settled are undeclared, and stripped on reload', () => {
    const declared = Object.keys(ToolCallRecordSchema.shape);
    expect(declared).not.toContain('exited');
    expect(declared).not.toContain('settled');

    const parsed = ToolCallRecordSchema.parse({
      ...storedCall,
      exited: { id: 'job_0123456789ab', exit_code: 0 },
      settled: { tool_call_id: 'call_1', outcome: 'Job exited 0.', verdict: 'settled' },
    }) as Record<string, unknown>;

    expect(parsed.exited).toBeUndefined();
    expect(parsed.settled).toBeUndefined();
    expect(parsed.detail).toBe('ok');
  });

  test('a reason on a tool call is kept rather than stripped', () => {
    expect(
      ToolCallRecordSchema.parse({ ...storedCall, reason: 'The user asked for the run.' })
    ).toHaveProperty('reason', 'The user asked for the run.');
  });

  test('a preview on a tool call is kept rather than stripped', () => {
    expect(
      ToolCallRecordSchema.parse({ ...storedCall, preview: 'Run `bun test` in /srv/zone.' })
    ).toHaveProperty('preview', 'Run `bun test` in /srv/zone.');
  });

  test('questions on a tool call are kept rather than stripped', () => {
    const questions = [
      {
        header: 'Scope',
        question: 'How far should this go?',
        choices: [
          {
            label: 'Backfill',
            description: 'Rewrite every existing row.',
            recommended: true,
            free_text: false,
          },
          {
            label: 'Other',
            description: 'Something else — type it below.',
            recommended: false,
            free_text: true,
          },
        ],
        multi_select: false,
        required: true,
      },
    ];

    expect(ToolCallRecordSchema.parse({ ...storedCall, questions })).toHaveProperty(
      'questions',
      questions
    );
  });

  test('unreadable questions cost the questions, never the row they sit on', () => {
    const parsed = MessageMetadataSchema.parse({
      tool_calls: [{ ...storedCall, questions: { nope: 1 } }],
    });

    expect(parsed.tool_calls).toHaveLength(1);
    expect(parsed.tool_calls?.[0].questions).toBeUndefined();
    expect(parsed.tool_calls?.[0].detail).toBe('ok');
  });

  test('an unreadable preview costs the preview, never the row it sits on', () => {
    const parsed = MessageMetadataSchema.parse({
      tool_calls: [{ ...storedCall, preview: 42 }],
    });

    expect(parsed.tool_calls).toHaveLength(1);
    expect(parsed.tool_calls?.[0].preview).toBeUndefined();
    expect(parsed.tool_calls?.[0].detail).toBe('ok');
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
    expect(parsed.tool_calls?.[0].preview).toBeUndefined();
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

/**
 * A question card is rendered straight from what the server sent. The schema is
 * not passthrough, so a field the server adds and the console does not declare
 * is stripped before the card is drawn — a choice that vanishes, or a
 * `multi_select` that reads as single, and the reader answers a different
 * question from the one the agent asked. Nothing links the two definitions, so
 * this reads the Rust struct and asserts the shapes are the same.
 */
describe('the question card the console draws is the one the server sent', () => {
  const source = readFileSync(QUESTION_RS, 'utf8');

  test('Choice carries the same fields on both sides', () => {
    expect(zodFields(ChoiceSchema)).toEqual(rustStructFields(source, 'Choice'));
  });

  test('Question carries the same fields on both sides, optional where serde may omit', () => {
    expect(zodFields(QuestionSchema)).toEqual(rustStructFields(source, 'Question'));
  });
});

/**
 * The job and wait cards are drawn from the same four payloads twice over: once
 * from the live frame that announces them, and again from the record the server
 * persisted, which is where a reload finds them. `ToolCallRecordSchema` is not
 * passthrough, so a field the server adds and the console does not declare is
 * stripped before either drawing — a job card with no pid, a wait card with no
 * deadline. Nothing in the type system links the two definitions.
 *
 * Each of the four travels as a named Rust struct rather than inside an enum
 * variant precisely so `structFields` can read it; an enum payload would be
 * unreadable here and so unpinned.
 */
describe('the job and wait cards the console draws are the ones the server sent', () => {
  const job = readFileSync(JOB_RS, 'utf8');
  const wait = readFileSync(WAIT_RS, 'utf8');
  const agent = readFileSync(AGENT_RS, 'utf8');

  test('JobStarted carries the same fields on both sides', () => {
    expect(zodFields(JobStartedSchema)).toEqual(rustStructFields(job, 'JobStarted'));
  });

  test('JobExited carries the same fields on both sides, optional where serde may omit', () => {
    expect(zodFields(JobExitedSchema)).toEqual(rustStructFields(job, 'JobExited'));
  });

  test('Waiting carries the same fields on both sides, optional where serde may omit', () => {
    expect(zodFields(WaitingSchema)).toEqual(rustStructFields(wait, 'Waiting'));
  });

  test('WaitSettled carries the same fields on both sides', () => {
    expect(zodFields(WaitSettledSchema)).toEqual(rustStructFields(wait, 'WaitSettled'));
  });

  test('ToolCallRecord carries the same fields on both sides, the two new cards included', () => {
    const rust = rustStructFields(agent, 'ToolCallRecord');

    expect(rust).toHaveProperty('job', true);
    expect(rust).toHaveProperty('waiting', true);
    expect(rust).not.toHaveProperty('exited');
    expect(rust).not.toHaveProperty('settled');
    expect(zodFields(ToolCallRecordSchema)).toEqual(rust);
  });
});
