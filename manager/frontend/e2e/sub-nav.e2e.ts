import type { Locator, Page } from '@playwright/test';
import { expect, test } from './fixtures';
import { setupAdminAuth, setupCommonRoutes } from './layout-fixtures';
import { blockServiceWorker } from './test-utils';

test.describe('Sub nav resize', () => {
  test.use({ viewport: { width: 1440, height: 1000 } });

  test.beforeEach(async ({ page, context }) => {
    await blockServiceWorker(context);
    await setupCommonRoutes(page, true);
    await page.goto('/login', { waitUntil: 'domcontentloaded' });
    await setupAdminAuth(page);
  });

  test('drags the chat list wider and restores the width after reload', async ({
    page,
  }) => {
    await page.goto('/chats');
    await expect(page.locator('.chat-item').first()).toBeVisible();
    await dragWider(page, page.locator('.chats-sidebar'), 'Resize chat list');
  });

  test('drags the project list wider', async ({ page }) => {
    await page.goto('/projects');
    await expect(page.locator('.project-card').first()).toBeVisible();
    await dragWider(
      page,
      page.locator('.projects-list-pane'),
      'Resize project list'
    );
  });
});

async function dragWider(page: Page, pane: Locator, label: string): Promise<void> {
  const handle = page.getByRole('separator', { name: label });
  await expect(handle).toBeVisible();
  const before = await pane.evaluate((element) => element.getBoundingClientRect().width);
  const box = await handle.boundingBox();
  expect(box).toBeTruthy();
  const x = box!.x + box!.width / 2;
  const y = box!.y + 48;
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x + 80, y);
  await page.mouse.up();
  const after = await pane.evaluate((element) => element.getBoundingClientRect().width);
  expect(after).toBeGreaterThan(before + 40);
  await page.reload();
  await expect(handle).toBeVisible();
  const restored = await pane.evaluate((element) => element.getBoundingClientRect().width);
  expect(restored).toBeCloseTo(after, 0);
}
