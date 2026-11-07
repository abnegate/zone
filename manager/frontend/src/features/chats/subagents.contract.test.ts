/**
 * A subagent reaches the console over REST and the chat socket in the shape the
 * server's `ChatSubagent` serialises to. Nothing in the type system links the
 * two, so this reads the Rust definition and asserts the console's schemas and
 * status lists still match it. The steps and statuses come from
 * `abnegate-agent-cli`, whose source isn't in this repository, so
 * `subagents.contract.json` stands between them: `chat_subagent.rs` holds the
 * crate's types to that file, and this test holds the console to it.
 */

import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

import { structFields } from '../../test/rust';
import { ChatSubagentSchema, SubagentStepSchema } from './subagentSchemas';
import contract from './subagents.contract.json';
import { STEP_STATUSES, SUBAGENT_STATUSES } from './subagentTypes';

const RUNNER = join(import.meta.dir, '../../../../../runner');
const CHAT_SUBAGENT_RS = join(RUNNER, 'zone_server/src/services/chat/subagent/chat_subagent.rs');

function chatSubagentFields() {
  return structFields(readFileSync(CHAT_SUBAGENT_RS, 'utf8'), 'ChatSubagent');
}

function zodKeys(schema: { shape: Record<string, unknown> }): string[] {
  return Object.keys(schema.shape).sort();
}

describe('a chat subagent reads as the server sends it', () => {
  test('ChatSubagent carries the same fields on both sides', () => {
    expect(zodKeys(ChatSubagentSchema)).toEqual(
      chatSubagentFields()
        .map((field) => field.name)
        .sort()
    );
  });

  test('SubagentStep carries the same fields on both sides', () => {
    expect(zodKeys(SubagentStepSchema)).toEqual(contract.step_fields);
  });

  test('no field is left out of the wire, so none is optional on the server', () => {
    expect(chatSubagentFields().filter((field) => field.optional)).toEqual([]);
  });

  test.each([
    ['SubagentStatus', SUBAGENT_STATUSES, contract.subagent_statuses],
    ['StepStatus', STEP_STATUSES, contract.step_statuses],
  ] as const)('%s values match', (_name, values, expected) => {
    expect([...values].sort()).toEqual(expected);
  });
});
