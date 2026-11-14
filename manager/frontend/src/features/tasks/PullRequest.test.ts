import { describe, expect, it } from 'bun:test';
import { PullRequest } from './PullRequest';

describe('PullRequest', () => {
  it('is absent when the task has neither an address nor a status', () => {
    expect(PullRequest.of({ pr_url: null, pr_status: null })).toBeNull();
  });

  it('numbers a GitHub pull request from its address', () => {
    const pullRequest = PullRequest.of({
      pr_url: 'https://github.com/zone/app/pull/220',
      pr_status: 'open',
    });
    expect(pullRequest?.number).toBe('220');
    expect(pullRequest?.address).toBe('https://github.com/zone/app/pull/220');
    expect(pullRequest?.label).toBe('PR #220 · open');
  });

  it('numbers a GitLab merge request from its address', () => {
    const pullRequest = PullRequest.of({
      pr_url: 'http://git.internal/zone/app/-/merge_requests/7',
      pr_status: 'merged',
    });
    expect(pullRequest?.label).toBe('PR #7 · merged');
  });

  it('numbers the pull request, not a later number in its address', () => {
    expect(
      PullRequest.of({
        pr_url: 'https://github.com/zone/app/pull/220/files/3',
        pr_status: 'open',
      })?.label
    ).toBe('PR #220 · open');
    expect(
      PullRequest.of({
        pr_url: 'http://git.internal/zone/app/-/merge_requests/7/diffs/2',
        pr_status: 'open',
      })?.label
    ).toBe('PR #7 · open');
    expect(
      PullRequest.of({
        pr_url: 'https://bitbucket.org/zone/app/pull-requests/12/commits/4',
        pr_status: 'open',
      })?.label
    ).toBe('PR #12 · open');
  });

  it('reads the request, not an owner or group that happens to be named like one', () => {
    expect(
      PullRequest.of({ pr_url: 'https://github.com/pull/2048/pull/7', pr_status: 'open' })?.label
    ).toBe('PR #7 · open');
  });

  it('falls back to the last number for a host with no request segment', () => {
    expect(
      PullRequest.of({ pr_url: 'https://review.example.com/c/app/+/42/3', pr_status: 'open' })
        ?.label
    ).toBe('PR #3 · open');
  });

  it('reads the number of a request whose address goes on past it', () => {
    const pullRequest = PullRequest.of({
      pr_url: 'https://github.com/zone/2048/pull/220/files?diff=split#top',
      pr_status: 'closed',
    });
    expect(pullRequest?.label).toBe('PR #220 · closed');
  });

  it('leaves the number out of an address that has none', () => {
    const pullRequest = PullRequest.of({
      pr_url: 'https://review.example.com/changes/latest',
      pr_status: 'open',
    });
    expect(pullRequest?.number).toBeNull();
    expect(pullRequest?.address).toBe('https://review.example.com/changes/latest');
    expect(pullRequest?.label).toBe('PR · open');
  });

  it('keeps the status of a pull request that has no address yet', () => {
    const pullRequest = PullRequest.of({ pr_url: null, pr_status: 'pending' });
    expect(pullRequest?.address).toBeNull();
    expect(pullRequest?.label).toBe('PR · pending');
  });

  it('names a pull request that has an address but no status', () => {
    const pullRequest = PullRequest.of({
      pr_url: 'https://github.com/zone/app/pull/9',
      pr_status: null,
    });
    expect(pullRequest?.label).toBe('PR #9');
  });

  for (const address of [
    'javascript:alert(document.cookie)//pull/7',
    'data:text/html,pull/7',
    'ftp://example.com/pull/7',
    'pull/7',
    '',
  ]) {
    it(`drops the address and the number of ${address || 'an empty address'}`, () => {
      const pullRequest = PullRequest.of({ pr_url: address, pr_status: 'open' });
      expect(pullRequest?.address).toBeNull();
      expect(pullRequest?.number).toBeNull();
      expect(pullRequest?.label).toBe('PR · open');
      expect(PullRequest.of({ pr_url: address, pr_status: null })).toBeNull();
    });
  }
});
