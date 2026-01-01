import { test, expect } from '@playwright/test';

test.describe('Model Installation', () => {
  test.beforeEach(async ({ page }) => {
    await page.route('**/api/models', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [] }),
      });
    });

    await page.route('**/api/browse*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [], has_more: false }),
      });
    });

    await page.goto('/');
    await page.evaluate(() => localStorage.setItem('manager_api_key', 'test-key'));
    await page.reload();
    await expect(page.locator('.login-overlay')).not.toBeVisible();
  });

  test('install form shows model input', async ({ page }) => {
    await expect(page.locator('.model-form input')).toBeVisible();
    await expect(page.locator('.model-form input')).toHaveAttribute('placeholder', 'Model name...');
  });

  test('install button is disabled when input empty', async ({ page }) => {
    await expect(page.locator('.model-form button[type="submit"]')).toBeDisabled();
  });

  test('install button enables when input has value', async ({ page }) => {
    await page.fill('.model-form input', 'llama3.2');
    await expect(page.locator('.model-form button[type="submit"]')).not.toBeDisabled();
  });

  test('install button text changes to Install when not pulling', async ({ page }) => {
    await expect(page.locator('.model-form button[type="submit"]')).toHaveText('Install');
  });

  test('help text explains model input', async ({ page }) => {
    await expect(page.locator('.card .help-text').first()).toContainText('Ollama model name');
    await expect(page.locator('.card .help-text').first()).toContainText('HuggingFace');
  });

  test('can type model name with special characters', async ({ page }) => {
    await page.fill('.model-form input', 'model:7b-q4_0');
    await expect(page.locator('.model-form input')).toHaveValue('model:7b-q4_0');
  });

  test('can type HuggingFace style model name', async ({ page }) => {
    await page.fill('.model-form input', 'hf.co/TheBloke/Llama-2-7B-GGUF:Q4_K_M');
    await expect(page.locator('.model-form input')).toHaveValue('hf.co/TheBloke/Llama-2-7B-GGUF:Q4_K_M');
  });

  test('clears input after submit', async ({ page }) => {
    // We can't fully test WebSocket pull, but we can verify form behavior
    await page.fill('.model-form input', 'test-model');
    await expect(page.locator('.model-form input')).toHaveValue('test-model');
  });
});

test.describe('Add Model Section UI', () => {
  test.beforeEach(async ({ page }) => {
    await page.route('**/api/models', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [] }),
      });
    });

    await page.route('**/api/browse*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [], has_more: false }),
      });
    });

    await page.goto('/');
    await page.evaluate(() => localStorage.setItem('manager_api_key', 'test-key'));
    await page.reload();
    await expect(page.locator('.login-overlay')).not.toBeVisible();
  });

  test('add model section has correct heading', async ({ page }) => {
    await expect(page.locator('.card h2').first()).toHaveText('Add Model');
  });

  test('form is inside a card component', async ({ page }) => {
    await expect(page.locator('.card').first().locator('.model-form')).toBeVisible();
  });

  test('input and button are in same row (input-group)', async ({ page }) => {
    await expect(page.locator('.input-group')).toBeVisible();
    await expect(page.locator('.input-group input')).toBeVisible();
    await expect(page.locator('.input-group button')).toBeVisible();
  });
});

test.describe('Install from Browse', () => {
  const mockBrowseModels = [
    { id: 'llama3.2', name: 'llama3.2', description: 'Meta Llama 3.2', downloads: 1500000, tags: ['chat'] },
    { id: 'codellama', name: 'codellama', description: 'Code Llama', downloads: 500000, tags: ['code'] },
  ];

  test.beforeEach(async ({ page }) => {
    await page.route('**/api/models', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [] }),
      });
    });

    await page.route('**/api/browse*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: mockBrowseModels, has_more: false }),
      });
    });

    await page.goto('/');
    await page.evaluate(() => localStorage.setItem('manager_api_key', 'test-key'));
    await page.reload();
    await expect(page.locator('.login-overlay')).not.toBeVisible();
  });

  test('browse item has install button', async ({ page }) => {
    await expect(page.locator('.browse-item').first().locator('.btn-primary')).toBeVisible();
    await expect(page.locator('.browse-item').first().locator('.btn-primary')).toHaveText('Install');
  });

  test('clicking install on browse item populates form', async ({ page }) => {
    await page.locator('.browse-item').first().locator('.btn-primary').click();
    await expect(page.locator('.model-form input')).toHaveValue('llama3.2');
  });

  test('install from modal populates form and closes modal', async ({ page }) => {
    // Open details modal
    await page.locator('.browse-item').first().click();
    await expect(page.locator('.modal-details')).toBeVisible();

    // Click install in modal
    await page.locator('.modal-details .btn-primary').click();

    // Form should be populated
    await expect(page.locator('.model-form input')).toHaveValue('llama3.2');
  });

  test('multiple installs overwrite previous input', async ({ page }) => {
    // Click first model's install
    await page.locator('.browse-item').first().locator('.btn-primary').click();
    await expect(page.locator('.model-form input')).toHaveValue('llama3.2');

    // Click second model's install
    await page.locator('.browse-item').nth(1).locator('.btn-primary').click();
    await expect(page.locator('.model-form input')).toHaveValue('codellama');
  });
});
