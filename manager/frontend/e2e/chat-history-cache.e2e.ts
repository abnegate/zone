import type { Page } from '@playwright/test';
import { expect, test } from './fixtures';
import { mockCommonEndpoints, setupAuth } from './helpers/auth';
import { blockServiceWorker, routeApi } from './test-utils';

const SCREENSHOT = process.env.CHAT_HISTORY_SCREENSHOT;
const PAGE_SIZE = 25;

const message = (id: string, chatId: string, role: 'user' | 'assistant', content: string) => ({
  id,
  chat_id: chatId,
  role,
  content,
  created_at: new Date().toISOString(),
});

const chatOf = (id: string, title: string, messages: ReturnType<typeof message>[]) => ({
  id,
  title,
  model_name: 'llama3.2',
  created_at: new Date().toISOString(),
  updated_at: new Date().toISOString(),
  archived: false,
  agent_enabled: false,
  messages,
});

type Listed = ReturnType<typeof chatOf>;

const GARDEN_PULL_REQUEST = {
  id: 'pr-garden',
  provider: 'github',
  owner: 'acme',
  repo: 'garden',
  number: 7,
  url: 'https://github.com/acme/garden/pull/7',
  title: 'Plant the tomatoes in the south bed',
  dismissed: false,
  created_at: new Date().toISOString(),
  status: null,
  writable: true,
  credentialed: true,
};

const LIST = 'list';

interface Server {
  fetches: (id: string) => number;
  hold: (id: string) => void;
  release: (id: string) => void;
}

async function serve(
  page: Page,
  chats: Listed[],
  answer: (chat: Listed) => Listed
): Promise<Server> {
  const fetches = new Map<string, number>();
  const held = new Set<string>();
  const waiting = new Map<string, () => void>();
  await routeApi(page, /\/api\/chats(\/[^/?]+)?($|\?)/i, async (route) => {
    const url = new URL(route.request().url());
    const id = url.pathname.match(/\/api\/chats\/([^/?]+)$/)?.[1];
    if (!id) {
      const offset = Number(url.searchParams.get('offset') ?? 0);
      if (offset > 0 && held.has(LIST)) {
        await new Promise<void>((resolve) => waiting.set(LIST, resolve));
      }
      const limit = Number(url.searchParams.get('limit') ?? chats.length);
      const listed = chats.slice(offset, offset + limit).map(({ messages: _, ...chat }) => chat);
      await route.fulfill({ json: { chats: listed, total: chats.length, limit, offset } });
      return;
    }
    const chat = chats.find((candidate) => candidate.id === id);
    if (!chat) {
      await route.fulfill({ status: 404, json: { error: 'Chat not found' } });
      return;
    }
    fetches.set(id, (fetches.get(id) ?? 0) + 1);
    if (held.has(id)) await new Promise<void>((resolve) => waiting.set(id, resolve));
    await route.fulfill({ json: { chat: answer(chat) } });
  });
  await page.routeWebSocket(/\/ws\/chats\//, () => {});
  return {
    fetches: (id) => fetches.get(id) ?? 0,
    hold: (id) => held.add(id),
    release: (id) => {
      held.delete(id);
      waiting.get(id)?.();
    },
  };
}

test.beforeEach(async ({ context, page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await blockServiceWorker(context);
  await mockCommonEndpoints(page);
  await setupAuth(page);
});

test('a listed chat opens at once from its preloaded history and reconciles behind it', async ({
  page,
}) => {
  const garden = chatOf('chat-garden', 'Garden plans', [
    message('u1', 'chat-garden', 'user', 'Where should the tomatoes go?'),
    message('a1', 'chat-garden', 'assistant', 'Put them in the sunniest bed.'),
  ]);
  const trip = chatOf('chat-trip', 'Trip ideas', [
    message('u2', 'chat-trip', 'user', 'Somewhere warm in March?'),
  ]);
  let reconciled = false;
  const server = await serve(page, [garden, trip], (chat) =>
    reconciled && chat.id === garden.id
      ? {
          ...chat,
          messages: [
            ...chat.messages,
            message('a2', 'chat-garden', 'assistant', 'Water them every morning.'),
          ],
        }
      : chat
  );

  let pullRequestReads = 0;
  let releasePullRequests: () => void = () => {};
  await routeApi(page, '**/api/chats/chat-garden/pull-requests', async (route) => {
    pullRequestReads += 1;
    if (pullRequestReads > 1) await new Promise<void>((resolve) => (releasePullRequests = resolve));
    await route.fulfill({ json: { pull_requests: [GARDEN_PULL_REQUEST] } });
  });

  await page.goto('/chats');
  await expect.poll(() => server.fetches(garden.id)).toBe(1);
  await expect.poll(() => server.fetches(trip.id)).toBe(1);
  await expect.poll(() => pullRequestReads).toBe(1);

  server.hold(garden.id);
  reconciled = true;
  await page.getByText('Garden plans', { exact: true }).click();
  await expect(page.getByText('Put them in the sunniest bed.')).toBeVisible();
  await expect(page.getByText(GARDEN_PULL_REQUEST.title)).toBeVisible();
  await expect(page.locator('.chat-placeholder .chats-loading')).toHaveCount(0);
  await expect.poll(() => pullRequestReads).toBe(2);
  await expect.poll(() => server.fetches(garden.id)).toBe(2);
  if (SCREENSHOT) await page.screenshot({ path: `${SCREENSHOT}-preloaded.png` });

  server.release(garden.id);
  releasePullRequests();
  await expect(page.getByText('Water them every morning.')).toBeVisible();

  await page.getByText('Trip ideas', { exact: true }).click();
  await expect(page.getByText('Somewhere warm in March?')).toBeVisible();
  server.hold(garden.id);
  await page.getByText('Garden plans', { exact: true }).click();
  await expect(page.getByText('Water them every morning.')).toBeVisible();
  await expect(page.locator('.chat-placeholder .chats-loading')).toHaveCount(0);
  server.release(garden.id);
});

test('the chat list pages, and each page it loads is preloaded', async ({ page }) => {
  const chats = Array.from({ length: PAGE_SIZE + 5 }, (_, index) =>
    chatOf(`chat-${index}`, `Chat number ${index}`, [
      message(`u-${index}`, `chat-${index}`, 'user', `Question ${index}`),
    ])
  );
  const last = chats[PAGE_SIZE + 4];
  const server = await serve(page, chats, (chat) => chat);

  await page.goto('/chats');
  await expect(page.getByText('Chat number 0', { exact: true })).toBeVisible();
  await expect(page.locator('.chat-item')).toHaveCount(PAGE_SIZE);
  server.hold(LIST);
  await page.getByRole('button', { name: /Load more chats \(5\)/ }).scrollIntoViewIfNeeded();
  await expect(page.getByRole('button', { name: 'Loading more chats…' })).toBeDisabled();
  if (SCREENSHOT) await page.screenshot({ path: `${SCREENSHOT}-more.png` });
  server.release(LIST);
  await expect(page.locator('.chat-item')).toHaveCount(PAGE_SIZE + 5);
  await expect(page.getByRole('button', { name: /Load more chats/ })).toHaveCount(0);
  await expect.poll(() => server.fetches(last.id)).toBe(1);

  server.hold(last.id);
  await page.getByText(last.title, { exact: true }).click();
  await expect(page.getByText(`Question ${PAGE_SIZE + 4}`)).toBeVisible();
  await expect(page.locator('.chat-placeholder .chats-loading')).toHaveCount(0);
  if (SCREENSHOT) await page.screenshot({ path: `${SCREENSHOT}-paged.png` });
  server.release(last.id);
});
