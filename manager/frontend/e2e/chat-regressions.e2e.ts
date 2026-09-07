import { test, expect } from './fixtures';
import { setupAuth, mockCommonEndpoints } from './helpers/auth';
import {
  GENERATED_ARTIFACT_URL,
  MOCK_PNG_BYTES,
  generatedAttachment,
  installChatSocketMock,
  type ChatSocketController,
} from './helpers/chatSocket';
import { blockServiceWorker, routeApi } from './test-utils';

const chat = {
  id: 'chat-1',
  title: 'Regression Chat',
  model_name: 'llama3.1',
  created_at: new Date().toISOString(),
  updated_at: new Date().toISOString(),
  archived: false,
  agent_enabled: true,
  reasoning: true,
};

const userMessage = {
  id: 'msg-user',
  chat_id: 'chat-1',
  role: 'user',
  content: 'Inspect the workspace',
  created_at: new Date().toISOString(),
};

test.describe('Chat regressions', () => {
  let socket: ChatSocketController;

  test.beforeEach(async ({ context, page }) => {
    socket = await installChatSocketMock(page);
    await blockServiceWorker(context);
    await mockCommonEndpoints(page);
    await page.unroute('**/api/models*');
    await routeApi(page, '**/api/models*', (route) => {
      const url = new URL(route.request().url());
      if (url.pathname.endsWith('/api/models/disk')) {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            used_bytes: 40,
            total_bytes: 100,
            available_bytes: 60,
            percent: 40,
          }),
        });
        return;
      }
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          models: [
            {
              name: 'llama3.1',
              size: 1,
              modified_at: '',
              completion: true,
              tools: true,
              reasoning: true,
              capabilities: ['completion', 'tools', 'reasoning'],
            },
          ],
        }),
      });
    });

    await page.goto('/');
    await setupAuth(page);
    await page.reload();
    await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
    await page.click('a[href="/chats"]');
    await expect(page).toHaveURL('/chats');
  });

  async function mockChatRoutes(
    page: Parameters<typeof routeApi>[0],
    messages: unknown[] = [],
    options: { failListAfter?: number; holdChat?: Promise<void> } = {}
  ) {
    let listGets = 0;
    await routeApi(page, /\/api\/chats($|\?|\/)/i, async (route) => {
      const url = route.request().url();
      const method = route.request().method();
      if (url.includes('/chat-1') && method === 'GET' && !url.includes('/context')) {
        await options.holdChat;
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ chat: { ...chat, messages } }),
        });
        return;
      }
      if (method === 'GET' && !url.includes('/chat-1')) {
        listGets += 1;
        if (options.failListAfter && listGets > options.failListAfter) {
          route.fulfill({
            status: 500,
            contentType: 'application/json',
            body: JSON.stringify({ error: 'list failed' }),
          });
          return;
        }
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ chats: [chat] }),
        });
        return;
      }
      route.continue();
    });
  }

  async function openChat(page: Parameters<typeof routeApi>[0]) {
    await expect(page.locator('.chat-item')).toHaveCount(1);
    await page.click('.chat-item');
    await expect(page.locator('.message-form textarea')).toBeVisible();
  }

  test('shows thinking interleaved with tools, open by default', async ({ page }, testInfo) => {
    await mockChatRoutes(page, []);
    await page.reload();
    await page.click('a[href="/chats"]');
    await openChat(page);

    socket.setOnSend(async () => {
      await socket.emit({ type: 'message_start', message_id: 'a1', role: 'assistant' });
      await socket.emit({ type: 'reasoning', content: '> Search the workspace first.' });
      await socket.emit({
        type: 'tool_call',
        message_id: 'a1',
        tool_call_id: 'call_1',
        name: 'search_knowledge',
        arguments: '{"query":"deploys"}',
        reasoning: '> Search the workspace first.',
      });
      await socket.emit({
        type: 'tool_result',
        message_id: 'a1',
        tool_call_id: 'call_1',
        name: 'search_knowledge',
        success: true,
        detail: '3 passages',
        duration_ms: 12,
      });
      await socket.emit({ type: 'reasoning', content: 'That hit looks right; read it.' });
      await socket.emit({
        type: 'tool_call',
        message_id: 'a1',
        tool_call_id: 'call_2',
        name: 'read_document',
        arguments: '{"id":"doc-1"}',
        reasoning: 'That hit looks right; read it.',
      });
      await socket.emit({
        type: 'tool_result',
        message_id: 'a1',
        tool_call_id: 'call_2',
        name: 'read_document',
        success: true,
        detail: '1 document',
        duration_ms: 9,
      });
      await socket.emit({ type: 'reasoning', content: 'Fridays are the deploy window.' });
      await socket.emit({
        type: 'message_end',
        message_id: 'a1',
        content: 'We deploy on Fridays.',
        metadata: {
          tool_calls: [
            {
              id: 'call_1',
              name: 'search_knowledge',
              arguments: '{"query":"deploys"}',
              success: true,
              detail: '3 passages',
              duration_ms: 12,
              reasoning: '> Search the workspace first.',
            },
            {
              id: 'call_2',
              name: 'read_document',
              arguments: '{"id":"doc-1"}',
              success: true,
              detail: '1 document',
              duration_ms: 9,
              reasoning: 'That hit looks right; read it.',
            },
          ],
          reasoning: 'Fridays are the deploy window.',
        },
      });
    });

    await page.fill('.message-form textarea', 'When do we deploy?');
    await page.locator('.message-form').getByRole('button', { name: 'Send' }).click();

    await expect(page.getByText('Search the workspace first.')).toBeVisible();
    await expect(page.getByText('Searched the knowledge base')).toBeVisible();
    await expect(page.getByText('That hit looks right; read it.')).toBeVisible();
    await expect(page.getByText('Read a workspace document')).toBeVisible();
    await expect(page.getByText('Fridays are the deploy window.')).toBeVisible();
    await expect(page.getByText('We deploy on Fridays.')).toBeVisible();

    const order = await page.locator('.messages-container').innerText();
    expect(order.indexOf('Search the workspace first.')).toBeLessThan(
      order.indexOf('Searched the knowledge base')
    );
    expect(order.indexOf('Searched the knowledge base')).toBeLessThan(
      order.indexOf('That hit looks right; read it.')
    );
    expect(order.indexOf('That hit looks right; read it.')).toBeLessThan(
      order.indexOf('Read a workspace document')
    );

    const openBlocks = page.locator('[data-testid="reasoning"][open]');
    await expect(openBlocks).toHaveCount(2);
    await expect(page.locator('[data-testid="reasoning"] blockquote')).toHaveCount(0);
    await page.screenshot({ path: testInfo.outputPath('thinking-between-tools.png'), fullPage: true });
  });

  test('streams a fenced-code answer as it arrives instead of holding a blank reply', async ({
    page,
  }, testInfo) => {
    await mockChatRoutes(page, []);
    await page.reload();
    await page.click('a[href="/chats"]');
    await openChat(page);

    socket.setOnSend(async () => {
      await socket.emit({ type: 'message_start', message_id: 'a1', role: 'assistant' });
      await socket.emit({ type: 'chunk', content: '```rust\nfn main() {\n', index: 0 });
      await socket.emit({ type: 'status', message: 'Generating response…' });
    });

    await page.fill('.message-form textarea', 'Show a rust main');
    await page.locator('.message-form').getByRole('button', { name: 'Send' }).click();
    await expect(page.locator('.message-assistant .message-content')).toContainText('fn main()');
    await socket.emit({ type: 'chunk', content: '}\n```', index: 1 });
    await socket.emit({
      type: 'message_end',
      message_id: 'a1',
      content: '```rust\nfn main() {\n}\n```',
    });
    await expect(page.locator('.message-assistant .message-content')).toContainText('fn main()');
    await page.screenshot({ path: testInfo.outputPath('fenced-code-stream.png'), fullPage: true });
  });

  test('streams preamble that arrived after native tool deltas', async ({ page }, testInfo) => {
    await mockChatRoutes(page, []);
    await page.reload();
    await page.click('a[href="/chats"]');
    await openChat(page);

    socket.setOnSend(async () => {
      await socket.emit({ type: 'message_start', message_id: 'a1', role: 'assistant' });
      await socket.emit({
        type: 'tool_call',
        message_id: 'a1',
        tool_call_id: 'call_0',
        name: 'read_file',
        arguments: '{}',
      });
      await socket.emit({ type: 'chunk', content: 'Looking at the file.', index: 0 });
      await socket.emit({
        type: 'tool_result',
        message_id: 'a1',
        tool_call_id: 'call_0',
        name: 'read_file',
        success: false,
        detail: 'A path is required.',
        duration_ms: 4,
      });
      await socket.emit({
        type: 'message_end',
        message_id: 'a1',
        content: 'Looking at the file.A path is required.',
      });
    });

    await page.fill('.message-form textarea', 'Open the file');
    await page.locator('.message-form').getByRole('button', { name: 'Send' }).click();
    await expect(page.getByText('Looking at the file.')).toBeVisible();
    await page.screenshot({ path: testInfo.outputPath('preamble-after-tools.png'), fullPage: true });
  });

  test('Stop finishes in-flight tools instead of leaving Running…', async ({ page }, testInfo) => {
    await mockChatRoutes(page, []);
    await page.reload();
    await page.click('a[href="/chats"]');
    await openChat(page);

    socket.setOnSend(async () => {
      await socket.emit({ type: 'message_start', message_id: 'a1', role: 'assistant' });
      await socket.emit({
        type: 'tool_call',
        message_id: 'a1',
        tool_call_id: 'tool',
        name: 'read_file',
        arguments: '{}',
      });
    });
    socket.setOnCancel(async () => {
      await socket.emit({ type: 'cancelled', message_id: 'a1' });
    });

    await page.fill('.message-form textarea', 'Read something');
    await page.locator('.message-form').getByRole('button', { name: 'Send' }).click();
    await expect(page.getByText('Running…')).toBeVisible();
    await page.getByRole('button', { name: 'Stop', exact: true }).click();
    await expect(page.getByText('Did not finish')).toBeVisible();
    await expect(page.getByText('[Stopped before answering]')).toBeVisible();
    await expect(page.getByText('Running…')).toHaveCount(0);
    await page.screenshot({ path: testInfo.outputPath('stop-settles-tools.png'), fullPage: true });
  });

  test('a failed send reuses the pending user bubble on retry', async ({ page }, testInfo) => {
    await mockChatRoutes(page, []);
    await page.reload();
    await page.click('a[href="/chats"]');
    await openChat(page);

    let sends = 0;
    socket.setOnSend(async (payload) => {
      sends += 1;
      if (sends === 1) {
        await socket.emit({ type: 'error', message: 'Rate limit exceeded' });
        return;
      }
      await socket.emit({
        type: 'message_saved',
        message_id: 'saved-retry',
        role: 'user',
        content: payload.content,
      });
      await socket.emit({ type: 'message_start', message_id: 'a1', role: 'assistant' });
      await socket.emit({ type: 'chunk', content: 'ok', index: 0 });
      await socket.emit({ type: 'message_end', message_id: 'a1', content: 'ok' });
    });

    await page.fill('.message-form textarea', 'First try');
    await page.locator('.message-form').getByRole('button', { name: 'Send' }).click();
    await expect(page.getByRole('alert')).toContainText('Rate limit exceeded');
    await expect(page.getByText('First try')).toHaveCount(1);

    await page.fill('.message-form textarea', 'Second try');
    await page.locator('.message-form').getByRole('button', { name: 'Send' }).click();
    await expect(page.getByText('Second try')).toHaveCount(1);
    await expect(page.getByText('First try')).toHaveCount(0);
    await page.screenshot({ path: testInfo.outputPath('retry-reuses-pending.png'), fullPage: true });
  });

  test('a sidebar refresh failure is not reported as a failed send', async ({ page }) => {
    const control = { failList: false };
    await mockChatRoutes(page, []);
    await page.unroute(/\/api\/chats($|\?|\/)/i);
    await routeApi(page, /\/api\/chats($|\?|\/)/i, (route) => {
      const url = route.request().url();
      const method = route.request().method();
      if (url.includes('/chat-1') && method === 'GET' && !url.includes('/context')) {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ chat: { ...chat, messages: [] } }),
        });
        return;
      }
      if (method === 'GET' && !url.includes('/chat-1')) {
        if (control.failList) {
          route.fulfill({
            status: 500,
            contentType: 'application/json',
            body: JSON.stringify({ error: 'list failed' }),
          });
          return;
        }
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ chats: [chat] }),
        });
        return;
      }
      route.continue();
    });
    await page.reload();
    await page.click('a[href="/chats"]');
    await openChat(page);
    control.failList = true;

    socket.setOnSend(async (payload) => {
      await socket.emit({
        type: 'message_saved',
        message_id: 'saved',
        role: 'user',
        content: payload.content,
      });
      await socket.emit({ type: 'message_start', message_id: 'a1', role: 'assistant' });
      await socket.emit({ type: 'message_end', message_id: 'a1', content: 'Got it.' });
    });

    await page.fill('.message-form textarea', 'Keep going');
    await page.locator('.message-form').getByRole('button', { name: 'Send' }).click();
    await expect(page.getByText('Got it.')).toBeVisible();
    await expect(page.getByRole('alert')).toHaveCount(0);
    await expect(page.getByText('Failed to send message')).toHaveCount(0);
  });

  test('does not yank scroll when the reader has moved up during streaming', async ({ page }) => {
    const history = Array.from({ length: 24 }, (_, index) => ({
      id: `old-${index}`,
      chat_id: 'chat-1',
      role: index % 2 === 0 ? 'user' : 'assistant',
      content: `History line ${index} ${'lorem '.repeat(20)}`,
      created_at: new Date().toISOString(),
    }));
    await mockChatRoutes(page, history);
    await page.reload();
    await page.click('a[href="/chats"]');
    await openChat(page);

    socket.setOnSend(async () => {
      await socket.emit({ type: 'message_start', message_id: 'a1', role: 'assistant' });
      await socket.emit({ type: 'chunk', content: 'start', index: 0 });
    });

    await page.fill('.message-form textarea', 'Continue');
    await page.locator('.message-form').getByRole('button', { name: 'Send' }).click();
    await expect(page.getByText('start', { exact: true })).toBeVisible();

    await page.locator('.messages-container').evaluate((node) => {
      node.scrollTop = 0;
    });
    const before = await page.locator('.messages-container').evaluate((node) => node.scrollTop);
    await socket.emit({ type: 'chunk', content: ' more tokens that would have yanked', index: 1 });
    await expect(page.getByText('start more tokens that would have yanked')).toBeVisible();
    const after = await page.locator('.messages-container').evaluate((node) => node.scrollTop);
    expect(after).toBeLessThanOrEqual(before + 40);
  });

  test('reconnects after a drop so the next send still works', async ({ page }, testInfo) => {
    await mockChatRoutes(page, []);
    await page.reload();
    await page.click('a[href="/chats"]');
    await openChat(page);

    socket.setOnSend(async (payload) => {
      await socket.emit({
        type: 'message_saved',
        message_id: 'saved',
        role: 'user',
        content: payload.content,
      });
      await socket.emit({ type: 'message_start', message_id: 'a1', role: 'assistant' });
      await socket.emit({ type: 'chunk', content: 'Reconnected.', index: 0 });
      await socket.emit({ type: 'message_end', message_id: 'a1', content: 'Reconnected.' });
    });

    await socket.disconnect();
    await page.waitForFunction(() => {
      const sockets =
        (window as Window & { __chatSockets?: Array<{ readyState: number }> }).__chatSockets ?? [];
      return sockets.some((candidate) => candidate.readyState === 1);
    });
    await page.fill('.message-form textarea', 'Are you there?');
    await page.locator('.message-form').getByRole('button', { name: 'Send' }).click();
    await expect(page.getByText('Reconnected.')).toBeVisible();
    await page.screenshot({ path: testInfo.outputPath('reconnect.png'), fullPage: true });
  });

  test('a dropped socket rejoins the reply instead of ending it', async ({ page }, testInfo) => {
    await mockChatRoutes(page, []);
    await page.reload();
    await page.click('a[href="/chats"]');
    await openChat(page);

    socket.setOnSend(async () => {
      await socket.emit({ type: 'message_start', message_id: 'a1', role: 'assistant' });
      await socket.emit({ type: 'chunk', content: 'Half a', index: 0 });
      await socket.emit({
        type: 'tool_call',
        message_id: 'a1',
        tool_call_id: 'tool',
        name: 'read_file',
        arguments: '{}',
      });
    });

    await page.fill('.message-form textarea', 'Read something');
    await page.locator('.message-form').getByRole('button', { name: 'Send' }).click();
    await expect(page.getByText('Running…')).toBeVisible();

    await socket.disconnect();
    await page.waitForFunction(() => {
      const sockets =
        (window as Window & { __chatSockets?: Array<{ readyState: number }> }).__chatSockets ?? [];
      return sockets.some((candidate) => candidate.readyState === 1);
    });

    // What the server replays to a connection that joins a turn in flight.
    await socket.emit({
      type: 'message_start',
      message_id: 'a1',
      role: 'assistant',
      resumed: true,
    });
    await socket.emit({ type: 'chunk', content: 'Half a', index: 0 });
    await socket.emit({
      type: 'tool_call',
      message_id: 'a1',
      tool_call_id: 'tool',
      name: 'read_file',
      arguments: '{}',
    });
    await expect(page.getByText('Running…')).toBeVisible();
    await expect(page.getByText('Did not finish')).toHaveCount(0);

    await socket.emit({
      type: 'tool_result',
      message_id: 'a1',
      tool_call_id: 'tool',
      name: 'read_file',
      success: true,
      detail: 'Read 20 lines',
      duration_ms: 12,
    });
    await socket.emit({ type: 'chunk', content: ' reply, finished after the drop.', index: 1 });
    await socket.emit({
      type: 'message_end',
      message_id: 'a1',
      content: 'Half a reply, finished after the drop.',
    });

    await expect(page.getByText('Half a reply, finished after the drop.')).toBeVisible();
    await expect(page.getByText('Read 20 lines')).toBeVisible();
    await expect(page.getByText('[Stopped before answering]')).toHaveCount(0);
    await page.screenshot({
      path: testInfo.outputPath('resumes-after-drop.png'),
      fullPage: true,
    });
  });

  test('shows a replay that arrived before the chat had loaded', async ({ page }, testInfo) => {
    let release: () => void = () => {};
    const holdChat = new Promise<void>((resolve) => {
      release = resolve;
    });
    await mockChatRoutes(page, [userMessage], { holdChat });
    await page.reload();
    await page.click('a[href="/chats"]');
    await expect(page.locator('.chat-item')).toHaveCount(1);
    await page.click('.chat-item');

    // The socket connects while the chat itself is still being fetched.
    await page.waitForFunction(() => {
      const sockets =
        (window as Window & { __chatSockets?: Array<{ readyState: number }> }).__chatSockets ?? [];
      return sockets.some((candidate) => candidate.readyState === 1);
    });
    await socket.emit({
      type: 'message_start',
      message_id: 'a1',
      role: 'assistant',
      resumed: true,
    });
    await socket.emit({ type: 'chunk', content: 'Streamed before the chat loaded.', index: 0 });
    await socket.emit({
      type: 'tool_call',
      message_id: 'a1',
      tool_call_id: 'tool',
      name: 'read_file',
      arguments: '{}',
    });

    release();
    await expect(page.getByText('Streamed before the chat loaded.')).toBeVisible();
    await expect(page.getByText('Running…')).toBeVisible();
    await page.screenshot({
      path: testInfo.outputPath('replay-before-load.png'),
      fullPage: true,
    });
  });

  test('a stored partial assistant reply is still visible after reload', async ({ page }, testInfo) => {
    await mockChatRoutes(page, [
      userMessage,
      {
        id: 'msg-partial',
        chat_id: 'chat-1',
        role: 'assistant',
        content: 'partial reply that would have vanished',
        created_at: new Date().toISOString(),
      },
    ]);
    await page.reload();
    await page.click('a[href="/chats"]');
    await openChat(page);
    await expect(page.getByText('partial reply that would have vanished')).toBeVisible();
    await page.screenshot({
      path: testInfo.outputPath('partial-survives-reload.png'),
      fullPage: true,
    });
  });

  test('opening a protected image full size uses a blob that survives unmount', async ({
    page,
  }) => {
    const opened: string[] = [];
    await page.exposeFunction('__recordOpened', (url: string) => {
      opened.push(url);
    });
    await page.addInitScript(() => {
      const original = window.open;
      window.open = ((url?: string | URL, ...rest: unknown[]) => {
        (
          window as Window & { __recordOpened?: (url: string) => void }
        ).__recordOpened?.(String(url ?? ''));
        return original?.call(window, url, ...(rest as [])) ?? null;
      }) as typeof window.open;
    });

    await routeApi(page, '**/api/artifacts/**', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'image/png',
        body: MOCK_PNG_BYTES,
      });
    });
    await mockChatRoutes(page, [
      userMessage,
      {
        id: 'msg-generated',
        chat_id: 'chat-1',
        role: 'assistant',
        content: 'Generated image.',
        created_at: new Date().toISOString(),
        metadata: { attachments: [generatedAttachment(GENERATED_ARTIFACT_URL)] },
      },
    ]);
    await page.reload();
    await page.click('a[href="/chats"]');
    await openChat(page);

    const image = page.getByRole('img', { name: 'generated-image-1.png' });
    await expect(image).toBeVisible();
    const displaySrc = await image.getAttribute('src');
    expect(displaySrc).toMatch(/^blob:/);
    await page.getByRole('link', { name: 'Open generated-image-1.png full size' }).click();
    await expect.poll(() => opened.at(-1)).toMatch(/^blob:/);
    expect(opened.at(-1)).not.toBe(displaySrc);

    await page.click('a[href="/projects"]');
    expect(opened.at(-1)).toMatch(/^blob:/);
  });
});
