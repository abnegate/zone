import { enabled, expect, shot, signIn, test } from './rig';

/**
 * A screenshot of an existing chat, for rows whose outcome arrives after the
 * lane that drove them has finished (reminders and watches firing later).
 * ZONE_SHOT_CHAT is text from the chat's title, ZONE_SHOT_NAME the file name.
 */

test.describe('screenshot of a chat', () => {
  const chat = process.env.ZONE_SHOT_CHAT ?? '';
  const route = process.env.ZONE_SHOT_ROUTE ?? '';
  const name = process.env.ZONE_SHOT_NAME ?? '';
  test.skip(
    !enabled || (!chat && !route) || !name,
    'set ZONE_SHOT_CHAT or ZONE_SHOT_ROUTE, and ZONE_SHOT_NAME',
  );

  test('opens a route and keeps a screenshot of it', async ({ page }) => {
    test.skip(!route, 'ZONE_SHOT_ROUTE');
    await signIn(page);
    await page.goto(route);
    await page.waitForTimeout(3_000);
    await shot(page, name);
  });

  test('opens the chat and keeps a screenshot of its end', async ({ page }) => {
    test.skip(!chat, 'ZONE_SHOT_CHAT');
    await signIn(page);
    await page.goto('/chats');
    const item = page.locator('.chat-item', { hasText: chat }).first();
    await expect(item).toBeVisible({ timeout: 60_000 });
    await item.click();
    await expect(page.locator('.message-user').first()).toBeVisible({
      timeout: 60_000,
    });
    await page.waitForTimeout(2_000);
    await page
      .locator('.message-assistant')
      .last()
      .scrollIntoViewIfNeeded()
      .catch(() => undefined);
    await shot(page, name);
  });
});
