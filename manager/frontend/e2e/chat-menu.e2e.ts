import { expect, type Page, test } from '@playwright/test';
import { mockCommonEndpoints, setupAuth } from './helpers/auth';
import { blockServiceWorker, routeApi } from './test-utils';

const chat = (id: string, title: string, archived = false) => ({
  id,
  title,
  model_name: 'llama3.2',
  created_at: new Date().toISOString(),
  updated_at: new Date().toISOString(),
  archived,
  agent_enabled: false,
  messages: [],
});

interface Calls {
  archived: string[];
  unarchived: string[];
  deleted: string[];
  opened: string[];
}

async function openChats(page: Page): Promise<Calls> {
  const calls: Calls = { archived: [], unarchived: [], deleted: [], opened: [] };
  const active = [chat('chat-1', 'Garden plans'), chat('chat-2', 'Trip notes')];
  const archived = [chat('chat-3', 'Old recipes', true)];
  await mockCommonEndpoints(page);
  await setupAuth(page);
  await routeApi(page, /\/api\/chats($|\?|\/)/i, async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    const action = url.pathname.match(/\/api\/chats\/([^/]+)\/(archive|unarchive)$/);
    if (action) {
      const [, id, verb] = action;
      (verb === 'archive' ? calls.archived : calls.unarchived).push(id!);
      return route.fulfill({ json: { chat: chat(id!, 'Changed', verb === 'archive') } });
    }
    const single = url.pathname.match(/\/api\/chats\/([^/]+)$/);
    if (single && request.method() === 'DELETE') {
      calls.deleted.push(single[1]!);
      return route.fulfill({ status: 204, body: '' });
    }
    if (single) {
      calls.opened.push(single[1]!);
      const found = [...active, ...archived].find((item) => item.id === single[1]);
      return route.fulfill({ json: { chat: found } });
    }
    const listed = url.searchParams.get('archived') === 'true' ? archived : active;
    return route.fulfill({ json: { chats: listed } });
  });
  await page.goto('/chats');
  await expect(page.locator('.chat-item')).toHaveCount(2);
  return calls;
}

const row = (page: Page, title: string) => page.locator('.chat-item', { hasText: title });
const trigger = (page: Page, title: string) =>
  page.getByRole('button', { name: `Actions for ${title}`, exact: true });
const triggerElement = (page: Page, title: string) =>
  row(page, title).locator('.ui-action-menu-trigger');
const opacity = (page: Page, title: string) =>
  row(page, title)
    .locator('.chat-item-actions')
    .evaluate((element) => getComputedStyle(element).opacity);

async function shoot(page: Page, name: string): Promise<void> {
  await page.screenshot({ path: test.info().outputPath(`${name}.png`), animations: 'disabled' });
}

test.describe('chat row menu', () => {
  test.beforeEach(async ({ context, page }) => {
    await blockServiceWorker(context);
    await page.setViewportSize({ width: 1280, height: 800 });
  });

  test('opens from a hovered row, unclipped by the sidebar, and chooses without opening the chat', async ({
    page,
  }) => {
    const calls = await openChats(page);
    const first = row(page, 'Garden plans');
    expect(await opacity(page, 'Garden plans')).toBe('0');
    await first.hover();
    await expect.poll(() => opacity(page, 'Garden plans')).toBe('1');
    await expect(trigger(page, 'Garden plans')).toHaveAttribute('aria-haspopup', 'menu');
    await shoot(page, 'closed-row');

    await trigger(page, 'Garden plans').click();
    const menu = page.getByRole('menu');
    await expect(menu).toBeVisible();
    await expect(triggerElement(page, 'Garden plans')).toHaveAttribute('aria-expanded', 'true');
    await expect(menu.getByRole('menuitem')).toHaveText(['Rename', 'Archive', 'Delete']);
    await page.mouse.move(0, 0);
    await expect.poll(() => opacity(page, 'Garden plans')).toBe('1');

    const box = await menu.boundingBox();
    const viewport = page.viewportSize();
    expect(box).not.toBeNull();
    expect(box!.x + box!.width).toBeLessThanOrEqual(viewport!.width);
    expect(box!.y + box!.height).toBeLessThanOrEqual(viewport!.height);
    const topmost = await page.evaluate(
      ({ x, y }) => document.elementFromPoint(x, y)?.closest('[role="menu"]') !== null,
      { x: box!.x + box!.width / 2, y: box!.y + box!.height - 8 }
    );
    expect(topmost).toBe(true);
    await shoot(page, 'open-menu-light');

    await menu.getByRole('menuitem', { name: 'Archive' }).click();
    await expect(menu).toHaveCount(0);
    await expect.poll(() => calls.archived).toEqual(['chat-1']);
    expect(calls.opened).toEqual([]);
    await expect(page.locator('.chat-item.active')).toHaveCount(0);

    await row(page, 'Trip notes').locator('.chat-title').click();
    await expect(page.locator('.chat-item.active')).toHaveCount(1);
    await expect.poll(() => calls.opened).toEqual(['chat-2']);
  });

  test('works from the keyboard and returns focus to the trigger', async ({
    page,
    browserName,
  }) => {
    const calls = await openChats(page);
    await row(page, 'Garden plans').focus();
    await page.keyboard.press(browserName === 'webkit' ? 'Alt+Tab' : 'Tab');
    await expect(trigger(page, 'Garden plans')).toBeFocused();
    await expect.poll(() => opacity(page, 'Garden plans')).toBe('1');

    await page.keyboard.press('Enter');
    const menu = page.getByRole('menu');
    await expect(menu.getByRole('menuitem', { name: 'Rename' })).toBeFocused();
    await page.keyboard.press('ArrowDown');
    await expect(menu.getByRole('menuitem', { name: 'Archive' })).toBeFocused();
    await page.keyboard.press('Escape');
    await expect(menu).toHaveCount(0);
    await expect(trigger(page, 'Garden plans')).toBeFocused();

    await page.keyboard.press('ArrowDown');
    await expect(menu).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(trigger(page, 'Garden plans')).toBeFocused();
    await page.keyboard.press(' ');
    await expect(menu).toBeVisible();
    await page.keyboard.press('Enter');
    const name = page.getByLabel('Chat name');
    await expect(name).toBeFocused();
    await expect(name).toHaveValue('Garden plans');
    await page.keyboard.press('Escape');
    await expect(name).toHaveCount(0);
    expect(calls.opened).toEqual([]);
  });

  test('closes on an outside click and keeps a single menu open', async ({ page }) => {
    const calls = await openChats(page);
    await row(page, 'Garden plans').hover();
    await trigger(page, 'Garden plans').click();
    await expect(page.getByRole('menu')).toHaveCount(1);

    await page.mouse.click(900, 400);
    await expect(page.getByRole('menu')).toHaveCount(0);

    await row(page, 'Garden plans').hover();
    await trigger(page, 'Garden plans').click();
    const other = await row(page, 'Trip notes').boundingBox();
    await page.mouse.click(other!.x + 24, other!.y + other!.height / 2);
    await expect(page.getByRole('menu')).toHaveCount(0);
    await expect(triggerElement(page, 'Garden plans')).toHaveAttribute('aria-expanded', 'false');

    await row(page, 'Trip notes').hover();
    await trigger(page, 'Trip notes').click();
    await expect(page.getByRole('menu')).toHaveCount(1);
    await expect(triggerElement(page, 'Trip notes')).toHaveAttribute('aria-expanded', 'true');
    await expect(triggerElement(page, 'Garden plans')).toHaveAttribute('aria-expanded', 'false');
    expect(calls.opened).toEqual([]);
  });

  test('keeps the delete confirmation and leaves the row clickable after the dialog', async ({
    page,
  }) => {
    const calls = await openChats(page);
    await row(page, 'Garden plans').hover();
    await trigger(page, 'Garden plans').click();
    await page.getByRole('menuitem', { name: 'Delete' }).click();
    const confirm = page.getByRole('dialog', { name: 'Delete Chat' });
    await expect(confirm).toBeVisible();
    expect(calls.deleted).toEqual([]);
    await confirm.getByRole('button', { name: 'Delete' }).click();
    await expect.poll(() => calls.deleted).toEqual(['chat-1']);
    await expect(confirm).toHaveCount(0);

    await row(page, 'Trip notes').locator('.chat-title').click();
    await expect.poll(() => calls.opened).toEqual(['chat-2']);
  });

  test('offers Unarchive in the archived view', async ({ page }) => {
    const calls = await openChats(page);
    await page.getByRole('tab', { name: 'Archived' }).click();
    await expect(page.locator('.chat-item')).toHaveCount(1);
    await row(page, 'Old recipes').hover();
    await trigger(page, 'Old recipes').click();
    const menu = page.getByRole('menu');
    await expect(menu.getByRole('menuitem')).toHaveText(['Rename', 'Unarchive', 'Delete']);
    await shoot(page, 'archived-menu');
    await menu.getByRole('menuitem', { name: 'Unarchive' }).click();
    await expect.poll(() => calls.unarchived).toEqual(['chat-3']);
    expect(calls.opened).toEqual([]);
  });

  test('shows the open menu in dark mode', async ({ page }) => {
    await openChats(page);
    await page.evaluate(() => document.documentElement.setAttribute('data-theme', 'dark'));
    await row(page, 'Garden plans').hover();
    await trigger(page, 'Garden plans').click();
    const destructive = page.getByRole('menuitem', { name: 'Delete' });
    await expect(destructive).toBeVisible();
    const colors = await page.evaluate(() => {
      const menu = document.querySelector('[role="menu"]') as HTMLElement;
      const item = [...menu.querySelectorAll('[role="menuitem"]')].at(-1) as HTMLElement;
      return {
        menu: getComputedStyle(menu).backgroundColor,
        body: getComputedStyle(document.body).backgroundColor,
        text: getComputedStyle(item).color,
      };
    });
    expect(colors.menu).not.toBe('rgb(255, 255, 255)');
    expect(colors.text).not.toBe(colors.menu);
    await shoot(page, 'open-menu-dark');
  });
});

test.describe('chat row menu on a phone', () => {
  test.use({ viewport: { width: 375, height: 812 }, hasTouch: true });

  test.beforeEach(async ({ context }) => {
    await blockServiceWorker(context);
  });

  test('shows the trigger without hover and keeps the menu on screen', async ({
    page,
    browserName,
  }) => {
    const calls = await openChats(page);
    if (browserName === 'chromium') {
      expect(await opacity(page, 'Garden plans')).toBe('1');
    } else {
      await row(page, 'Garden plans').hover();
    }
    await shoot(page, 'phone-closed');
    await trigger(page, 'Garden plans').click();
    const menu = page.getByRole('menu');
    await expect(menu).toBeVisible();
    const box = await menu.boundingBox();
    expect(box!.x).toBeGreaterThanOrEqual(0);
    expect(box!.x + box!.width).toBeLessThanOrEqual(375);
    await shoot(page, 'phone-open');
    await menu.getByRole('menuitem', { name: 'Rename' }).click();
    await expect(page.getByLabel('Chat name')).toBeFocused();
    expect(calls.opened).toEqual([]);
  });
});
