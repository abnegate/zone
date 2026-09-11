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
import { AnswersResponseSchema, PendingQuestionSchema, TaskRunResponseSchema } from './schemas';

const TASK_WORKER_RS = join(
  import.meta.dir,
  '../../../../../runner/zone_server/src/workers/task.rs'
);

const TASKS_ROUTE_RS = join(
  import.meta.dir,
  '../../../../../runner/zone_server/src/routes/tasks.rs'
);

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
