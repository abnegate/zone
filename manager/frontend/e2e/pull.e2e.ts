import type { WebSocketRoute } from '@playwright/test';
import { expect, test } from './fixtures';
import { mockCommonEndpoints, setupAuth } from './helpers/auth';
import { blockServiceWorker } from './test-utils';

test('authenticates model pulls and displays provider errors before allowing a successful retry', async ({
  context,
  page,
}, testInfo) => {
  await blockServiceWorker(context);
  await mockCommonEndpoints(page);
  await setupAuth(page);

  const messages: unknown[] = [];
  let socket: WebSocketRoute | undefined;
  await page.routeWebSocket('**/ws/pull?*', (connection) => {
    socket = connection;
    connection.onMessage((message) => messages.push(JSON.parse(message.toString())));
  });

  await page.goto('/models');
  const input = page.locator('.model-form input');
  const install = page.locator('.model-form button[type="submit"]');
  const panel = page.locator('.models-install-panel');
  await input.fill('qwen/qwen3.8-27b');
  await install.click();

  const token = await page.evaluate(() => localStorage.getItem('manager_access_token'));
  await expect.poll(() => messages).toEqual([{ type: 'auth', token }]);
  await expect(install).toBeDisabled();
  expect(socket).toBeDefined();
  socket?.send(JSON.stringify({ type: 'authenticated' }));
  await expect
    .poll(() => messages)
    .toEqual([{ type: 'auth', token }, { model: 'qwen/qwen3.8-27b' }]);
  socket?.send(
    JSON.stringify({ type: 'error', message: 'pull model manifest: file does not exist' })
  );
  await expect(panel).toContainText('Installation failed');
  await expect(panel.locator('.result-error')).toHaveText(
    'pull model manifest: file does not exist'
  );
  await expect(install).toBeEnabled();
  await expect(input).toHaveValue('qwen/qwen3.8-27b');
  await expect(page.getByRole('heading', { name: 'Installed Models', exact: true })).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath('pull-error.png'), fullPage: true });

  await input.fill('llama3.2');
  await install.click();
  await expect.poll(() => messages.length).toBe(3);
  expect(messages[2]).toEqual({ type: 'auth', token });
  socket?.send(JSON.stringify({ type: 'authenticated' }));
  await expect.poll(() => messages.length).toBe(4);
  expect(messages[3]).toEqual({ model: 'llama3.2' });
  socket?.send(JSON.stringify({ type: 'progress', percent: 50 }));
  await expect(panel.locator('.progress-text')).toHaveText('50%');
  const refresh = page.waitForRequest(
    (request) => new URL(request.url()).pathname === '/api/models'
  );
  socket?.send(
    JSON.stringify({ type: 'complete', success: true, message: 'Model installed successfully' })
  );
  await refresh;
  await expect(panel).toContainText('Installation complete');
  await expect(input).toHaveValue('');
  expect(messages).toHaveLength(4);
});
