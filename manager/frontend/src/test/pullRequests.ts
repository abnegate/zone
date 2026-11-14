export const HEAD = 'a'.repeat(40);
export const BASE = 'b'.repeat(40);
export const SOURCE_ID = '7f1c2b4e-3d5a-4c6b-8e9f-0a1b2c3d4e5f';

export const wirePullRequest = {
  source_id: SOURCE_ID,
  repository: 'acme/zone',
  repository_url: 'https://github.com/acme/zone',
  number: 42,
  title: 'Add the pull requests page',
  url: 'https://github.com/acme/zone/pull/42',
  author: 'octocat',
  updated_at: '2026-11-13T10:00:00Z',
  draft: false,
  head: HEAD,
  head_ref: 'feat/pull-requests',
  base_ref: 'main',
  readiness: 'not_ready',
  blockers: [{ code: 'checks_failed', detail: '1 check failed' }],
  checks: {
    state: 'failure',
    complete: true,
    counts: {
      total: 3,
      passed: 2,
      failed: 1,
      running: 0,
      queued: 0,
      in_progress: 0,
      unknown: 0,
    },
  },
  check_details: [
    {
      kind: 'check_run',
      id: 9001,
      name: 'test',
      state: 'failure',
      url: 'https://github.com/acme/zone/actions/runs/1/job/9001',
      updated_at: '2026-11-13T09:58:00Z',
    },
  ],
  reviews: [
    {
      reviewer: 'greptile',
      comment_id: 501,
      url: 'https://github.com/acme/zone/pull/42#issuecomment-501',
      confidence: { score: 5, scale: 5 },
      reviewed_commit: HEAD,
      created_at: '2026-11-13T09:50:00Z',
    },
  ],
  review_current: true,
  unresolved_threads: 0,
  threads_complete: true,
  comments_complete: true,
  assessed_at: '2026-11-13T10:00:05Z',
  rank: 0,
};

export const wireSourceStatus = {
  source_id: SOURCE_ID,
  name: 'Zone',
  repository: 'acme/zone',
  repository_url: 'https://github.com/acme/zone',
  freshness: 'refreshed',
  generated_at: '2026-11-13T10:00:05Z',
  warning: null,
};

export const wirePullRequests = {
  generated_at: '2026-11-13T10:00:05Z',
  stale: false,
  partial: false,
  ttl_secs: 60,
  refresh_floor_secs: 10,
  counts: { ready: 0, in_progress: 0, not_ready: 1, total: 1 },
  sources: [wireSourceStatus],
  pull_requests: [wirePullRequest],
  warnings: [],
};

export const wireDetail = {
  pull_request: wirePullRequest,
  threads: [
    {
      id: 'PRRT_kwDOA',
      path: 'src/main.rs',
      line: 12,
      outdated: false,
      url: 'https://github.com/acme/zone/pull/42#discussion_r1',
      comments: [
        {
          id: 1,
          author: 'reviewer',
          body: 'Handle the error here.',
          url: 'https://github.com/acme/zone/pull/42#discussion_r1',
          created_at: '2026-11-13T09:55:00Z',
        },
      ],
    },
  ],
  comments: [
    {
      reviewer: 'greptile',
      body: 'Confidence score: 5/5',
      url: 'https://github.com/acme/zone/pull/42#issuecomment-501',
      created_at: '2026-11-13T09:50:00Z',
    },
  ],
};

export const wireDiff = {
  number: 42,
  head: HEAD,
  base: BASE,
  complete: true,
  warning: null,
  files: [
    {
      path: 'src/main.rs',
      previous_path: null,
      status: 'modified',
      additions: 1,
      deletions: 1,
      changes: 2,
      binary: false,
      truncated: false,
      blob_url: `https://github.com/acme/zone/blob/${HEAD}/src/main.rs`,
      hunks: [
        {
          header: '@@ -1,2 +1,2 @@',
          old_start: 1,
          old_lines: 2,
          new_start: 1,
          new_lines: 2,
          lines: [
            { kind: 'context', content: 'fn main() {', old_line: 1, new_line: 1 },
            { kind: 'deletion', content: '    old();', old_line: 2, new_line: null },
            { kind: 'addition', content: '    new();', old_line: null, new_line: 2 },
          ],
        },
      ],
    },
  ],
};

export const wireCommits = {
  number: 42,
  head: HEAD,
  base: BASE,
  complete: true,
  commits: [
    {
      sha: HEAD,
      message: 'feat(console): add the pull requests page',
      author_name: 'Octo Cat',
      author_login: 'octocat',
      authored_at: '2026-11-13T09:00:00Z',
      url: `https://github.com/acme/zone/commit/${HEAD}`,
    },
  ],
};

export const wireLog = {
  job_id: 9001,
  run_id: 1,
  head: HEAD,
  text: 'error: test failed',
  truncated: false,
  fetched_at: '2026-11-13T10:01:00Z',
};

export const wireMerged = {
  number: 42,
  sha: 'c'.repeat(40),
  admin: false,
  url: 'https://github.com/acme/zone/pull/42',
};
