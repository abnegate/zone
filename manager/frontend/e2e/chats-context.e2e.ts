import { mkdir } from 'node:fs/promises';
import { test, expect } from './fixtures';
import { setupAuth, mockCommonEndpoints } from './helpers/auth';
import { installChatSocketMock } from './helpers/chatSocket';
import { blockServiceWorker, routeApi } from './test-utils';
import type { ContextUsage } from '../src/features/chats/types';
const initial: ContextUsage = {
  model: 'llama3.2', used: 8400, limit: 32768, reserved: 4096, threshold: 22938, remaining: 14538,
  estimated: true, incomplete: false, source: 'configured', status: 'ready', revision: 1,
  compacted_messages: 4, updated_at: '2026-09-06T00:00:00Z',
  breakdown: { instructions: 800, conversation: 5000, tools: 600, results: 1200, summary: 600, attachments: 0, overhead: 200 },
};
const chat = { id: 'chat-context', title: 'Context-aware conversation', model_name: initial.model, created_at: initial.updated_at, updated_at: initial.updated_at, archived: false, agent_enabled: true };
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
        await route.fulfill({ json: { chat: { ...chat, context: usage, messages: [{ id: 'welcome', chat_id: chat.id, role: 'assistant', content: 'I have checked the earlier results. We can continue from here.', created_at: initial.updated_at }] } } });
      } else await route.fulfill({ json: { chats: [chat] } });
    });
    await page.goto('/');
    await setupAuth(page);
    await page.goto('/chats?chat=chat-context');
    await expect(page.locator('.chat-item')).toHaveCount(1);
    await page.locator('.chat-item').click();
    const meter = page.getByRole('button', { name: /^Context / });
    await expect(meter).toContainText('26%');
    await page.locator('.message-form textarea').fill('Continue from the previous results.');
    await expect.poll(() => draft).toContain('Continue');
    await mkdir('/tmp/zone-context-ui-artifacts', { recursive: true });
    await page.screenshot({ path: `/tmp/zone-context-ui-artifacts/${width}-collapsed.png`, fullPage: true });
    await meter.click();
    await expect(page.getByRole('region', { name: 'Context usage details' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Attach files' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Send', exact: true })).toBeVisible();
    await page.screenshot({ path: `/tmp/zone-context-ui-artifacts/${width}-expanded.png`, fullPage: true });
    await page.evaluate(() => document.documentElement.setAttribute('data-theme', 'dark'));
    await page.screenshot({ path: `/tmp/zone-context-ui-artifacts/${width}-dark-expanded.png`, fullPage: true });
    await page.evaluate(() => document.documentElement.setAttribute('data-theme', 'light'));
    await meter.press('Escape');
    await expect(meter).toHaveAttribute('aria-expanded', 'false');
    socket.setOnSend(async () => {
      await socket.emit({ type: 'message_start', message_id: 'generation', role: 'assistant' });
      await socket.emit({ type: 'context', chat_id: chat.id, message_id: 'generation', usage: { ...initial, status: 'compacting' } });
    });
    await page.getByRole('button', { name: 'Send', exact: true }).click();
    await expect(meter).toContainText('Compacting');
    await expect(page.getByRole('button', { name: 'Stop', exact: true })).toBeVisible();
    usage = { ...initial, used: 2000, remaining: 20938, breakdown: { ...initial.breakdown, conversation: 200, results: 0, summary: 200 }, status: 'compacted', compacted_messages: 8, revision: 2 };
    await socket.emit({ type: 'context', chat_id: chat.id, message_id: 'generation', usage });
    await expect(meter).toContainText('Compacted');
    await socket.emit({ type: 'message_end', message_id: 'generation', content: 'Ready to continue.' });
    await page.reload();
    await expect(meter).toContainText('Compacted');
    usage = { ...initial, limit: null, threshold: null, remaining: null, source: 'unknown', incomplete: true, reason: 'The provider did not report an effective context capacity.' };
    await page.locator('.message-form textarea').fill('Another draft');
    await expect(meter).not.toContainText('%');
    await meter.click();
    await expect(page.getByText(usage.reason!)).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
    if (width === 390) {
      await page.setViewportSize({ width: 844, height: 390 });
      await page.screenshot({ path: '/tmp/zone-context-ui-artifacts/landscape-expanded.png', fullPage: true });
      await expect(page.getByRole('button', { name: 'Send', exact: true })).toBeInViewport();
    }
  });
}
