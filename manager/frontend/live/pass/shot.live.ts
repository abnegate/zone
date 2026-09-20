import { enabled, expect, shot, signIn, test } from './rig';

/**
 * A screenshot of an existing chat, for rows whose outcome arrives after the
 * lane that drove them has finished (reminders and watches firing later).
 * ZONE_SHOT_CHAT is text from the chat's title, ZONE_SHOT_NAME the file name.
 */

test.describe('screenshot of a chat', () => {
  const chat = process.env.ZONE_SHOT_CHAT ?? '';
  const name = process.env.ZONE_SHOT_NAME ?? '';
  test.skip(
    !enabled || !chat || !name,
    'set ZONE_SHOT_CHAT and ZONE_SHOT_NAME',
  );

  test('opens the chat and keeps a screenshot of its end', async ({ page }) => {
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
