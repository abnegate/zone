import type { Locator, Page } from '@playwright/test';
import { expect, test } from './fixtures';
import { mockCommonEndpoints, setupAuth } from './helpers/auth';
import { installChatSocketMock } from './helpers/chatSocket';
import { blockServiceWorker, CHAT_PROVIDERS, routeApi } from './test-utils';

const sentAt = '2026-11-05T09:30:00.000Z';
const PENDING_RELEASE = 'Coming with the next abnegate-vcs release.';

const chat = {
  id: 'chat-1',
  title: 'Widget catalogue',
  model_name: 'llama3.1',
  created_at: sentAt,
  updated_at: sentAt,
  archived: false,
  agent_enabled: true,
};

function picture(from: string, to: string): string {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="240" height="240"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="${from}"/><stop offset="1" stop-color="${to}"/></linearGradient></defs><rect width="240" height="240" fill="url(#g)"/><circle cx="170" cy="70" r="34" fill="#fff" fill-opacity=".55"/></svg>`;
  return `data:image/svg+xml;base64,${Buffer.from(svg).toString('base64')}`;
}

const PICTURES = [
  { name: 'dawn.svg', url: picture('#f97316', '#7c3aed') },
  { name: 'harbour.svg', url: picture('#0ea5e9', '#14532d') },
  { name: 'meadow.svg', url: picture('#84cc16', '#0f766e') },
  { name: 'dusk.svg', url: picture('#be123c', '#1e1b4b') },
];

const REVISION = '4f1c2e0a9b8c7d6e5f4a3b2c1d0e9f8a7b6c5d4e';

function citation(overrides: Record<string, unknown>) {
  return {
    kind: 'github_issue',
    revision: REVISION,
    observed_at: '2026-11-05T09:14:00.000Z',
    complete: true,
    outcome: 'pending',
    provenance: 'server_execution',
    ...overrides,
  };
}

const ART_CITATIONS = [
  citation({
    title: '#12 Add the widget catalogue with search and filters',
    url: 'https://github.com/acme/widgets/pull/12',
    observed_at: '2026-11-05T09:02:00.000Z',
    note: 'Checks queued.',
  }),
  citation({
    kind: 'web',
    title: 'Catalogue cover guidelines',
    identifier: 'web:c0ffee',
    url: 'https://design.acme.test/catalogue/cover',
    revision: null,
    observed_at: '2026-11-05T09:01:00.000Z',
    outcome: 'observed',
  }),
];

const PULL_REQUEST_CITATIONS = [
  citation({
    title: '#12 Add the widget catalogue with search and filters',
    url: 'https://github.com/acme/widgets/pull/12',
    note: 'Review bot confidence missing; 20 of 21 checks still running.',
  }),
  citation({
    title: '#13 Fix the price rounding in the cart',
    url: 'https://github.com/acme/widgets/pull/13',
    observed_at: '2026-11-05T09:13:00.000Z',
    complete: false,
    outcome: 'incomplete',
    note: '3 unresolved review threads; checks could not be fully read.',
  }),
  citation({
    kind: 'github_build',
    title: `widgets ${REVISION.slice(0, 7)}`,
    url: `https://github.com/acme/widgets/commit/${REVISION}`,
    observed_at: '2026-11-05T09:15:00.000Z',
    note: 'Observed CI only; required branch checks are not evaluated.',
  }),
];

const messages = [
  {
    id: 'msg-user',
    chat_id: 'chat-1',
    role: 'user',
    content: 'Paint four takes on the catalogue cover, then open a pull request with the widget.',
    created_at: sentAt,
    metadata: {
      attachments: [{ name: PICTURES[0].name, mime: 'image/svg+xml', url: PICTURES[0].url }],
    },
  },
  {
    id: 'msg-art',
    chat_id: 'chat-1',
    role: 'assistant',
    content:
      'Here are the other three covers, following the cover guidelines [web:c0ffee], and a short theme for the launch video.',
    created_at: sentAt,
    metadata: {
      attachments: [
        ...PICTURES.slice(1).map((item) => ({ ...item, mime: 'image/svg+xml' })),
        { name: 'launch-theme.mp3', mime: 'audio/mpeg', url: 'data:audio/mpeg;base64,AA==' },
      ],
      citations: ART_CITATIONS,
    },
  },
  {
    id: 'msg-pr',
    chat_id: 'chat-1',
    role: 'assistant',
    content: 'I opened acme/widgets#12 with the new widget and its tests.',
    created_at: sentAt,
    metadata: { citations: PULL_REQUEST_CITATIONS },
  },
];

function status(overrides: Record<string, unknown>) {
  return {
    title: '',
    state: 'open',
    draft: false,
    additions: 120,
    deletions: 18,
    changed_files: 6,
    checks: 'passing',
    mergeable: 'clean',
    auto_merge: { enabled: false, unavailable: PENDING_RELEASE },
    draft_unavailable: PENDING_RELEASE,
    threads: { open: 0, resolved: 0 },
    observed_at: sentAt,
    ...overrides,
  };
}

function pullRequest(number: number, title: string, overrides: Record<string, unknown>) {
  return {
    id: `pr-${number}`,
    provider: 'github',
    owner: 'acme',
    repo: 'widgets',
    number,
    url: `https://github.com/acme/widgets/pull/${number}`,
    title,
    dismissed: false,
    created_at: sentAt,
    status: status({ title, ...overrides }),
    writable: true,
    credentialed: true,
  };
}

const PULL_REQUESTS = [
  pullRequest(12, 'Add the widget catalogue with search and filters', {
    mergeable: 'blocked',
    checks: 'pending',
    threads: { open: 2, resolved: 3 },
  }),
  pullRequest(13, 'Fix the price rounding in the cart', {
    checks: 'failing',
    mergeable: 'unstable',
    additions: 9,
    deletions: 4,
    threads: { open: 0, resolved: 4 },
  }),
  pullRequest(14, 'Draft: rework the catalogue cover', {
    draft: true,
    mergeable: 'draft',
    additions: 310,
    deletions: 77,
  }),
  pullRequest(11, 'Bump the image pipeline', {
    state: 'merged',
    additions: 2,
    deletions: 2,
    auto_merge: { enabled: false, unavailable: 'It is no longer open.' },
    draft_unavailable: 'It is no longer open.',
    threads: null,
  }),
];

function task(id: string, title: string, overrides: Record<string, unknown>) {
  return {
    id,
    title,
    title_pending: false,
    status: 'created',
    is_agentic: true,
    href: `/tasks?id=${id}`,
    created_at: sentAt,
    updated_at: sentAt,
    run: null,
    pull_request: null,
    ...overrides,
  };
}

function run(id: string, overrides: Record<string, unknown>) {
  return {
    id,
    status: 'running',
    phase: 'acting',
    progress: 0,
    started_at: sentAt,
    completed_at: null,
    ...overrides,
  };
}

const TASKS = [
  task('task-3', 'Round cart prices to the cent', {
    status: 'in_progress',
    run: run('run-3', { progress: 45 }),
  }),
  task('task-2', 'Build the widget catalogue', {
    status: 'review',
    run: run('run-2', { status: 'completed', phase: 'complete', progress: 100 }),
    pull_request: { url: 'https://github.com/acme/widgets/pull/12', status: 'open' },
  }),
  task('task-1', 'Benchmark the image pipeline', {
    status: 'blocked',
    run: run('run-1', { status: 'failed', phase: 'error', progress: 30 }),
  }),
  task('task-0', 'Write the catalogue release notes', { is_agentic: false }),
];

const DIFF = [
  'diff --git a/src/catalogue.rs b/src/catalogue.rs',
  'index 1..2 100644',
  '--- a/src/catalogue.rs',
  '+++ b/src/catalogue.rs',
  '@@ -1,5 +1,7 @@',
  ' use crate::widget::Widget;',
  '-pub fn list() -> Vec<Widget> {',
  '-    Vec::new()',
  '+pub fn list(filter: &Filter) -> Vec<Widget> {',
  '+    let widgets = store::all();',
  '+    widgets.into_iter().filter(|widget| filter.matches(widget)).collect()',
  ' }',
  'diff --git a/src/filter.rs b/src/filter.rs',
  'new file mode 100644',
  '--- /dev/null',
  '+++ b/src/filter.rs',
  '@@ -0,0 +1,3 @@',
  '+pub struct Filter {',
  '+    pub query: String,',
  '+}',
  '',
].join('\n');

function uncited() {
  return messages.map(({ metadata, ...message }) => {
    if (!metadata) return message;
    const { citations: _, ...rest } = metadata as Record<string, unknown>;
    return Object.keys(rest).length > 0 ? { ...message, metadata: rest } : message;
  });
}

async function mockTasks(page: Page, tasks: readonly unknown[]) {
  await routeApi(page, '**/api/chats/chat-1/tasks', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ tasks }),
    })
  );
}

async function mockChat(page: Page, transcript: readonly unknown[] = messages) {
  await routeApi(page, /\/api\/chats($|\?|\/)/i, (route) => {
    if (CHAT_PROVIDERS.test(route.request().url())) return route.fallback();
    const url = route.request().url();
    if (route.request().method() !== 'GET') return route.continue();
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify(
        url.includes('/chat-1') ? { chat: { ...chat, messages: transcript } } : { chats: [chat] }
      ),
    });
  });
  await routeApi(page, '**/api/chats/chat-1/pull-requests', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ pull_requests: PULL_REQUESTS }),
    })
  );
  await routeApi(page, '**/api/chats/chat-1/pull-requests/pr-12/diff', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ diff: DIFF, truncated: false }),
    })
  );
  await mockTasks(page, []);
}

function boxOrder(container: Locator): Promise<string[]> {
  return container
    .locator('.chat-panel-slot')
    .evaluateAll((slots) => slots.map((slot) => (slot as HTMLElement).dataset.box ?? ''));
}

/**
 * dnd-kit's keyboard sensor starts listening for arrow keys one task after the
 * pick-up, so an arrow pressed sooner is lost. A task queued now runs after it.
 */
function keyboardSensorListening(page: Page): Promise<void> {
  return page.evaluate(() => new Promise<void>((resolve) => setTimeout(resolve)));
}

async function openChat(page: Page) {
  await page.click('a[href="/chats"]');
  await page.click('.chat-item');
  await expect(page.locator('.message-form textarea')).toBeVisible();
  await expect(
    page.getByRole('link', { name: PULL_REQUESTS[0].title, exact: true })
  ).toBeVisible();
}

test.describe('Chat changes and gallery panel', () => {
  test.beforeEach(async ({ context, page }) => {
    await installChatSocketMock(page);
    await blockServiceWorker(context);
    await mockCommonEndpoints(page);
    await mockChat(page);
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.goto('/');
    await setupAuth(page);
    await page.reload();
    await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
    await openChat(page);
  });

  test('sits beside the transcript, resizes from its edge and keeps the width', async ({
    page,
  }, testInfo) => {
    const capture = async (name: string) => {
      if (testInfo.project.name !== 'chromium') return;
      await page.mouse.move(0, 0);
      await page.screenshot({ path: `screenshots/${name}.png`, animations: 'disabled' });
    };
    const panel = page.getByRole('complementary', { name: 'Changes, agents, tasks, media and sources' });
    await expect(panel).toBeVisible();
    await expect(panel.getByRole('button', { name: 'Gallery, 5' })).toBeVisible();
    await expect(panel.getByText('widgets #13')).toBeVisible();
    await expect(panel.getByRole('img', { name: 'Checks failing' })).toBeVisible();
    await expect(panel.getByRole('img', { name: 'Merged' })).toBeVisible();
    await expect(
      panel.getByRole('img', { name: '2 open review threads, 3 resolved' })
    ).toBeVisible();
    await expect(
      panel.getByRole('img', { name: '0 open review threads, 4 resolved' })
    ).toBeVisible();

    const transcript = await page.locator('.messages-container').boundingBox();
    const beside = await panel.boundingBox();
    expect(transcript && beside).toBeTruthy();
    expect(transcript!.x + transcript!.width).toBeLessThanOrEqual(beside!.x + 1);
    await capture('chats-panel');

    await page.evaluate(() => document.documentElement.setAttribute('data-theme', 'dark'));
    await capture('chats-panel-dark');
    await page.evaluate(() => document.documentElement.setAttribute('data-theme', 'light'));

    const separator = panel.getByRole('separator', { name: 'Resize changes, agents, tasks, media and sources' });
    const before = Number(await separator.getAttribute('aria-valuenow'));
    await separator.focus();
    await page.keyboard.press('ArrowLeft');
    await page.keyboard.press('ArrowLeft');
    await expect(separator).toHaveAttribute('aria-valuenow', String(before + 32));

    const edge = await separator.boundingBox();
    await page.mouse.move(edge!.x + edge!.width / 2, edge!.y + 200);
    await page.mouse.down();
    await page.mouse.move(edge!.x - 60, edge!.y + 200, { steps: 4 });
    await page.mouse.up();
    const dragged = Number(await separator.getAttribute('aria-valuenow'));
    expect(dragged).toBeGreaterThan(before + 32);

    await page.reload();
    await openChat(page);
    await expect(
      page
        .getByRole('complementary', { name: 'Changes, agents, tasks, media and sources' })
        .getByRole('separator', { name: 'Resize changes, agents, tasks, media and sources' })
    ).toHaveAttribute('aria-valuenow', String(dragged));
    await capture('chats-panel-wide');

    await page.getByRole('button', { name: 'Hide changes, agents, tasks, media and sources' }).click();
    await expect(page.getByRole('separator', { name: 'Resize changes, agents, tasks, media and sources' })).toHaveCount(0);
    await capture('chats-panel-hidden');
    await page.getByRole('button', { name: /Show changes, agents, tasks, media and sources/ }).click();
    await expect(page.getByRole('separator', { name: 'Resize changes, agents, tasks, media and sources' })).toBeVisible();
  });

  test('offers what Zone can do to a pull request, and shows its changes', async ({
    page,
  }, testInfo) => {
    const capture = async (name: string) => {
      if (testInfo.project.name !== 'chromium') return;
      await page.screenshot({ path: `screenshots/${name}.png`, animations: 'disabled' });
    };
    await page.getByRole('button', { name: 'Actions for widgets #14' }).click();
    const merge = page.getByRole('menuitem', { name: /Merge when ready/ });
    await expect(merge).toHaveAttribute('aria-disabled', 'true');
    await expect(merge).toContainText(PENDING_RELEASE);
    const ready = page.getByRole('menuitem', { name: /^Ready for review/ });
    await expect(ready).toHaveAttribute('aria-disabled', 'true');
    await expect(ready).toContainText(PENDING_RELEASE);
    await expect(page.getByRole('menuitem', { name: 'Open on GitHub' })).toBeEnabled();
    await capture('chats-panel-menu');
    await page.keyboard.press('Escape');

    await page.getByRole('button', { name: 'Actions for widgets #12' }).click();
    await page.getByRole('menuitem', { name: 'View changes' }).click();
    const diff = page.getByRole('dialog', { name: 'Changes in widgets #12' });
    await expect(diff.getByText('src/filter.rs')).toBeVisible();
    await expect(diff.getByText('+pub struct Filter {')).toBeVisible();
    await capture('chats-panel-diff');
  });

  test('opens media full size and steps through it from the keyboard', async ({
    page,
  }, testInfo) => {
    await page.getByRole('button', { name: 'Open harbour.svg' }).click();
    const lightbox = page.getByRole('dialog', { name: 'harbour.svg' });
    await expect(lightbox.getByText('2 of 5')).toBeVisible();
    await expect(lightbox.getByRole('img', { name: 'harbour.svg' })).toBeVisible();
    if (testInfo.project.name === 'chromium') {
      await page.screenshot({ path: 'screenshots/chats-panel-lightbox.png', animations: 'disabled' });
    }

    await page.keyboard.press('ArrowRight');
    await expect(page.getByRole('dialog', { name: 'meadow.svg' })).toBeVisible();
    await page.keyboard.press('ArrowLeft');
    await page.keyboard.press('ArrowLeft');
    await expect(page.getByRole('dialog', { name: 'dawn.svg' })).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(page.getByRole('dialog')).toHaveCount(0);

    await page.getByRole('button', { name: 'Open dusk.svg' }).click();
    await page.getByRole('button', { name: 'Show in chat' }).click();
    await expect(page).toHaveURL(/message=msg-art/);
    await expect(page.locator('#chat-message-msg-art')).toHaveClass(/message--linked/);
  });

  test('lists the chat\'s sources once each, newest first, and collapses them', async ({
    page,
  }, testInfo) => {
    const capture = async (name: string) => {
      if (testInfo.project.name !== 'chromium') return;
      await page.mouse.move(0, 0);
      await page.screenshot({ path: `screenshots/${name}.png`, animations: 'disabled' });
    };
    const panel = page.getByRole('complementary', { name: 'Changes, agents, tasks, media and sources' });
    await panel.getByRole('button', { name: 'Gallery, 5' }).click();
    const toggle = panel.getByRole('button', { name: 'Sources, 4' });
    await expect(toggle).toHaveAttribute('aria-expanded', 'true');
    const items = panel.getByTestId('citation');
    await expect(items).toHaveCount(4);
    await expect(items.first()).toContainText(`widgets ${REVISION.slice(0, 7)}`);
    await expect(items.first()).toContainText('GitHub build');
    await expect(items.nth(1)).toContainText('#12 Add the widget catalogue');
    await expect(items.nth(1)).toContainText('20 of 21 checks still running');
    await expect(panel.getByText('Checks queued.')).toHaveCount(0);
    await expect(items.nth(2)).toContainText('Incomplete evidence');
    await expect(page.locator('.message-citations')).toHaveCount(0);
    await toggle.scrollIntoViewIfNeeded();
    await capture('chats-panel-sources');

    await toggle.click();
    await expect(toggle).toHaveAttribute('aria-expanded', 'false');
    await expect(items.first()).toBeHidden();
    await capture('chats-panel-sources-collapsed');

    await page.reload();
    await openChat(page);
    const reopened = page
      .getByRole('complementary', { name: 'Changes, agents, tasks, media and sources' })
      .getByRole('button', { name: /^Sources, \d+$/ });
    await expect(reopened).toHaveAttribute('aria-expanded', 'false');

    await page.locator('#chat-message-msg-pr').getByRole('button', { name: '3 sources' }).click();
    await expect(reopened).toHaveAttribute('aria-expanded', 'true');
    await expect(panel.getByText('Cited in this reply')).toBeVisible();
    await expect(panel.getByTestId('citation')).toHaveCount(3);
    await expect(panel.locator('.citation-link').first()).toBeFocused();

    await page.locator('#chat-message-msg-art').getByRole('button', { name: '2 sources' }).click();
    const own = panel.getByTestId('citation');
    await expect(own).toHaveCount(2);
    await expect(own.first()).toContainText('Checks queued.');
    await expect(panel.getByText('20 of 21 checks still running')).toHaveCount(0);
    await capture('chats-panel-sources-shown');

    await panel.getByRole('button', { name: 'All 4 sources' }).click();
    await expect(panel.getByTestId('citation')).toHaveCount(4);
    await expect(panel.getByText('Cited in this reply')).toHaveCount(0);

    await page
      .locator('#chat-message-msg-art')
      .getByRole('button', { name: '[web:c0ffee] (source: Catalogue cover guidelines)' })
      .click();
    const marked = panel.locator('.citation.is-highlighted');
    await expect(marked).toHaveCount(1);
    await expect(marked).toContainText('Catalogue cover guidelines');
    await capture('chats-panel-sources-marker');
  });

  test('leaves the sources out of a chat whose replies cite nothing', async ({
    page,
  }, testInfo) => {
    await mockChat(page, uncited());
    await page.reload();
    await openChat(page);
    const panel = page.getByRole('complementary', { name: 'Changes, agents, tasks, media and sources' });
    await expect(panel.getByRole('button', { name: 'Gallery, 5' })).toBeVisible();
    await expect(panel.getByRole('button', { name: /^Sources/ })).toHaveCount(0);
    await expect(page.locator('.message-sources')).toHaveCount(0);
    if (testInfo.project.name === 'chromium') {
      await page.mouse.move(0, 0);
      await page.screenshot({
        path: 'screenshots/chats-panel-no-sources.png',
        animations: 'disabled',
      });
    }
  });

  test('lets the viewer drag the boxes into their own order, and keeps it', async ({
    page,
  }, testInfo) => {
    const capture = async (name: string) => {
      if (testInfo.project.name !== 'chromium') return;
      await page.screenshot({ path: `screenshots/${name}.png`, animations: 'disabled' });
    };
    const panel = page.getByRole('complementary', { name: 'Changes, agents, tasks, media and sources' });
    await panel.getByRole('button', { name: 'Sources, 4' }).click();
    await expect(boxOrder(panel)).resolves.toEqual(['changes', 'sources', 'gallery']);
    await page.mouse.move(0, 0);
    await capture('chats-panel-order-default');

    const grip = await panel.getByRole('button', { name: 'Move Gallery' }).boundingBox();
    const changes = await panel.locator('[data-box="changes"]').boundingBox();
    await page.mouse.move(grip!.x + grip!.width / 2, grip!.y + grip!.height / 2);
    await page.mouse.down();
    await page.mouse.move(grip!.x + grip!.width / 2, changes!.y + 12, { steps: 12 });
    await expect(panel.locator('[data-box="changes"]')).toHaveAttribute('data-drop', 'before');
    await expect(panel.locator('[data-box="gallery"]')).toHaveAttribute('data-dragging', 'true');
    await expect(page.locator('.chat-panel-box-ghost')).toHaveText('Gallery');
    await capture('chats-panel-order-dragging');
    await page.mouse.up();

    await expect(panel.locator('[data-drop]')).toHaveCount(0);
    await expect(boxOrder(panel)).resolves.toEqual(['gallery', 'changes', 'sources']);
    await page.reload();
    await openChat(page);
    await expect(boxOrder(panel)).resolves.toEqual(['gallery', 'changes', 'sources']);
    await page.mouse.move(0, 0);
    await capture('chats-panel-order-saved');
  });

  test('moves a box from the keyboard in the sheet on a phone', async ({ page }) => {
    await page.setViewportSize({ width: 375, height: 812 });
    await page.getByRole('button', { name: /^Changes, agents, tasks, media and sources: / }).click();
    const sheet = page.getByRole('dialog', { name: 'Changes, agents, tasks, media and sources' });
    const grip = sheet.getByRole('button', { name: 'Move Changes' });

    await grip.focus();
    await page.keyboard.press('Space');
    await expect(sheet.getByText('Picked up Changes, at position 1 of 3.')).toBeAttached();
    await keyboardSensorListening(page);
    await page.keyboard.press('ArrowDown');
    await expect(sheet.getByText('Changes would move to position 2 of 3.')).toBeAttached();
    await page.keyboard.press('Escape');
    await expect(
      sheet.getByText('Moving Changes cancelled. It stays at position 1 of 3.')
    ).toBeAttached();
    await expect(sheet).toBeVisible();
    await expect(boxOrder(sheet)).resolves.toEqual(['changes', 'sources', 'gallery']);

    await grip.focus();
    await page.keyboard.press('Space');
    await expect(sheet.getByText('Picked up Changes, at position 1 of 3.')).toBeAttached();
    await keyboardSensorListening(page);
    await page.keyboard.press('ArrowDown');
    await expect(sheet.getByText('Changes would move to position 2 of 3.')).toBeAttached();
    await page.keyboard.press('ArrowDown');
    await expect(sheet.getByText('Changes would move to position 3 of 3.')).toBeAttached();
    await page.keyboard.press('Space');

    await expect(sheet.getByText('Changes dropped at position 3 of 3.')).toBeAttached();
    await expect(boxOrder(sheet)).resolves.toEqual(['sources', 'gallery', 'changes']);
    await expect(sheet.getByRole('button', { name: 'Move Changes' })).toBeFocused();
  });

  test('lists the tasks the chat started, newest first, and opens one', async ({
    page,
  }, testInfo) => {
    const capture = async (name: string) => {
      if (testInfo.project.name !== 'chromium') return;
      await page.mouse.move(0, 0);
      await page.screenshot({ path: `screenshots/${name}.png`, animations: 'disabled' });
    };
    await mockTasks(page, TASKS);
    await page.reload();
    await openChat(page);
    const panel = page.getByRole('complementary', { name: 'Changes, agents, tasks, media and sources' });
    const toggle = panel.getByRole('button', { name: 'Tasks, 4' });
    await expect(toggle).toHaveAttribute('aria-expanded', 'true');
    const rows = panel.locator('.chat-task');
    await expect(rows).toHaveCount(4);
    await expect(rows.first()).toContainText('Round cart prices to the cent');
    await expect(rows.first()).toContainText('In progress');
    await expect(rows.first()).toContainText('Running, using tools');
    await expect(
      panel.getByRole('progressbar', { name: 'Round cart prices to the cent progress' })
    ).toHaveAttribute('aria-valuenow', '45');
    await expect(rows.nth(1)).toContainText('In review');
    await expect(rows.nth(1).getByRole('link', { name: 'widgets #12' })).toHaveAttribute(
      'href',
      'https://github.com/acme/widgets/pull/12'
    );
    await expect(rows.nth(2)).toContainText('Run failed');
    await expect(rows.nth(3)).toContainText('Created');
    await toggle.scrollIntoViewIfNeeded();
    await capture('chats-panel-tasks');

    await toggle.click();
    await expect(toggle).toHaveAttribute('aria-expanded', 'false');
    await expect(rows.first()).toBeHidden();
    await capture('chats-panel-tasks-collapsed');

    await page.reload();
    await openChat(page);
    const reopened = page
      .getByRole('complementary', { name: 'Changes, agents, tasks, media and sources' })
      .getByRole('button', { name: 'Tasks, 4' });
    await expect(reopened).toHaveAttribute('aria-expanded', 'false');
    await reopened.click();

    await page
      .getByRole('link', { name: 'Round cart prices to the cent', exact: true })
      .click();
    await expect(page).toHaveURL(/\/tasks\?id=task-3$/);
  });

  test('leaves the tasks out of a chat that started none', async ({ page }, testInfo) => {
    const panel = page.getByRole('complementary', { name: 'Changes, agents, tasks, media and sources' });
    await expect(panel.getByRole('button', { name: 'Gallery, 5' })).toBeVisible();
    await expect(panel.getByRole('button', { name: /^Tasks/ })).toHaveCount(0);
    if (testInfo.project.name === 'chromium') {
      await page.mouse.move(0, 0);
      await page.screenshot({
        path: 'screenshots/chats-panel-no-tasks.png',
        animations: 'disabled',
      });
    }
  });

  test('shows the tasks in the sheet on a phone', async ({ page }, testInfo) => {
    await mockTasks(page, TASKS);
    await page.reload();
    await openChat(page);
    await page.setViewportSize({ width: 375, height: 812 });
    await page
      .getByRole('button', {
        name: 'Changes, agents, tasks, media and sources: 4 changes, 4 tasks, 5 media items, 4 sources',
      })
      .click();
    const sheet = page.getByRole('dialog', { name: 'Changes, agents, tasks, media and sources' });
    await sheet.getByRole('button', { name: 'Changes, 4' }).click();
    await expect(sheet.getByRole('button', { name: 'Tasks, 4' })).toBeVisible();
    await expect(sheet.getByText('Round cart prices to the cent')).toBeVisible();
    if (testInfo.project.name === 'chromium') {
      await page.screenshot({
        path: 'screenshots/chats-panel-phone-tasks.png',
        animations: 'disabled',
      });
    }
  });

  test('becomes a sheet on a phone instead of covering the transcript', async ({
    page,
  }, testInfo) => {
    const capture = async (name: string) => {
      if (testInfo.project.name !== 'chromium') return;
      await page.screenshot({ path: `screenshots/${name}.png`, animations: 'disabled' });
    };
    await page.setViewportSize({ width: 375, height: 812 });
    await expect(page.getByRole('complementary', { name: 'Changes, agents, tasks, media and sources' })).toBeHidden();
    const toggle = page.getByRole('button', {
      name: 'Changes, agents, tasks, media and sources: 4 changes, 5 media items, 4 sources',
    });
    await expect(toggle).toHaveText('4 changes, 5 media items, 4 sources');
    await expect(toggle).toBeVisible();

    await expect
      .poll(async () => {
        const toggleBox = await toggle.boundingBox();
        const transcript = await page.locator('.messages-container').boundingBox();
        return Boolean(
          toggleBox &&
            transcript &&
            toggleBox.y + toggleBox.height <= transcript.y + 1 &&
            toggleBox.x + toggleBox.width <= 375
        );
      })
      .toBe(true);
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(
      375
    );
    await capture('chats-panel-phone');

    await toggle.click();
    const sheet = page.getByRole('dialog', { name: 'Changes, agents, tasks, media and sources' });
    await expect(sheet.getByText('widgets #12')).toBeVisible();
    await expect(sheet.getByRole('button', { name: 'Open dawn.svg' })).toBeVisible();
    await expect(sheet.getByRole('button', { name: 'Sources, 4' })).toBeVisible();
    await capture('chats-panel-phone-sheet');
    await page.keyboard.press('Escape');

    await page.locator('#chat-message-msg-art').getByRole('button', { name: '2 sources' }).click();
    const shown = page.getByRole('dialog', { name: 'Changes, agents, tasks, media and sources' });
    await expect(shown.getByText('Cited in this reply')).toBeVisible();
    await expect(shown.getByTestId('citation')).toHaveCount(2);
    await expect(shown.getByText('Checks queued.')).toBeVisible();
    await capture('chats-panel-phone-sources');
  });
});
