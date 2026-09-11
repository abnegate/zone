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

import { PendingQuestionSchema } from './schemas';

const TASK_WORKER_RS = join(
  import.meta.dir,
  '../../../../../runner/zone_server/src/workers/task.rs'
);

/// Reads the keys of the `serde_json::json!({..})` literal bound to `let NAME`.
function jsonLiteralKeys(source: string, binding: string): string[] {
  const body = source.match(
    new RegExp(`let ${binding} = serde_json::json!\\(\\{([^}]*)\\}\\);`)
  )?.[1];
  if (!body) throw new Error(`${binding} is not a json! literal in the Rust source`);
  return [...body.matchAll(/"([a-z_0-9]+)":/g)].map((match) => match[1]).sort();
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
