import type { Page } from '@playwright/test';
import { expect, test } from './fixtures';
import { setupAdminAuth, setupCommonRoutes } from './layout-fixtures';
import { blockServiceWorker, routeApi } from './test-utils';

const screens = [
  { path: '/org-settings', title: 'Organization Settings', content: '.settings-form' },
  { path: '/settings', title: 'Workspace Settings', content: '#font-family' },
  { path: '/models', title: 'Models', content: '.model-item' },
  { path: '/projects', title: 'Projects', content: '.project-card' },
  { path: '/wiki', title: 'Knowledge Base', content: '.knowledge-card' },
  { path: '/tasks', title: 'Tasks', content: '.task-card:not(.skeleton-card)' },
];

async function open(page: Page, screen: (typeof screens)[number]): Promise<void> {
  await page.goto(screen.path, { waitUntil: 'domcontentloaded' });
  await expect(
    page.getByRole('heading', { level: 1, name: screen.title, exact: true })
  ).toBeVisible();
  await expect(page.locator(screen.content).first()).toBeVisible();
  await page.evaluate(() => document.fonts.ready);
}

function width(page: Page): Promise<{ scroll: number; client: number; left: number }> {
  return page.locator('main.main-content').evaluate((main) => ({
    scroll: main.scrollWidth,
    client: main.clientWidth,
    left: main.scrollLeft,
  }));
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
    const main = await width(page);
    expect(main.scroll).toBeLessThanOrEqual(main.client);
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
      const main = await width(page);
      expect(main.scroll).toBeLessThanOrEqual(main.client);
    });
  }
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
