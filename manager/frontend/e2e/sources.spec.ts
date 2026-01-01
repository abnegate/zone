import { test, expect } from '@playwright/test';
import { setupAuth, mockCommonEndpoints } from './helpers/auth';

// Mock data generators
const generateMockSource = (
  id: string,
  name: string,
  type: string,
  options: {
    url?: string;
    description?: string;
    is_active?: boolean;
    last_verified_at?: string | null;
    last_error?: string | null;
  } = {}
) => ({
  id,
  name,
  source_type: type,
  category: type === 'github' || type === 'gitlab' || type === 'filesystem' ? 'file' : 'web',
  config: { owner: 'test', repo: 'test' },
  url: options.url || `https://example.com/${type}/${name}`,
  description: options.description || null,
  is_active: options.is_active ?? true,
  last_verified_at: options.last_verified_at ?? null,
  last_error: options.last_error ?? null,
  created_at: new Date().toISOString(),
  updated_at: new Date().toISOString(),
});

const mockSources = [
  generateMockSource('src-1', 'acme/frontend', 'github', {
    url: 'https://github.com/acme/frontend',
    description: 'Main frontend repo',
    last_verified_at: new Date().toISOString(),
  }),
  generateMockSource('src-2', 'acme/backend', 'gitlab', {
    url: 'https://gitlab.com/acme/backend',
    is_active: false,
  }),
  generateMockSource('src-3', 'Work Calendar', 'ical', {
    url: 'https://calendar.google.com/feed.ics',
    last_error: 'Connection timed out',
  }),
];

test.describe('Sources Page', () => {
  test.beforeEach(async ({ page }) => {
    // Set up API mocks
    await mockCommonEndpoints(page);

    // Default sources mock
    await page.route('**/api/sources*', (route) => {
      if (route.request().method() === 'GET') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ sources: [] }),
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

    // Navigate to sources page
    await page.click('a[href="/sources"]');
    await expect(page).toHaveURL('/sources');
  });

  test.describe('Page Header', () => {
    test('displays page title and subtitle', async ({ page }) => {
      await expect(page.locator('.page-header h1')).toContainText('Sources');
      await expect(page.locator('.page-header .subtitle')).toContainText('repositories');
    });

    test('shows add source button', async ({ page }) => {
      await expect(page.locator('.page-header .btn-primary')).toContainText('Add Source');
    });
  });

  test.describe('Empty State', () => {
    test('shows empty state when no sources exist', async ({ page }) => {
      await expect(page.locator('.empty-state')).toBeVisible();
      await expect(page.locator('.empty-state')).toContainText('No sources configured');
    });
  });

  test.describe('Source List', () => {
    test('displays list of source cards', async ({ page }) => {
      await page.unroute('**/api/sources*');
      await page.route('**/api/sources*', (route) => {
        if (route.request().method() === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ sources: mockSources }),
          });
        }
      });

      await page.reload();
      await page.click('a[href="/sources"]');

      await expect(page.locator('.source-card')).toHaveCount(3);
    });

    test('displays source name and type badge', async ({ page }) => {
      await page.unroute('**/api/sources*');
      await page.route('**/api/sources*', (route) => {
        if (route.request().method() === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ sources: mockSources }),
          });
        }
      });

      await page.reload();
      await page.click('a[href="/sources"]');

      await expect(page.locator('.source-card h3').first()).toContainText('acme/frontend');
      await expect(page.locator('.source-type-badge').first()).toContainText('GitHub');
    });

    test('shows verified status for verified sources', async ({ page }) => {
      await page.unroute('**/api/sources*');
      await page.route('**/api/sources*', (route) => {
        if (route.request().method() === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ sources: mockSources }),
          });
        }
      });

      await page.reload();
      await page.click('a[href="/sources"]');

      await expect(page.locator('.source-status-badge.badge-green').first()).toContainText('Verified');
    });

    test('shows inactive status for disabled sources', async ({ page }) => {
      await page.unroute('**/api/sources*');
      await page.route('**/api/sources*', (route) => {
        if (route.request().method() === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ sources: mockSources }),
          });
        }
      });

      await page.reload();
      await page.click('a[href="/sources"]');

      await expect(page.locator('.source-status-badge.badge-gray')).toContainText('Inactive');
    });

    test('shows error status for sources with errors', async ({ page }) => {
      await page.unroute('**/api/sources*');
      await page.route('**/api/sources*', (route) => {
        if (route.request().method() === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ sources: mockSources }),
          });
        }
      });

      await page.reload();
      await page.click('a[href="/sources"]');

      await expect(page.locator('.source-status-badge.badge-red')).toContainText('Error');
      await expect(page.locator('.source-error')).toContainText('Connection timed out');
    });

    test('displays source URL as link', async ({ page }) => {
      await page.unroute('**/api/sources*');
      await page.route('**/api/sources*', (route) => {
        if (route.request().method() === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ sources: mockSources }),
          });
        }
      });

      await page.reload();
      await page.click('a[href="/sources"]');

      const sourceUrl = page.locator('.source-url a').first();
      await expect(sourceUrl).toHaveAttribute('href', 'https://github.com/acme/frontend');
      await expect(sourceUrl).toHaveAttribute('target', '_blank');
    });
  });

  test.describe('Create Source Modal', () => {
    test('opens create modal from header button', async ({ page }) => {
      await page.click('.page-header .btn-primary');
      await expect(page.locator('.modal-content h2')).toContainText('Add Source');
    });

    test('shows all source type options', async ({ page }) => {
      await page.click('.page-header .btn-primary');

      await expect(page.locator('.source-type-card')).toHaveCount(7);
      await expect(page.locator('.source-type-name').filter({ hasText: 'GitHub' })).toBeVisible();
      await expect(page.locator('.source-type-name').filter({ hasText: 'GitLab' })).toBeVisible();
      await expect(page.locator('.source-type-name').filter({ hasText: 'Filesystem' })).toBeVisible();
    });

    test('shows GitHub form fields by default', async ({ page }) => {
      await page.click('.page-header .btn-primary');

      await expect(page.locator('#ghOwner')).toBeVisible();
      await expect(page.locator('#ghRepo')).toBeVisible();
      await expect(page.locator('#ghBranch')).toBeVisible();
    });

    test('switches to GitLab form when selected', async ({ page }) => {
      await page.click('.page-header .btn-primary');

      await page.click('.source-type-card:has-text("GitLab")');

      await expect(page.locator('#glHost')).toBeVisible();
      await expect(page.locator('#glProjectId')).toBeVisible();
      await expect(page.locator('#glBranch')).toBeVisible();
    });

    test('switches to Filesystem form when selected', async ({ page }) => {
      await page.click('.page-header .btn-primary');

      await page.click('.source-type-card:has-text("Filesystem")');

      await expect(page.locator('#fsBasePath')).toBeVisible();
      await expect(page.locator('.toggle-title')).toContainText('Allow write operations');
    });

    test('switches to Web URL form when selected', async ({ page }) => {
      await page.click('.page-header .btn-primary');

      await page.click('.source-type-card:has-text("Web URL")');

      await expect(page.locator('#webUrl')).toBeVisible();
    });

    test('switches to Text form when selected', async ({ page }) => {
      await page.click('.page-header .btn-primary');

      await page.click('.source-type-card:has-text("Text")');

      await expect(page.locator('#textLabel')).toBeVisible();
      await expect(page.locator('#textContent')).toBeVisible();
    });

    test('creates GitHub source successfully', async ({ page }) => {
      const newSource = generateMockSource('new-src', 'test/repo', 'github', {
        url: 'https://github.com/test/repo',
      });

      await page.route('**/api/sources', (route) => {
        if (route.request().method() === 'POST') {
          route.fulfill({
            status: 201,
            contentType: 'application/json',
            body: JSON.stringify({ source: newSource }),
          });
        }
      });

      await page.click('.page-header .btn-primary');
      await page.fill('#ghOwner', 'test');
      await page.fill('#ghRepo', 'repo');
      await page.click('.form-actions .btn-primary');

      await expect(page.locator('.modal-content')).not.toBeVisible({ timeout: 5000 });
    });

    test('shows loading state during creation', async ({ page }) => {
      await page.route('**/api/sources', async (route) => {
        if (route.request().method() === 'POST') {
          await new Promise((resolve) => setTimeout(resolve, 500));
          route.fulfill({
            status: 201,
            contentType: 'application/json',
            body: JSON.stringify({
              source: generateMockSource('new-src', 'test/repo', 'github'),
            }),
          });
        }
      });

      await page.click('.page-header .btn-primary');
      await page.fill('#ghOwner', 'test');
      await page.fill('#ghRepo', 'repo');
      await page.click('.form-actions .btn-primary');

      await expect(page.locator('.form-actions .btn-primary')).toContainText('Adding...');
    });

    test('shows error when creation fails', async ({ page }) => {
      await page.route('**/api/sources', (route) => {
        if (route.request().method() === 'POST') {
          route.fulfill({
            status: 400,
            contentType: 'application/json',
            body: JSON.stringify({ error: 'Invalid repository URL' }),
          });
        }
      });

      await page.click('.page-header .btn-primary');
      await page.fill('#ghOwner', 'test');
      await page.fill('#ghRepo', 'repo');
      await page.click('.form-actions .btn-primary');

      await expect(page.locator('.form-error')).toBeVisible();
    });

    test('closes modal on cancel', async ({ page }) => {
      await page.click('.page-header .btn-primary');
      await page.click('.form-actions .btn-secondary');

      await expect(page.locator('.modal-content')).not.toBeVisible();
    });

    test('closes modal on backdrop click', async ({ page }) => {
      await page.click('.page-header .btn-primary');
      await page.click('.modal-overlay', { position: { x: 10, y: 10 } });

      await expect(page.locator('.modal-content')).not.toBeVisible();
    });

    test('expands additional options section', async ({ page }) => {
      await page.click('.page-header .btn-primary');

      await page.click('.form-section-collapsed summary');

      await expect(page.locator('#name')).toBeVisible();
      await expect(page.locator('#description')).toBeVisible();
    });
  });

  test.describe('Source Actions', () => {
    test.beforeEach(async ({ page }) => {
      await page.unroute('**/api/sources*');
      await page.route('**/api/sources*', (route) => {
        const url = route.request().url();
        const method = route.request().method();

        if (method === 'GET' && !url.includes('/src-')) {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ sources: mockSources }),
          });
        } else {
          route.continue();
        }
      });

      await page.reload();
      await page.click('a[href="/sources"]');
      await expect(page.locator('.source-card')).toHaveCount(3);
    });

    test('verify button triggers verification', async ({ page }) => {
      await page.route('**/api/sources/src-1/verify', (route) => {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ success: true, message: 'Source verified' }),
        });
      });

      await page.route('**/api/sources/src-1', (route) => {
        if (route.request().method() === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({
              source: { ...mockSources[0], last_verified_at: new Date().toISOString() },
            }),
          });
        }
      });

      const verifyBtn = page.locator('.source-card').first().locator('button:has-text("Verify")');
      await verifyBtn.click();

      await expect(verifyBtn).toContainText('Verifying...');
    });

    test('enable/disable button toggles source status', async ({ page }) => {
      await page.route('**/api/sources/src-1', (route) => {
        if (route.request().method() === 'PATCH') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({
              source: { ...mockSources[0], is_active: false },
            }),
          });
        }
      });

      const disableBtn = page.locator('.source-card').first().locator('button:has-text("Disable")');
      await disableBtn.click();

      await expect(page.locator('.source-card').first().locator('button:has-text("Enable")')).toBeVisible();
    });

    test('delete button removes source after confirmation', async ({ page }) => {
      await page.evaluate(() => {
        window.confirm = () => true;
      });

      await page.route('**/api/sources/src-1', (route) => {
        if (route.request().method() === 'DELETE') {
          route.fulfill({ status: 204 });
        }
      });

      const deleteBtn = page.locator('.source-card').first().locator('button:has-text("Delete")');
      await deleteBtn.click();

      await expect(page.locator('.source-card')).toHaveCount(2);
    });
  });

  test.describe('Error Handling', () => {
    test('shows error when loading sources fails', async ({ page }) => {
      await page.unroute('**/api/sources*');
      await page.route('**/api/sources*', (route) => {
        route.fulfill({
          status: 500,
          contentType: 'application/json',
          body: JSON.stringify({ error: 'Server error' }),
        });
      });

      await page.reload();
      await page.click('a[href="/sources"]');

      await expect(page.locator('.error-banner')).toBeVisible();
    });
  });

  test.describe('Loading State', () => {
    test('shows skeleton cards while loading', async ({ page }) => {
      await page.unroute('**/api/sources*');
      await page.route('**/api/sources*', async (route) => {
        await new Promise((resolve) => setTimeout(resolve, 500));
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ sources: mockSources }),
        });
      });

      await page.reload();
      await page.click('a[href="/sources"]');

      await expect(page.locator('.skeleton-card').first()).toBeVisible();
    });
  });
});
