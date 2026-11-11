/** The database constraint, the server's STATUSES and the console must admit the same statuses. */

import { describe, expect, test } from 'bun:test';
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

import { TASK_STATUSES } from './status';

const SERVER = join(import.meta.dir, '../../../../../runner/zone_server');
const MIGRATIONS = join(SERVER, 'migrations');
const TASKS_RS = join(SERVER, 'src/db/tasks.rs');

const CONSTRAINT = 'tasks_status_check';
const STRING_LITERAL = /^'([a-z_]+)'(?:::text)?$/;
const RUST_STRING = /^"([a-z_]+)"$/;
const RUST_IDENTIFIER = /^[A-Z_][A-Z_0-9]*$/;

function sorted(values: readonly string[]): string[] {
  return [...values].sort();
}

function newestConstraintMigration(): string {
  const files = readdirSync(MIGRATIONS)
    .filter((file) => file.endsWith('.sql'))
    .sort();
  const defining = files.filter((file) =>
    readFileSync(join(MIGRATIONS, file), 'utf8').includes(`${CONSTRAINT} CHECK`)
  );
  const newest = defining.at(-1);
  if (!newest) throw new Error(`No migration defines ${CONSTRAINT}`);
  return newest;
}

function constraintStatuses(sql: string): string[] {
  const definition = sql.slice(sql.lastIndexOf(`${CONSTRAINT} CHECK`));
  const array = definition.match(/ARRAY\[([^\]]*)\]/)?.[1];
  if (!array) throw new Error(`${CONSTRAINT} does not check against an ARRAY`);
  return array.split(',').map((element) => {
    const literal = STRING_LITERAL.exec(element.trim())?.[1];
    if (!literal) throw new Error(`Unexpected element in ${CONSTRAINT}: ${element.trim()}`);
    return literal;
  });
}

function rustStringConstant(source: string, name: string): string {
  const value = source.match(new RegExp(`pub const ${name}: &str = "([^"]+)";`))?.[1];
  if (!value) throw new Error(`${name} is not a string constant in the Rust source`);
  return value;
}

function rustStatuses(source: string): string[] {
  const declaration = source.match(/pub const STATUSES: \[&str; (\d+)\] = \[([^\]]*)\];/);
  if (!declaration) throw new Error('STATUSES is not a string array constant in the Rust source');
  const [, length, body] = declaration;
  const statuses = body
    .split(',')
    .map((element) => element.trim())
    .filter((element) => element.length > 0)
    .map((element) => {
      const literal = RUST_STRING.exec(element)?.[1];
      if (literal) return literal;
      if (RUST_IDENTIFIER.test(element)) return rustStringConstant(source, element);
      throw new Error(`Unexpected element in STATUSES: ${element}`);
    });
  if (statuses.length !== Number(length))
    throw new Error(`STATUSES declares ${length} elements but lists ${statuses.length}`);
  return statuses;
}

describe('the statuses the board offers are the ones the database admits', () => {
  test('the newest migration redefining the constraint admits exactly the console statuses', () => {
    const migration = newestConstraintMigration();
    const statuses = constraintStatuses(readFileSync(join(MIGRATIONS, migration), 'utf8'));

    expect(statuses).toHaveLength(new Set(statuses).size);
    expect(sorted(statuses)).toEqual(sorted(TASK_STATUSES));
  });

  test('the server validates against exactly the console statuses', () => {
    const statuses = rustStatuses(readFileSync(TASKS_RS, 'utf8'));

    expect(statuses).toHaveLength(new Set(statuses).size);
    expect(sorted(statuses)).toEqual(sorted(TASK_STATUSES));
  });

  test('the constraint reader takes both the cast and the bare form of a literal', () => {
    expect(
      constraintStatuses(`${CONSTRAINT} CHECK (status = ANY (ARRAY['a'::text, 'b']))`)
    ).toEqual(['a', 'b']);
  });

  test('the server reader resolves a status named by a constant', () => {
    const source = [
      'pub const HELD: &str = "held";',
      'pub const STATUSES: [&str; 2] = [',
      '    "open",',
      '    HELD,',
      '];',
    ].join('\n');

    expect(rustStatuses(source)).toEqual(['open', 'held']);
  });
});
