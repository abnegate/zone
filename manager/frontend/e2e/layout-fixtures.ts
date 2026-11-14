import type { Page } from '@playwright/test';
import type { SourceStatus } from '../src/features/github/sourceStatus';
import {
  wireCommits,
  wireDetail,
  wireDiff,
  wireLog,
  wirePullRequest,
} from '../src/test/pullRequests';
import type { PullRequest } from '../src/features/pull-requests/types';
import type {
  OptionsResponse,
  PipelineRun,
  Release,
  ReleasesResponse,
  SourceOptions,
} from '../src/features/releases/types';
import type {
  AiSettings,
  WorkspaceAiSettings,
} from '../src/features/settings/workspace/types';
import {
  adminPermissions,
  createMockJwt,
  mockAdminUser,
  routeApi,
} from './test-utils';

const mockOrganization = {
  id: 'org-1',
  name: 'Acme Corp',
  slug: 'acme-corp',
  description: null,
  is_active: true,
  created_at: '2024-01-01T00:00:00Z',
  updated_at: '2024-01-01T00:00:00Z',
};

const mockWorkspace = {
  id: 'ws-1',
  name: 'Engineering',
  slug: 'engineering',
  description: null,
  organization_id: 'org-1',
  is_active: true,
  created_at: '2024-01-01T00:00:00Z',
  updated_at: '2024-01-01T00:00:00Z',
};

const mockAiSettings: AiSettings = {
  provider: 'self_hosted',
  has_litellm_key: false,
  litellm_host: 'http://ollama:11434',
  has_openai_api_key: false,
  openai_base_url: null,
  has_anthropic_api_key: false,
  anthropic_base_url: null,
  has_runpod_api_key: false,
  bedrock_region: null,
  bedrock_use_iam_role: false,
  has_bedrock_credentials: false,
  model_fast: 'llama3.2:latest',
  model_reasoning: 'llama3.2:latest',
  model_embedding: null,
  model_image: null,
  model_video: null,
  model_audio: null,
  completions_routed: true,
};

const workspaceAiSettingsPath = /\/workspaces\/[^/]+\/settings\/ai$/;

export function workspaceAiSettingsResponse(
  settings: AiSettings = mockAiSettings,
): WorkspaceAiSettings {
  return {
    ...settings,
    bedrock_use_iam_role: null,
    overrides: false,
    inherits_provider: true,
    organization_keys: { litellm: false, openai: false, anthropic: false },
  };
}

export function aiSettingsResponse(
  path: string,
  settings: AiSettings = mockAiSettings,
): AiSettings | WorkspaceAiSettings {
  return workspaceAiSettingsPath.test(path)
    ? workspaceAiSettingsResponse(settings)
    : settings;
}

const mockMembers = [
  {
    user_id: mockAdminUser.id,
    email: 'admin@example.com',
    display_name: 'Admin User',
    role: 'admin',
  },
  {
    user_id: 'user-2',
    email: 'dev@example.com',
    display_name: 'Developer',
    role: 'member',
  },
  {
    user_id: 'user-3',
    email: 'designer@example.com',
    display_name: 'Designer',
    role: 'member',
  },
].map((member, index) => ({
  ...member,
  id: `member-${index}`,
  organization_id: 'org-1',
  workspace_id: 'ws-1',
  is_active: true,
  joined_at: '2024-01-01T00:00:00Z',
}));

export const mockModels = [
  {
    name: 'llama3.2:latest',
    size: 4700000000,
    modified_at: '2024-01-15T10:30:00Z',
    details: { family: 'llama', description: 'Meta Llama 3.2' },
  },
  {
    name: 'mistral:7b',
    size: 4100000000,
    modified_at: '2024-01-14T08:00:00Z',
    details: { family: 'mistral', description: 'Mistral 7B' },
  },
  {
    name: 'codellama:13b',
    size: 7300000000,
    modified_at: '2024-01-13T14:20:00Z',
    details: { family: 'llama', description: 'Code Llama 13B' },
  },
];

export const mockChats = [
  {
    id: 'chat-1',
    title: 'Code Review Discussion',
    model: 'llama3.2:latest',
    model_name: 'llama3.2:latest',
    archived: false,
    agent_enabled: false,
    created_at: '2024-01-15T10:00:00Z',
    updated_at: '2024-01-15T12:30:00Z',
    message_count: 12,
  },
  {
    id: 'chat-2',
    title: 'API Design Planning',
    model: 'mistral:7b',
    model_name: 'mistral:7b',
    archived: false,
    agent_enabled: false,
    created_at: '2024-01-14T09:00:00Z',
    updated_at: '2024-01-14T16:45:00Z',
    message_count: 8,
  },
];

export const mockProjects = [
  {
    id: 'proj-1',
    name: 'Zone Platform',
    description: 'Main platform development',
    status: 'active',
    github_repo_url: null,
    source_id: null,
    created_at: '2024-01-01T00:00:00Z',
    updated_at: '2024-01-15T10:00:00Z',
  },
  {
    id: 'proj-2',
    name: 'Mobile App',
    description: 'iOS and Android applications',
    status: 'active',
    github_repo_url: null,
    source_id: null,
    created_at: '2024-01-05T00:00:00Z',
    updated_at: '2024-01-14T15:00:00Z',
  },
];

export const tasksNow = new Date('2024-01-16T12:00:00Z');

const taskDefaults = {
  workspace_id: 'ws-1',
  acceptance_criteria: null,
  priority: null,
  model_name: null,
  dependencies: [] as string[],
  started_at: null,
  completed_at: null,
  is_agentic: false,
  github_repo_url: null,
  source_id: null,
  source_ids: [],
  queued_at: null,
  worker_id: null,
  pr_url: null,
  branch_name: null,
  pr_status: null,
  pr_created_at: null,
};

export const mockTasks = [
  {
    id: 'task-1',
    project_ids: ['proj-1'],
    workspace_id: 'ws-1',
    title: 'Implement authentication',
    description: 'Add OAuth2 support for the platform',
    acceptance_criteria: 'Users can login with Google and GitHub',
    status: 'in_progress',
    priority: 1,
    model_name: 'llama3.2:latest',
    dependencies: ['task-2'],
    created_at: '2024-01-10T00:00:00Z',
    updated_at: '2024-01-15T08:00:00Z',
    started_at: '2024-01-12T09:00:00Z',
    completed_at: null,
    is_agentic: true,
    github_repo_url: null,
    source_id: null,
    source_ids: [],
    queued_at: '2024-01-12T08:30:00Z',
    worker_id: 'worker-1',
    pr_url: 'https://github.com/example/repo/pull/61',
    branch_name: 'feature/auth',
    pr_status: 'open',
    pr_created_at: '2024-01-15T08:00:00Z',
  },
  {
    id: 'task-2',
    project_ids: ['proj-1'],
    workspace_id: 'ws-1',
    title: 'Setup CI/CD pipeline',
    description: 'Configure GitHub Actions for automated testing',
    acceptance_criteria: 'All tests run on PR',
    status: 'complete',
    priority: 2,
    model_name: 'mistral:7b',
    dependencies: [],
    created_at: '2024-01-08T00:00:00Z',
    updated_at: '2024-01-12T16:00:00Z',
    started_at: '2024-01-09T10:00:00Z',
    completed_at: '2024-01-12T16:00:00Z',
    is_agentic: false,
    github_repo_url: null,
    source_id: null,
    source_ids: [],
    queued_at: '2024-01-09T09:00:00Z',
    worker_id: null,
    pr_url: 'https://github.com/example/repo/pull/42',
    branch_name: 'feature/ci-cd',
    pr_status: 'merged',
    pr_created_at: '2024-01-11T14:00:00Z',
  },
  {
    ...taskDefaults,
    id: 'task-3',
    project_ids: ['proj-2'],
    title: 'Ship offline mode for the mobile app',
    description: 'Cache the last synced workspace so the app opens without a connection',
    status: 'queued',
    priority: 2,
    is_agentic: true,
    dependencies: ['task-1', 'task-7', 'task-5', 'task-6', 'task-archived'],
    created_at: '2024-01-15T09:00:00Z',
    updated_at: '2024-01-16T09:30:00Z',
    queued_at: '2024-01-16T09:30:00Z',
  },
  {
    ...taskDefaults,
    id: 'task-4',
    project_ids: ['proj-1'],
    title: 'Users on Safari are sent back to the login page every time they sign in',
    title_pending: true,
    description: 'Users on Safari are sent back to the login page every time they sign in.',
    status: 'created',
    created_at: '2024-01-16T11:59:30Z',
    updated_at: '2024-01-16T11:59:30Z',
  },
  {
    ...taskDefaults,
    id: 'task-5',
    project_ids: ['proj-1', 'proj-2'],
    title:
      'Migrate the notification service from the legacy queue to the event bus without dropping in-flight messages',
    description: 'Move every producer and consumer to the event bus',
    status: 'blocked',
    priority: 1,
    is_agentic: true,
    dependencies: ['task-2'],
    created_at: '2024-01-11T00:00:00Z',
    updated_at: '2024-01-15T17:00:00Z',
    started_at: '2024-01-13T10:00:00Z',
    branch_name: 'feature/event-bus',
    pr_status: 'pending',
  },
  {
    ...taskDefaults,
    id: 'task-6',
    project_ids: ['proj-2'],
    title: 'Redesign the onboarding flow',
    description: 'Shorten sign-up to two screens',
    status: 'paused',
    priority: 3,
    created_at: '2024-01-09T00:00:00Z',
    updated_at: '2024-01-14T11:00:00Z',
    started_at: '2024-01-10T10:00:00Z',
  },
  {
    ...taskDefaults,
    id: 'task-7',
    project_ids: ['proj-1'],
    title: 'Rate limit the public API',
    description: 'Add per-key token buckets to every public endpoint',
    status: 'review',
    priority: 1,
    is_agentic: true,
    dependencies: ['task-1', 'task-2'],
    created_at: '2024-01-12T00:00:00Z',
    updated_at: '2024-01-16T10:00:00Z',
    started_at: '2024-01-14T09:00:00Z',
    branch_name: 'feature/rate-limit',
    pr_url: 'https://github.com/example/repo/pull/57',
    pr_status: 'open',
    pr_created_at: '2024-01-16T10:00:00Z',
  },
  {
    ...taskDefaults,
    id: 'task-8',
    project_ids: [],
    title: 'Evaluate a second vector database',
    description: 'Superseded by the decision to stay on pgvector',
    status: 'cancelled',
    created_at: '2024-01-05T00:00:00Z',
    updated_at: '2024-01-13T15:00:00Z',
  },
];

export const mockSources = [
  {
    id: 'src-1',
    name: 'Documentation',
    source_type: 'web',
    category: 'web',
    config: { url: 'https://docs.example.com' },
    description: 'Project documentation website',
    url: 'https://docs.example.com',
    is_active: true,
    last_verified_at: '2024-01-15T06:00:00Z',
    last_error: null,
    created_at: '2024-01-10T00:00:00Z',
    updated_at: '2024-01-15T06:00:00Z',
  },
  {
    id: 'src-2',
    name: 'Main Repository',
    source_type: 'github',
    category: 'file',
    config: { owner: 'example', repo: 'main-repo', branch: 'main' },
    description: 'Primary codebase',
    url: 'https://github.com/example/main-repo',
    is_active: true,
    last_verified_at: '2024-01-14T12:00:00Z',
    last_error: null,
    created_at: '2024-01-05T00:00:00Z',
    updated_at: '2024-01-14T12:00:00Z',
  },
];

const sourceTypes = [
  { id: 'github', name: 'GitHub', category: 'file', description: 'GitHub repository', enabled: true },
  { id: 'gitlab', name: 'GitLab', category: 'file', description: 'GitLab project', enabled: true },
  { id: 'filesystem', name: 'Filesystem', category: 'file', description: 'Local directory', enabled: true },
  { id: 'web', name: 'Web URL', category: 'web', description: 'Web page', enabled: true },
  { id: 'text', name: 'Text', category: 'text', description: 'Raw text', enabled: true },
  { id: 'ical', name: 'Calendar', category: 'calendar', description: 'iCal feed', enabled: false },
  { id: 'imap', name: 'Mail', category: 'mail', description: 'IMAP mailbox', enabled: false },
  { id: 'slack', name: 'Slack', category: 'chat', description: 'Slack workspace', enabled: false },
  { id: 'discord', name: 'Discord', category: 'chat', description: 'Discord server', enabled: false },
];

const mockSessions = [
  {
    id: 'sess-1',
    user_id: 'user-1',
    ip_address: '192.168.1.1',
    user_agent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)',
    device_info: 'Chrome on macOS',
    location: 'San Francisco, CA',
    created_at: '2024-01-15T08:00:00Z',
    last_active_at: '2024-01-15T12:00:00Z',
    expires_at: '2024-01-22T08:00:00Z',
    is_current: true,
  },
  {
    id: 'sess-2',
    user_id: 'user-1',
    ip_address: '192.168.1.2',
    user_agent: 'Mozilla/5.0 (iPhone; CPU iPhone OS 17_0)',
    device_info: 'Safari on iPhone',
    location: 'San Francisco, CA',
    created_at: '2024-01-14T06:00:00Z',
    last_active_at: '2024-01-14T18:00:00Z',
    expires_at: '2024-01-21T06:00:00Z',
    is_current: false,
  },
];

export const mockKnowledge = [
  {
    id: 'kb-1',
    workspace_id: 'ws-1',
    title: 'Getting Started Guide',
    content: 'Welcome to Zone! This guide will help you get started...',
    fetched_content:
      'Welcome to Zone! This guide will help you get started with the platform.',
    type: 'text',
    url: null,
    tags: ['guide', 'getting-started'],
    last_refreshed_at: '2024-01-15T10:00:00Z',
    created_at: '2024-01-10T00:00:00Z',
    updated_at: '2024-01-15T10:00:00Z',
  },
  {
    id: 'kb-2',
    workspace_id: 'ws-1',
    title: 'API Reference',
    content: 'Complete API documentation for Zone...',
    fetched_content:
      'Complete API documentation for Zone platform including all endpoints.',
    type: 'text',
    url: null,
    tags: ['api', 'documentation'],
    last_refreshed_at: '2024-01-14T15:00:00Z',
    created_at: '2024-01-08T00:00:00Z',
    updated_at: '2024-01-14T15:00:00Z',
  },
];

export const githubNow = new Date('2026-11-10T12:00:00Z');

const MINUTE = 60_000;

const MINUTES_IN_HOUR = 60;

function minutesBefore(minutes: number): string {
  return new Date(githubNow.getTime() - minutes * MINUTE).toISOString();
}

const GITHUB = 'https://github.com';

const zoneRepository = {
  source_id: wirePullRequest.source_id,
  name: 'Zone',
  repository: 'acme/zone',
  repository_url: `${GITHUB}/acme/zone`,
};

const consoleRepository = {
  source_id: '9b8c7d6e-5f4a-4b3c-8d2e-1f0a9b8c7d6e',
  name: 'Console',
  repository: 'acme/console',
  repository_url: `${GITHUB}/acme/console`,
};

const cratesRepository = {
  source_id: '4d5e6f70-8192-4a3b-9c4d-5e6f708192a3',
  name: 'Crates',
  repository: 'acme/crates',
  repository_url: `${GITHUB}/acme/crates`,
};

type Repository = typeof zoneRepository;

function sourceStatus(repository: Repository): SourceStatus {
  return {
    ...repository,
    freshness: 'cached',
    generated_at: minutesBefore(1),
    warning: null,
  };
}

const checkCounts = {
  total: 3,
  passed: 3,
  failed: 0,
  running: 0,
  queued: 0,
  in_progress: 0,
  unknown: 0,
};

export const readyPullRequest: PullRequest = {
  ...wirePullRequest,
  source_id: consoleRepository.source_id,
  repository: consoleRepository.repository,
  repository_url: consoleRepository.repository_url,
  number: 18,
  title: 'Add the release wizard',
  url: `${consoleRepository.repository_url}/pull/18`,
  author: 'hubot',
  updated_at: minutesBefore(25),
  head: 'd'.repeat(40),
  head_ref: 'feat/release-wizard',
  readiness: 'ready',
  blockers: [],
  checks: { state: 'success', complete: true, counts: checkCounts },
  check_details: [],
  reviews: [],
  review_current: true,
  assessed_at: minutesBefore(1),
  rank: 0,
};

export const inProgressPullRequest: PullRequest = {
  ...wirePullRequest,
  number: 57,
  title: 'Rate limit the public API',
  url: `${zoneRepository.repository_url}/pull/57`,
  updated_at: minutesBefore(40),
  head: 'e'.repeat(40),
  head_ref: 'feat/rate-limit',
  readiness: 'in_progress',
  blockers: [{ code: 'checks_running', detail: '1 check is still running' }],
  checks: {
    state: 'pending',
    complete: true,
    counts: { ...checkCounts, passed: 2, running: 1 },
  },
  check_details: [],
  reviews: [],
  assessed_at: minutesBefore(1),
  rank: 1,
};

export const notReadyPullRequest: PullRequest = {
  ...wirePullRequest,
  updated_at: minutesBefore(70),
  readiness: 'not_ready',
  blockers: [
    { code: 'checks_failed', detail: '1 check failed' },
    { code: 'threads_unresolved', detail: '1 review thread is unresolved' },
  ],
  checks: {
    state: 'failure',
    complete: true,
    counts: { ...checkCounts, passed: 2, failed: 1 },
  },
  check_details: wirePullRequest.check_details.map((check) => ({
    ...check,
    kind: 'check_run',
    state: 'failure',
  })),
  reviews: wirePullRequest.reviews.map((review) => ({
    ...review,
    created_at: minutesBefore(80),
  })),
  unresolved_threads: 1,
  assessed_at: minutesBefore(1),
  rank: 2,
};

const mockPullRequests = [readyPullRequest, inProgressPullRequest, notReadyPullRequest];

function pullRequestListing(pullRequests: readonly PullRequest[], sources: Repository[]) {
  return {
    generated_at: minutesBefore(1),
    stale: false,
    partial: false,
    ttl_secs: 60,
    refresh_floor_secs: 10,
    counts: {
      ready: pullRequests.filter((pullRequest) => pullRequest.readiness === 'ready').length,
      in_progress: pullRequests.filter((pullRequest) => pullRequest.readiness === 'in_progress')
        .length,
      not_ready: pullRequests.filter((pullRequest) => pullRequest.readiness === 'not_ready').length,
      total: pullRequests.length,
    },
    sources: sources.map(sourceStatus),
    pull_requests: pullRequests,
    warnings: [],
  };
}

export const emptyPullRequests = pullRequestListing([], []);

export const populatedPullRequests = pullRequestListing(mockPullRequests, [
  zoneRepository,
  consoleRepository,
]);

function pullRequestDetail(pullRequest: PullRequest) {
  const blocked = pullRequest.readiness === 'not_ready';
  return {
    pull_request: pullRequest,
    threads: blocked ? wireDetail.threads : [],
    comments: blocked ? wireDetail.comments : [],
  };
}

function pullRequestDiff(pullRequest: PullRequest) {
  return { diff: { ...wireDiff, number: pullRequest.number, head: pullRequest.head } };
}

function pullRequestCommits(pullRequest: PullRequest) {
  return { commits: { ...wireCommits, number: pullRequest.number, head: pullRequest.head } };
}

export const checkLog = { log: wireLog };

function pipelineRun(
  repository: Repository,
  id: number,
  name: string,
  state: PipelineRun['state'],
  minutes: number
): PipelineRun {
  return {
    id,
    workflow_id: id,
    attempt: 1,
    name,
    path: `.github/workflows/${name.toLowerCase()}.yml`,
    state,
    head_sha: 'a'.repeat(40),
    created_at: minutesBefore(minutes),
    started_at: minutesBefore(minutes),
    updated_at: minutesBefore(minutes - 1),
    url: `${repository.repository_url}/actions/runs/${id}`,
  };
}

interface ReleaseShape {
  repository: Repository;
  id: number;
  tag: string;
  name?: string;
  minutes: number;
  prerelease?: boolean;
  runs: PipelineRun[];
  state: Release['state'];
  blockers?: Release['blockers'];
  pulls?: { number: number; title: string }[];
}

function release(shape: ReleaseShape): Release {
  const { repository } = shape;
  const url = `${repository.repository_url}/releases/tag/${shape.tag}`;
  const published = minutesBefore(shape.minutes);
  const succeeded = shape.state === 'succeeded';
  return {
    source_id: repository.source_id,
    repository: repository.repository,
    repository_url: repository.repository_url,
    id: shape.id,
    tag: shape.tag,
    name: shape.name ?? shape.tag,
    url,
    published_at: published,
    prerelease: shape.prerelease ?? false,
    pulls: (shape.pulls ?? []).map((pull) => ({
      ...pull,
      url: `${repository.repository_url}/pull/${pull.number}`,
    })),
    pipeline: {
      release: {
        repository: repository.repository,
        id: shape.id,
        tag: shape.tag,
        published_at: published,
        url,
      },
      lookup: 'complete',
      runs: shape.runs,
      checked_at: minutesBefore(1),
    },
    state: shape.state,
    succeeded,
    complete: shape.state !== 'running',
    blockers: shape.blockers ?? [],
  };
}

export const runningRelease = release({
  repository: zoneRepository,
  id: 1,
  tag: 'v1.4.0',
  minutes: 20,
  runs: [
    pipelineRun(zoneRepository, 11, 'Release', 'running', 20),
    pipelineRun(zoneRepository, 12, 'Docker', 'queued', 20),
  ],
  state: 'running',
  blockers: [{ code: 'runs_active', detail: '2 release runs are still going' }],
  pulls: [
    { number: 812, title: 'Add the Releases page with day groups and pipeline chips' },
    { number: 809, title: 'Poll release pipelines while a run is active' },
    { number: 805, title: 'Fix a crash when a source has no default branch' },
  ],
});

export const prerelease = release({
  repository: consoleRepository,
  id: 2,
  tag: 'v0.9.0-beta.1',
  name: 'Console beta with the new navigation',
  minutes: 3 * MINUTES_IN_HOUR,
  prerelease: true,
  runs: [pipelineRun(consoleRepository, 21, 'Release', 'succeeded', 3 * MINUTES_IN_HOUR)],
  state: 'succeeded',
  pulls: [{ number: 41, title: 'Ship the beta navigation' }],
});

export const yesterdayRelease = release({
  repository: zoneRepository,
  id: 3,
  tag: 'v1.3.2',
  minutes: 26 * MINUTES_IN_HOUR,
  runs: ['Release', 'Docker', 'Docs', 'Homebrew'].map((name, index) =>
    pipelineRun(zoneRepository, 31 + index, name, 'succeeded', 26 * MINUTES_IN_HOUR)
  ),
  state: 'succeeded',
  pulls: [{ number: 790, title: 'Harden the merge guard' }],
});

export const failedRelease = release({
  repository: consoleRepository,
  id: 4,
  tag: 'v0.8.4',
  minutes: 50 * MINUTES_IN_HOUR,
  runs: [pipelineRun(consoleRepository, 41, 'Release', 'failed', 50 * MINUTES_IN_HOUR)],
  state: 'failed',
  blockers: [{ code: 'runs_failed', detail: 'Release failed on the publish step' }],
});

const mockReleases = [runningRelease, prerelease, yesterdayRelease, failedRelease];

function releaseListing(releases: Release[], sources: Repository[]): ReleasesResponse {
  return {
    generated_at: minutesBefore(1),
    stale: false,
    partial: false,
    window_days: 7,
    ttl_secs: 120,
    sources: sources.map(sourceStatus),
    releases,
    warnings: [],
  };
}

export const emptyReleases = releaseListing([], []);

const populatedReleases = releaseListing(mockReleases, [zoneRepository, consoleRepository]);

type TagLine = SourceOptions['lines'][number];

function sourceOptions(repository: Repository, lines: [TagLine, ...TagLine[]]): SourceOptions {
  const [dominant] = lines;
  return {
    source_id: repository.source_id,
    repository: repository.repository,
    repository_url: repository.repository_url,
    default_branch: 'main',
    latest_tag: dominant.latest_tag,
    next_tag: dominant.next_tag,
    previous_tags: dominant.previous_tags,
    lines,
  };
}

export const cratesLines: [TagLine, TagLine] = [
  {
    prefix: 'abnegate-vcs',
    latest_tag: 'abnegate-vcs/v0.2.1',
    next_tag: 'abnegate-vcs/v0.2.2',
    previous_tags: ['abnegate-vcs/v0.3.0-rc.1', 'abnegate-vcs/v0.2.1', 'abnegate-vcs/v0.2.0'],
  },
  {
    prefix: 'abnegate-llm',
    latest_tag: 'abnegate-llm/v1.0.0',
    next_tag: 'abnegate-llm/v1.0.1',
    previous_tags: ['abnegate-llm/v1.0.0', 'abnegate-llm/v0.9.2'],
  },
];

export const releaseOptions: OptionsResponse = {
  generated_at: minutesBefore(1),
  sources: [
    sourceOptions(zoneRepository, [
      {
        prefix: null,
        latest_tag: runningRelease.tag,
        next_tag: 'v1.4.1',
        previous_tags: [runningRelease.tag, yesterdayRelease.tag, 'v1.3.1'],
      },
    ]),
    sourceOptions(consoleRepository, [
      {
        prefix: null,
        latest_tag: failedRelease.tag,
        next_tag: 'v0.8.5',
        previous_tags: [prerelease.tag, failedRelease.tag],
      },
    ]),
    sourceOptions(cratesRepository, cratesLines),
  ],
  warnings: [],
};

export function releaseVerdict(subject: Release) {
  const { pipeline, state, succeeded, complete, blockers } = subject;
  return { pipeline, state, succeeded, complete, blockers };
}

const pullRequestsPath = /\/workspaces\/[^/]+\/pull-requests$/;
const pullRequestPath =
  /\/workspaces\/[^/]+\/pull-requests\/([^/]+)\/(\d+)(?:\/(diff|commits|jobs\/\d+\/log))?$/;
const releasesPath = /\/workspaces\/[^/]+\/releases$/;
const releaseOptionsPath = /\/workspaces\/[^/]+\/releases\/options$/;
const releasePipelinePath = /\/workspaces\/[^/]+\/releases\/([^/]+)\/(\d+)\/pipeline$/;

const PullRequestArtifact = {
  diff: 'diff',
  commits: 'commits',
} as const;

function pullRequestBody(sourceId: string, number: number, artifact: string | undefined) {
  const pullRequest = mockPullRequests.find(
    (candidate) => candidate.source_id === sourceId && candidate.number === number
  );
  if (!pullRequest) return undefined;
  if (artifact === undefined) return pullRequestDetail(pullRequest);
  if (artifact === PullRequestArtifact.diff) return pullRequestDiff(pullRequest);
  if (artifact === PullRequestArtifact.commits) return pullRequestCommits(pullRequest);
  return checkLog;
}

function githubBody(path: string, populated: boolean): unknown {
  if (pullRequestsPath.test(path))
    return populated ? populatedPullRequests : emptyPullRequests;
  const pullRequest = pullRequestPath.exec(path);
  if (pullRequest)
    return pullRequestBody(
      decodeURIComponent(pullRequest[1]),
      Number(pullRequest[2]),
      pullRequest[3]
    );
  if (releasesPath.test(path)) return populated ? populatedReleases : emptyReleases;
  if (releaseOptionsPath.test(path))
    return populated ? releaseOptions : { ...releaseOptions, sources: [] };
  const pipeline = releasePipelinePath.exec(path);
  if (pipeline) {
    const sourceId = decodeURIComponent(pipeline[1]);
    const id = Number(pipeline[2]);
    const found = mockReleases.find(
      (candidate) => candidate.source_id === sourceId && candidate.id === id
    );
    return found && releaseVerdict(found);
  }
  return undefined;
}

export async function routeWorkspaceRole(page: Page, role: string): Promise<void> {
  await routeApi(page, /\/api\/organizations\/[^/]+\/workspaces(\?|$)/, (route) =>
    route.fulfill({ status: 200, json: { workspaces: [{ ...mockWorkspace, role }] } })
  );
}

export async function setupAdminAuth(page: Page, withheld: readonly string[] = []) {
  const fullAdminPermissions = [
    ...adminPermissions,
    'organizations:read',
    'organizations:update',
    'organizations:delete',
    'workspaces:read',
    'workspaces:update',
    'workspaces:delete',
  ].filter((permission) => !withheld.includes(permission));

  const token = createMockJwt({
    sub: mockAdminUser.id,
    email: mockAdminUser.email,
    roles: ['admin', 'user'],
    permissions: fullAdminPermissions,
    exp: Math.floor(Date.now() / 1000) + 3600,
  });

  await page.evaluate(
    ({ token, user }) => {
      localStorage.setItem('manager_access_token', token);
      localStorage.setItem('manager_refresh_token', 'mock-refresh-token');
      localStorage.setItem('manager_user', JSON.stringify(user));
      localStorage.setItem(`manager_setup_complete:${user.id}`, '1');
    },
    { token, user: mockAdminUser }
  );
}

export async function setupCommonRoutes(
  page: Page,
  populated = false
): Promise<void> {
  await routeApi(page, /\/api\//, async (route) => {
    const url = new URL(route.request().url());
    const path = url.pathname;
    let body: unknown;
    if (route.request().method() === 'GET') {
      const github = githubBody(path, populated);
      if (github !== undefined) {
        await route.fulfill({ status: 200, json: github });
        return;
      }
    }
    if (path.endsWith('/theme')) {
      await route.fulfill({ status: 404, json: { error: 'Theme not found' } });
      return;
    }
    if (path === '/api/host-mounts') {
      body = {
        in_container: true,
        host_root: '/Users/you',
        container_root: '/host',
        ready: true,
        hint: 'Folders must live under /Users/you.',
      };
    } else if (path.endsWith('/host-directories')) {
      if (route.request().method() === 'PUT') {
        const posted = route.request().postDataJSON() as { directories?: string[] } | null;
        const directories = posted?.directories ?? [];
        await route.fulfill({
          status: 200,
          json: {
            directories,
            folders: directories.map((host) => ({ host, mapped: `/host${host}`, exists: true })),
          },
        });
        return;
      }
      body = { directories: [], folders: [] };
    } else if (path.endsWith('/members'))
      body = { members: populated ? mockMembers : [] };
    else if (path.endsWith('/devices')) body = { devices: [] };
    else if (path.endsWith('/device-policy')) body = { mode: 'open' };
    else if (/\/settings\/ai(?:\/effective)?$/.test(path))
      body = aiSettingsResponse(path);
    else if (path === '/api/organizations')
      body = { organizations: [mockOrganization] };
    else if (/\/organizations\/[^/]+\/workspaces$/.test(path))
      body = { workspaces: [mockWorkspace] };
    else if (/\/organizations\/[^/]+$/.test(path))
      body = { organization: mockOrganization };
    else if (/\/workspaces\/[^/]+$/.test(path))
      body = { workspace: mockWorkspace };
    else if (path === '/api/models/setup')
      body = {
        ram_bytes: 68719476736,
        ram_label: '64 GB',
        disk_free_bytes: 500000000000,
        disk_free_label: '500 GB',
        vision_min_ram_bytes: 17179869184,
        disk_margin_bytes: 10000000000,
        chat_preset: '32gb',
        recommended_preset: '32gb',
        chat_presets: [],
        features: [],
        wants_all: true,
        gate: null,
        totals: {
          size_bytes: 0,
          size_label: '0 B',
          present_bytes: 0,
          present_label: '0 B',
          needed_bytes: 0,
          needed_label: '0 B',
          working_space_bytes: 0,
          working_space_label: '0 B',
          required_free_bytes: 0,
          required_free_label: '0 B',
          free_now_bytes: 0,
          free_now_label: '0 B',
          short_by_bytes: 0,
          short_by_label: null,
        },
        licenses: [],
        artifacts: [],
        pulls: [],
      };
    else if (path === '/api/models')
      body = { models: populated ? mockModels : [], next_cursor: null };
    else if (path === '/api/models/train') {
      await route.fulfill({ status: 204, body: '' });
      return;
    }
    else if (path === '/api/chats')
      body = { chats: populated ? mockChats : [] };
    else if (path === '/api/projects')
      body = { projects: populated ? mockProjects : [] };
    else if (path.endsWith('/sync')) body = { configs: [] };
    else if (path.endsWith('/tasks'))
      body = { tasks: populated ? mockTasks : [] };
    else if (path.endsWith('/runs')) body = { runs: [] };
    else if (path.endsWith('/sources/types')) body = { types: sourceTypes };
    else if (path.endsWith('/sources'))
      body = { sources: populated ? mockSources : [] };
    else if (path === '/api/knowledge')
      body = { entries: populated ? mockKnowledge : [] };
    else if (path === '/api/auth/sessions')
      body = { sessions: populated ? mockSessions : [] };
    else if (path === '/api/context/search') body = { results: [], total: 0 };
    else {
      await route.fallback();
      return;
    }
    await route.fulfill({ status: 200, json: body });
  });
}
