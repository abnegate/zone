import { test, expect } from './fixtures';
import { setupAuth, mockCommonEndpoints } from './helpers/auth';
import { installChatSocketMock, type ChatSocketController } from './helpers/chatSocket';
import { blockServiceWorker, routeApi } from './test-utils';

const GENERATED_AUDIO_URL =
  '/api/artifacts/00000000-0000-0000-0000-000000000001/chat-1/msg-generated/generated-audio-1.flac';

const AUDIO_PROMPT = 'make a background audio track that sounds like shuffling through a forest';

const generatedAudio = (url = GENERATED_AUDIO_URL) => ({
  name: 'generated-audio-1.flac',
  mime: 'audio/flac',
  url,
});

const mockChat = {
  id: 'chat-1',
  title: 'Audio Chat',
  model_name: 'llava:7b',
  created_at: new Date().toISOString(),
  updated_at: new Date().toISOString(),
  archived: false,
  agent_enabled: false,
};

test.describe('Chat audio', () => {
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

  test('shows generated audio already on the thread', async ({ page }) => {
    await mockChatRoutes(page, [
      {
        id: 'msg-user',
        chat_id: 'chat-1',
        role: 'user',
        content: AUDIO_PROMPT,
        created_at: new Date().toISOString(),
      },
      {
        id: 'msg-generated',
        chat_id: 'chat-1',
        role: 'assistant',
        content: 'Generated audio.',
        created_at: new Date().toISOString(),
        metadata: { attachments: [generatedAudio('data:audio/flac;base64,AAAA')] },
      },
    ]);
    await page.reload();
    await page.click('a[href="/chats"]');
    await expect(page.locator('.chat-item')).toHaveCount(1);
    await page.click('.chat-item');

    const audio = page.getByLabel('generated-audio-1.flac');
    await expect(audio).toBeVisible();
    await expect(audio).toHaveAttribute('controls');
    await expect(page.locator('.message-assistant .message-content')).toContainText(
      'Generated audio.'
    );
  });

  test('streams generated audio after send', async ({ page }) => {
    const attachment = generatedAudio('data:audio/flac;base64,AAAA');
    socket.setOnSend(async () => {
      await socket.emit({ type: 'status', message: 'Generating audio...' });
    });

    await mockChatRoutes(page, []);
    await page.reload();
    await page.click('a[href="/chats"]');
    await expect(page.locator('.chat-item')).toHaveCount(1);
    await page.click('.chat-item');
    await expect(page.locator('.message-form textarea')).toBeVisible();

    await page.fill('.message-form textarea', AUDIO_PROMPT);
    await page.locator('.message-form').getByRole('button', { name: 'Send' }).click();

    await expect(page.getByRole('status')).toHaveText('Generating audio...');
    await socket.emit({ type: 'message_start', message_id: 'msg-generated', role: 'assistant' });
    await socket.emit({
      type: 'audio',
      message_id: 'msg-generated',
      attachment,
    });
    await socket.emit({
      type: 'message_end',
      message_id: 'msg-generated',
      content: 'Generated audio.',
      metadata: { attachments: [attachment] },
    });

    await expect(page.getByRole('status')).toHaveCount(0);
    await expect(page.getByLabel('generated-audio-1.flac')).toBeVisible();
    await expect(page.locator('.message-assistant .message-content')).toContainText(
      'Generated audio.'
    );
  });
});
