/**
 * A parked run stores its question in `task_runs.pending_question`, written by
 * the worker as a JSON envelope and read back by the console to draw the card.
 * Nothing in either type system links the two, and the column is persisted:
 * a key renamed on one side alone leaves every already-parked run showing a
 * card with no questions on it, and no way to answer.
 *
 * This reads the envelope the worker actually writes and asserts the schema
 * models the same keys.
 */

import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

import { structFields } from '../../test/rust';
import { WaitingSchema } from '../chats/schemas';
import {
  AnswersResponseSchema,
  PendingQuestionSchema,
  TaskRunResponseSchema,
  TaskRunSchema,
} from './schemas';

const TASK_WORKER_RS = join(
  import.meta.dir,
  '../../../../../runner/zone_server/src/workers/task.rs'
);

const TASKS_ROUTE_RS = join(
  import.meta.dir,
  '../../../../../runner/zone_server/src/routes/tasks.rs'
);

const WAIT_RS = join(import.meta.dir, '../../../../../runner/zone_server/src/agent/wait.rs');

/**
 * On the row, and formatted for a task by the same route, but not on the run it
 * sends. The console keeps its model of a run whole; this is what the route
 * still owes it, and the test that reads it fails the day the route pays so the
 * allowance goes with the debt.
 */
const OWED_BY_THE_ROUTE = ['completed_at', 'started_at'];

const running = {
  id: 'run-1',
  task_id: 'task-1',
  status: 'running',
  current_phase: 'acting',
  progress_percent: 40,
  error_message: null,
  pending_question: null,
};

/// Reads the keys of the `serde_json::json!({..})` literal bound to `let NAME`.
function jsonLiteralKeys(source: string, binding: string): string[] {
  const body = source.match(
    new RegExp(`let ${binding} = serde_json::json!\\(\\{([^}]*)\\}\\);`)
  )?.[1];
  if (!body) throw new Error(`${binding} is not a json! literal in the Rust source`);
  return [...body.matchAll(/"([a-z_0-9]+)":/g)].map((match) => match[1]).sort();
}

/// The names the struct's fields travel under, sorted. Its own fields are
/// private, since nothing outside the module constructs them.
function serialisedNames(source: string, structName: string): string[] {
  return structFields(source, structName)
    .map((field) => field.name)
    .sort();
}

/// Each field's wire name to whether serde may omit it, shaped like a schema.
function rustFields(source: string, structName: string): Record<string, boolean> {
  return Object.fromEntries(
    structFields(source, structName).map((field) => [field.name, field.optional])
  );
}

function zodFields(schema: {
  shape: Record<string, { isOptional(): boolean }>;
}): Record<string, boolean> {
  return Object.fromEntries(
    Object.entries(schema.shape).map(([key, value]) => [key, value.isOptional()])
  );
}

describe('the parked question the console reads is the one the worker wrote', () => {
  test('the stored envelope carries exactly the keys the schema declares', () => {
    const rust = jsonLiteralKeys(readFileSync(TASK_WORKER_RS, 'utf8'), 'pending');

    expect(rust).toEqual(Object.keys(PendingQuestionSchema.shape).sort());
  });

  test('an envelope missing the tool call it answers is rejected, not half-drawn', () => {
    expect(PendingQuestionSchema.safeParse({ questions: [] }).success).toBe(false);
  });
});

/**
 * The answering route replies with a receipt for the submission rather than
 * with the run: the waiter it hands the answers to resumes the worker out of
 * band, so there is no resumed run to send back yet. A client parsing the wrong
 * shape fails every accepted answer, and the reader is told their answer was
 * rejected by the one call that actually went through.
 */
describe('the confirmation the console parses is the one the route sends', () => {
  test('the accepted body carries exactly the fields the schema declares', () => {
    const rust = serialisedNames(readFileSync(TASKS_ROUTE_RS, 'utf8'), 'AnswersResponse');

    expect(rust).toEqual(['answered', 'run_id']);
    expect(rust).toEqual(Object.keys(AnswersResponseSchema.shape).sort());
  });

  test('the run envelope the client used to parse rejects what the route sends', () => {
    const accepted = { run_id: 'run-1', answered: 2 };

    expect(AnswersResponseSchema.safeParse(accepted).success).toBe(true);
    expect(TaskRunResponseSchema.safeParse(accepted).success).toBe(false);
  });

  test('a count that is not a whole number of answers is rejected', () => {
    expect(AnswersResponseSchema.safeParse({ run_id: 'run-1', answered: -1 }).success).toBe(false);
  });
});

/**
 * A run parked on a wait carries the `Waiting` the worker registered, stored in
 * `task_runs.pending_wait` and sent back as `waiting_on`. The line beside the
 * badge names the subject from `kind`, `id` and `reference` and counts down to
 * `deadline`, so a key renamed on one side alone leaves every parked run with
 * a badge and no subject.
 */
describe('the wait the console describes is the one the worker parked on', () => {
  test('Waiting carries the same fields on both sides, optional where serde may omit', () => {
    expect(zodFields(WaitingSchema)).toEqual(rustFields(readFileSync(WAIT_RS, 'utf8'), 'Waiting'));
  });

  test('the run keeps a wait of that shape and drops one that is not', () => {
    const waiting_on = {
      kind: 'check',
      id: 'a1b2c3',
      reference: 'main',
      deadline: '2026-09-13T17:00:00Z',
    };
    const parked = { ...running, status: 'waiting' };

    expect(TaskRunSchema.parse({ ...parked, waiting_on }).waiting_on).toEqual(waiting_on);
    expect(
      TaskRunSchema.parse({ ...parked, waiting_on: { kind: 'check' } }).waiting_on
    ).toBeUndefined();
  });
});

/**
 * One struct wraps into both the by-id and the list route, so it is the whole
 * run contract. Nothing compared it to the schema before, which is how a wait
 * park reached the column and not the wire: the schema declared the field, the
 * route never sent it, and nothing noticed.
 */
describe('the run the console reads is the one the route sends', () => {
  test('every field the route sends has a schema entry, and none is declared that it does not send', () => {
    const declared = Object.keys(TaskRunSchema.shape)
      .filter((name) => !OWED_BY_THE_ROUTE.includes(name))
      .sort();

    expect(serialisedNames(readFileSync(TASKS_ROUTE_RS, 'utf8'), 'TaskRunData')).toEqual(declared);
  });

  test('the wait a run parks on reaches the wire, absent rather than null when there is none', () => {
    expect(rustFields(readFileSync(TASKS_ROUTE_RS, 'utf8'), 'TaskRunData').waiting_on).toBe(true);
    expect(TaskRunSchema.parse(running)).not.toHaveProperty('waiting_on');
  });

  test('a field the route may omit is one the schema can do without', () => {
    const declared = zodFields(TaskRunSchema);

    for (const [name, omittable] of Object.entries(
      rustFields(readFileSync(TASKS_ROUTE_RS, 'utf8'), 'TaskRunData')
    )) {
      if (omittable) expect(declared[name]).toBe(true);
    }
  });

  test('the route owes exactly the two timestamps the row carries, and no more', () => {
    const sent = serialisedNames(readFileSync(TASKS_ROUTE_RS, 'utf8'), 'TaskRunData');
    const declared = zodFields(TaskRunSchema);

    for (const name of OWED_BY_THE_ROUTE) {
      expect(sent).not.toContain(name);
      expect(declared[name]).toBe(true);
    }
  });
});
