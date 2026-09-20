import { api, expect, signIn, state, test, tokenFor } from './harness';

/**
 * The layout contract every page is held to, measured live at the smallest
 * supported desktop viewport: nothing is clipped without a scrollbar, the page
 * bar stays one 48px row, sibling cards share a height, and a fieldset's title
 * sits inside its box rather than on its border.
 */

const VIEWPORT = { width: 1280, height: 720 };
const BAR_HEIGHT = 48;
const ROUTES = [
  '/chats',
  '/projects',
  '/tasks',
  '/sources',
  '/search',
  '/models',
  '/wiki',
  '/org-settings',
  '/settings',
  '/sessions',
];

type Clipped = { text: string; bottom: number };

/** Every visible element that ends below the viewport must sit inside a scroller. */
async function clippedWithoutScroll(page: import('@playwright/test').Page): Promise<Clipped[]> {
  return page.evaluate((viewportHeight) => {
    const scrolls = (element: Element) => {
      const style = getComputedStyle(element);
      return (
        /(auto|scroll)/.test(style.overflowY) && element.scrollHeight > element.clientHeight + 1
      );
    };
    const insideScroller = (element: Element) => {
      for (let node = element.parentElement; node; node = node.parentElement) {
        if (scrolls(node)) return true;
      }
      return document.documentElement.scrollHeight > viewportHeight + 1;
    };
    const offenders: { text: string; bottom: number }[] = [];
    for (const element of document.body.querySelectorAll('*')) {
      if (element.closest('[role="dialog"], .toast-container, [data-sonner-toaster]')) continue;
      const style = getComputedStyle(element);
      if (style.position === 'fixed' || style.visibility === 'hidden') continue;
      const rect = element.getBoundingClientRect();
      if (rect.height === 0 || rect.width === 0) continue;
      if (rect.bottom <= viewportHeight + 1) continue;
      const text = (element.textContent ?? '').trim().slice(0, 40);
      if (!text && element.tagName !== 'IMG') continue;
      if (insideScroller(element)) continue;
      offenders.push({ text: `${element.tagName.toLowerCase()} ${text}`, bottom: rect.bottom });
    }
    return offenders.slice(0, 5);
  }, VIEWPORT.height);
}

test.use({ viewport: VIEWPORT });

/** The frame never scrolls: the document is exactly the viewport on every route. */
async function documentOverflow(page: import('@playwright/test').Page): Promise<number> {
  return page.evaluate(() => {
    const root = document.scrollingElement ?? document.documentElement;
    return root.scrollHeight - root.clientHeight;
  });
}

test('nothing is clipped without a scrollbar on any workspace page', async ({
  page,
  consoleErrors,
}) => {
  await signIn(page);
  const clipped: Record<string, Clipped[]> = {};
  const overflowing: Record<string, number> = {};

  for (const route of ROUTES) {
    await page.goto(route);
    await page.waitForLoadState('networkidle').catch(() => undefined);
    const offenders = await clippedWithoutScroll(page);
    if (offenders.length > 0) clipped[route] = offenders;
    const overflow = await documentOverflow(page);
    if (overflow !== 0) overflowing[route] = overflow;
  }

  expect(clipped).toEqual({});
  expect(overflowing).toEqual({});
  expect(consoleErrors.filter((error) => !/\b400\b.*search/.test(error))).toEqual([]);
});

test('no conversation stretches the document past the viewport', async ({ page }) => {
  await signIn(page);
  const token = await tokenFor(state.owner);
  const listed = await api('GET', `/api/chats?workspace_id=${state.owner.workspace.id}`, {
    token,
  });
  const body = listed.body as { chats?: { id: string }[] } | { id: string }[];
  const chats = Array.isArray(body) ? body : (body.chats ?? []);
  expect(chats.length).toBeGreaterThan(0);

  const overflowing: Record<string, number> = {};
  for (const chat of chats) {
    await page.goto(`/chats?id=${chat.id}`);
    await page.locator('.messages-container').waitFor({ timeout: 15_000 }).catch(() => undefined);
    await page.waitForLoadState('networkidle').catch(() => undefined);
    const overflow = await documentOverflow(page);
    if (overflow !== 0) overflowing[chat.id] = overflow;
  }

  expect(overflowing).toEqual({});
});

test('the models tabs stay scrollable and the page bar stays one row', async ({ page }) => {
  await signIn(page);
  await page.goto('/models');

  for (const tab of ['Installed', 'Browse', 'Train']) {
    await page.getByRole('tab', { name: tab }).click();
    await page.waitForLoadState('networkidle').catch(() => undefined);
    expect(await clippedWithoutScroll(page), tab).toEqual([]);
  }

  const bar = page.locator('.page-bar').first();
  const box = await bar.boundingBox();
  expect(box?.height).toBeLessThanOrEqual(BAR_HEIGHT);
  const overflowing = await bar.evaluate((element) => {
    const own = element.getBoundingClientRect();
    return [...element.querySelectorAll('*')]
      .map((child) => child.getBoundingClientRect())
      .filter((rect) => rect.height > 0 && (rect.top < own.top - 1 || rect.bottom > own.bottom + 1))
      .length;
  });
  expect(overflowing, 'the bar wrapped').toBe(0);
});

test('chat titles keep the row width and the conversation header stays one row', async ({
  page,
}) => {
  await signIn(page);
  await page.goto('/chats');
  const item = page.locator('.chat-item').first();
  await expect(item).toBeVisible();

  const widths = await item.evaluate((element) => {
    const title = element.querySelector('.chat-title');
    return {
      item: element.getBoundingClientRect().width,
      title: title?.getBoundingClientRect().width ?? 0,
      height: Math.round(element.getBoundingClientRect().height),
    };
  });
  expect(widths.title).toBeGreaterThanOrEqual(widths.item * 0.6);
  expect(widths.height).toBe(52);

  await item.click();
  const header = page.locator('.chat-header');
  await expect(header).toBeVisible();
  const box = await header.boundingBox();
  expect(box?.height).toBeLessThanOrEqual(BAR_HEIGHT);
  const wrapped = await header.evaluate((element) => {
    const own = element.getBoundingClientRect();
    return [...element.querySelectorAll('*')]
      .map((child) => child.getBoundingClientRect())
      .filter((rect) => rect.height > 0 && (rect.top < own.top - 1 || rect.bottom > own.bottom + 1))
      .length;
  });
  expect(wrapped, 'the chat header wrapped').toBe(0);

  const bubble = page.locator('.message-user .message-content').first();
  if (await bubble.count()) {
    const column = await page.locator('.messages-container').evaluate((element) => {
      const style = getComputedStyle(element);
      return element.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight);
    });
    const width = (await bubble.boundingBox())?.width ?? 0;
    expect(width).toBeLessThanOrEqual(column * 0.72 + 1);
  }
});

test('the add source tiles are one 56px row each', async ({ page }) => {
  await signIn(page);
  await page.goto('/sources');
  await page.getByRole('button', { name: /Add source/i }).first().click();

  const tiles = page.locator('.source-type-option');
  await expect(tiles.first()).toBeVisible();
  await page.waitForFunction(() =>
    document.getAnimations().every((animation) => animation.playState !== 'running')
  );
  const heights = await tiles.evaluateAll((elements) =>
    elements.map((element) => Math.round(element.getBoundingClientRect().height))
  );
  expect(heights.every((height) => height === 56), heights.join(',')).toBe(true);

  const wrapped = await tiles.evaluateAll((elements) =>
    elements.filter((element) =>
      [...element.querySelectorAll('.source-type-name, .source-type-desc')].some(
        (line) => line.getBoundingClientRect().height > 24
      )
    ).length
  );
  expect(wrapped, 'a tile line wrapped').toBe(0);
});

test('sibling cards in a grid share one height', async ({ page }) => {
  await signIn(page);

  for (const [route, selector] of [
    ['/wiki', '.knowledge-card'],
    ['/tasks', '.task-card'],
  ] as const) {
    await page.goto(route);
    await page.waitForLoadState('networkidle').catch(() => undefined);
    const heights = await page.locator(selector).evaluateAll((cards) =>
      cards.map((card) => Math.round(card.getBoundingClientRect().height))
    );
    if (heights.length < 2) continue;
    expect(new Set(heights).size, `${selector} heights ${heights.join(',')}`).toBe(1);
  }
});

test('the settings save footer stays on the pane edge at both ends of the scroll', async ({
  page,
}) => {
  await signIn(page);

  for (const [route, tab] of [
    ['/org-settings', 'AI Settings'],
    ['/settings', 'Theme'],
  ] as const) {
    await page.goto(route);
    await page.getByRole('tab', { name: tab }).click();
    const footer = page.locator('.settings-actions');
    await expect(footer).toBeVisible();

    const edges = await page.locator('.page-body').evaluate((body) => {
      const bottom = () => body.querySelector('.settings-actions')?.getBoundingClientRect().bottom;
      body.scrollTop = 0;
      const atTop = bottom();
      body.scrollTop = body.scrollHeight;
      const atEnd = bottom();
      return { pane: body.getBoundingClientRect().bottom, atTop, atEnd };
    });
    expect(Math.abs((edges.atTop ?? 0) - edges.pane), `${route} while scrolling`).toBeLessThanOrEqual(
      1
    );
    expect(
      Math.abs((edges.atEnd ?? 0) - edges.pane),
      `${route} at the end of the scroll`
    ).toBeLessThanOrEqual(1);
  }
});

test('a training target title sits inside its box, not on the border', async ({ page }) => {
  await signIn(page);
  await page.goto('/models');
  await page.getByRole('tab', { name: 'Train' }).click();

  const pixel =
    'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==';
  await page.getByLabel('Target images').setInputFiles([
    { name: 'frame-0000.png', mimeType: 'image/png', buffer: Buffer.from(pixel, 'base64') },
    { name: 'frame-0001.png', mimeType: 'image/png', buffer: Buffer.from(pixel, 'base64') },
  ]);

  const pairs = page.locator('.train-pair');
  await expect(pairs).toHaveCount(2);
  expect(await page.locator('legend').count()).toBe(0);

  const geometry = await pairs.first().evaluate((fieldset) => {
    const title = fieldset.querySelector('.train-pair-head');
    const box = fieldset.getBoundingClientRect();
    const head = title?.getBoundingClientRect();
    return { top: box.top, height: box.height, headTop: head?.top ?? -1 };
  });
  expect(geometry.headTop).toBeGreaterThanOrEqual(geometry.top + 8);
  expect(geometry.height).toBe(72);
});
