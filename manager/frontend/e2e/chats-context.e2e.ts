import { readFileSync } from 'node:fs';
import { mkdir } from 'node:fs/promises';
import type { Page, Route } from '@playwright/test';
import { ContextUsageSchema } from '../src/features/chats/schemas';
import { test, expect } from './fixtures';
import { setupAuth, mockCommonEndpoints } from './helpers/auth';
import { installChatSocketMock } from './helpers/chatSocket';
import { blockServiceWorker, routeApi } from './test-utils';
import type { ContextUsage } from '../src/features/chats/types';
const initial = ContextUsageSchema.parse(
  JSON.parse(
    readFileSync(
      new URL('../../../runner/zone_server/tests/fixtures/context.json', import.meta.url),
      'utf8'
    )
  )
);

async function setTheme(page: Page, theme: 'dark' | 'light'): Promise<void> {
  const toggle = page.getByRole('button', { name: `Switch to ${theme} mode` });
  const opened = (page.viewportSize()?.width ?? 1280) <= 768;
  if (opened) await page.getByRole('button', { name: 'Toggle menu' }).click();
  await toggle.click();
  if (opened) await page.getByRole('button', { name: 'Toggle menu' }).click();
}

const chat = {
  id: 'chat-context',
  title: 'Context-aware conversation',
  model_name: initial.model,
  created_at: initial.updated_at,
  updated_at: initial.updated_at,
  archived: false,
  agent_enabled: true,
};
for (const width of [1280, 390]) {
  test(`context drafting and compaction at ${width}px`, async ({ context, page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await blockServiceWorker(context);
    const socket = await installChatSocketMock(page);
    await mockCommonEndpoints(page);
    let usage = initial;
    let draft = '';
    await routeApi(page, /\/api\/chats($|\?|\/)/i, async (route) => {
      if (route.request().url().endsWith('/context')) {
        draft = route.request().postDataJSON().content;
        await route.fulfill({ json: { context: usage } });
      } else if (route.request().url().includes('/chat-context')) {
        await route.fulfill({
          json: {
            chat: {
              ...chat,
              context: usage,
              messages: [
                {
                  id: 'welcome',
                  chat_id: chat.id,
                  role: 'assistant',
                  content: 'I have checked the earlier results. We can continue from here.',
                  created_at: initial.updated_at,
                },
              ],
            },
          },
        });
      } else await route.fulfill({ json: { chats: [chat] } });
    });
    await page.goto('/');
    await setupAuth(page);
    await page.goto('/chats?chat=chat-context');
    await expect(page.locator('.chat-item')).toHaveCount(1);
    await page.locator('.chat-item').click();
    const meter = page.getByRole('button', { name: /^Context / });
    await expect(meter).toContainText('9%');
    await page.locator('.message-form textarea').fill('Continue from the previous results.');
    await expect.poll(() => draft).toContain('Continue');
    await mkdir('/tmp/zone-context-ui-integration-artifacts', { recursive: true });
    await page.screenshot({
      path: `/tmp/zone-context-ui-integration-artifacts/${width}-collapsed.png`,
      fullPage: true,
      animations: 'disabled',
    });
    await meter.click();
    await expect(page.getByRole('region', { name: 'Context usage details' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Attach files' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Send', exact: true })).toBeVisible();
    await page.screenshot({
      path: `/tmp/zone-context-ui-integration-artifacts/${width}-expanded.png`,
      fullPage: true,
      animations: 'disabled',
    });
    await setTheme(page, 'dark');
    await page.screenshot({
      path: `/tmp/zone-context-ui-integration-artifacts/${width}-dark-expanded.png`,
      fullPage: true,
      animations: 'disabled',
    });
    await setTheme(page, 'light');
    await meter.press('Escape');
    await expect(meter).toHaveAttribute('aria-expanded', 'false');
    socket.setOnSend(async () => {
      await socket.emit({ type: 'message_start', message_id: 'generation', role: 'assistant' });
      await socket.emit({
        type: 'context',
        chat_id: chat.id,
        message_id: 'generation',
        usage: { ...initial, status: 'compacting' },
      });
    });
    await page.getByRole('button', { name: 'Send', exact: true }).click();
    await expect(meter).toContainText('Compacting');
    await expect(page.getByRole('button', { name: 'Stop', exact: true })).toBeVisible();
    usage = {
      ...initial,
      used: 1864,
      remaining: 21074,
      breakdown: { ...initial.breakdown, conversation: 200, results: 0, summary: 200 },
      status: 'compacted',
      compacted_messages: 8,
      revision: 2,
    };
    await socket.emit({ type: 'context', chat_id: chat.id, message_id: 'generation', usage });
    await expect(meter).toContainText('Compacted');
    await socket.emit({
      type: 'message_end',
      message_id: 'generation',
      content: 'Ready to continue.',
    });
    await page.reload();
    await expect(meter).toContainText('Compacted');
    usage = {
      ...initial,
      limit: null,
      threshold: null,
      remaining: null,
      source: 'unknown',
      incomplete: true,
      reason: 'The provider did not report an effective context capacity.',
    };
    await page.locator('.message-form textarea').fill('Another draft');
    await expect(meter).not.toContainText('%');
    await meter.click();
    await expect(page.getByText(usage.reason!)).toBeVisible();
    expect(
      await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)
    ).toBe(true);
    if (width === 390) {
      await page.setViewportSize({ width: 844, height: 390 });
      await page.screenshot({
        path: '/tmp/zone-context-ui-integration-artifacts/landscape-expanded.png',
        fullPage: true,
        animations: 'disabled',
      });
      await expect(page.getByRole('button', { name: 'Send', exact: true })).toBeInViewport();
    }
  });
}

test('empty chat previews and context updates survive adversarial frame ordering', async ({
  browserName,
  context,
  page,
}) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await blockServiceWorker(context);
  const socket = await installChatSocketMock(page);
  await mockCommonEndpoints(page);
  let usage: ContextUsage = initial;
  let held: Route | null = null;
  let hold = false;
  let draft = '';
  await routeApi(page, /\/api\/chats($|\?|\/)/i, async (route) => {
    if (route.request().url().endsWith('/context')) {
      draft = route.request().postDataJSON().content;
      if (hold) {
        held = route;
        return;
      }
      await route.fulfill({ json: { context: usage } });
    } else if (route.request().url().includes('/chat-context')) {
      await route.fulfill({ json: { chat: { ...chat, messages: [], context: usage } } });
    } else await route.fulfill({ json: { chats: [chat] } });
  });
  await page.goto('/');
  await setupAuth(page);
  await page.goto('/chats?chat=chat-context');
  await page.locator('.chat-item').click();
  const meter = page.getByRole('button', { name: /^Context / });
  const input = page.locator('.message-form textarea');
  await expect(meter).toContainText('9%');
  await meter.focus();
  await page.keyboard.press('Enter');
  await expect(page.getByRole('region', { name: 'Context usage details' })).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(meter).toBeFocused();
  const forwardTab = browserName === 'webkit' ? 'Alt+Tab' : 'Tab';
  await page.keyboard.press(forwardTab);
  await expect(page.getByRole('button', { name: 'Attach files' })).toBeFocused();
  await page.keyboard.press(forwardTab);
  await expect(input).toBeFocused();
  hold = true;
  await page.keyboard.type('First draft');
  await expect.poll(() => held !== null).toBe(true);
  expect(draft).toBe('First draft');
  usage = {
    ...initial,
    used: 4064,
    remaining: 18874,
    breakdown: { ...initial.breakdown, conversation: 2200 },
  };
  await socket.emit({ type: 'context', chat_id: chat.id, message_id: null, usage });
  await expect(meter).toContainText('12%');
  hold = false;
  await (held as unknown as Route).fulfill({ json: { context: initial } });
  await expect(meter).toContainText('12%');
  let generation = 'first';
  socket.setOnSend(async () => {
    await socket.emit({ type: 'message_start', message_id: generation, role: 'assistant' });
    await socket.emit({
      type: 'context',
      chat_id: chat.id,
      message_id: generation,
      usage: { ...usage, status: 'compacting' },
    });
  });
  await input.press('Enter');
  await expect(meter).toContainText('Compacting');
  await socket.emit({ type: 'context', chat_id: chat.id, message_id: 'first', usage });
  await socket.emit({ type: 'message_end', message_id: 'first', content: 'First answer' });
  generation = 'second';
  await input.fill('Continue');
  await input.press('Enter');
  await expect(page.getByRole('button', { name: 'Stop', exact: true })).toBeVisible();
  await socket.emit({ type: 'message_end', message_id: 'first', content: 'Stale terminal' });
  await socket.emit({ type: 'cancelled', message_id: 'first' });
  await socket.emit({
    type: 'context',
    chat_id: chat.id,
    message_id: 'first',
    usage: { ...usage, status: 'blocked' },
  });
  await socket.emit({ type: 'context', chat_id: chat.id, message_id: 'second', usage });
  await expect(meter).not.toContainText('Needs attention');
  await expect(page.getByRole('button', { name: 'Stop', exact: true })).toBeVisible();
  socket.setOnCancel(async () => socket.emit({ type: 'cancelled', message_id: 'second' }));
  await page.getByRole('button', { name: 'Stop', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Send', exact: true })).toBeVisible();
  await socket.emit({
    type: 'context',
    chat_id: chat.id,
    message_id: 'second',
    usage: { ...usage, status: 'compacting' },
  });
  await expect(meter).not.toContainText('Compacting');
  usage = {
    ...initial,
    status: 'blocked',
    reason: 'The summary could not be prepared. History is unchanged.',
  };
  await input.fill('Another draft');
  await expect(meter).toContainText('Needs attention');
  await meter.click();
  await expect(page.getByText(usage.reason!)).toBeInViewport();
  await mkdir('/tmp/zone-context-ui-integration-artifacts', { recursive: true });
  await page.screenshot({
    path: '/tmp/zone-context-ui-integration-artifacts/blocked-expanded.png',
    fullPage: true,
    animations: 'disabled',
  });
  usage = {
    ...initial,
    incomplete: true,
    breakdown: { ...initial.breakdown, attachments: null },
    reason: 'Image token costs are unknown.',
  };
  await input.fill('Draft with an image');
  await expect(meter).not.toContainText('%');
  await expect(page.getByText('Image token costs are unknown.')).toBeVisible();
  hold = true;
  held = null;
  await socket.emit({ type: 'message_start', message_id: 'interrupted', role: 'assistant' });
  await socket.emit({
    type: 'context',
    chat_id: chat.id,
    message_id: 'interrupted',
    usage: { ...initial, status: 'compacting' },
  });
  await socket.disconnect();
  await expect(meter).toContainText('Unavailable');
  await expect(
    page.getByText(/Connection interrupted during generation.*last observation/)
  ).toBeInViewport();
  await page.screenshot({
    path: '/tmp/zone-context-ui-integration-artifacts/interrupted-expanded.png',
    fullPage: true,
    animations: 'disabled',
  });
  await page.setViewportSize({ width: 390, height: 900 });
  await page.screenshot({
    path: '/tmp/zone-context-ui-integration-artifacts/interrupted-mobile-expanded.png',
    fullPage: true,
    animations: 'disabled',
  });
  await setTheme(page, 'dark');
  await page.screenshot({
    path: '/tmp/zone-context-ui-integration-artifacts/interrupted-mobile-dark-expanded.png',
    fullPage: true,
    animations: 'disabled',
  });
  await expect(
    page.getByText(/Connection interrupted during generation.*last observation/)
  ).toBeInViewport();
  await expect(page.getByRole('button', { name: 'Send', exact: true })).toBeInViewport();
  await setTheme(page, 'light');

  await expect.poll(() => held !== null).toBe(true);
  hold = false;
  usage = initial;
  await (held as unknown as Route).fulfill({ json: { context: initial } });
  await expect(meter).toContainText('9%');
  await expect(meter).not.toContainText('Unavailable');
  await page.reload();
  await expect(meter).toContainText('9%');
});
