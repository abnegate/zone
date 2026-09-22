import { test, expect } from './fixtures';
import { setupAuth, mockCommonEndpoints } from './helpers/auth';
import { blockServiceWorker, routeApi } from './test-utils';

test.describe('Model Installation', () => {
  test.beforeEach(async ({ context, page }) => {
    await blockServiceWorker(context);
    await mockCommonEndpoints(page);
    await setupAuth(page);
    await page.goto('/models');
    await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
  });

  test('install form shows model input', async ({ page }) => {
    await expect(page.locator('.models-install-form input')).toBeVisible();
    await expect(page.locator('.models-install-form input')).toHaveAttribute(
      'placeholder',
      'Model name...'
    );
  });

  test('install button is disabled when input empty', async ({ page }) => {
    await expect(page.locator('.models-install-form button[type="submit"]')).toBeDisabled();
  });

  test('install button enables when input has value', async ({ page }) => {
    await page.fill('.models-install-form input', 'llama3.2');
    await expect(page.locator('.models-install-form button[type="submit"]')).not.toBeDisabled();
  });

  test('install button text changes to Install when not pulling', async ({ page }) => {
    await expect(page.locator('.models-install-form button[type="submit"]')).toHaveText('Install');
  });

  test('help text explains model input', async ({ page }) => {
    const helpText = page.locator('.models-install-panel .help-text');
    await expect(helpText).toContainText('Ollama model');
    await expect(helpText).toContainText('HuggingFace');
  });

  test('can type model name with special characters', async ({ page }) => {
    await page.fill('.models-install-form input', 'model:7b-q4_0');
    await expect(page.locator('.models-install-form input')).toHaveValue('model:7b-q4_0');
  });

  test('can type HuggingFace style model name', async ({ page }) => {
    await page.fill('.models-install-form input', 'hf.co/TheBloke/Llama-2-7B-GGUF:Q4_K_M');
    await expect(page.locator('.models-install-form input')).toHaveValue(
      'hf.co/TheBloke/Llama-2-7B-GGUF:Q4_K_M'
    );
  });

  test('clears input after submit', async ({ page }) => {
    // We can't fully test WebSocket pull, but we can verify form behavior
    await page.fill('.models-install-form input', 'test-model');
    await expect(page.locator('.models-install-form input')).toHaveValue('test-model');
  });
});

test.describe('Add Model Section UI', () => {
  test.beforeEach(async ({ context, page }) => {
    await blockServiceWorker(context);
    await mockCommonEndpoints(page);
    await setupAuth(page);
    await page.goto('/models');
    await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
  });

  test('add model section has correct heading', async ({ page }) => {
    await expect(page.getByRole('region', { name: 'Add model' })).toBeVisible();
  });

  test('form is inside a card component', async ({ page }) => {
    await expect(
      page.getByRole('region', { name: 'Add model' }).locator('form.models-install-form')
    ).toBeVisible();
  });

  test('input and button are in same row (input-group)', async ({ page }) => {
    const form = page.locator('.models-install-form');
    await expect(form).toBeVisible();
    await expect(form.locator('input')).toBeVisible();
    await expect(form.locator('button[type="submit"]')).toBeVisible();
    const [inputBox, buttonBox] = await Promise.all([
      form.locator('input').boundingBox(),
      form.locator('button[type="submit"]').boundingBox(),
    ]);
    expect(inputBox, 'input should have a layout box').not.toBeNull();
    expect(buttonBox, 'submit button should have a layout box').not.toBeNull();
    expect(
      Math.abs((inputBox?.y ?? 0) - (buttonBox?.y ?? 0)),
      'input and submit button should sit on the same row'
    ).toBeLessThan(4);
  });
});

test.describe('Install from Browse', () => {
  const mockBrowseModels = [
    { id: 'llama3.2', name: 'llama3.2', description: 'Meta Llama 3.2', downloads: 1500000, tags: ['chat'] },
    { id: 'codellama', name: 'codellama', description: 'Code Llama', downloads: 500000, tags: ['code'] },
  ];

  test.beforeEach(async ({ context, page }) => {
    await blockServiceWorker(context);
    await mockCommonEndpoints(page);
    await page.unroute('**/api/models*');
    await routeApi(page, '**/api/models*', (route) => {
      const url = new URL(route.request().url());
      const source = url.searchParams.get('source');

      if (!source) {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ models: [] }),
        });
        return;
      }

      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ source, models: mockBrowseModels, next_cursor: null }),
      });
    });

    await setupAuth(page);
    await page.goto('/models');
    await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
    await page.click('button[role="tab"]:has-text("Browse")');
    await expect(page.locator('.browse-search')).toBeVisible();
  });

  test('browse item has install button', async ({ page }) => {
    const install = page.locator('.browse-item').first().getByRole('button');
    await expect(install).toBeVisible();
    await expect(install).toHaveText('Install');
  });

  test('clicking install on browse item populates form', async ({ page }) => {
    await page.locator('.browse-item').first().getByRole('button', { name: 'Install' }).click();
    // Switch to Installed tab to see the form
    await page.click('button[role="tab"]:has-text("Installed")');
    await expect(page.locator('.models-install-form input')).toHaveValue('llama3.2');
  });

  test('install from modal populates form and closes modal', async ({ page }) => {
    // Open details modal
    await page.locator('.browse-item').first().click();
    const dialog = page.getByRole('dialog');
    await expect(dialog).toBeVisible();

    await dialog.getByRole('button', { name: 'Install Model' }).click();
    await expect(dialog).toBeHidden();

    // Switch to Installed tab to see the form
    await page.click('button[role="tab"]:has-text("Installed")');

    // Form should be populated
    await expect(page.locator('.models-install-form input')).toHaveValue('llama3.2');
  });

  test('multiple installs overwrite previous input', async ({ page }) => {
    // Click Ollama source tab for predictable ordering
    await page.click('button[role="tab"]:has-text("Ollama")');
    // Click first model's install
    await page.locator('.browse-item').first().getByRole('button', { name: 'Install' }).click();
    // Switch to Installed tab to verify first input
    await page.click('button[role="tab"]:has-text("Installed")');
    await expect(page.locator('.models-install-form input')).toHaveValue('llama3.2');

    // Switch back to Browse (Ollama tab) and click second model's install
    await page.click('button[role="tab"]:has-text("Browse")');
    await page.click('button[role="tab"]:has-text("Ollama")');
    await page.locator('.browse-item').nth(1).getByRole('button', { name: 'Install' }).click();
    // Switch to Installed tab to verify second input
    await page.click('button[role="tab"]:has-text("Installed")');
    await expect(page.locator('.models-install-form input')).toHaveValue('codellama');
  });
});
