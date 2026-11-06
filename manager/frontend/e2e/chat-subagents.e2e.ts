import type { Page, TestInfo } from '@playwright/test';
import { expect, test } from './fixtures';
import { mockCommonEndpoints, setupAuth } from './helpers/auth';
import { type ChatSocketController, installChatSocketMock } from './helpers/chatSocket';
import { blockServiceWorker, CHAT_PROVIDERS, routeApi } from './test-utils';

const CHAT_ID = 'chat-1';
const MESSAGE_ID = 'msg-agents';
const PANEL_LABEL = 'Changes, agents, tasks, media and sources';
const sentAt = '2026-11-05T09:30:00.000Z';
const RUNNING_FOR = 95_000;

const chat = {
  id: CHAT_ID,
  title: 'Widget catalogue',
  model_name: 'claude-code',
  created_at: sentAt,
  updated_at: sentAt,
  archived: false,
  agent_enabled: true,
};

function picture(from: string, to: string): string {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="240" height="240"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="${from}"/><stop offset="1" stop-color="${to}"/></linearGradient></defs><rect width="240" height="240" fill="url(#g)"/></svg>`;
  return `data:image/svg+xml;base64,${Buffer.from(svg).toString('base64')}`;
}

const messages = [
  {
    id: 'msg-user',
    chat_id: CHAT_ID,
    role: 'user',
    content: 'Review the catalogue change, benchmark the pipeline and draft the release notes.',
    created_at: sentAt,
    metadata: {
      attachments: [
        { name: 'cover.svg', mime: 'image/svg+xml', url: picture('#0ea5e9', '#7c3aed') },
      ],
    },
  },
  {
    id: MESSAGE_ID,
    chat_id: CHAT_ID,
    role: 'assistant',
    content:
      'I handed the review, the benchmark and the release notes to three agents; their progress is in the Agents box.',
    created_at: sentAt,
    metadata: {
      citations: [
        {
          kind: 'github_issue',
          title: '#12 Add the widget catalogue with search and filters',
          url: 'https://github.com/acme/widgets/pull/12',
          revision: '4f1c2e0a9b8c7d6e5f4a3b2c1d0e9f8a7b6c5d4e',
          observed_at: sentAt,
          complete: true,
          outcome: 'pending',
          provenance: 'server_execution',
        },
      ],
    },
  },
];

const PULL_REQUESTS = [
  {
    id: 'pr-12',
    provider: 'github',
    owner: 'acme',
    repo: 'widgets',
    number: 12,
    url: 'https://github.com/acme/widgets/pull/12',
    title: 'Add the widget catalogue with search and filters',
    dismissed: false,
    created_at: sentAt,
    status: {
      title: 'Add the widget catalogue with search and filters',
      state: 'open',
      draft: false,
      additions: 120,
      deletions: 18,
      changed_files: 6,
      checks: 'pending',
      mergeable: 'blocked',
      auto_merge: { enabled: false, unavailable: 'Coming soon.' },
      draft_unavailable: 'Coming soon.',
      observed_at: sentAt,
    },
    writable: true,
    credentialed: true,
  },
];

const TASKS = [
  {
    id: 'task-1',
    title: 'Write the catalogue release notes',
    title_pending: false,
    status: 'created',
    is_agentic: false,
    href: '/tasks?id=task-1',
    created_at: sentAt,
    updated_at: sentAt,
    run: null,
    pull_request: null,
  },
];

function subagent(overrides: Record<string, unknown>) {
  return {
    chat_id: CHAT_ID,
    message_id: MESSAGE_ID,
    kind: 'general-purpose',
    model: 'sonnet',
    background: false,
    status: 'completed',
    tool_uses: null,
    duration_ms: null,
    tokens: null,
    last_tool: null,
    summary: null,
    error: null,
    step_count: 0,
    revision: 1,
    started_at: sentAt,
    finished_at: null,
    updated_at: sentAt,
    ...overrides,
  };
}

const REVIEW_SUMMARY = [
  'No blocking issues. Two naming nits in filter.rs: `matches_all` reads better as `matches_every`,',
  'and `Filter::new` takes its options in a different order from `Search::new`.',
  'The catalogue tests cover search, paging and the empty state; the price filter has no test',
  'for a range whose bounds are equal, which the query builder turns into an empty set.',
  'Everything else follows the existing patterns in the crate.',
].join(' ');

const REVIEW = subagent({
  id: 'agent-review',
  external_id: 'toolu_01Review',
  description: 'Review the catalogue diff',
  tool_uses: 3,
  duration_ms: 42_000,
  tokens: 18_000,
  last_tool: 'Grep',
  summary: REVIEW_SUMMARY,
  step_count: 3,
  revision: 4,
  started_at: '2026-11-05T09:30:05.000Z',
  finished_at: '2026-11-05T09:30:47.000Z',
  updated_at: '2026-11-05T09:30:47.000Z',
});

const benchmarkStarted = new Date(Date.now() - RUNNING_FOR).toISOString();

const BENCHMARK = subagent({
  id: 'agent-benchmark',
  external_id: 'toolu_01Bench',
  description: 'Benchmark the image pipeline',
  model: 'haiku',
  background: true,
  status: 'running',
  tool_uses: 2,
  last_tool: 'Bash',
  step_count: 2,
  revision: 2,
  started_at: benchmarkStarted,
  updated_at: benchmarkStarted,
});

const NOTES = subagent({
  id: 'agent-notes',
  external_id: 'call_notes',
  description: 'Draft the release notes',
  kind: null,
  model: 'gpt-5-codex',
  status: 'failed',
  duration_ms: 8_000,
  error: 'The agent hit its usage limit before it finished.',
  started_at: '2026-11-05T09:30:10.000Z',
  finished_at: '2026-11-05T09:30:18.000Z',
  updated_at: '2026-11-05T09:30:18.000Z',
});

const STEPS: Record<string, unknown[]> = {
  'agent-review': [
    { id: 'toolu_r1', name: 'Read', subject: 'src/catalogue.rs', status: 'succeeded' },
    { id: 'toolu_r2', name: 'Bash', subject: 'cargo test -p widgets', status: 'succeeded' },
    { id: 'toolu_r3', name: 'Grep', subject: 'fn matches', status: 'succeeded' },
  ],
  'agent-benchmark': [
    { id: 'toolu_b1', name: 'Read', subject: 'benches/resize.rs', status: 'succeeded' },
    { id: 'toolu_b2', name: 'Bash', subject: 'cargo bench resize', status: 'running' },
  ],
};

const BENCHMARK_FINISHED = {
  ...BENCHMARK,
  status: 'completed',
  tool_uses: 3,
  duration_ms: 101_000,
  summary: 'Thumbnails render 38% faster with the new resize path.',
  step_count: 3,
  revision: 3,
  last_tool: 'Write',
  finished_at: new Date().toISOString(),
  updated_at: new Date(Date.now() + 1_000).toISOString(),
};

const BENCHMARK_FINISHED_STEPS = [
  { id: 'toolu_b2', name: 'Bash', subject: 'cargo bench resize', status: 'succeeded' },
  { id: 'toolu_b3', name: 'Write', subject: 'benches/RESULTS.md', status: 'succeeded' },
];

const STEPS_PATH = new RegExp(`/api/chats/${CHAT_ID}/subagents/([^/]+)/steps$`);

function json(body: unknown) {
  return { status: 200, contentType: 'application/json', body: JSON.stringify(body) };
}

async function mockChat(page: Page, stepReads: string[]) {
  await routeApi(page, /\/api\/chats($|\?|\/)/i, (route) => {
    if (CHAT_PROVIDERS.test(route.request().url())) return route.fallback();
    if (route.request().method() !== 'GET') return route.continue();
    const url = route.request().url();
    return route.fulfill(
      json(url.includes(`/${CHAT_ID}`) ? { chat: { ...chat, messages } } : { chats: [chat] })
    );
  });
  await routeApi(page, `**/api/chats/${CHAT_ID}/pull-requests`, (route) =>
    route.fulfill(json({ pull_requests: PULL_REQUESTS }))
  );
  await routeApi(page, `**/api/chats/${CHAT_ID}/tasks`, (route) =>
    route.fulfill(json({ tasks: TASKS }))
  );
  await routeApi(page, `**/api/chats/${CHAT_ID}/subagents`, (route) =>
    route.fulfill(json({ subagents: [REVIEW, BENCHMARK, NOTES] }))
  );
  await routeApi(page, STEPS_PATH, (route) => {
    const id = decodeURIComponent(STEPS_PATH.exec(new URL(route.request().url()).pathname)?.[1] ?? '');
    stepReads.push(id);
    return route.fulfill(json({ steps: STEPS[id] ?? [] }));
  });
}

function chipName(description: string, status: string): string {
  return `Agent: ${description}, ${status}. Show in the agents box`;
}

test.describe('Chat subagents', () => {
  let socket: ChatSocketController;
  let stepReads: string[];

  test.beforeEach(async ({ context, page }) => {
    stepReads = [];
    socket = await installChatSocketMock(page);
    await blockServiceWorker(context);
    await mockCommonEndpoints(page);
    await mockChat(page, stepReads);
    await page.setViewportSize({ width: 1440, height: 1000 });
    await page.goto('/');
    await setupAuth(page);
    await page.reload();
    await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
    await page.click('a[href="/chats"]');
    await page.click('.chat-item');
    await expect(page.locator('.message-form textarea')).toBeVisible();
  });

  test('shows each agent in the message that started it and in the Agents box, live', async ({
    page,
  }, testInfo: TestInfo) => {
    const capture = async (name: string) => {
      if (testInfo.project.name !== 'chromium') return;
      await page.mouse.move(0, 0);
      await page.screenshot({ path: `screenshots/${name}.png`, animations: 'disabled' });
    };
    const message = page.locator(`#chat-message-${MESSAGE_ID}`);
    const chips = message.getByRole('list', { name: 'Agents' });
    await expect(chips.getByRole('button')).toHaveCount(3);
    const review = chips.getByRole('button', {
      name: chipName('Review the catalogue diff', 'Completed'),
    });
    await expect(review).toContainText('Agent: Review the catalogue diff');
    await expect(review.locator('svg')).toBeVisible();
    await expect(
      chips.getByRole('button', { name: chipName('Benchmark the image pipeline', 'Running') })
    ).toBeVisible();
    await expect(
      chips.getByRole('button', { name: chipName('Draft the release notes', 'Failed') })
    ).toBeVisible();
    await expect(page.locator('#chat-message-msg-user .chat-subagent-chips')).toHaveCount(0);

    const panel = page.getByRole('complementary', { name: PANEL_LABEL });
    for (const box of ['Changes, 1', 'Agents, 3', 'Tasks, 1', 'Sources, 1', 'Gallery, 1']) {
      await expect(panel.getByRole('button', { name: box })).toBeVisible();
    }
    const benchmark = panel.locator('[data-subagent="agent-benchmark"]');
    await expect(benchmark).toContainText('Running');
    await expect(benchmark).toContainText('general-purpose');
    await expect(benchmark).toContainText('haiku');
    await expect(benchmark).toContainText('2 tools');
    await expect(benchmark.locator('.chat-subagent-duration')).toHaveText(/^1m \d\ds$/);
    await expect(benchmark).toContainText('Bash');
    const notes = panel.locator('[data-subagent="agent-notes"]');
    await expect(notes).toContainText('gpt-5-codex');
    await expect(notes).toContainText('8s');
    await expect(notes.locator('.chat-subagent-error')).toHaveText(
      'The agent hit its usage limit before it finished.'
    );
    await expect(notes.getByRole('button', { expanded: false })).toHaveCount(0);
    await message.scrollIntoViewIfNeeded();
    await capture('chats-agent-chip');
    expect(stepReads).toEqual([]);

    const benchmarkTitle = benchmark.getByRole('button', { name: 'Benchmark the image pipeline' });
    await benchmarkTitle.click();
    await expect(benchmarkTitle).toHaveAttribute('aria-expanded', 'true');
    const benchmarkSteps = benchmark.locator('.chat-subagent-step');
    await expect(benchmarkSteps).toHaveCount(2);
    await expect(benchmarkSteps.nth(1)).toHaveClass(/chat-subagent-step--running/);
    expect(stepReads).toEqual(['agent-benchmark']);

    await socket.emit({
      type: 'subagent',
      chat_id: CHAT_ID,
      subagent: BENCHMARK_FINISHED,
      steps: BENCHMARK_FINISHED_STEPS,
    });
    await expect(
      chips.getByRole('button', { name: chipName('Benchmark the image pipeline', 'Completed') })
    ).toBeVisible();
    await expect(benchmark).toContainText('3 tools');
    await expect(benchmark.locator('.chat-subagent-duration')).toHaveText('1m 41s');
    await expect(benchmark.locator('.chat-subagent-summary')).toHaveText(
      'Thumbnails render 38% faster with the new resize path.'
    );
    await expect(benchmarkSteps).toHaveCount(3);
    await expect(benchmarkSteps.nth(1)).toHaveClass(/chat-subagent-step--succeeded/);
    await expect(benchmarkSteps.nth(2)).toContainText('benches/RESULTS.md');
    expect(stepReads).toEqual(['agent-benchmark']);
    await benchmarkTitle.click();
    await expect(benchmarkTitle).toHaveAttribute('aria-expanded', 'false');

    await panel.getByRole('button', { name: 'Agents, 3' }).click();
    await expect(panel.getByRole('button', { name: 'Agents, 3' })).toHaveAttribute(
      'aria-expanded',
      'false'
    );
    const resize = page.getByRole('separator', { name: `Resize ${PANEL_LABEL.toLowerCase()}` });
    await page.getByRole('button', { name: `Hide ${PANEL_LABEL.toLowerCase()}` }).click();
    await expect(resize).toHaveCount(0);

    await review.click();
    await expect(resize).toBeVisible();
    await expect(panel.getByRole('button', { name: 'Agents, 3' })).toHaveAttribute(
      'aria-expanded',
      'true'
    );
    const reviewed = panel.locator('[data-subagent="agent-review"]');
    const title = reviewed.getByRole('button', { name: 'Review the catalogue diff' });
    await expect(title).toHaveAttribute('aria-expanded', 'true');
    await expect(title).toBeFocused();
    const steps = reviewed.locator('.chat-subagent-step');
    await expect(steps).toHaveCount(3);
    await expect(steps.nth(1)).toContainText('Bash');
    await expect(steps.nth(1)).toContainText('cargo test -p widgets');
    expect(stepReads).toEqual(['agent-benchmark', 'agent-review']);
    const summary = reviewed.locator('.chat-subagent-summary');
    await expect(summary).toHaveText(REVIEW_SUMMARY);
    await expect(summary).toHaveAttribute('data-clamped', 'true');
    const more = reviewed.getByRole('button', { name: 'Show more' });
    await expect(more).toBeVisible();
    for (const box of ['Changes, 1', 'Tasks, 1', 'Sources, 1', 'Gallery, 1']) {
      await expect(panel.getByRole('button', { name: box })).toBeVisible();
    }
    await capture('chats-panel-agents');

    await more.click();
    await expect(summary).toHaveAttribute('data-clamped', 'false');
    await expect(reviewed.getByRole('button', { name: 'Show less' })).toBeVisible();

    await socket.emit({ type: 'subagents_removed', chat_id: CHAT_ID, ids: ['agent-notes'] });
    await expect(chips.getByRole('button')).toHaveCount(2);
    await expect(panel.getByRole('button', { name: 'Agents, 2' })).toBeVisible();
    await expect(notes).toHaveCount(0);
  });
});
