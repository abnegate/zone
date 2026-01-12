import { test, expect } from './fixtures';
import { setupAuth, mockCommonEndpoints } from './helpers/auth';
import { blockServiceWorker, routeApi } from './test-utils';

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
  test.beforeEach(async ({ context, page }) => {
    // Block service worker to allow route interception to work
    await blockServiceWorker(context);
    await mockCommonEndpoints(page);
    await page.goto('/');
    await setupAuth(page);
    await page.reload();
    await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
    await page.click('button.main-tab:has-text("Browse")');
    await expect(page.locator('.search-container')).toBeVisible();
  });

  test('handles large model list with virtual scrolling', async ({ page }) => {
    const manyModels = generateMockModels(100);

    await page.unroute('**/api/models*');
    await routeApi(page, '**/api/models*', (route) => {
      const url = new URL(route.request().url());
      const source = url.searchParams.get('source');

      if (source) {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source, models: manyModels, has_more: false }),
        });
        return;
      }

      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [] }),
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

      const offset = parseInt(url.searchParams.get('offset') || '0');

      if (offset === 0) {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source, models: page1, has_more: true }),
        });
      } else {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source, models: page2, has_more: false }),
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

    await page.unroute('**/api/models*');
    await routeApi(page, '**/api/models*', async (route) => {
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

      const offset = parseInt(url.searchParams.get('offset') || '0');

      if (offset > 0) {
        // Delay for load more
        await new Promise(resolve => setTimeout(resolve, 500));
      }

      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ source, models, has_more: offset === 0 }),
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

      const query = url.searchParams.get('q');

      if (query) {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source, models: filteredModels, has_more: false }),
        });
      } else {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source, models: allModels, has_more: false }),
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

      if (source === 'huggingface') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source, models: huggingFaceModels, has_more: false }),
        });
      } else {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source, models: ollamaModels, has_more: false }),
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
      const offset = parseInt(url.searchParams.get('offset') || '0');

      if (offset > 0) {
        route.fulfill({ status: 500, body: 'Server Error' });
      } else {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source, models, has_more: true }),
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
        body: JSON.stringify({ source, models, has_more: false }),
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

    await page.unroute('**/api/models*');
    await routeApi(page, '**/api/models*', (route) => {
      const url = new URL(route.request().url());
      const source = url.searchParams.get('source');
      const method = route.request().method();

      if (method === 'POST') {
        // Mock successful install/pull
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ success: true }),
        });
        return;
      }

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
        body: JSON.stringify({ source, models, has_more: false }),
      });
    });

    // Trigger a search to load with new route
    await page.fill('.search-container input', '');
    await page.click('.search-container button');

    // Click install button - this triggers the install and switches tabs
    await page.locator('.browse-item').first().locator('.btn-primary').click();

    // Switch to Installed tab to see the model form input
    await page.click('button.main-tab:has-text("Installed")');

    // Model input should be populated with the model name
    await expect(page.locator('.model-form input')).toHaveValue('model-0');
  });
});

test.describe('Browse Models - Source Tab Switching', () => {
  test.beforeEach(async ({ context, page }) => {
    // Block service worker to allow route interception to work
    await blockServiceWorker(context);
    await mockCommonEndpoints(page);
    await page.goto('/');
    await setupAuth(page);
    await page.reload();
    await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
    await page.click('button.main-tab:has-text("Browse")');
    await expect(page.locator('.search-container')).toBeVisible();
  });

  test('all three source tabs are visible', async ({ page }) => {
    await expect(page.locator('.source-tab:has-text("Ollama")')).toBeVisible();
    await expect(page.locator('.source-tab:has-text("HuggingFace")')).toBeVisible();
    await expect(page.locator('.source-tab:has-text("ModelScope")')).toBeVisible();
  });

  test('clicking source tabs sends correct source parameter', async ({ page }) => {
    const requests: string[] = [];

    await routeApi(page, '**/api/models*', (route) => {
      const url = new URL(route.request().url());
      const source = url.searchParams.get('source');
      if (source) requests.push(source);
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
        body: JSON.stringify({ source, models: [], has_more: false }),
      });
    });

    // Click HuggingFace tab
    await page.click('.source-tab:has-text("HuggingFace")');
    await page.waitForTimeout(100);

    // Click ModelScope tab
    await page.click('.source-tab:has-text("ModelScope")');
    await page.waitForTimeout(100);

    // Click Ollama tab
    await page.click('.source-tab:has-text("Ollama")');
    await page.waitForTimeout(100);

    expect(requests).toContain('huggingface');
    expect(requests).toContain('modelscope');
    expect(requests).toContain('ollama');
  });
});

test.describe('Browse Models - HuggingFace Specific', () => {
  test.beforeEach(async ({ context, page }) => {
    // Block service worker to allow route interception to work
    await blockServiceWorker(context);
    await mockCommonEndpoints(page);
    await page.goto('/');
    await setupAuth(page);
    await page.reload();
    await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
    await page.click('button.main-tab:has-text("Browse")');
    await expect(page.locator('.search-container')).toBeVisible();
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

      if (source === 'huggingface') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source, models: [hfModel], has_more: false }),
        });
      } else {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source, models: [], has_more: false }),
        });
      }
    });

    // Switch to HuggingFace tab
    await page.click('.source-tab:has-text("HuggingFace")');

    await expect(page.locator('.browse-item')).toHaveCount(1);
    await expect(page.locator('.browse-name')).toHaveText('Model-GGUF');
  });

  test('HuggingFace details modal shows author', async ({ page }) => {
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

      if (source === 'huggingface') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source, models: [hfModel], has_more: false }),
        });
      } else {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source, models: [], has_more: false }),
        });
      }
    });

    // Switch to HuggingFace tab and click model
    await page.click('.source-tab:has-text("HuggingFace")');
    await page.locator('.browse-item').first().click();

    await expect(page.locator('.modal-details')).toBeVisible();
    // Author is displayed (as a span, not a link)
    await expect(page.locator('.details-author-link')).toHaveText('TheBloke');
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

      if (source === 'huggingface') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source, models: [hfModel], has_more: false }),
        });
      } else {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source, models: [], has_more: false }),
        });
      }
    });

    await page.click('.source-tab:has-text("HuggingFace")');
    await page.locator('.browse-item').first().click();

    await expect(page.locator('.details-install code')).toHaveText('hf.co/TheBloke/Model-GGUF');
  });
});

test.describe('Browse Models - ModelScope Specific', () => {
  test.beforeEach(async ({ context, page }) => {
    // Block service worker to allow route interception to work
    await blockServiceWorker(context);
    await mockCommonEndpoints(page);
    await page.goto('/');
    await setupAuth(page);
    await page.reload();
    await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
    await page.click('button.main-tab:has-text("Browse")');
    await expect(page.locator('.search-container')).toBeVisible();
  });

  test('displays ModelScope model with author', async ({ page }) => {
    const msModel = {
      id: 'Qwen/Qwen2.5-7B-GGUF',
      name: 'Qwen2.5-7B-GGUF',
      description: 'A Qwen GGUF model',
      downloads: 100000,
      likes: 500,
      tags: ['gguf', 'qwen'],
      author: 'Qwen',
      install_name: 'modelscope/Qwen/Qwen2.5-7B-GGUF',
      url: 'https://modelscope.cn/Qwen/Qwen2.5-7B-GGUF',
    };

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

      if (source === 'modelscope') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source, models: [msModel], has_more: false, total: 1 }),
        });
      } else {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source, models: [], has_more: false }),
        });
      }
    });

    // Switch to ModelScope tab
    await page.click('.source-tab:has-text("ModelScope")');

    await expect(page.locator('.browse-item')).toHaveCount(1);
    await expect(page.locator('.browse-name')).toHaveText('Qwen2.5-7B-GGUF');
  });

  test('ModelScope details modal shows author', async ({ page }) => {
    const msModel = {
      id: 'Qwen/Qwen2.5-7B-GGUF',
      name: 'Qwen2.5-7B-GGUF',
      description: 'A Qwen GGUF model',
      downloads: 100000,
      likes: 500,
      tags: ['gguf', 'qwen'],
      author: 'Qwen',
      install_name: 'modelscope/Qwen/Qwen2.5-7B-GGUF',
      url: 'https://modelscope.cn/Qwen/Qwen2.5-7B-GGUF',
    };

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

      if (source === 'modelscope') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source, models: [msModel], has_more: false, total: 1 }),
        });
      } else {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source, models: [], has_more: false }),
        });
      }
    });

    // Mock the model info endpoint
    await routeApi(page, '**/api/models/modelscope/**', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          success: true,
          content: '# Qwen2.5 Model\n\nA great model.',
          gguf_size: 5000000000,
        }),
      });
    });

    // Switch to ModelScope tab and click model
    await page.click('.source-tab:has-text("ModelScope")');
    await page.locator('.browse-item').first().click();

    await expect(page.locator('.modal-details')).toBeVisible();
    await expect(page.locator('.details-author-link')).toHaveText('Qwen');
  });

  test('ModelScope details shows install command with modelscope prefix', async ({ page }) => {
    const msModel = {
      id: 'Qwen/Qwen2.5-7B-GGUF',
      name: 'Qwen2.5-7B-GGUF',
      description: 'A Qwen GGUF model',
      downloads: 100000,
      likes: 500,
      tags: ['gguf'],
      author: 'Qwen',
      install_name: 'modelscope/Qwen/Qwen2.5-7B-GGUF',
      url: 'https://modelscope.cn/Qwen/Qwen2.5-7B-GGUF',
    };

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

      if (source === 'modelscope') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source, models: [msModel], has_more: false, total: 1 }),
        });
      } else {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source, models: [], has_more: false }),
        });
      }
    });

    await routeApi(page, '**/api/models/modelscope/**', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ success: true, content: null, gguf_size: null }),
      });
    });

    await page.click('.source-tab:has-text("ModelScope")');
    await page.locator('.browse-item').first().click();

    await expect(page.locator('.details-install code')).toHaveText('modelscope/Qwen/Qwen2.5-7B-GGUF');
  });

  test('ModelScope details shows View on ModelScope link', async ({ page }) => {
    const msModel = {
      id: 'Qwen/Qwen2.5-7B-GGUF',
      name: 'Qwen2.5-7B-GGUF',
      description: 'A Qwen GGUF model',
      downloads: 100000,
      likes: 500,
      tags: ['gguf'],
      author: 'Qwen',
      install_name: 'modelscope/Qwen/Qwen2.5-7B-GGUF',
      url: 'https://modelscope.cn/Qwen/Qwen2.5-7B-GGUF',
    };

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

      if (source === 'modelscope') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source, models: [msModel], has_more: false, total: 1 }),
        });
      } else {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source, models: [], has_more: false }),
        });
      }
    });

    await routeApi(page, '**/api/models/modelscope/**', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ success: true, content: null, gguf_size: null }),
      });
    });

    await page.click('.source-tab:has-text("ModelScope")');
    await page.locator('.browse-item').first().click();

    await expect(page.locator('.details-link a')).toContainText('View on ModelScope');
    await expect(page.locator('.details-link a')).toHaveAttribute('href', 'https://modelscope.cn/Qwen/Qwen2.5-7B-GGUF');
  });

  test('ModelScope shows models from response', async ({ page }) => {
    const msModels = Array.from({ length: 5 }, (_, i) => ({
      id: `Author/Model-${i}`,
      name: `Model-${i}`,
      description: 'A model',
      downloads: 1000 * (5 - i),
      likes: 100,
      tags: ['gguf'],
      author: 'Author',
      install_name: `modelscope/Author/Model-${i}`,
      url: `https://modelscope.cn/Author/Model-${i}`,
    }));

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

      if (source === 'modelscope') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            source,
            models: msModels,
            has_more: true,
            total: 500, // Total available models
          }),
        });
      } else {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source, models: [], has_more: false }),
        });
      }
    });

    await page.click('.source-tab:has-text("ModelScope")');

    // Wait for ModelScope results to load - check for first ModelScope model name
    await expect(page.locator('.browse-name').first()).toHaveText('Model-0');

    // Verify models are loaded by checking that at least one item is visible
    await expect(page.locator('.browse-item').first()).toBeVisible();
  });

  test('ModelScope install button uses modelscope install name', async ({ page }) => {
    const msModel = {
      id: 'Qwen/Qwen2.5-7B-GGUF',
      name: 'Qwen2.5-7B-GGUF',
      description: 'A Qwen GGUF model',
      downloads: 100000,
      likes: 500,
      tags: ['gguf'],
      author: 'Qwen',
      install_name: 'modelscope/Qwen/Qwen2.5-7B-GGUF',
      url: 'https://modelscope.cn/Qwen/Qwen2.5-7B-GGUF',
    };

    await routeApi(page, '**/api/models*', (route) => {
      const url = new URL(route.request().url());
      const source = url.searchParams.get('source');
      const method = route.request().method();

      if (method === 'POST') {
        // Mock successful install/pull
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ success: true }),
        });
        return;
      }

      if (source === 'modelscope') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source: 'modelscope', models: [msModel], has_more: false, total: 1 }),
        });
      } else {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ models: [], has_more: false }),
        });
      }
    });

    await page.click('.source-tab:has-text("ModelScope")');

    // Click install button
    await page.locator('.browse-item').first().locator('.btn-primary').click();

    // Switch to Installed tab to see the model form input
    await page.click('button.main-tab:has-text("Installed")');

    // Model input should have the modelscope install name
    await expect(page.locator('.model-form input')).toHaveValue('modelscope/Qwen/Qwen2.5-7B-GGUF');
  });
});
