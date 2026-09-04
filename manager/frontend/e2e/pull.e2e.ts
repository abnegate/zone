import type { WebSocketRoute } from '@playwright/test';
import { expect, test } from './fixtures';
import { mockCommonEndpoints, setupAuth } from './helpers/auth';
import { blockServiceWorker, routeApi } from './test-utils';

test.use({ viewport: { width: 1440, height: 1000 } });

test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem('manager_theme', 'dark'));
});

test('authenticates model pulls and displays provider errors before allowing a successful retry', async ({
  context,
  page,
}, testInfo) => {
  await blockServiceWorker(context);
  await mockCommonEndpoints(page);
  await setupAuth(page);
  await routeApi(page, '**/api/models', (route) =>
    route.fulfill({
      json: {
        models: [{ name: 'llama3.2:3b', size: 2000000000, modified_at: '2026-09-04T00:00:00Z' }],
      },
    })
  );

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
  await expect(panel).toContainText('Ollama model:tag');
  await expect(panel).toContainText('hf.co/owner/Model-GGUF');
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
  socket?.send(JSON.stringify({ type: 'step', status: 'pulling manifest' }));
  await expect(panel.locator('.step-pending')).toHaveText('○pulling manifest');
  const message =
    'pull model manifest: file does not exist. Ollama could not find "qwen/qwen3.8-27b". Use an Ollama model:tag or hf.co/owner/GGUF-repository reference.';
  socket?.send(JSON.stringify({ type: 'error', message }));
  await expect(panel).toContainText('Installation failed');
  await expect(panel.locator('.result-error')).toHaveText(message);
  await expect(panel.locator('.step-error')).toHaveText('✗pulling manifest');
  await expect(panel.locator('.step-pending, .spinner')).toHaveCount(0);
  await expect(install).toBeEnabled();
  await expect(input).toHaveValue('qwen/qwen3.8-27b');
  await expect(page.getByRole('heading', { name: 'Installed Models', exact: true })).toBeVisible();
  await expect(page.locator('.models-list-panel .model-name')).toHaveText('llama3.2:3b');
  await page.screenshot({
    path: testInfo.outputPath('pull-error.png'),
    fullPage: true,
    animations: 'disabled',
  });

  await input.fill('qwen3.8:27b');
  await install.click();
  await expect(panel.locator('.step-item, .result-message, .progress-text')).toHaveCount(0);
  await expect.poll(() => messages.length).toBe(3);
  expect(messages[2]).toEqual({ type: 'auth', token });
  socket?.send(JSON.stringify({ type: 'authenticated' }));
  await expect.poll(() => messages.length).toBe(4);
  expect(messages[3]).toEqual({ model: 'qwen3.8:27b' });
  socket?.send(JSON.stringify({ type: 'step', status: 'downloading' }));
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
  await expect(panel.locator('.step-success')).toHaveText('✓downloading');
  await expect(panel.locator('.step-pending, .spinner')).toHaveCount(0);
  await expect(input).toHaveValue('');
  await expect(page.getByRole('heading', { name: 'Installed Models', exact: true })).toBeVisible();
  expect(messages).toHaveLength(4);
});

test.describe('catalog download references', () => {
  const catalog = {
    ollama: [{ name: 'qwen3.8:27b', description: 'An Ollama model' }],
    huggingface: [
      {
        name: 'Owner/Model-GGUF:Q4_K_M',
        description: 'A HuggingFace GGUF model',
        modified_at: '2026-09-04T00:00:00Z',
        details: { format: 'gguf' },
      },
    ],
    openrouter: [
      {
        name: 'qwen/qwen3.8-27b',
        description: 'A remote OpenRouter model',
        modified_at: '2026-09-04T00:00:00Z',
        details: { format: 'api' },
      },
    ],
    gpt4all: [{ name: 'model.gguf', description: 'A GPT4All model' }],
  };

  test.beforeEach(async ({ context, page }) => {
    await blockServiceWorker(context);
    await mockCommonEndpoints(page);
    await setupAuth(page);
    await routeApi(page, '**/api/models*', (route) => {
      const source = new URL(route.request().url()).searchParams.get('source');
      return route.fulfill({
        json: { models: catalog[source as keyof typeof catalog] ?? [], next_cursor: null },
      });
    });
    await routeApi(page, '**/api/models/**', (route) =>
      route.fulfill({ json: { content: null, gguf_size: null } })
    );
  });

  test('remote and unsupported sources explain download eligibility in rows and details', async ({
    page,
  }, testInfo) => {
    const connections: WebSocketRoute[] = [];
    await page.routeWebSocket('**/ws/pull?*', (connection) => connections.push(connection));
    await page.goto('/models');
    await page.getByRole('tab', { name: 'Browse', exact: true }).click();
    const rows = page.locator('.browse-item');
    const remote = rows.filter({ hasText: 'qwen/qwen3.8-27b' });
    await expect(remote.getByRole('button', { name: 'Remote API', exact: true })).toBeDisabled();
    await expect(remote).toContainText('cannot be installed through Ollama');
    await expect(
      rows.filter({ hasText: 'model.gguf' }).getByRole('button', {
        name: 'Download unavailable',
        exact: true,
      })
    ).toBeDisabled();
    await page.screenshot({
      path: testInfo.outputPath('browse-eligibility.png'),
      fullPage: true,
      animations: 'disabled',
    });

    for (const [name, label, source] of [
      ['qwen/qwen3.8-27b', 'Remote API', 'OpenRouter'],
      ['model.gguf', 'Download unavailable', 'GPT4All'],
    ]) {
      await page.getByRole('tab', { name: source, exact: true }).click();
      const row = rows.filter({ hasText: name });
      await expect(row.getByRole('button', { name: label, exact: true })).toBeDisabled();
      await row.locator('.browse-name').click();
      const details = page.locator('.modal-details');
      await expect(details.getByRole('button', { name: label, exact: true })).toBeDisabled();
      await expect(details).toContainText('cannot be installed through Ollama');
      await expect(details.locator('.details-install')).toHaveCount(0);
      await expect(details.getByRole('button', { name: 'Delete Model' })).toHaveCount(0);
      if (source === 'OpenRouter') {
        await page.screenshot({
          path: testInfo.outputPath('remote-details.png'),
          fullPage: true,
          animations: 'disabled',
        });
      }
      await details.getByRole('button', { name: 'Close', exact: true }).click();
    }
    expect(connections).toHaveLength(0);
  });

  for (const [source, name, reference] of [
    ['Ollama', 'qwen3.8:27b', 'qwen3.8:27b'],
    ['HuggingFace', 'Owner/Model-GGUF:Q4_K_M', 'hf.co/Owner/Model-GGUF:Q4_K_M'],
  ]) {
    for (const entry of ['row', 'details']) {
      test(`${source} ${entry} sends the exact Ollama download reference`, async ({
        page,
      }, testInfo) => {
        const requests: string[] = [];
        await page.routeWebSocket('**/ws/pull?*', (connection) => {
          connection.onMessage((message) => {
            const frame = JSON.parse(message.toString());
            if (frame.type === 'auth') {
              connection.send(JSON.stringify({ type: 'authenticated' }));
            } else {
              requests.push(frame.model);
              connection.send(JSON.stringify({ type: 'complete', success: true }));
            }
          });
        });
        await page.goto('/models');
        await page.getByRole('tab', { name: 'Browse', exact: true }).click();
        await page.getByRole('tab', { name: source, exact: true }).click();
        const row = page.locator('.browse-item').filter({ hasText: name });
        if (entry === 'details') {
          await row.locator('.browse-name').click();
          const details = page.locator('.modal-details');
          await expect(details.locator('.details-install code')).toHaveText(reference);
          await expect(details.getByRole('button', { name: 'Delete Model' })).toHaveCount(0);
          if (source === 'HuggingFace') {
            await page.screenshot({
              path: testInfo.outputPath('huggingface-details.png'),
              fullPage: true,
              animations: 'disabled',
            });
          }
          await details.getByRole('button', { name: 'Install Model', exact: true }).click();
        } else {
          await row.getByRole('button', { name: 'Install', exact: true }).click();
        }
        await expect.poll(() => requests).toEqual([reference]);
        await page.getByRole('tab', { name: 'Installed', exact: true }).click();
        await expect(page.locator('.models-install-panel')).toContainText('Installation complete');
        await expect(
          page.getByRole('heading', { name: 'Installed Models', exact: true })
        ).toBeVisible();
      });
    }
  }
});
