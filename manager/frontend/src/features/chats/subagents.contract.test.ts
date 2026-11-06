/**
 * A subagent reaches the console over REST and the chat socket in the shape the
 * server's `ChatSubagent` serialises to. Nothing in the type system links the
 * two, so this reads the Rust definitions and asserts the console's schemas and
 * status lists still match them.
 */

import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

import { rustVariants, structFields } from '../../test/rust';
import { ChatSubagentSchema, SubagentStepSchema } from './subagentSchemas';
import { STEP_STATUSES, SUBAGENT_STATUSES } from './subagentTypes';

const RUNNER = join(import.meta.dir, '../../../../../runner');
const CHAT_SUBAGENT_RS = join(RUNNER, 'zone_server/src/services/chat/subagent/chat_subagent.rs');
const SUBAGENT_RS = join(RUNNER, 'zone_core/src/llm/provider/subagent');

function read(path: string): string {
  return readFileSync(path, 'utf8');
}

function rustKeys(source: string, structName: string): string[] {
  return structFields(source, structName)
    .map((field) => field.name)
    .sort();
}

function zodKeys(schema: { shape: Record<string, unknown> }): string[] {
  return Object.keys(schema.shape).sort();
}

describe('a chat subagent reads as the server sends it', () => {
  test('ChatSubagent carries the same fields on both sides', () => {
    expect(zodKeys(ChatSubagentSchema)).toEqual(rustKeys(read(CHAT_SUBAGENT_RS), 'ChatSubagent'));
  });

  test('SubagentStep carries the same fields on both sides', () => {
    expect(zodKeys(SubagentStepSchema)).toEqual(
      rustKeys(read(join(SUBAGENT_RS, 'step.rs')), 'SubagentStep')
    );
  });

  test('no field is left out of the wire, so none is optional on the server', () => {
    const fields = [
      ...structFields(read(CHAT_SUBAGENT_RS), 'ChatSubagent'),
      ...structFields(read(join(SUBAGENT_RS, 'step.rs')), 'SubagentStep'),
    ];
    expect(fields.filter((field) => field.optional)).toEqual([]);
  });

  test.each([
    ['status.rs', 'SubagentStatus', SUBAGENT_STATUSES],
    ['step_status.rs', 'StepStatus', STEP_STATUSES],
  ] as const)('%s variants match', (file, name, values) => {
    const source = read(join(SUBAGENT_RS, file));
    expect([...values].sort()).toEqual(rustVariants(source, name).sort());
  });
});
