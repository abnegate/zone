import { test, expect } from './fixtures';
import { setupAuth, mockCommonEndpoints } from './helpers/auth';
import { installChatSocketMock, type ChatSocketController } from './helpers/chatSocket';
import { blockServiceWorker, routeApi } from './test-utils';

const GENERATED_VIDEO_URL =
  '/api/artifacts/00000000-0000-0000-0000-000000000001/chat-1/msg-generated/generated-video-1.webm';

const generatedVideo = (url = GENERATED_VIDEO_URL) => ({
  name: 'generated-video-1.webm',
  mime: 'video/webm',
  url,
});

const mockChat = {
  id: 'chat-1',
  title: 'Video Chat',
  model_name: 'llava:7b',
  created_at: new Date().toISOString(),
  updated_at: new Date().toISOString(),
  archived: false,
  agent_enabled: false,
};

test.describe('Chat videos', () => {
  let socket: ChatSocketController;

  test.beforeEach(async ({ context, page }) => {
    socket = await installChatSocketMock(page);
    await blockServiceWorker(context);
    await mockCommonEndpoints(page);

    await page.goto('/');
    await setupAuth(page);
    await page.reload();
    await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
    await page.click('a[href="/chats"]');
    await expect(page).toHaveURL('/chats');
  });

  async function mockChatRoutes(page: Parameters<typeof routeApi>[0], messages: unknown[] = []) {
    await routeApi(page, /\/api\/chats($|\?|\/)/i, (route) => {
      const url = route.request().url();
      const method = route.request().method();
      if (url.includes('/chat-1') && method === 'GET') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ chat: { ...mockChat, messages } }),
        });
        return;
      }
      if (method === 'GET') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ chats: [mockChat] }),
        });
        return;
      }
      route.continue();
    });
  }

  test('shows a generated video already on the thread', async ({ page }) => {
    await mockChatRoutes(page, [
      {
        id: 'msg-user',
        chat_id: 'chat-1',
        role: 'user',
        content: 'Generate a video of a fox running',
        created_at: new Date().toISOString(),
      },
      {
        id: 'msg-generated',
        chat_id: 'chat-1',
        role: 'assistant',
        content: 'Generated video.',
        created_at: new Date().toISOString(),
        metadata: { attachments: [generatedVideo('data:video/webm;base64,AAAA')] },
      },
    ]);
    await page.reload();
    await page.click('a[href="/chats"]');
    await expect(page.locator('.chat-item')).toHaveCount(1);
    await page.click('.chat-item');

    const video = page.getByLabel('generated-video-1.webm');
    await expect(video).toBeVisible();
    await expect(video).toHaveAttribute('controls');
    await expect(page.locator('.message-assistant .message-content')).toContainText(
      'Generated video.'
    );
  });

  test('streams a generated video after send', async ({ page }) => {
    const attachment = generatedVideo('data:video/webm;base64,AAAA');
    socket.setOnSend(async () => {
      await socket.emit({ type: 'status', message: 'Generating video...' });
    });

    await mockChatRoutes(page, []);
    await page.reload();
    await page.click('a[href="/chats"]');
    await expect(page.locator('.chat-item')).toHaveCount(1);
    await page.click('.chat-item');
    await expect(page.locator('.message-form textarea')).toBeVisible();

    await page.fill('.message-form textarea', 'Generate a video of a fox running');
    await page.locator('.message-form').getByRole('button', { name: 'Send' }).click();

    await expect(page.getByRole('status')).toHaveText('Generating video...');
    await socket.emit({ type: 'message_start', message_id: 'msg-generated', role: 'assistant' });
    await socket.emit({
      type: 'video',
      message_id: 'msg-generated',
      attachment,
    });
    await socket.emit({
      type: 'message_end',
      message_id: 'msg-generated',
      content: 'Generated video.',
      metadata: { attachments: [attachment] },
    });

    await expect(page.getByRole('status')).toHaveCount(0);
    await expect(page.getByLabel('generated-video-1.webm')).toBeVisible();
    await expect(page.locator('.message-assistant .message-content')).toContainText(
      'Generated video.'
    );
  });
});
