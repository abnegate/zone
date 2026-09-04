import { test, expect } from './fixtures';
import { setupAuth, mockCommonEndpoints } from './helpers/auth';
import {
  GENERATED_ARTIFACT_URL,
  MOCK_PNG_BYTES,
  MOCK_PNG_DATA_URL,
  generatedAttachment,
  installChatSocketMock,
  type ChatSocketController,
} from './helpers/chatSocket';
import { blockServiceWorker, routeApi } from './test-utils';

const mockChat = {
  id: 'chat-1',
  title: 'Image Chat',
  model_name: 'llava:7b',
  created_at: new Date().toISOString(),
  updated_at: new Date().toISOString(),
  archived: false,
  agent_enabled: false,
  agent_sandboxed: true,
};

const userPrompt = {
  id: 'msg-user',
  chat_id: 'chat-1',
  role: 'user',
  content: 'Make a pretty picture of a blue fox',
  created_at: new Date().toISOString(),
};

function assistantImageMessage(url: string) {
  return {
    id: 'msg-generated',
    chat_id: 'chat-1',
    role: 'assistant',
    content: 'Generated image.',
    created_at: new Date().toISOString(),
    metadata: { attachments: [generatedAttachment(url)] },
  };
}

test.describe('Chat images', () => {
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

  async function mockChatRoutes(
    page: Parameters<typeof routeApi>[0],
    messages: unknown[] = []
  ) {
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

  async function openImageChat(page: Parameters<typeof routeApi>[0]) {
    await expect(page.locator('.chat-item')).toHaveCount(1);
    await page.click('.chat-item');
    await expect(page.locator('.message-form textarea')).toBeVisible();
  }

  test('shows a generated image already on the thread', async ({ page }) => {
    await mockChatRoutes(page, [userPrompt, assistantImageMessage(MOCK_PNG_DATA_URL)]);
    await page.reload();
    await page.click('a[href="/chats"]');
    await openImageChat(page);

    const image = page.getByRole('img', { name: 'generated-image-1.png' });
    await expect(image).toBeVisible();
    await expect(image).toHaveAttribute('src', MOCK_PNG_DATA_URL);
    await expect(
      page.getByRole('link', { name: 'Open generated-image-1.png full size' })
    ).toHaveAttribute('target', '_blank');
  });

  test('fetches a protected artifact with the bearer token', async ({ page }) => {
    let artifactAuth: string | undefined;
    await routeApi(page, '**/api/artifacts/**', (route) => {
      artifactAuth = route.request().headers().authorization;
      route.fulfill({
        status: 200,
        contentType: 'image/png',
        body: MOCK_PNG_BYTES,
      });
    });

    await mockChatRoutes(page, [userPrompt, assistantImageMessage(GENERATED_ARTIFACT_URL)]);
    await page.reload();
    await page.click('a[href="/chats"]');
    await openImageChat(page);

    await expect(page.getByTestId('message-image')).toBeVisible();
    await expect(page.getByRole('img', { name: 'generated-image-1.png' })).toBeVisible();
    expect(artifactAuth).toMatch(/^Bearer\s.+/);
  });

  test('shows Image unavailable when the artifact cannot be loaded', async ({ page }) => {
    await routeApi(page, '**/api/artifacts/**', (route) => {
      route.fulfill({ status: 403, body: 'denied' });
    });

    await mockChatRoutes(page, [userPrompt, assistantImageMessage(GENERATED_ARTIFACT_URL)]);
    await page.reload();
    await page.click('a[href="/chats"]');
    await openImageChat(page);

    await expect(page.getByRole('alert')).toHaveText('Image unavailable');
    await expect(page.getByTestId('message-image')).toHaveCount(0);
  });

  test('streams a generated image after send', async ({ page }) => {
    const attachment = generatedAttachment();
    socket.setOnSend(async () => {
      await expect(page.getByText('Make a pretty picture of a blue fox')).toBeVisible();
      await socket.emit({ type: 'status', message: 'Generating image…' });
    });

    await mockChatRoutes(page, []);
    await page.reload();
    await page.click('a[href="/chats"]');
    await openImageChat(page);

    await page.fill('.message-form textarea', 'Make a pretty picture of a blue fox');
    await page.locator('.message-form').getByRole('button', { name: 'Send' }).click();

    await expect(page.getByRole('status')).toHaveText('Generating image…');

    await socket.emit({ type: 'message_start', message_id: 'msg-generated', role: 'assistant' });
    await socket.emit({
      type: 'image',
      message_id: 'msg-generated',
      attachment,
    });
    await socket.emit({
      type: 'message_end',
      message_id: 'msg-generated',
      content: 'Generated image.',
      metadata: { attachments: [attachment] },
    });

    await expect(page.getByRole('status')).toHaveCount(0);
    await expect(page.getByRole('img', { name: 'generated-image-1.png' })).toBeVisible();
    await expect(page.locator('.message-assistant .message-content')).toContainText(
      'Generated image.'
    );
    await expect(page.getByText('Make a pretty picture of a blue fox')).toBeVisible();
  });

  test('generation failure never leaves an empty assistant message', async ({ page }) => {
    socket.setOnSend(async () => {
      await expect(page.getByText('Generate an image of a failed request')).toBeVisible();
      await socket.emit({
        type: 'error',
        message: 'Image generation failed: ComfyUI is not configured',
      });
    });

    await mockChatRoutes(page, []);
    await page.reload();
    await page.click('a[href="/chats"]');
    await openImageChat(page);

    await page.fill('.message-form textarea', 'Generate an image of a failed request');
    await page.locator('.message-form').getByRole('button', { name: 'Send' }).click();

    await expect(page.getByText('Generate an image of a failed request')).toBeVisible();
    await expect(page.locator('.message-assistant')).toHaveCount(0);
    await expect(page.getByTestId('message-image')).toHaveCount(0);
  });
});
