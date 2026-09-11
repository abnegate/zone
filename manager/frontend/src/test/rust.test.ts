/**
 * The contract tests are only as good as what they read. A reader that reports
 * the Rust identifier where serde renamed the field asserts the console against
 * a wire shape the server never sends, and passes while every client fails.
 */

import { describe, expect, test } from 'bun:test';

import { structFields } from './rust';

const PLAIN = `
#[derive(Debug, Serialize)]
pub struct Receipt {
    pub run_id: String,
    answered: usize,
}
`;

const RENAMED = `
#[derive(Debug, Serialize)]
pub struct Receipt {
    pub run_id: String,
    #[serde(rename = "count")]
    answered: usize,
}
`;

const SKIPPED = `
#[derive(Debug, Serialize)]
pub struct Question {
    pub header: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
}
`;

const RENAMED_ALL = `
/// A struct the console has no mapping for.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Receipt {
    pub run_id: String,
}
`;

const RENAMED_ALL_WRAPPED = `
#[derive(Debug, Serialize)]
#[serde(
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub struct Receipt {
    pub run_id: String,
}
`;

const EMPTY = `
pub struct Receipt {
}
`;

describe('the reader reports what travels, not what the field is called', () => {
  test('a plain field is read under its own name, private or public', () => {
    expect(structFields(PLAIN, 'Receipt')).toEqual([
      { name: 'run_id', optional: false },
      { name: 'answered', optional: false },
    ]);
  });

  test('a renamed field is read under the name serde sends it as', () => {
    expect(structFields(RENAMED, 'Receipt')).toEqual([
      { name: 'run_id', optional: false },
      { name: 'count', optional: false },
    ]);
  });

  test('a field serde may omit is optional, and its neighbour is not', () => {
    expect(structFields(SKIPPED, 'Question')).toEqual([
      { name: 'header', optional: false },
      { name: 'preview', optional: true },
    ]);
  });
});

describe('a shape the console cannot mirror fails rather than passes', () => {
  test('a struct that renames every field is refused, because nothing maps it', () => {
    expect(() => structFields(RENAMED_ALL, 'Receipt')).toThrow(/rename_all/);
  });

  test('a rename_all spread over several lines is refused too', () => {
    expect(() => structFields(RENAMED_ALL_WRAPPED, 'Receipt')).toThrow(/rename_all/);
  });

  test('a struct that has been renamed or moved away is refused, not read as empty', () => {
    expect(() => structFields(PLAIN, 'Confirmation')).toThrow(/Confirmation/);
  });

  test('a struct with no fields is refused', () => {
    expect(() => structFields(EMPTY, 'Receipt')).toThrow(/no fields/);
  });
});
