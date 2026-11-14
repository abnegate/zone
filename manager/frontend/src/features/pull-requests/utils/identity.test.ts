import { describe, expect, it } from 'bun:test';
import { SOURCE_ID } from '../../../test/pullRequests';
import { parsePullRequestKey, pullRequestKey, samePullRequest } from './identity';

describe('pull request identity', () => {
  it('names a pull request by its source and number', () => {
    expect(pullRequestKey({ source_id: SOURCE_ID, number: 42 })).toBe(`${SOURCE_ID}:42`);
  });

  it('reads back the pull request it named', () => {
    const reference = { source_id: SOURCE_ID, number: 42 };
    expect(parsePullRequestKey(pullRequestKey(reference))).toEqual(reference);
  });

  it('splits on the last separator', () => {
    expect(parsePullRequestKey('source:with:colons:7')).toEqual({
      source_id: 'source:with:colons',
      number: 7,
    });
  });

  it.each([
    ['nothing', null],
    ['an empty value', ''],
    ['no separator', SOURCE_ID],
    ['no source', ':42'],
    ['no number', `${SOURCE_ID}:`],
    ['a zero number', `${SOURCE_ID}:0`],
    ['a negative number', `${SOURCE_ID}:-1`],
    ['a fractional number', `${SOURCE_ID}:4.2`],
    ['a padded number', `${SOURCE_ID}:042`],
    ['a word', `${SOURCE_ID}:latest`],
    ['an unsafe number', `${SOURCE_ID}:99999999999999999999`],
  ])('refuses %s', (_case, value) => {
    expect(parsePullRequestKey(value)).toBeNull();
  });

  it('matches the same pull request whatever else it carries', () => {
    expect(
      samePullRequest({ source_id: SOURCE_ID, number: 42 }, { source_id: SOURCE_ID, number: 42 })
    ).toBe(true);
  });

  it('tells pull requests apart by source and by number', () => {
    const reference = { source_id: SOURCE_ID, number: 42 };
    expect(samePullRequest(reference, { source_id: 'other', number: 42 })).toBe(false);
    expect(samePullRequest(reference, { source_id: SOURCE_ID, number: 43 })).toBe(false);
  });
});
