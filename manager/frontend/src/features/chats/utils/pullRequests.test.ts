import { describe, expect, it } from 'bun:test';
import type { ChatPullRequest, PullRequestStatus } from '../types';
import {
  mergeReading,
  pullRequestKind,
  pullRequestName,
  pullRequestTitle,
  threadsLabel,
} from './pullRequests';

const STATUS: PullRequestStatus = {
  title: 'Add widgets',
  state: 'open',
  draft: false,
  additions: 1,
  deletions: 1,
  changed_files: 1,
  checks: 'passing',
  mergeable: 'clean',
  auto_merge: { enabled: false, unavailable: null },
  draft_unavailable: null,
  threads: null,
  observed_at: '2026-11-05T10:00:00Z',
};

function pullRequest(status: Partial<PullRequestStatus> | null): ChatPullRequest {
  return {
    id: 'pr-1',
    provider: 'github',
    owner: 'acme',
    repo: 'widgets',
    number: 12,
    url: 'https://github.com/acme/widgets/pull/12',
    title: null,
    dismissed: false,
    created_at: '2026-11-05T10:00:00Z',
    status: status === null ? null : { ...STATUS, ...status },
    writable: true,
    credentialed: true,
  };
}

describe('pull request kinds', () => {
  it('reads a settled state before a draft, and a draft before its checks', () => {
    expect(pullRequestKind(pullRequest(null))).toBe('unknown');
    expect(pullRequestKind(pullRequest({ state: 'merged', draft: true }))).toBe('merged');
    expect(pullRequestKind(pullRequest({ state: 'closed' }))).toBe('closed');
    expect(pullRequestKind(pullRequest({ draft: true, checks: 'failing' }))).toBe('draft');
    expect(pullRequestKind(pullRequest({ checks: 'failing' }))).toBe('failing');
    expect(pullRequestKind(pullRequest({ checks: 'pending' }))).toBe('pending');
    expect(pullRequestKind(pullRequest({ checks: 'passing' }))).toBe('passing');
    expect(pullRequestKind(pullRequest({ checks: 'none' }))).toBe('open');
  });

  it('dots only an open, ready pull request whose mergeability is known', () => {
    expect(mergeReading(pullRequest({ mergeable: 'clean' }).status)?.signal).toBe('ready');
    expect(mergeReading(pullRequest({ mergeable: 'behind' }).status)?.signal).toBe('waiting');
    expect(mergeReading(pullRequest({ mergeable: 'conflicted' }).status)?.signal).toBe(
      'conflicted'
    );
    expect(mergeReading(pullRequest({ mergeable: 'unknown' }).status)).toBeNull();
    expect(mergeReading(pullRequest({ draft: true }).status)).toBeNull();
    expect(mergeReading(pullRequest({ state: 'merged' }).status)).toBeNull();
    expect(mergeReading(null)).toBeNull();
  });

  it('names one by its repository and number when GitHub gave no title', () => {
    expect(pullRequestTitle(pullRequest(null))).toBe('Pull request #12');
    expect(pullRequestName(pullRequest(null))).toBe('widgets #12');
  });
});

describe('threadsLabel', () => {
  it('names open and resolved review threads apart', () => {
    expect(threadsLabel({ open: 2, resolved: 3 })).toBe('2 open and 3 resolved review threads');
  });

  it('names a lone review thread in the singular', () => {
    expect(threadsLabel({ open: 1, resolved: 0 })).toBe('1 open and 0 resolved review thread');
  });
});
