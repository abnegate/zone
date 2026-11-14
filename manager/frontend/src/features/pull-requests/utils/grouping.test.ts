import { describe, expect, it } from 'bun:test';
import { wirePullRequest } from '../../../test/pullRequests';
import { PullRequestSchema } from '../schemas';
import type { PullRequest, Readiness } from '../types';
import {
  authorsOf,
  filterPullRequests,
  groupPullRequests,
  READINESS_LABELS,
  READINESS_ORDER,
  READINESS_TINTS,
  repositoriesOf,
} from './grouping';

const base = PullRequestSchema.parse(wirePullRequest);

function pullRequest(
  number: number,
  readiness: Readiness,
  changes: Partial<PullRequest> = {}
): PullRequest {
  return { ...base, number, readiness, ...changes };
}

describe('readiness order', () => {
  it('reads ready first and not ready last', () => {
    expect(READINESS_ORDER).toEqual(['ready', 'in_progress', 'not_ready']);
  });

  it('labels and tints every readiness', () => {
    expect(READINESS_LABELS).toEqual({
      ready: 'Ready',
      in_progress: 'In progress',
      not_ready: 'Not ready',
    });
    expect(READINESS_TINTS).toEqual({
      ready: 'success',
      in_progress: 'info',
      not_ready: 'warning',
    });
  });
});

describe('groupPullRequests', () => {
  it('buckets pull requests by readiness in readiness order', () => {
    const groups = groupPullRequests([
      pullRequest(1, 'not_ready'),
      pullRequest(2, 'ready'),
      pullRequest(3, 'in_progress'),
      pullRequest(4, 'ready'),
    ]);
    expect(groups.map((group) => group.readiness)).toEqual(['ready', 'in_progress', 'not_ready']);
    expect(groups.map((group) => group.pullRequests.map((each) => each.number))).toEqual([
      [2, 4],
      [3],
      [1],
    ]);
  });

  it('keeps the order the server ranked within each group', () => {
    const [group] = groupPullRequests([
      pullRequest(9, 'ready'),
      pullRequest(3, 'ready'),
      pullRequest(5, 'ready'),
    ]);
    expect(group.pullRequests.map((each) => each.number)).toEqual([9, 3, 5]);
  });

  it('labels and tints each group', () => {
    const [group] = groupPullRequests([pullRequest(1, 'in_progress')]);
    expect(group.label).toBe('In progress');
    expect(group.tint).toBe('info');
  });

  it('leaves out readiness nothing is in', () => {
    expect(
      groupPullRequests([pullRequest(1, 'not_ready')]).map((group) => group.readiness)
    ).toEqual(['not_ready']);
  });

  it('has no groups without pull requests', () => {
    expect(groupPullRequests([])).toEqual([]);
  });
});

describe('filterPullRequests', () => {
  const listed = [
    pullRequest(1, 'ready', { repository: 'acme/zone', author: 'octocat' }),
    pullRequest(2, 'not_ready', { repository: 'acme/api', author: 'octocat' }),
    pullRequest(3, 'ready', { repository: 'acme/api', author: 'hubot' }),
    pullRequest(4, 'in_progress', { repository: 'acme/zone', author: null }),
  ];

  function numbers(filtered: readonly PullRequest[]): number[] {
    return filtered.map((each) => each.number);
  }

  it('keeps everything without a filter', () => {
    expect(numbers(filterPullRequests(listed, {}))).toEqual([1, 2, 3, 4]);
    expect(
      numbers(filterPullRequests(listed, { readiness: null, repository: null, author: null }))
    ).toEqual([1, 2, 3, 4]);
  });

  it('keeps one readiness', () => {
    expect(numbers(filterPullRequests(listed, { readiness: 'ready' }))).toEqual([1, 3]);
  });

  it('keeps one repository', () => {
    expect(numbers(filterPullRequests(listed, { repository: 'acme/api' }))).toEqual([2, 3]);
  });

  it('keeps one author', () => {
    expect(numbers(filterPullRequests(listed, { author: 'octocat' }))).toEqual([1, 2]);
  });

  it('applies every filter together', () => {
    expect(
      numbers(filterPullRequests(listed, { readiness: 'ready', repository: 'acme/api' }))
    ).toEqual([3]);
    expect(
      numbers(filterPullRequests(listed, { repository: 'acme/zone', author: 'hubot' }))
    ).toEqual([]);
  });
});

describe('filter options', () => {
  const listed = [
    pullRequest(1, 'ready', { repository: 'acme/zone', author: 'octocat' }),
    pullRequest(2, 'ready', { repository: 'acme/api', author: 'octocat' }),
    pullRequest(3, 'ready', { repository: 'acme/zone', author: null }),
    pullRequest(4, 'ready', { repository: 'acme/zone', author: 'hubot' }),
  ];

  it('lists each repository once, sorted', () => {
    expect(repositoriesOf(listed)).toEqual(['acme/api', 'acme/zone']);
  });

  it('lists each known author once, sorted', () => {
    expect(authorsOf(listed)).toEqual(['hubot', 'octocat']);
  });

  it('lists nothing for no pull requests', () => {
    expect(repositoriesOf([])).toEqual([]);
    expect(authorsOf([])).toEqual([]);
  });
});
