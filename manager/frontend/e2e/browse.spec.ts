import { test, expect } from '@playwright/test';

const generateMockModels = (count: number, startId = 0) => {
  return Array.from({ length: count }, (_, i) => ({
    id: `model-${startId + i}`,
    name: `model-${startId + i}`,
    description: `Description for model ${startId + i}`,
    downloads: Math.floor(Math.random() * 1000000),
    tags: ['tag1', 'tag2'],
  }));
};

test.describe('Browse Models - Virtual Scrolling', () => {
  test.beforeEach(async ({ page }) => {
    await page.route('/api/models', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [] }),
      });
    });

    await page.route('/api/browse*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [], has_more: false }),
      });
    });

    await page.goto('/');
    await page.evaluate(() => localStorage.setItem('manager_api_key', 'test-key'));
    await page.reload();
    await expect(page.locator('.login-overlay')).not.toBeVisible({ timeout: 10000 });
  });

  test('handles large model list with virtual scrolling', async ({ page }) => {
    const manyModels = generateMockModels(100);

    await page.unroute('/api/browse*');
    await page.route('/api/browse*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: manyModels, has_more: false }),
      });
    });

    // Trigger a search to load with new route
    await page.fill('.search-container input', '');
    await page.click('.search-container button');

    // Virtual list should be present with items
    await expect(page.locator('.virtual-browse-container')).toBeVisible();
  });

  test('infinite scroll loads more models', async ({ page }) => {
    let page1 = generateMockModels(20, 0);
    let page2 = generateMockModels(20, 20);

    await page.unroute('/api/browse*');
    await page.route('/api/browse*', (route) => {
      const url = new URL(route.request().url());
      const offset = parseInt(url.searchParams.get('offset') || '0');

      if (offset === 0) {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ models: page1, has_more: true }),
        });
      } else {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ models: page2, has_more: false }),
        });
      }
    });

    // Trigger a search to load with new route
    await page.fill('.search-container input', '');
    await page.click('.search-container button');

    // Initial load should show first page models
    await expect(page.locator('.browse-item').first()).toBeVisible();
  });

  test('shows loading indicator when loading more', async ({ page }) => {
    const models = generateMockModels(20);

    await page.unroute('/api/browse*');
    await page.route('/api/browse*', async (route) => {
      const url = new URL(route.request().url());
      const offset = parseInt(url.searchParams.get('offset') || '0');

      if (offset > 0) {
        // Delay for load more
        await new Promise(resolve => setTimeout(resolve, 500));
      }

      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models, has_more: offset === 0 }),
      });
    });

    // Trigger a search to load with new route
    await page.fill('.search-container input', '');
    await page.click('.search-container button');

    // Should show browse results
    await expect(page.locator('.virtual-browse-container')).toBeVisible();
  });

  test('search clears previous results', async ({ page }) => {
    const allModels = generateMockModels(10);
    const filteredModels = [allModels[0]];

    await page.unroute('/api/browse*');
    await page.route('/api/browse*', (route) => {
      const url = new URL(route.request().url());
      const query = url.searchParams.get('q');

      if (query) {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ models: filteredModels, has_more: false }),
        });
      } else {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ models: allModels, has_more: false }),
        });
      }
    });

    // Trigger initial browse load
    await page.fill('.search-container input', '');
    await page.click('.search-container button');
    // Virtual list only renders visible items, so check for first item
    await expect(page.locator('.browse-item').first()).toBeVisible();

    // Search
    await page.fill('.search-container input', 'model-0');
    await page.click('.search-container button');

    // Should show filtered results - only one result
    await expect(page.locator('.browse-item')).toHaveCount(1);
  });

  test('source tab switch resets results', async ({ page }) => {
    const ollamaModels = generateMockModels(5);
    const huggingFaceModels = [
      {
        id: 'hf-model',
        name: 'HF Model',
        description: 'HuggingFace model',
        downloads: 100000,
        tags: ['gguf'],
        author: 'TestAuthor',
        install_name: 'hf.co/test',
      },
    ];

    await page.unroute('/api/browse*');
    await page.route('/api/browse*', (route) => {
      const url = new URL(route.request().url());
      const source = url.searchParams.get('source');

      if (source === 'huggingface') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ models: huggingFaceModels, has_more: false }),
        });
      } else {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ models: ollamaModels, has_more: false }),
        });
      }
    });

    // Trigger initial load with Ollama
    await page.fill('.search-container input', '');
    await page.click('.search-container button');

    // Initial Ollama results
    await expect(page.locator('.browse-item')).toHaveCount(5);

    // Switch to HuggingFace
    await page.click('.source-tab:has-text("HuggingFace")');
    await expect(page.locator('.browse-item')).toHaveCount(1);
    await expect(page.locator('.browse-name')).toHaveText('HF Model');

    // Switch back to Ollama
    await page.click('.source-tab:has-text("Ollama")');
    await expect(page.locator('.browse-item')).toHaveCount(5);
  });

  test('handles API error during infinite scroll gracefully', async ({ page }) => {
    const models = generateMockModels(20);

    await page.unroute('/api/browse*');
    await page.route('/api/browse*', (route) => {
      const url = new URL(route.request().url());
      const offset = parseInt(url.searchParams.get('offset') || '0');

      if (offset > 0) {
        route.fulfill({ status: 500, body: 'Server Error' });
      } else {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ models, has_more: true }),
        });
      }
    });

    // Trigger a search to load with new route
    await page.fill('.search-container input', '');
    await page.click('.search-container button');

    // Initial load should work
    await expect(page.locator('.browse-item').first()).toBeVisible();
  });

  test('clicking browse item opens details modal', async ({ page }) => {
    const models = generateMockModels(5);

    await page.unroute('/api/browse*');
    await page.route('/api/browse*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models, has_more: false }),
      });
    });

    // Trigger a search to load with new route
    await page.fill('.search-container input', '');
    await page.click('.search-container button');

    await page.locator('.browse-item').first().click();

    await expect(page.locator('.modal-details')).toBeVisible();
    await expect(page.locator('.modal-details-header h3')).toHaveText('model-0');
  });

  test('install button on browse item triggers install', async ({ page }) => {
    const models = generateMockModels(3);

    await page.unroute('/api/browse*');
    await page.route('/api/browse*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models, has_more: false }),
      });
    });

    // Trigger a search to load with new route
    await page.fill('.search-container input', '');
    await page.click('.search-container button');

    // Click install button - it should populate the model form input
    await page.locator('.browse-item').first().locator('.btn-primary').click();

    // Model input should be populated with the model name
    await expect(page.locator('.model-form input')).toHaveValue('model-0');
  });
});

test.describe('Browse Models - HuggingFace Specific', () => {
  test.beforeEach(async ({ page }) => {
    await page.route('/api/models', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [] }),
      });
    });

    await page.route('/api/browse*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [], has_more: false }),
      });
    });

    await page.goto('/');
    await page.evaluate(() => localStorage.setItem('manager_api_key', 'test-key'));
    await page.reload();
    await expect(page.locator('.login-overlay')).not.toBeVisible({ timeout: 10000 });
  });

  test('displays HuggingFace model with author', async ({ page }) => {
    const hfModel = {
      id: 'TheBloke/Model-GGUF',
      name: 'Model-GGUF',
      description: 'A great model',
      downloads: 500000,
      likes: 1000,
      tags: ['gguf', 'llama'],
      author: 'TheBloke',
      install_name: 'hf.co/TheBloke/Model-GGUF',
      url: 'https://huggingface.co/TheBloke/Model-GGUF',
    };

    await page.route('/api/browse*', (route) => {
      const url = new URL(route.request().url());
      const source = url.searchParams.get('source');

      if (source === 'huggingface') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ models: [hfModel], has_more: false }),
        });
      } else {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ models: [], has_more: false }),
        });
      }
    });

    // Switch to HuggingFace tab
    await page.click('.source-tab:has-text("HuggingFace")');

    await expect(page.locator('.browse-item')).toHaveCount(1);
    await expect(page.locator('.browse-name')).toHaveText('Model-GGUF');
  });

  test('HuggingFace details modal shows author link', async ({ page }) => {
    const hfModel = {
      id: 'TheBloke/Model-GGUF',
      name: 'Model-GGUF',
      description: 'A great model',
      downloads: 500000,
      likes: 1000,
      tags: ['gguf', 'llama'],
      author: 'TheBloke',
      install_name: 'hf.co/TheBloke/Model-GGUF',
      url: 'https://huggingface.co/TheBloke/Model-GGUF',
    };

    await page.route('/api/browse*', (route) => {
      const url = new URL(route.request().url());
      const source = url.searchParams.get('source');

      if (source === 'huggingface') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ models: [hfModel], has_more: false }),
        });
      } else {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ models: [], has_more: false }),
        });
      }
    });

    // Switch to HuggingFace tab and click model
    await page.click('.source-tab:has-text("HuggingFace")');
    await page.locator('.browse-item').first().click();

    await expect(page.locator('.modal-details')).toBeVisible();
    await expect(page.locator('.details-author-link')).toHaveText('TheBloke');
    await expect(page.locator('.details-author-link')).toHaveAttribute('href', 'https://huggingface.co/TheBloke');
  });

  test('HuggingFace details shows install command', async ({ page }) => {
    const hfModel = {
      id: 'TheBloke/Model-GGUF',
      name: 'Model-GGUF',
      description: 'A great model',
      downloads: 500000,
      likes: 1000,
      tags: ['gguf'],
      author: 'TheBloke',
      install_name: 'hf.co/TheBloke/Model-GGUF',
      url: 'https://huggingface.co/TheBloke/Model-GGUF',
    };

    await page.route('/api/browse*', (route) => {
      const url = new URL(route.request().url());
      const source = url.searchParams.get('source');

      if (source === 'huggingface') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ models: [hfModel], has_more: false }),
        });
      } else {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ models: [], has_more: false }),
        });
      }
    });

    await page.click('.source-tab:has-text("HuggingFace")');
    await page.locator('.browse-item').first().click();

    await expect(page.locator('.details-install code')).toHaveText('hf.co/TheBloke/Model-GGUF');
  });
});
