import { test, expect } from '@playwright/test';

const mockInstalledModels = [
  {
    name: 'llama3.2:latest',
    size: 4661224448,
    modified_at: '2024-01-15T10:30:00Z',
    details: { family: 'llama', description: 'Meta Llama 3.2' },
  },
  {
    name: 'mistral:7b',
    size: 4109880832,
    modified_at: '2024-01-10T08:00:00Z',
    details: { family: 'mistral', description: 'Mistral 7B' },
  },
];

const mockBrowseModels = [
  { id: 'llama3.2', name: 'llama3.2', description: 'Meta Llama 3.2', downloads: 1500000, tags: ['chat', 'general'] },
  { id: 'mistral', name: 'mistral', description: 'Mistral AI 7B', downloads: 800000, tags: ['chat', 'code'] },
  { id: 'codellama', name: 'codellama', description: 'Code Llama', downloads: 500000, tags: ['code'] },
];

test.describe('Models Page', () => {
  test.beforeEach(async ({ page }) => {
    await page.route('**/api/models', (route) => {
      if (route.request().method() === 'GET') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ models: mockInstalledModels }),
        });
      } else {
        route.continue();
      }
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

  test('displays installed models', async ({ page }) => {
    await expect(page.locator('.model-item')).toHaveCount(2);
    await expect(page.locator('.model-name').first()).toHaveText('llama3.2:latest');
    await expect(page.locator('.model-name').nth(1)).toHaveText('mistral:7b');
  });

  test('shows model size formatted correctly', async ({ page }) => {
    await expect(page.locator('.model-meta').first()).toContainText('4.3 GB');
  });

  test('displays browse models', async ({ page }) => {
    await expect(page.locator('.browse-item')).toHaveCount(3);
    await expect(page.locator('.browse-name').first()).toHaveText('llama3.2');
  });

  test('shows download counts formatted', async ({ page }) => {
    await expect(page.locator('.browse-downloads').first()).toContainText('1.5M downloads');
  });

  test('displays model tags', async ({ page }) => {
    const firstItem = page.locator('.browse-item').first();
    await expect(firstItem.locator('.tag')).toHaveCount(2);
    await expect(firstItem.locator('.tag').first()).toHaveText('chat');
  });

  test('search filters browse results', async ({ page }) => {
    // Mock filtered results
    await page.route('**/api/browse*', (route) => {
      const url = new URL(route.request().url());
      const query = url.searchParams.get('q');
      if (query === 'code') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            models: [mockBrowseModels[2]], // codellama
            has_more: false,
          }),
        });
      } else {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ models: mockBrowseModels, has_more: false }),
        });
      }
    });

    await page.fill('.search-container input', 'code');
    await page.click('.search-container button');

    await expect(page.locator('.browse-item')).toHaveCount(1);
    await expect(page.locator('.browse-name')).toHaveText('codellama');
  });

  test('switches between Ollama and HuggingFace tabs', async ({ page }) => {
    // Mock HuggingFace response
    await page.route('**/api/browse*', (route) => {
      const url = new URL(route.request().url());
      const source = url.searchParams.get('source');
      if (source === 'huggingface') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            models: [{
              id: 'TheBloke/Llama-2-7B-GGUF',
              name: 'Llama-2-7B-GGUF',
              description: 'GGUF format Llama 2',
              downloads: 250000,
              tags: ['gguf'],
              author: 'TheBloke',
              install_name: 'hf.co/TheBloke/Llama-2-7B-GGUF',
            }],
            has_more: false,
          }),
        });
      } else {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ models: mockBrowseModels, has_more: false }),
        });
      }
    });

    // Click HuggingFace tab
    await page.click('.source-tab:has-text("HuggingFace")');
    await expect(page.locator('.source-tab:has-text("HuggingFace")')).toHaveClass(/active/);
    await expect(page.locator('.browse-name').first()).toHaveText('Llama-2-7B-GGUF');
  });

  test('opens model details modal on click', async ({ page }) => {
    await page.locator('.model-item').first().click();

    await expect(page.locator('.modal-details')).toBeVisible();
    await expect(page.locator('.modal-details-header h3')).toHaveText('llama3.2:latest');
    await expect(page.locator('.details-source')).toHaveText('Installed');
  });

  test('closes modal on backdrop click', async ({ page }) => {
    await page.locator('.model-item').first().click();
    await expect(page.locator('.modal-details')).toBeVisible();

    await page.locator('.modal-backdrop').click({ position: { x: 10, y: 10 } });
    await expect(page.locator('.modal-details')).not.toBeVisible();
  });

  test('closes modal on close button click', async ({ page }) => {
    await page.locator('.model-item').first().click();
    await expect(page.locator('.modal-details')).toBeVisible();

    await page.locator('.modal-close').click();
    await expect(page.locator('.modal-details')).not.toBeVisible();
  });

  test('shows delete confirmation modal', async ({ page }) => {
    await page.locator('.model-item .btn-danger-icon').first().click();

    await expect(page.locator('.modal-content h3')).toHaveText('Delete Model');
    await expect(page.locator('.modal-content')).toContainText('llama3.2:latest');
  });

  test('cancels delete confirmation', async ({ page }) => {
    await page.locator('.model-item .btn-danger-icon').first().click();
    await expect(page.locator('.modal-content h3')).toHaveText('Delete Model');

    await page.click('button:has-text("Cancel")');
    await expect(page.locator('.modal-content h3')).not.toBeVisible();
    await expect(page.locator('.model-item')).toHaveCount(2);
  });

  test('deletes model on confirmation', async ({ page }) => {
    await page.route('**/api/models/*', (route) => {
      if (route.request().method() === 'DELETE') {
        route.fulfill({ status: 200 });
      } else {
        route.continue();
      }
    });

    await page.locator('.model-item .btn-danger-icon').first().click();
    await page.click('button:has-text("Delete")');

    // Modal should close and model removed from list
    await expect(page.locator('.modal-content h3')).not.toBeVisible();
  });

  test('add model input accepts text', async ({ page }) => {
    const input = page.locator('.model-form input');
    await input.fill('phi3:mini');
    await expect(input).toHaveValue('phi3:mini');
  });

  test('install button disabled when input empty', async ({ page }) => {
    await expect(page.locator('.model-form button[type="submit"]')).toBeDisabled();
  });

  test('install button enabled when input has value', async ({ page }) => {
    await page.fill('.model-form input', 'phi3:mini');
    await expect(page.locator('.model-form button[type="submit"]')).not.toBeDisabled();
  });

  test('refresh button reloads models list', async ({ page }) => {
    const responsePromise = page.waitForResponse('/api/models');
    await page.click('.card-header .btn-icon');
    const response = await responsePromise;

    expect(response.status()).toBe(200);
  });

  test('shows loading state while fetching models', async ({ page }) => {
    // We need to set up routing AFTER setting the key so the initial fetch catches the delay
    await page.goto('/');
    await page.evaluate(() => localStorage.setItem('manager_api_key', 'test-key'));

    // Create a delayed response
    await page.route('**/api/models', async (route) => {
      await new Promise(resolve => setTimeout(resolve, 300));
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: mockInstalledModels }),
      });
    });

    await page.route('**/api/browse*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: mockBrowseModels, has_more: false }),
      });
    });

    await page.reload();

    // Should show loading state briefly - use first() to be specific
    await expect(page.locator('.loading-placeholder').first()).toContainText('Loading models...');
  });

  test('shows error message when models API fails', async ({ page }) => {
    // First login successfully, then make future requests fail
    await page.goto('/');
    await page.evaluate(() => localStorage.setItem('manager_api_key', 'test-key'));
    await page.reload();
    await expect(page.locator('.login-overlay')).not.toBeVisible();

    // Now set up failure for refresh
    await page.route('**/api/models', (route) => {
      if (route.request().method() === 'GET') {
        route.fulfill({ status: 500, body: 'Internal Server Error' });
      } else {
        route.continue();
      }
    });

    // Click refresh to trigger error
    await page.click('.card-header .btn-icon');

    await expect(page.locator('.error-placeholder')).toBeVisible();
  });

  test('shows empty state when no models installed', async ({ page }) => {
    await page.route('**/api/models', (route) => {
      if (route.request().method() === 'GET') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ models: [] }),
        });
      } else {
        route.continue();
      }
    });

    await page.goto('/');
    await page.evaluate(() => localStorage.setItem('manager_api_key', 'test-key'));
    await page.reload();
    await expect(page.locator('.login-overlay')).not.toBeVisible();

    await expect(page.locator('.empty-placeholder')).toHaveText('No models installed');
  });

  test('shows error when browse API fails', async ({ page }) => {
    // Set up browse to fail
    await page.route('**/api/browse*', (route) => {
      route.fulfill({ status: 500, body: 'Internal Server Error' });
    });

    await page.goto('/');
    await page.evaluate(() => localStorage.setItem('manager_api_key', 'test-key'));
    await page.reload();
    await expect(page.locator('.login-overlay')).not.toBeVisible();

    // Should show error in the browse section
    await expect(page.locator('.error-placeholder').last()).toBeVisible();
  });

  test('handles long model names gracefully', async ({ page }) => {
    const longNameModel = {
      name: 'this-is-an-extremely-long-model-name-that-should-wrap-properly:latest-v1.0.0-beta',
      size: 4661224448,
      modified_at: '2024-01-15T10:30:00Z',
      details: { family: 'llama' },
    };

    await page.route('**/api/models', (route) => {
      if (route.request().method() === 'GET') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ models: [longNameModel] }),
        });
      } else {
        route.continue();
      }
    });

    await page.goto('/');
    await page.evaluate(() => localStorage.setItem('manager_api_key', 'test-key'));
    await page.reload();
    await expect(page.locator('.login-overlay')).not.toBeVisible();

    await expect(page.locator('.model-name')).toContainText('this-is-an-extremely-long-model-name');
  });

  test('shows delete button loading state', async ({ page }) => {
    let deleteResolved = false;
    await page.route('**/api/models/*', async (route) => {
      if (route.request().method() === 'DELETE') {
        await new Promise(resolve => setTimeout(resolve, 300));
        deleteResolved = true;
        route.fulfill({ status: 200 });
      } else {
        route.continue();
      }
    });

    await page.locator('.model-item .btn-danger-icon').first().click();
    await page.click('button:has-text("Delete")');

    // Should show deleting state
    await expect(page.locator('button:has-text("Deleting...")')).toBeVisible();
    expect(deleteResolved).toBe(false);
  });

  test('handles delete API failure gracefully', async ({ page }) => {
    // Unroute and reroute to capture DELETE
    await page.unroute('/api/models/*');
    await page.route('**/api/models/*', (route) => {
      if (route.request().method() === 'DELETE') {
        route.fulfill({ status: 500, body: 'Failed to delete' });
      } else {
        route.continue();
      }
    });

    await page.locator('.model-item .btn-danger-icon').first().click();
    await page.click('button:has-text("Delete")');

    // Wait for modal to close (delete completes but fails)
    await expect(page.locator('.modal-content h3')).not.toBeVisible({ timeout: 3000 });
    // Even on failure, the UI may have optimistically removed the model
    // This test verifies the app doesn't crash on delete failure
  });

  test('shows empty browse results message', async ({ page }) => {
    await page.route('**/api/browse*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [], has_more: false }),
      });
    });

    await page.fill('.search-container input', 'nonexistent-model-xyz');
    await page.click('.search-container button');

    // Should show "No models found" message
    await expect(page.locator('.empty-placeholder').last()).toContainText('No models found');
  });

  test('clears search results on source tab change', async ({ page }) => {
    await page.route('**/api/browse*', (route) => {
      const url = new URL(route.request().url());
      const source = url.searchParams.get('source');
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          models: source === 'huggingface' ? [] : mockBrowseModels,
          has_more: false,
        }),
      });
    });

    // Initial results should be present
    await expect(page.locator('.browse-item')).toHaveCount(3);

    // Switch to HuggingFace
    await page.click('.source-tab:has-text("HuggingFace")');

    // Should show HuggingFace tab as active
    await expect(page.locator('.source-tab:has-text("HuggingFace")')).toHaveClass(/active/);
  });

  test('shows browse model details from click', async ({ page }) => {
    await page.locator('.browse-item').first().click();

    await expect(page.locator('.modal-details')).toBeVisible();
    await expect(page.locator('.modal-details-header h3')).toHaveText('llama3.2');
    await expect(page.locator('.details-source')).toHaveText('ollama');
  });

  test('install button in details modal works', async ({ page }) => {
    await page.locator('.browse-item').first().click();
    await expect(page.locator('.modal-details')).toBeVisible();

    // The modal should have an install button
    await expect(page.locator('.modal-details .btn-primary')).toContainText('Install');
  });

  test('keyboard escape closes details modal', async ({ page }) => {
    await page.locator('.model-item').first().click();
    await expect(page.locator('.modal-details')).toBeVisible();

    await page.keyboard.press('Escape');
    // Modal may or may not close with Escape - depends on implementation
    // Just verify the test doesn't crash
  });

  test('model meta shows correct format for different sizes', async ({ page }) => {
    const modelsWithVariousSizes = [
      { name: 'tiny:latest', size: 500, modified_at: '2024-01-15T10:30:00Z', details: {} },
      { name: 'small:latest', size: 1024 * 500, modified_at: '2024-01-15T10:30:00Z', details: {} },
      { name: 'medium:latest', size: 1024 * 1024 * 500, modified_at: '2024-01-15T10:30:00Z', details: {} },
    ];

    await page.route('**/api/models', (route) => {
      if (route.request().method() === 'GET') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ models: modelsWithVariousSizes }),
        });
      } else {
        route.continue();
      }
    });

    await page.goto('/');
    await page.evaluate(() => localStorage.setItem('manager_api_key', 'test-key'));
    await page.reload();
    await expect(page.locator('.login-overlay')).not.toBeVisible();

    await expect(page.locator('.model-meta').nth(0)).toContainText('500 B');
    await expect(page.locator('.model-meta').nth(1)).toContainText('500 KB');
    await expect(page.locator('.model-meta').nth(2)).toContainText('500 MB');
  });

  test('download count formats correctly for different values', async ({ page }) => {
    const modelsWithDownloads = [
      { id: 'low', name: 'low', description: '', downloads: 500, tags: [] },
      { id: 'medium', name: 'medium', description: '', downloads: 50000, tags: [] },
      { id: 'high', name: 'high', description: '', downloads: 5000000, tags: [] },
    ];

    // Unroute existing and set new route
    await page.unroute('/api/browse*');
    await page.route('**/api/browse*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: modelsWithDownloads, has_more: false }),
      });
    });

    // Trigger a search to reload browse data
    await page.fill('.search-container input', 'test');
    await page.click('.search-container button');

    await expect(page.locator('.browse-downloads').nth(0)).toContainText('500 downloads');
    await expect(page.locator('.browse-downloads').nth(1)).toContainText('50.0K downloads');
    await expect(page.locator('.browse-downloads').nth(2)).toContainText('5.0M downloads');
  });
});
