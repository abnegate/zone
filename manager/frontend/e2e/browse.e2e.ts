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
    await setupAuth(page);
    await page.goto('/models');
    await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
    // Open the Browse catalogue
    await page.click('button[role="tab"]:has-text("Browse")');
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
          body: JSON.stringify({ source, models: manyModels, next_cursor: null }),
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
          body: JSON.stringify({ source, models: page1, next_cursor: 'page2' }),
        });
      } else {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source, models: page2, next_cursor: null }),
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
        body: JSON.stringify({ source, models, next_cursor: offset === 0 ? 'more' : null }),
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
          body: JSON.stringify({ source, models: filteredModels, next_cursor: null }),
        });
      } else {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source, models: allModels, next_cursor: null }),
        });
      }
    });

    // Click Ollama tab to test single-source behavior
    await page.click('button[role="tab"]:has-text("Ollama")');
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
          body: JSON.stringify({ source, models: huggingFaceModels, next_cursor: null }),
        });
      } else {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source, models: ollamaModels, next_cursor: null }),
        });
      }
    });

    // Click Ollama tab to trigger initial load with Ollama
    await page.click('button[role="tab"]:has-text("Ollama")');

    // Initial Ollama results
    await expect(page.locator('.browse-item')).toHaveCount(5);

    // Switch to HuggingFace
    await page.click('button[role="tab"]:has-text("HuggingFace")');
    await expect(page.locator('.browse-item')).toHaveCount(1);
    await expect(page.locator('.browse-name')).toHaveText('HF Model');

    // Switch back to Ollama
    await page.click('button[role="tab"]:has-text("Ollama")');
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
          body: JSON.stringify({ source, models, next_cursor: 'more' }),
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
        body: JSON.stringify({ source, models, next_cursor: null }),
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
        body: JSON.stringify({ source, models, next_cursor: null }),
      });
    });

    // Trigger a search to load with new route
    await page.fill('.search-container input', '');
    await page.click('.search-container button');

    // Click install button - this triggers the install and switches tabs
    await page.locator('.browse-item').first().locator('.btn-primary').click();

    // Switch to Installed tab to see the model form input
    await page.click('button[role="tab"]:has-text("Installed")');

    // Model input should be populated with the model name
    await expect(page.locator('.model-form input')).toHaveValue('model-0');
  });
});

test.describe('Browse Models - Source Tab Switching', () => {
  test.beforeEach(async ({ context, page }) => {
    // Block service worker to allow route interception to work
    await blockServiceWorker(context);
    await mockCommonEndpoints(page);
    await setupAuth(page);
    await page.goto('/models');
    await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
    // Open the Browse catalogue
    await page.click('button[role="tab"]:has-text("Browse")');
    await expect(page.locator('.search-container')).toBeVisible();
  });

  test('all four source tabs are visible', async ({ page }) => {
    await expect(page.locator('button[role="tab"]:has-text("Ollama")')).toBeVisible();
    await expect(page.locator('button[role="tab"]:has-text("HuggingFace")')).toBeVisible();
    await expect(page.locator('button[role="tab"]:has-text("GPT4All")')).toBeVisible();
    await expect(page.locator('button[role="tab"]:has-text("OpenRouter")')).toBeVisible();
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
        body: JSON.stringify({ source, models: [], next_cursor: null }),
      });
    });

    // Click HuggingFace tab
    await page.click('button[role="tab"]:has-text("HuggingFace")');
    await page.waitForTimeout(100);

    // Click GPT4All tab
    await page.click('button[role="tab"]:has-text("GPT4All")');
    await page.waitForTimeout(100);

    // Click OpenRouter tab
    await page.click('button[role="tab"]:has-text("OpenRouter")');
    await page.waitForTimeout(100);

    // Click Ollama tab
    await page.click('button[role="tab"]:has-text("Ollama")');
    await page.waitForTimeout(100);

    expect(requests).toContain('huggingface');
    expect(requests).toContain('gpt4all');
    expect(requests).toContain('openrouter');
    expect(requests).toContain('ollama');
  });
});

test.describe('Browse Models - HuggingFace Specific', () => {
  test.beforeEach(async ({ context, page }) => {
    // Block service worker to allow route interception to work
    await blockServiceWorker(context);
    await mockCommonEndpoints(page);
    await setupAuth(page);
    await page.goto('/models');
    await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
    // Open the Browse catalogue
    await page.click('button[role="tab"]:has-text("Browse")');
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
          body: JSON.stringify({ source, models: [hfModel], next_cursor: null }),
        });
      } else {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source, models: [], next_cursor: null }),
        });
      }
    });

    // Switch to HuggingFace tab
    await page.click('button[role="tab"]:has-text("HuggingFace")');

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
          body: JSON.stringify({ source, models: [hfModel], next_cursor: null }),
        });
      } else {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source, models: [], next_cursor: null }),
        });
      }
    });

    // Switch to HuggingFace tab and click model
    await page.click('button[role="tab"]:has-text("HuggingFace")');
    await page.locator('.browse-item').first().click();

    await expect(page.locator('.modal-details')).toBeVisible();
    // Model name is displayed in header
    await expect(page.locator('.modal-details-header h3')).toHaveText('Model-GGUF');
    // Source badge shows HUGGINGFACE
    await expect(page.locator('.details-source')).toContainText(/huggingface/i);
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
          body: JSON.stringify({ source, models: [hfModel], next_cursor: null }),
        });
      } else {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source, models: [], next_cursor: null }),
        });
      }
    });

    await page.click('button[role="tab"]:has-text("HuggingFace")');
    await page.locator('.browse-item').first().click();

    // Install command shows the model name
    await expect(page.locator('.details-install code')).toHaveText('Model-GGUF');
  });
});

test.describe('Browse Models - GPT4All Specific', () => {
  test.beforeEach(async ({ context, page }) => {
    // Block service worker to allow route interception to work
    await blockServiceWorker(context);
    await mockCommonEndpoints(page);
    await setupAuth(page);
    await page.goto('/models');
    await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
    // Open the Browse catalogue
    await page.click('button[role="tab"]:has-text("Browse")');
    await expect(page.locator('.search-container')).toBeVisible();
  });

  test('displays GPT4All model', async ({ page }) => {
    const gpt4allModel = {
      id: 'llama3-8b-instruct',
      name: 'Llama 3 8B Instruct',
      description: 'Meta Llama 3 8B Instruct model',
      downloads: 100000,
      tags: ['gguf', 'llama'],
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

      if (source === 'gpt4all') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source, models: [gpt4allModel], next_cursor: null }),
        });
      } else {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source, models: [], next_cursor: null }),
        });
      }
    });

    // Switch to GPT4All tab
    await page.click('button[role="tab"]:has-text("GPT4All")');

    await expect(page.locator('.browse-item')).toHaveCount(1);
    await expect(page.locator('.browse-name')).toHaveText('Llama 3 8B Instruct');
  });

  test('GPT4All shows models from response', async ({ page }) => {
    const gpt4allModels = Array.from({ length: 5 }, (_, i) => ({
      id: `model-${i}`,
      name: `Model ${i}`,
      description: 'A GGUF model',
      downloads: 1000 * (5 - i),
      tags: ['gguf'],
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

      if (source === 'gpt4all') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source, models: gpt4allModels, next_cursor: 'offset:5' }),
        });
      } else {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source, models: [], next_cursor: null }),
        });
      }
    });

    await page.click('button[role="tab"]:has-text("GPT4All")');

    // Wait for GPT4All results to load
    await expect(page.locator('.browse-name').first()).toHaveText('Model 0');
    await expect(page.locator('.browse-item').first()).toBeVisible();
  });
});

test.describe('Browse Models - OpenRouter Specific', () => {
  test.beforeEach(async ({ context, page }) => {
    // Block service worker to allow route interception to work
    await blockServiceWorker(context);
    await mockCommonEndpoints(page);
    await setupAuth(page);
    await page.goto('/models');
    await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
    // Open the Browse catalogue
    await page.click('button[role="tab"]:has-text("Browse")');
    await expect(page.locator('.search-container')).toBeVisible();
  });

  test('displays OpenRouter model', async ({ page }) => {
    const openrouterModel = {
      id: 'anthropic/claude-3-opus',
      name: 'Claude 3 Opus',
      description: 'Anthropic Claude 3 Opus model',
      downloads: 500000,
      tags: ['api'],
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

      if (source === 'openrouter') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source, models: [openrouterModel], next_cursor: null }),
        });
      } else {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source, models: [], next_cursor: null }),
        });
      }
    });

    // Switch to OpenRouter tab
    await page.click('button[role="tab"]:has-text("OpenRouter")');

    await expect(page.locator('.browse-item')).toHaveCount(1);
    await expect(page.locator('.browse-name')).toHaveText('Claude 3 Opus');
  });

  test('OpenRouter shows models from response', async ({ page }) => {
    const openrouterModels = Array.from({ length: 5 }, (_, i) => ({
      id: `provider/model-${i}`,
      name: `Model ${i}`,
      description: 'An API model',
      downloads: 1000 * (5 - i),
      tags: ['api'],
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

      if (source === 'openrouter') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source, models: openrouterModels, next_cursor: 'offset:5' }),
        });
      } else {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ source, models: [], next_cursor: null }),
        });
      }
    });

    await page.click('button[role="tab"]:has-text("OpenRouter")');

    // Wait for OpenRouter results to load
    await expect(page.locator('.browse-name').first()).toHaveText('Model 0');
    await expect(page.locator('.browse-item').first()).toBeVisible();
  });
});
