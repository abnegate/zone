import { describe, expect, it } from 'bun:test';
import {
  wireCommits,
  wireDetail,
  wireDiff,
  wireLog,
  wireMerged,
  wirePullRequest,
  wirePullRequests,
} from '../../test/pullRequests';
import {
  BLOCKER_CODES,
  BlockerSchema,
  BotCommentSchema,
  CheckDetailSchema,
  CheckLogResponseSchema,
  CommitSchema,
  CommitsResponseSchema,
  DiffFileSchema,
  DiffResponseSchema,
  MergedResponseSchema,
  MergedSchema,
  MergeRequestSchema,
  PullRequestDetailSchema,
  PullRequestSchema,
  PullRequestsResponseSchema,
  RefusalSchema,
  ReviewSummarySchema,
  ThreadCommentSchema,
  ThreadSchema,
} from './schemas';

describe('pull request list', () => {
  it('accepts the listing the server sends', () => {
    const listing = PullRequestsResponseSchema.parse(wirePullRequests);
    expect(listing.counts).toEqual({ ready: 0, in_progress: 0, not_ready: 1, total: 1 });
    expect(listing.pull_requests[0]).toEqual(wirePullRequest as never);
    expect(listing.sources[0].freshness).toBe('refreshed');
  });

  it('reads a listing without warnings as having none', () => {
    const { warnings: _, ...listing } = wirePullRequests;
    expect(PullRequestsResponseSchema.parse(listing).warnings).toEqual([]);
  });

  it('keeps the warnings it is given', () => {
    const listing = { ...wirePullRequests, partial: true, warnings: ['acme/api: rate limited'] };
    expect(PullRequestsResponseSchema.parse(listing).warnings).toEqual(['acme/api: rate limited']);
  });

  it('rejects a listing with no pull requests field', () => {
    const { pull_requests: _, ...listing } = wirePullRequests;
    expect(PullRequestsResponseSchema.safeParse(listing).success).toBe(false);
  });
});

describe('pull request row', () => {
  it.each(['ready', 'in_progress', 'not_ready'])('keeps readiness %s', (readiness) => {
    expect(PullRequestSchema.parse({ ...wirePullRequest, readiness }).readiness).toBe(readiness);
  });

  it.each(['merging', 42, null])('reads unknown readiness %p as not ready', (readiness) => {
    expect(PullRequestSchema.parse({ ...wirePullRequest, readiness }).readiness).toBe('not_ready');
  });

  it.each(['success', 'failure', 'pending', 'none', 'unknown'])(
    'keeps check summary state %s',
    (state) => {
      const checks = { ...wirePullRequest.checks, state };
      expect(PullRequestSchema.parse({ ...wirePullRequest, checks }).checks.state).toBe(state);
    }
  );

  it('reads an unknown check summary state as unknown', () => {
    const checks = { ...wirePullRequest.checks, state: 'neutral' };
    expect(PullRequestSchema.parse({ ...wirePullRequest, checks }).checks.state).toBe('unknown');
  });

  it.each(['workflow', 'check_run', 'status'])('keeps check kind %s', (kind) => {
    const check_details = [{ ...wirePullRequest.check_details[0], kind }];
    const row = PullRequestSchema.parse({ ...wirePullRequest, check_details });
    expect(row.check_details[0].kind).toBe(kind);
  });

  it('accepts a check with no Actions job id', () => {
    const check_details = [
      { ...wirePullRequest.check_details[0], kind: 'status', id: null },
      { ...wirePullRequest.check_details[0], kind: 'check_run', id: null },
    ];
    const row = PullRequestSchema.parse({ ...wirePullRequest, check_details });
    expect(row.check_details.map((check) => check.id)).toEqual([null, null]);
  });

  it('rejects a check whose job id is not a number', () => {
    const check_details = [{ ...wirePullRequest.check_details[0], id: '9001' }];
    expect(PullRequestSchema.safeParse({ ...wirePullRequest, check_details }).success).toBe(false);
  });

  it('reads an unknown check kind and state as unknown', () => {
    const check_details = [
      { ...wirePullRequest.check_details[0], kind: 'deployment', state: 'skipped' },
    ];
    const [check] = PullRequestSchema.parse({ ...wirePullRequest, check_details }).check_details;
    expect(check.kind).toBe('unknown');
    expect(check.state).toBe('unknown');
  });

  it('accepts a pull request with no author, head, review link or confidence', () => {
    const reviews = [
      { ...wirePullRequest.reviews[0], url: null, confidence: null, reviewed_commit: null },
    ];
    const row = PullRequestSchema.parse({ ...wirePullRequest, author: null, head: null, reviews });
    expect(row.author).toBeNull();
    expect(row.head).toBeNull();
    expect(row.reviews[0].confidence).toBeNull();
  });

  it('rejects a negative check count', () => {
    const checks = {
      ...wirePullRequest.checks,
      counts: { ...wirePullRequest.checks.counts, failed: -1 },
    };
    expect(PullRequestSchema.safeParse({ ...wirePullRequest, checks }).success).toBe(false);
  });

  it('rejects a pull request number that is not a positive integer', () => {
    for (const number of [0, 1.5, '42']) {
      expect(PullRequestSchema.safeParse({ ...wirePullRequest, number }).success).toBe(false);
    }
  });
});

describe('pull request detail', () => {
  it('accepts threads and bot comments', () => {
    const detail = PullRequestDetailSchema.parse(wireDetail);
    expect(detail.threads[0].comments[0].body).toBe('Handle the error here.');
    expect(detail.comments[0].reviewer).toBe('greptile');
  });

  it('accepts a thread off the diff with node identifiers on its comments', () => {
    const thread = {
      ...wireDetail.threads[0],
      path: null,
      line: null,
      url: null,
      comments: [{ ...wireDetail.threads[0].comments[0], id: 'PRRC_kwDOA' }],
    };
    const detail = PullRequestDetailSchema.parse({ ...wireDetail, threads: [thread] });
    expect(detail.threads[0].line).toBeNull();
    expect(detail.threads[0].comments[0].id).toBe('PRRC_kwDOA');
  });
});

describe('diff', () => {
  it('accepts files, hunks and numbered lines', () => {
    const { diff } = DiffResponseSchema.parse({ diff: wireDiff });
    const [file] = diff.files;
    expect(file.status).toBe('modified');
    expect(file.hunks[0].lines.map((line) => line.kind)).toEqual([
      'context',
      'deletion',
      'addition',
    ]);
    expect(file.hunks[0].lines[1].new_line).toBeNull();
  });

  it('reads an unknown file status as unknown and an unknown line kind as meta', () => {
    const [file] = wireDiff.files;
    const hunk = {
      ...file.hunks[0],
      lines: [{ kind: 'conflict', content: '<<<<<<<', old_line: null, new_line: null }],
    };
    const diff = DiffResponseSchema.parse({
      diff: { ...wireDiff, files: [{ ...file, status: 'typechange', hunks: [hunk] }] },
    }).diff;
    expect(diff.files[0].status).toBe('unknown');
    expect(diff.files[0].hunks[0].lines[0].kind).toBe('meta');
  });

  it('keeps the warning on a diff GitHub would not render whole', () => {
    const warning = 'GitHub would not render the whole diff; showing per-file patches.';
    const { diff } = DiffResponseSchema.parse({
      diff: { ...wireDiff, complete: false, warning },
    });
    expect(diff.complete).toBe(false);
    expect(diff.warning).toBe(warning);
  });
});

describe('commits, logs and merges', () => {
  it('accepts the commit list', () => {
    const { commits } = CommitsResponseSchema.parse({ commits: wireCommits });
    expect(commits.commits[0].author_login).toBe('octocat');
  });

  it('accepts a commit whose author has no GitHub login', () => {
    const commit = { ...wireCommits.commits[0], author_login: null, authored_at: null };
    const { commits } = CommitsResponseSchema.parse({
      commits: { ...wireCommits, commits: [commit] },
    });
    expect(commits.commits[0].author_login).toBeNull();
  });

  it('accepts a check log', () => {
    expect(CheckLogResponseSchema.parse({ log: wireLog }).log.text).toBe('error: test failed');
  });

  it('accepts a merge receipt with or without a merge commit', () => {
    expect(MergedResponseSchema.parse({ merged: wireMerged }).merged.sha).toBe('c'.repeat(40));
    expect(MergedResponseSchema.parse({ merged: { ...wireMerged, sha: null } }).merged.sha).toBe(
      null
    );
  });
});

describe('merge request', () => {
  it.each(['merge', 'squash', 'rebase'])('carries method %s', (method) => {
    expect(MergeRequestSchema.parse({ expected_head: 'a', method })).toEqual({
      expected_head: 'a',
      method: method as never,
    });
  });

  it('leaves the method to the server when none is given', () => {
    expect(MergeRequestSchema.parse({ expected_head: 'a' })).toEqual({ expected_head: 'a' });
  });

  it('refuses a merge without an expected head or with an unknown method', () => {
    expect(MergeRequestSchema.safeParse({ expected_head: '' }).success).toBe(false);
    expect(MergeRequestSchema.safeParse({ expected_head: 'a', method: 'fast' }).success).toBe(
      false
    );
  });
});

describe('refusal', () => {
  it('reads a moved head with the head it moved to', () => {
    const refusal = RefusalSchema.parse({
      success: false,
      error: 'The pull request head moved',
      code: 'head_moved',
      current_head: 'd'.repeat(40),
    });
    expect(refusal).toEqual({
      error: 'The pull request head moved',
      code: 'head_moved',
      current_head: 'd'.repeat(40),
    });
  });

  it('reads a not-ready refusal with its blockers', () => {
    const refusal = RefusalSchema.parse({
      error: 'Not ready',
      code: 'not_ready',
      blockers: wirePullRequest.blockers,
    });
    expect(refusal.blockers).toEqual(wirePullRequest.blockers);
  });

  it.each(['conflict', 'protected', 'merged', 'closed'])('reads a %s refusal', (code) => {
    expect(RefusalSchema.parse({ error: 'Refused', code }).code).toBe(code as never);
  });

  it('rejects a conflict that carries no refusal code', () => {
    expect(RefusalSchema.safeParse({ error: 'Refused' }).success).toBe(false);
    expect(RefusalSchema.safeParse({ error: 'Refused', code: 'tag_exists' }).success).toBe(false);
  });
});

describe('blocker codes', () => {
  it.each([...BLOCKER_CODES])('keeps the known code %s', (code) => {
    expect(BlockerSchema.parse({ code, detail: 'Blocked' }).code).toBe(code);
  });

  it('keeps a code newer than this client as sent', () => {
    expect(BlockerSchema.parse({ code: 'merge_queue_busy', detail: 'Queued' }).code).toBe(
      'merge_queue_busy'
    );
  });
});

describe('pull request links', () => {
  const unsafe = ['javascript:alert(1)', 'data:text/html,<script>alert(1)</script>', '/relative'];
  const [thread] = wireDetail.threads;
  const [comment] = thread.comments;
  const [botComment] = wireDetail.comments;
  const [file] = wireDiff.files;
  const [commit] = wireCommits.commits;
  const [check] = wirePullRequest.check_details;
  const [review] = wirePullRequest.reviews;

  it.each(unsafe)('refuses a pull request or repository url of %s', (url) => {
    expect(PullRequestSchema.safeParse({ ...wirePullRequest, url }).success).toBe(false);
    expect(PullRequestSchema.safeParse({ ...wirePullRequest, repository_url: url }).success).toBe(
      false
    );
    expect(
      PullRequestsResponseSchema.safeParse({
        ...wirePullRequests,
        sources: [{ ...wirePullRequests.sources[0], repository_url: url }],
      }).success
    ).toBe(false);
  });

  it.each(unsafe)('refuses a check or review url of %s', (url) => {
    expect(CheckDetailSchema.safeParse({ ...check, url }).success).toBe(false);
    expect(ReviewSummarySchema.safeParse({ ...review, url }).success).toBe(false);
  });

  it.each(unsafe)('refuses a thread, comment or bot review url of %s', (url) => {
    expect(ThreadSchema.safeParse({ ...thread, url }).success).toBe(false);
    expect(ThreadCommentSchema.safeParse({ ...comment, url }).success).toBe(false);
    expect(BotCommentSchema.safeParse({ ...botComment, url }).success).toBe(false);
  });

  it.each(unsafe)('refuses a file, commit or merge url of %s', (url) => {
    expect(DiffFileSchema.safeParse({ ...file, blob_url: url }).success).toBe(false);
    expect(CommitSchema.safeParse({ ...commit, url }).success).toBe(false);
    expect(MergedSchema.safeParse({ ...wireMerged, url }).success).toBe(false);
  });

  it('accepts a missing check, review or thread url', () => {
    expect(CheckDetailSchema.parse({ ...check, url: null }).url).toBeNull();
    expect(ReviewSummarySchema.parse({ ...review, url: null }).url).toBeNull();
    expect(ThreadSchema.parse({ ...thread, url: null }).url).toBeNull();
  });
});
