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

  test('streams a generated image after send', async ({ page }, testInfo) => {
    const attachment = generatedAttachment();
    let sends = 0;
    socket.setOnSend(async () => {
      sends += 1;
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
    await expect(page.getByRole('button', { name: 'Stop', exact: true })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Send', exact: true })).toHaveCount(0);
    await expect(page.locator('.chat-header')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Attach files' })).toBeVisible();
    await page.fill('.message-form textarea', 'Next prompt');
    await page.locator('.message-form textarea').press('Enter');
    expect(sends).toBe(1);
    await expect(page.getByRole('alert')).toHaveCount(0);
    await page.screenshot({ path: testInfo.outputPath('generation-pending.png'), fullPage: true });
    await page.getByRole('button', { name: 'Switch to dark mode' }).click();
    await page.screenshot({
      path: testInfo.outputPath('generation-pending-dark.png'),
      fullPage: true,
      animations: 'disabled',
    });

    await socket.emit({ type: 'message_start', message_id: 'msg-generated', role: 'assistant' });
    await expect(page.getByRole('status')).toHaveText('Generating response…');
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
    await expect(page.getByRole('button', { name: 'Send', exact: true })).toBeEnabled();
    await expect(page.getByRole('button', { name: 'Stop', exact: true })).toHaveCount(0);
    await expect(page.getByRole('img', { name: 'generated-image-1.png' })).toBeVisible();
    await expect(page.locator('.message-assistant .message-content')).toContainText(
      'Generated image.'
    );
    await expect(page.getByText('Make a pretty picture of a blue fox')).toBeVisible();
  });

  test('shows a failed image request without hiding history and permits retry', async ({
    page,
  }, testInfo) => {
    let sends = 0;
    socket.setOnSend(async () => {
      sends += 1;
      await socket.emit({ type: 'status', message: 'Generating image…' });
      if (sends === 1) {
        await expect(page.getByRole('status')).toBeInViewport({ ratio: 1 });
        await expect
          .poll(() =>
            page
              .locator('.messages-container')
              .evaluate(
                (element) => element.scrollHeight - element.scrollTop - element.clientHeight
              )
          )
          .toBeLessThan(2);
        await socket.emit({
          type: 'error',
          message:
            'Image generation failed: cannot reach ComfyUI. Start the image service and try again.',
        });
      }
    });

    const history = Array.from({ length: 10 }, (_, index) => ({
      ...userPrompt,
      id: `history-${index}`,
      content: `Earlier discussion of image composition ${index + 1}`,
    }));
    await mockChatRoutes(page, [...history, userPrompt]);
    await page.reload();
    await page.click('a[href="/chats"]');
    await openImageChat(page);

    await page.fill(
      '.message-form textarea',
      'Generate an image of the same rooster facing the other way'
    );
    await page.locator('.message-form').getByRole('button', { name: 'Send' }).click();

    await expect(page.locator('.chats-main').getByRole('alert')).toHaveText(
      'Image generation failed: cannot reach ComfyUI. Start the image service and try again.'
    );
    await expect(page.locator('.chats-main').getByRole('alert')).toBeInViewport({ ratio: 1 });
    expect(
      await page
        .locator('.messages-container')
        .evaluate((element) => element.scrollHeight > element.clientHeight)
    ).toBe(true);
    await expect(page.getByRole('status')).toHaveCount(0);
    await expect(
      page.getByText('Generate an image of the same rooster facing the other way')
    ).toBeVisible();
    await expect(page.getByText(userPrompt.content)).toBeVisible();
    await expect(page.locator('.message-assistant')).toHaveCount(0);
    await expect(page.getByTestId('message-image')).toHaveCount(0);
    await expect(page.locator('.chat-header')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Attach files' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Send', exact: true })).toBeVisible();
    await page.screenshot({ path: testInfo.outputPath('generation-failed.png'), fullPage: true });
    await page.getByRole('button', { name: 'Switch to dark mode' }).click();
    await page.screenshot({
      path: testInfo.outputPath('generation-failed-dark.png'),
      fullPage: true,
      animations: 'disabled',
    });

    await page.fill('.message-form textarea', 'Try the image again');
    await page.locator('.message-form textarea').press('Enter');
    await expect(page.getByRole('alert')).toHaveCount(0);
    await expect(page.getByRole('status')).toHaveText('Generating image…');
    expect(sends).toBe(2);
    await socket.emit({ type: 'message_start', message_id: 'retry', role: 'assistant' });
    await socket.emit({ type: 'message_end', message_id: 'retry', content: 'Retry completed' });
    await expect(page.getByText('Retry completed')).toBeVisible();
    await expect(page.getByRole('status')).toHaveCount(0);
  });

  test('keeps Stop pending until cancellation is acknowledged and can send again', async ({
    page,
  }) => {
    let cancellations = 0;
    socket.setOnCancel(() => {
      cancellations += 1;
    });
    await mockChatRoutes(page, [userPrompt]);
    await page.reload();
    await page.click('a[href="/chats"]');
    await openImageChat(page);
    await page.fill('.message-form textarea', 'Generate another image');
    await page.locator('.message-form textarea').press('Enter');

    await expect(page.getByRole('status')).toHaveText('Generating response…');
    await page.getByRole('button', { name: 'Stop', exact: true }).click();
    await expect.poll(() => cancellations).toBe(1);
    await expect(page.getByRole('status')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Stop', exact: true })).toBeVisible();
    await socket.emit({ type: 'cancelled', message_id: 'cancelled-image' });
    await expect(page.getByRole('status')).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Stop', exact: true })).toHaveCount(0);
    await expect(page.getByText(userPrompt.content)).toBeVisible();
    await page.fill('.message-form textarea', 'New request');
    await expect(page.getByRole('button', { name: 'Send', exact: true })).toBeEnabled();
    await page.getByRole('button', { name: 'Send', exact: true }).click();
    await expect(page.getByRole('status')).toHaveText('Generating response…');
  });
});
