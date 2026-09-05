import { test, expect } from './fixtures';
import { setupAuth, mockCommonEndpoints } from './helpers/auth';
import { blockServiceWorker, routeApi } from './test-utils';

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

const sourcesRoutePattern = /\/api\/workspaces\/[^/]+\/sources/;
const sourcesListPattern = /\/api\/workspaces\/[^/]+\/sources\/?$/;
const sourceDetailPattern = /\/api\/workspaces\/[^/]+\/sources\/src-1$/;
const sourceVerifyPattern = /\/api\/workspaces\/[^/]+\/sources\/src-1\/verify$/;

const isSourcesListRequest = (requestUrl: string) =>
  sourcesListPattern.test(new URL(requestUrl).pathname);

test.describe('Sources Page', () => {
  test.beforeEach(async ({ context, page }) => {
    // Block service worker
    await blockServiceWorker(context);

    // Set up API mocks
    await mockCommonEndpoints(page);

    // Mock organizations with query params
    await routeApi(page, '**/api/organizations?*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
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
        }),
      });
    });

    // Mock workspaces with query params
    await routeApi(page, '**/api/organizations/*/workspaces?*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
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
        }),
      });
    });

    // Default sources mock
    await routeApi(page, sourcesRoutePattern, (route) => {
      if (
        route.request().method() === 'GET' &&
        isSourcesListRequest(route.request().url())
      ) {
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
    await expect(page.locator('.sources-page')).toBeVisible({ timeout: 10000 });
  });

  test.describe('Page Header', () => {
    test('displays page title and subtitle', async ({ page }) => {
      await expect(
        page.getByRole('heading', { name: 'Sources', exact: true })
      ).toBeVisible();
      await expect(
        page.getByText('Connect repositories, calendars, email, and other data sources')
      ).toBeVisible();
    });

    test('shows add source button', async ({ page }) => {
      await expect(page.getByRole('button', { name: '+ Add Source' })).toBeVisible();
    });
  });

  test.describe('Empty State', () => {
    test('shows empty state when no sources exist', async ({ page }) => {
      await expect(
        page.getByRole('heading', { name: 'No sources configured' })
      ).toBeVisible();
      await expect(
        page.getByText(
          'Add code repositories, calendars, email inboxes, web URLs, or text content'
        )
      ).toBeVisible();
      await expect(
        page.getByRole('button', { name: 'Add Source', exact: true })
      ).toBeVisible();
    });
  });

  test.describe('Source List', () => {
    test('displays list of source cards', async ({ page }) => {
      await page.unroute(sourcesRoutePattern);
      await routeApi(page, sourcesRoutePattern, (route) => {
        if (
          route.request().method() === 'GET' &&
          isSourcesListRequest(route.request().url())
        ) {
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

    test('displays source name and type badge', async ({ page }) => {
      await page.unroute(sourcesRoutePattern);
      await routeApi(page, sourcesRoutePattern, (route) => {
        if (
          route.request().method() === 'GET' &&
          isSourcesListRequest(route.request().url())
        ) {
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

      const firstCard = page.locator('.source-card').first();
      await expect(firstCard.locator('h3')).toContainText('acme/frontend');
      await expect(firstCard.locator('.source-provider')).toContainText('GitHub');
    });

    test('shows verified status for verified sources', async ({ page }) => {
      await page.unroute(sourcesRoutePattern);
      await routeApi(page, sourcesRoutePattern, (route) => {
        if (
          route.request().method() === 'GET' &&
          isSourcesListRequest(route.request().url())
        ) {
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

      await expect(
        page.locator('.source-card').first().locator('.source-status')
      ).toContainText('Verified');
    });

    test('shows inactive status for disabled sources', async ({ page }) => {
      await page.unroute(sourcesRoutePattern);
      await routeApi(page, sourcesRoutePattern, (route) => {
        if (
          route.request().method() === 'GET' &&
          isSourcesListRequest(route.request().url())
        ) {
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

      await expect(
        page.locator('.source-card').nth(1).locator('.source-status')
      ).toContainText('Inactive');
    });

    test('shows error status for sources with errors', async ({ page }) => {
      await page.unroute(sourcesRoutePattern);
      await routeApi(page, sourcesRoutePattern, (route) => {
        if (
          route.request().method() === 'GET' &&
          isSourcesListRequest(route.request().url())
        ) {
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

      await expect(
        page.locator('.source-card').nth(2).locator('.source-status')
      ).toContainText('Error');
      await expect(page.locator('.source-error')).toContainText('Connection timed out');
    });

    test('displays source URL as link', async ({ page }) => {
      await page.unroute(sourcesRoutePattern);
      await routeApi(page, sourcesRoutePattern, (route) => {
        if (
          route.request().method() === 'GET' &&
          isSourcesListRequest(route.request().url())
        ) {
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

      const sourceUrl = page.locator('.source-url a').first();
      await expect(sourceUrl).toHaveAttribute('href', 'https://github.com/acme/frontend');
      await expect(sourceUrl).toHaveAttribute('target', '_blank');
    });
  });

  test.describe('Create Source Modal', () => {
    test('opens create modal from header button', async ({ page }) => {
      await page.getByRole('button', { name: '+ Add Source' }).click();
      await expect(page.getByRole('dialog', { name: 'Add Source' })).toBeVisible();
    });

    test('shows all source type options', async ({ page }) => {
      await page.getByRole('button', { name: '+ Add Source' }).click();

      await expect(page.locator('.source-type-option')).toHaveCount(7);
      await expect(page.locator('.source-type-name').filter({ hasText: 'GitHub' })).toBeVisible();
      await expect(page.locator('.source-type-name').filter({ hasText: 'GitLab' })).toBeVisible();
      await expect(page.locator('.source-type-name').filter({ hasText: 'Filesystem' })).toBeVisible();
    });

    test('shows GitHub form fields by default', async ({ page }) => {
      await page.getByRole('button', { name: '+ Add Source' }).click();
      await page.getByRole('button', { name: 'Next' }).click();

      await expect(page.locator('#ghOwner')).toBeVisible();
      await expect(page.locator('#ghRepo')).toBeVisible();
      await expect(page.locator('#ghBranch')).toBeVisible();
    });

    test('switches to GitLab form when selected', async ({ page }) => {
      await page.getByRole('button', { name: '+ Add Source' }).click();

      await page.getByRole('button', { name: /GitLab/i }).click();
      await page.getByRole('button', { name: 'Next' }).click();

      await expect(page.locator('#glHost')).toBeVisible();
      await expect(page.locator('#glProjectId')).toBeVisible();
      await expect(page.locator('#glBranch')).toBeVisible();
    });

    test('switches to Filesystem form when selected', async ({ page }) => {
      await page.getByRole('button', { name: '+ Add Source' }).click();

      await page.getByRole('button', { name: /Filesystem/i }).click();
      await page.getByRole('button', { name: 'Next' }).click();

      await expect(page.locator('#fsBasePath')).toBeVisible();
      await expect(page.locator('.toggle-title')).toContainText('Allow write operations');
    });

    test('switches to Web URL form when selected', async ({ page }) => {
      await page.getByRole('button', { name: '+ Add Source' }).click();

      await page.getByRole('button', { name: /Web URL/i }).click();
      await page.getByRole('button', { name: 'Next' }).click();

      await expect(page.locator('#webUrl')).toBeVisible();
    });

    test('switches to Text form when selected', async ({ page }) => {
      await page.getByRole('button', { name: '+ Add Source' }).click();

      await page.getByRole('button', { name: /Text/i }).click();
      await page.getByRole('button', { name: 'Next' }).click();

      await expect(page.locator('#textLabel')).toBeVisible();
      await expect(page.locator('#textContent')).toBeVisible();
    });

    test('creates GitHub source successfully', async ({ page }) => {
      const newSource = generateMockSource('new-src', 'test/repo', 'github', {
        url: 'https://github.com/test/repo',
      });

      await routeApi(page, sourcesRoutePattern, (route) => {
        if (
          route.request().method() === 'POST' &&
          isSourcesListRequest(route.request().url())
        ) {
          route.fulfill({
            status: 201,
            contentType: 'application/json',
            body: JSON.stringify({ source: newSource }),
          });
        } else {
          route.continue();
        }
      });

      await page.getByRole('button', { name: '+ Add Source' }).click();
      await page.getByRole('button', { name: 'Next' }).click();
      await page.fill('#ghOwner', 'test');
      await page.fill('#ghRepo', 'repo');
      await page.getByRole('button', { name: 'Next' }).click();
      await page.fill('#name', 'test/repo');
      await page
        .getByRole('dialog', { name: 'Add Source' })
        .getByRole('button', { name: 'Add Source' })
        .click();

      await expect(page.getByRole('dialog', { name: 'Add Source' })).toHaveCount(0);
    });

    test('shows loading state during creation', async ({ page }) => {
      await routeApi(page, sourcesRoutePattern, async (route) => {
        if (
          route.request().method() === 'POST' &&
          isSourcesListRequest(route.request().url())
        ) {
          await new Promise((resolve) => setTimeout(resolve, 500));
          route.fulfill({
            status: 201,
            contentType: 'application/json',
            body: JSON.stringify({
              source: generateMockSource('new-src', 'test/repo', 'github'),
            }),
          });
        } else {
          route.continue();
        }
      });

      await page.getByRole('button', { name: '+ Add Source' }).click();
      await page.getByRole('button', { name: 'Next' }).click();
      await page.fill('#ghOwner', 'test');
      await page.fill('#ghRepo', 'repo');
      await page.getByRole('button', { name: 'Next' }).click();
      await page.fill('#name', 'test/repo');
      await page
        .getByRole('dialog', { name: 'Add Source' })
        .getByRole('button', { name: 'Add Source' })
        .click();

      await expect(page.getByRole('button', { name: 'Adding...' })).toBeVisible();
    });

    test('shows error when creation fails', async ({ page }) => {
      await routeApi(page, sourcesRoutePattern, (route) => {
        if (
          route.request().method() === 'POST' &&
          isSourcesListRequest(route.request().url())
        ) {
          route.fulfill({
            status: 400,
            contentType: 'application/json',
            body: JSON.stringify({ error: 'Invalid repository URL' }),
          });
        } else {
          route.continue();
        }
      });

      await page.getByRole('button', { name: '+ Add Source' }).click();
      await page.getByRole('button', { name: 'Next' }).click();
      await page.fill('#ghOwner', 'test');
      await page.fill('#ghRepo', 'repo');
      await page.getByRole('button', { name: 'Next' }).click();
      await page.fill('#name', 'test/repo');
      await page
        .getByRole('dialog', { name: 'Add Source' })
        .getByRole('button', { name: 'Add Source' })
        .click();

      await expect(page.locator('.form-error')).toBeVisible();
    });

    test('closes modal on cancel', async ({ page }) => {
      await page.getByRole('button', { name: '+ Add Source' }).click();
      await page
        .getByRole('dialog', { name: 'Add Source' })
        .getByRole('button', { name: 'Cancel' })
        .click();

      await expect(page.getByRole('dialog', { name: 'Add Source' })).toHaveCount(0);
    });

    test('closes modal on backdrop click', async ({ page }) => {
      await page.getByRole('button', { name: '+ Add Source' }).click();
      await page.getByRole('button', { name: 'Close wizard' }).click();

      await expect(page.getByRole('dialog', { name: 'Add Source' })).toHaveCount(0);
    });

    test('shows details fields on final step', async ({ page }) => {
      await page.getByRole('button', { name: '+ Add Source' }).click();
      await page.getByRole('button', { name: 'Next' }).click();
      await page.fill('#ghOwner', 'test');
      await page.fill('#ghRepo', 'repo');
      await page.getByRole('button', { name: 'Next' }).click();

      await expect(page.locator('#name')).toBeVisible();
      await expect(page.locator('#description')).toBeVisible();
    });
  });

  test.describe('Source Actions', () => {
    test.beforeEach(async ({ page }) => {
      await page.unroute(sourcesRoutePattern);
      await routeApi(page, sourcesRoutePattern, (route) => {
        const method = route.request().method();
        if (method === 'GET' && isSourcesListRequest(route.request().url())) {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({
              sources: [{ ...mockSources[0], last_verified_at: null }, ...mockSources.slice(1)],
            }),
          });
        } else {
          route.continue();
        }
      });

      await page.reload();
      await page.click('a[href="/sources"]');
      await expect(page.locator('.source-card')).toHaveCount(3);
    });

    for (const verified of [true, false]) {
      test(`verification refreshes the source when verified=${verified}`, async ({ page }) => {
        const message = verified
          ? 'Source verified successfully'
          : 'Authentication failed - check your credentials';
        let refreshed = false;
        await routeApi(page, sourceVerifyPattern, async (route) => {
          if (route.request().method() === 'POST') {
            await new Promise((resolve) => setTimeout(resolve, 300));
            await route.fulfill({
              status: 200,
              contentType: 'application/json',
              body: JSON.stringify({ verified, message }),
            });
          } else {
            await route.continue();
          }
        });

        await routeApi(page, sourceDetailPattern, async (route) => {
          if (route.request().method() === 'GET') {
            refreshed = true;
            await route.fulfill({
              status: 200,
              contentType: 'application/json',
              body: JSON.stringify({
                source: {
                  ...mockSources[0],
                  last_verified_at: verified ? new Date().toISOString() : null,
                  last_error: verified ? null : message,
                },
              }),
            });
          } else {
            await route.continue();
          }
        });

        const card = page.locator('.source-card').first();
        await expect(card.locator('.source-status')).toContainText('Unverified');
        const button = card.getByRole('button', { name: 'Verify', exact: true });
        await button.click();
        await expect(card.getByRole('button', { name: 'Verifying...' })).toBeVisible();
        await expect(button).toBeEnabled();
        await expect.poll(() => refreshed).toBe(true);
        await expect(card.locator('.source-status')).toContainText(verified ? 'Verified' : 'Error');
        await expect(page.getByText('Validation failed:', { exact: false })).toHaveCount(0);
        if (!verified) {
          await expect(card.locator('.source-error')).toContainText(message);
        }
      });
    }

    test('enable/disable button toggles source status', async ({ page }) => {
      await routeApi(page, sourceDetailPattern, (route) => {
        if (route.request().method() === 'PUT') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({
              source: { ...mockSources[0], is_active: false },
            }),
          });
        } else {
          route.continue();
        }
      });

      const disableBtn = page
        .locator('.source-card')
        .first()
        .locator('button:has-text("Disable")');
      await disableBtn.click();

      await expect(
        page.locator('.source-card').first().locator('button:has-text("Enable")')
      ).toBeVisible();
    });

    test('delete button removes source after confirmation', async ({ page }) => {
      await page.evaluate(() => {
        window.confirm = () => true;
      });

      await routeApi(page, sourceDetailPattern, (route) => {
        if (route.request().method() === 'DELETE') {
          route.fulfill({ status: 204 });
        } else {
          route.continue();
        }
      });

      const deleteBtn = page.locator('.source-card').first().locator('button:has-text("Delete")');
      await deleteBtn.click();

      await expect(page.locator('.source-card')).toHaveCount(2);
    });
  });

  test.describe('Error Handling', () => {
    test('shows error when loading sources fails', async ({ page }) => {
      await page.unroute(sourcesRoutePattern);
      await routeApi(page, sourcesRoutePattern, (route) => {
        if (isSourcesListRequest(route.request().url())) {
          route.fulfill({
            status: 500,
            contentType: 'application/json',
            body: JSON.stringify({ error: 'Server error' }),
          });
        } else {
          route.continue();
        }
      });

      await page.reload();
      await page.click('a[href="/sources"]');

      await expect(page.getByText('Server error')).toBeVisible();
    });
  });

  test.describe('Loading State', () => {
    test('shows skeleton cards while loading', async ({ page }) => {
      await page.unroute(sourcesRoutePattern);
      await routeApi(page, sourcesRoutePattern, async (route) => {
        if (isSourcesListRequest(route.request().url())) {
          await new Promise((resolve) => setTimeout(resolve, 500));
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

      await expect(page.locator('.skeleton-card').first()).toBeVisible();
    });
  });
});
