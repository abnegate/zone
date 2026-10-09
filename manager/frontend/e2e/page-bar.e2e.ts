import type { Page } from '@playwright/test';
import { expect, test } from './fixtures';
import { setupAdminAuth, setupCommonRoutes } from './layout-fixtures';
import { blockServiceWorker, routeApi } from './test-utils';

const screens = [
  {
    path: '/org-settings',
    title: 'Organization Settings',
    content: '.settings-form',
    body: '.page-body',
    tabs: 'Organization settings',
  },
  {
    path: '/settings',
    title: 'Workspace Settings',
    content: '#font-family',
    body: '.page-body',
    tabs: 'Workspace settings',
  },
  { path: '/models', title: 'Models', content: '.model-item', tabs: true },
  { path: '/projects', title: 'Projects', content: '.project-card', tabs: true },
  { path: '/wiki', title: 'Knowledge Base', content: '.knowledge-card', tabs: true },
  { path: '/tasks', title: 'Tasks', content: '.task-card:not(.skeleton-card)' },
  { path: '/chats', title: 'Chats', content: '.chat-item', tabs: true },
];

async function open(page: Page, screen: (typeof screens)[number]): Promise<void> {
  await page.goto(screen.path, { waitUntil: 'domcontentloaded' });
  await expect(
    page.getByRole('heading', { level: 1, name: screen.title, exact: true })
  ).toBeVisible();
  await expect(page.locator(screen.content).first()).toBeVisible();
  await page.evaluate(() => document.fonts.ready);
}

function width(
  page: Page,
  selector = 'main.main-content'
): Promise<{ scroll: number; client: number; left: number }> {
  return page.locator(selector).evaluate((element) => ({
    scroll: element.scrollWidth,
    client: element.clientWidth,
    left: element.scrollLeft,
  }));
}

async function expectNoSidewaysScroll(
  page: Page,
  screen: (typeof screens)[number]
): Promise<void> {
  const main = await width(page);
  expect(main.scroll).toBeLessThanOrEqual(main.client);
  if (!screen.body) return;
  const body = await width(page, screen.body);
  expect(body.scroll, `${screen.body} scrolls sideways`).toBeLessThanOrEqual(body.client);
}

async function expectTabsBelowTitle(page: Page, screen: (typeof screens)[number]): Promise<void> {
  if (!screen.tabs) return;
  const title = page.locator('.page-bar-title').first();
  const tabs =
    typeof screen.tabs === 'string'
      ? page.getByRole('tablist', { name: screen.tabs })
      : page.locator('.page-bar .ui-tabs-list').first();
  const titleBox = await title.boundingBox();
  const tabsBox = await tabs.boundingBox();
  expect(titleBox).not.toBeNull();
  expect(tabsBox).not.toBeNull();
  expect(tabsBox!.y).toBeGreaterThanOrEqual(titleBox!.y + titleBox!.height - 1);
}

async function expectBoxContainsChildren(page: Page, selector: string): Promise<void> {
  const overflowing = await page.locator(selector).evaluate((element) => {
    const box = element.getBoundingClientRect();
    return [...element.children]
      .filter((child) => {
        const rect = child.getBoundingClientRect();
        if (rect.width < 1 || rect.height < 1) return false;
        return rect.right > box.right + 1 || rect.left < box.left - 1;
      })
      .map((child) => (child as HTMLElement).className || child.tagName);
  });
  expect(overflowing, `${selector} children overflow`).toEqual([]);
}

test.beforeEach(async ({ context, page }) => {
  await blockServiceWorker(context);
  await setupCommonRoutes(page, true);
  await routeApi(page, /\/organizations\/[^/]+\/audit-logs(?:\?|$)/, (route) =>
    route.fulfill({ json: { logs: [], total: 0 } })
  );
  await page.goto('/login', { waitUntil: 'domcontentloaded' });
  await setupAdminAuth(page);
});

test.describe('Page bar on a 375px phone', () => {
  test.use({ viewport: { width: 375, height: 812 } });

  test('organization settings never scrolls sideways', async ({ page }) => {
    await open(page, screens[0]);
    await expectNoSidewaysScroll(page, screens[0]);
    await expectTabsBelowTitle(page, screens[0]);
    await expectBoxContainsChildren(page, '.page-bar');
  });

  test('the last organization tab scrolls the tab list, not the page', async ({ page }) => {
    await open(page, screens[0]);
    const list = page.getByRole('tablist', { name: 'Organization settings' });
    const last = list.getByRole('tab', { name: 'Audit Logs', exact: true });

    await list.getByRole('tab', { name: 'AI Settings', exact: true }).focus();
    await page.keyboard.press('End');
    await expect(last).toBeFocused();
    await expect(last).toHaveAttribute('aria-selected', 'true');
    expect((await width(page)).left).toBe(0);

    await last.scrollIntoViewIfNeeded();
    await expect(last).toBeInViewport({ ratio: 0.95 });
    expect(await list.evaluate((element) => element.scrollLeft)).toBeGreaterThan(0);
    const main = await width(page);
    expect(main.left).toBe(0);
    expect(main.scroll).toBeLessThanOrEqual(main.client);
  });

  for (const screen of screens.slice(1)) {
    test(`${screen.title} never scrolls sideways`, async ({ page }) => {
      await open(page, screen);
      await expectNoSidewaysScroll(page, screen);
      await expectTabsBelowTitle(page, screen);
      await expectBoxContainsChildren(page, '.page-bar');
    });
  }

  test('conversation header wraps its controls instead of overflowing', async ({ page }) => {
    const model = 'orcarouter/Qwen3.8-27B-Instruct';
    const chat = {
      id: 'chat-1',
      title: 'Test 1',
      model_name: model,
      archived: false,
      agent_enabled: true,
      auto_approve: true,
      agent_sandboxed: true,
      offline: true,
      reasoning_effort: 'auto',
      context_tokens: 262144,
      created_at: '2024-01-15T10:00:00Z',
      updated_at: '2024-01-15T12:30:00Z',
      messages: [],
    };
    await routeApi(page, /\/api\/models(?:\?|$)/, (route) =>
      route.fulfill({
        json: {
          models: [
            {
              name: model,
              size: 8_000_000_000,
              modified_at: '2024-01-15T10:30:00Z',
              details: { context_length: 262144 },
              capabilities: ['tools', 'reasoning'],
              tools: true,
            },
          ],
          next_cursor: null,
        },
      })
    );
    await routeApi(page, /\/api\/chats(?:\?|$)/, (route) =>
      route.fulfill({ json: { chats: [chat] } })
    );
    await routeApi(page, /\/api\/chats\/chat-1(?:\?|$)/, (route) =>
      route.fulfill({ json: { chat } })
    );
    await page.routeWebSocket('**/ws/chats/**', (socket) => {
      socket.onMessage(() => socket.send(JSON.stringify({ type: 'authenticated' })));
    });

    await page.goto('/chats', { waitUntil: 'domcontentloaded' });
    await expect(page.locator('.chat-item').first()).toBeVisible();
    await page.locator('.chat-item').first().click();
    await expect(page.locator('.chat-header h3')).toHaveText('Test 1');
    await expect(page.getByTestId('auto-approve-toggle')).toBeVisible();
    await expect(page.getByTestId('agent-sandbox-toggle')).toBeVisible();
    await expect(page.getByTestId('context-tokens')).toBeVisible();
    await expect(page.getByTestId('reasoning-effort')).toBeVisible();
    await expect(page.getByTestId('chat-offline')).toBeVisible();

    const header = page.locator('.chat-header');
    const box = await width(page, '.chat-header');
    expect(box.scroll).toBeLessThanOrEqual(box.client);
    await expectBoxContainsChildren(page, '.chat-header');
    await expectBoxContainsChildren(page, '.chat-header-actions');
    await expectNoSidewaysScroll(page, screens[screens.length - 1]);
    const headerBox = await header.boundingBox();
    const actionsBox = await page.locator('.chat-header-actions').boundingBox();
    expect(headerBox).not.toBeNull();
    expect(actionsBox).not.toBeNull();
    expect(actionsBox!.y).toBeGreaterThanOrEqual(headerBox!.y);
    expect(actionsBox!.x + actionsBox!.width).toBeLessThanOrEqual(
      headerBox!.x + headerBox!.width + 1
    );
  });
});

test.describe('Page bar beside the sidebar at 769px', () => {
  test.use({ viewport: { width: 769, height: 900 } });

  for (const screen of screens) {
    test(`${screen.title} never scrolls sideways`, async ({ page }) => {
      await open(page, screen);
      const main = await width(page);
      expect(main.scroll).toBeLessThanOrEqual(main.client);
    });
  }
});

test.describe('Page bar on a 1440px desktop', () => {
  test.use({ viewport: { width: 1440, height: 1000 } });

  for (const screen of screens) {
    test(`${screen.title} keeps one 48px row`, async ({ page }) => {
      await open(page, screen);
      const bar = await page.locator('.page-bar').boundingBox();
      expect(bar?.height).toBeCloseTo(48, 1);
    });
  }
});
