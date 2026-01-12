import { test, expect } from './fixtures';
import { setupAuth, mockCommonEndpoints } from './helpers/auth';
import { blockServiceWorker, routeApi } from './test-utils';

test.describe('Wiki Page', () => {
  test.beforeEach(async ({ context, page }) => {
    // Block service worker first
    await blockServiceWorker(context);
    // Set up API mocks
    await mockCommonEndpoints(page);

    // Mock organizations (with and without query params)
    const orgMock = {
      organizations: [
        {
          id: '00000000-0000-0000-0000-000000000001',
          name: 'Default Org',
          slug: 'default',
          description: null,
          is_active: true,
          created_at: new Date().toISOString(),
          updated_at: new Date().toISOString(),
        },
      ],
    };
    await routeApi(page, '**/api/organizations?*', (route) =>
      route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(orgMock) })
    );
    await routeApi(page, '**/api/organizations', (route) =>
      route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(orgMock) })
    );

    // Mock workspaces (with and without query params)
    const workspaceMock = {
      workspaces: [
        {
          id: '00000000-0000-0000-0000-000000000001',
          organization_id: '00000000-0000-0000-0000-000000000001',
          name: 'Default Workspace',
          slug: 'default',
          description: null,
          is_active: true,
          created_at: new Date().toISOString(),
          updated_at: new Date().toISOString(),
        },
      ],
    };
    await routeApi(page, '**/api/organizations/*/workspaces?*', (route) =>
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(workspaceMock),
      })
    );
    await routeApi(page, '**/api/organizations/*/workspaces', (route) =>
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(workspaceMock),
      })
    );

    await routeApi(page, '**/api/knowledge*', (route) => {
      if (route.request().method() === 'GET') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ entries: [] }),
        });
      } else {
        route.continue();
      }
    });

    // Navigate and set up auth
    await page.goto('/');
    await setupAuth(page);
    await page.reload();

    // Wait for app to load
    await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });

    // Navigate to wiki page
    await page.click('a[href="/wiki"]');
    await expect(page).toHaveURL('/wiki');
  });

  test.describe('Page Header', () => {
    test('displays page title', async ({ page }) => {
      await expect(page.locator('.wiki-header h1')).toContainText('Knowledge Base');
    });

    test('displays subtitle', async ({ page }) => {
      await expect(page.locator('.wiki-subtitle')).toContainText('Manage documentation');
    });
  });

  test.describe('Page Actions', () => {
    test('shows search input', async ({ page }) => {
      await expect(page.locator('.wiki-search input')).toBeVisible();
      await expect(page.locator('.wiki-search input')).toHaveAttribute('placeholder', 'Search knowledge...');
    });

    test('shows add knowledge button', async ({ page }) => {
      await expect(page.locator('.add-knowledge-btn').first()).toContainText('Add Knowledge');
    });
  });

  test.describe('Filters', () => {
    test('shows all filter buttons', async ({ page }) => {
      const filters = page.locator('.wiki-filters .filter-btn');
      await expect(filters).toHaveCount(3);
      await expect(filters.nth(0)).toContainText('All');
      await expect(filters.nth(1)).toContainText('Text');
      await expect(filters.nth(2)).toContainText('URL');
    });
  });

  test.describe('Empty State', () => {
    test('shows empty state when no entries exist', async ({ page }) => {
      await expect(page.locator('.wiki-empty h2')).toContainText('No knowledge entries found');
      await expect(page.locator('.wiki-empty')).toContainText('Get started by adding your first knowledge entry');
    });
  });
});
