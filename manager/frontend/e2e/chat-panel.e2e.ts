import type { Page } from '@playwright/test';
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
    content: 'Here are the other three covers, and a short theme for the launch video.',
    created_at: sentAt,
    metadata: {
      attachments: [
        ...PICTURES.slice(1).map((item) => ({ ...item, mime: 'image/svg+xml' })),
        { name: 'launch-theme.mp3', mime: 'audio/mpeg', url: 'data:audio/mpeg;base64,AA==' },
      ],
    },
  },
  {
    id: 'msg-pr',
    chat_id: 'chat-1',
    role: 'assistant',
    content: 'I opened acme/widgets#12 with the new widget and its tests.',
    created_at: sentAt,
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
    actionable: true,
  };
}

const PULL_REQUESTS = [
  pullRequest(12, 'Add the widget catalogue with search and filters', {
    mergeable: 'blocked',
    checks: 'pending',
  }),
  pullRequest(13, 'Fix the price rounding in the cart', {
    checks: 'failing',
    mergeable: 'unstable',
    additions: 9,
    deletions: 4,
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
  }),
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

async function mockChat(page: Page) {
  await routeApi(page, /\/api\/chats($|\?|\/)/i, (route) => {
    if (CHAT_PROVIDERS.test(route.request().url())) return route.fallback();
    const url = route.request().url();
    if (route.request().method() !== 'GET') return route.continue();
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify(
        url.includes('/chat-1') ? { chat: { ...chat, messages } } : { chats: [chat] }
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
}

async function openChat(page: Page) {
  await page.click('a[href="/chats"]');
  await page.click('.chat-item');
  await expect(page.locator('.message-form textarea')).toBeVisible();
  await expect(page.getByRole('link', { name: PULL_REQUESTS[0].title })).toBeVisible();
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
    const panel = page.getByRole('complementary', { name: 'Changes and media' });
    await expect(panel).toBeVisible();
    await expect(panel.getByRole('button', { name: 'Gallery, 5' })).toBeVisible();
    await expect(panel.getByText('widgets #13')).toBeVisible();
    await expect(panel.getByRole('img', { name: 'Checks failing' })).toBeVisible();
    await expect(panel.getByRole('img', { name: 'Merged' })).toBeVisible();

    const transcript = await page.locator('.messages-container').boundingBox();
    const beside = await panel.boundingBox();
    expect(transcript && beside).toBeTruthy();
    expect(transcript!.x + transcript!.width).toBeLessThanOrEqual(beside!.x + 1);
    await capture('chats-panel');

    await page.evaluate(() => document.documentElement.setAttribute('data-theme', 'dark'));
    await capture('chats-panel-dark');
    await page.evaluate(() => document.documentElement.setAttribute('data-theme', 'light'));

    const separator = panel.getByRole('separator', { name: 'Resize changes and media' });
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
        .getByRole('complementary', { name: 'Changes and media' })
        .getByRole('separator', { name: 'Resize changes and media' })
    ).toHaveAttribute('aria-valuenow', String(dragged));
    await capture('chats-panel-wide');

    await page.getByRole('button', { name: 'Hide changes and media' }).click();
    await expect(page.getByRole('separator', { name: 'Resize changes and media' })).toHaveCount(0);
    await capture('chats-panel-hidden');
    await page.getByRole('button', { name: /Show changes and media/ }).click();
    await expect(page.getByRole('separator', { name: 'Resize changes and media' })).toBeVisible();
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

  test('becomes a sheet on a phone instead of covering the transcript', async ({
    page,
  }, testInfo) => {
    const capture = async (name: string) => {
      if (testInfo.project.name !== 'chromium') return;
      await page.screenshot({ path: `screenshots/${name}.png`, animations: 'disabled' });
    };
    await page.setViewportSize({ width: 375, height: 812 });
    await expect(page.getByRole('complementary', { name: 'Changes and media' })).toBeHidden();
    const toggle = page.getByRole('button', { name: /^Changes and media/ });
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
    const sheet = page.getByRole('dialog', { name: 'Changes and media' });
    await expect(sheet.getByText('widgets #12')).toBeVisible();
    await expect(sheet.getByRole('button', { name: 'Open dawn.svg' })).toBeVisible();
    await capture('chats-panel-phone-sheet');
  });
});
