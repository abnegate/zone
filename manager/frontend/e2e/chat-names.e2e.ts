import { expect, test } from '@playwright/test';
import { mockCommonEndpoints, setupAuth } from './helpers/auth';
import { blockServiceWorker, routeApi } from './test-utils';

for (const width of [390, 1280]) {
  test(`chat names stay synchronized at ${width}px`, async ({ context, page }) => {
    await page.setViewportSize({ width, height: 900 });
    await blockServiceWorker(context);
    await mockCommonEndpoints(page);
    await setupAuth(page);
    const chat = {
      id: 'chat-1',
      title: 'Chat with llama3.2',
      model_name: 'llama3.2',
      created_at: new Date().toISOString(),
      updated_at: new Date().toISOString(),
      archived: false,
      agent_enabled: true,
      messages: [],
    };
    await routeApi(page, /\/api\/chats($|\?|\/)/i, async (route) => {
      if (route.request().method() === 'PUT') {
        expect(route.request().postDataJSON()).toEqual({ title: 'Planning a garden' });
        chat.title = 'Planning a garden';
      }
      const single = new URL(route.request().url()).pathname.endsWith('/chat-1');
      await route.fulfill({ json: single ? { chat } : { chats: [chat] } });
    });
    await page.routeWebSocket(/\/ws\/chats\/chat-1/, (socket) => {
      socket.onMessage((message) => {
        const payload = JSON.parse(message.toString());
        if (payload.type === 'send') {
          chat.title = 'Garden ideas';
          socket.send(
            JSON.stringify({ type: 'message_start', message_id: 'reply', role: 'assistant' })
          );
          socket.send(JSON.stringify({ type: 'chunk', content: 'Choose a sunny spot.', index: 0 }));
          socket.send(
            JSON.stringify({ type: 'title_updated', chat_id: chat.id, title: 'Garden ideas' })
          );
        }
      });
    });
    await page.goto('/chats');
    await page.getByText(chat.title, { exact: true }).click();
    await expect(page.getByRole('heading', { name: chat.title })).toBeVisible();
    const row = page.locator('.chat-item');
    if (width < 768) await page.getByRole('button', { name: 'Back to chats' }).click();
    await row.hover();
    await expect(row.getByTitle('Archive', { exact: true })).toBeVisible();
    await expect(row.getByTitle('Delete', { exact: true })).toBeVisible();
    if (width < 768) await row.click();
    await expect(page.getByTestId('agent-toggle')).toBeVisible();
    await expect(page.getByTestId('sandbox-toggle')).toHaveCount(0);
    const input = page.getByPlaceholder('Type a message, or drop a file...');
    await input.fill('What should I plant?');
    await input.press('Enter');
    await expect(page.getByRole('heading', { name: 'Garden ideas' })).toBeVisible();
    await expect(row.locator('.chat-title')).toHaveText('Garden ideas');
    await expect(page.getByText('Choose a sunny spot.')).toBeVisible();
    await page.screenshot({
      path: test.info().outputPath(`automatic-${width}.png`),
      fullPage: true,
    });
    if (width < 768) await page.getByRole('button', { name: 'Back to chats' }).click();
    await row.getByRole('button', { name: `Rename ${chat.title}` }).click();
    const name = page.getByLabel('Chat name');
    await expect(name).toHaveValue(chat.title);
    await name.fill('  Planning a garden  ');
    await page.screenshot({
      path: test.info().outputPath(`rename-${width}.png`),
      fullPage: true,
      animations: 'disabled',
    });
    await page.getByRole('button', { name: 'Save name' }).click();
    await expect(row.locator('.chat-title')).toHaveText('Planning a garden');
    if (width < 768) await row.click();
    await expect(page.getByRole('heading', { name: 'Planning a garden' })).toBeVisible();
    await expect(row.locator('.chat-title')).toHaveText('Planning a garden');
    await page.screenshot({ path: test.info().outputPath(`renamed-${width}.png`), fullPage: true });
  });
}
