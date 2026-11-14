import type { Page } from '@playwright/test';
import { test, expect } from './fixtures';
import { setupAuth, mockCommonEndpoints } from './helpers/auth';
import { blockServiceWorker, routeApi, SOURCES_PATH } from './test-utils';

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

const SHORT_VIEWPORT = { width: 1280, height: 600 };

const isSourcesListRequest = (requestUrl: string) =>
  sourcesListPattern.test(new URL(requestUrl).pathname);

const addSourceButton = (page: Page) =>
  page.locator('.sources-section .section-row-actions').getByRole('button', { name: 'Add source' });

const openAccessTokenFields = (page: Page) =>
  page.getByRole('button', { name: 'Use an access token instead' }).click();

test.describe('Sources Section', () => {
  test.beforeEach(async ({ context, page }) => {
    await blockServiceWorker(context);

    await mockCommonEndpoints(page);

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

    await routeApi(page, '**/api/organizations/*/github/repositories*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ repositories: [], installations: [], total: 0, next_page: null }),
      });
    });

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

    await page.goto('/');
    await setupAuth(page);
    await page.reload();

    await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });

    await page.click('a[href="/settings"]');
    await page
      .getByRole('navigation', { name: 'Settings' })
      .getByRole('link', { name: 'Sources', exact: true })
      .click();
    await expect(page).toHaveURL(SOURCES_PATH);
    await expect(page.locator('.sources-section')).toBeVisible({ timeout: 10000 });
  });

  test.describe('Section Header', () => {
    test('displays section title and description', async ({ page }) => {
      await expect(
        page
          .locator('.sources-section .section-row')
          .getByRole('heading', { level: 2, name: 'Sources', exact: true })
      ).toBeVisible();
      await expect(
        page.getByText('Connect repositories, calendars, email, and other data sources')
      ).toBeVisible();
    });

    test('shows add source button', async ({ page }) => {
      await expect(addSourceButton(page)).toBeVisible();
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
        page.locator('.sources-section .ui-empty').getByRole('button', { name: 'Add source' })
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

      const row = page.locator('.source-card', { hasText: 'acme/frontend' });
      await expect(row.locator('.source-name')).toHaveText('acme/frontend');
      await expect(row.locator('.source-provider')).toContainText('GitHub');
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

      await expect(
        page.locator('.source-card', { hasText: 'acme/frontend' }).locator('.source-status')
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

      await expect(
        page.locator('.source-card', { hasText: 'acme/backend' }).locator('.source-status')
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

      await expect(
        page.locator('.source-card', { hasText: 'Work Calendar' }).locator('.source-status')
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

      const sourceUrl = page.locator('a.source-url').first();
      await expect(sourceUrl).toHaveAttribute('href', 'https://github.com/acme/frontend');
      await expect(sourceUrl).toHaveAttribute('target', '_blank');
    });
  });

  test.describe('Create Source Modal', () => {
    test('opens create modal from header button', async ({ page }) => {
      await addSourceButton(page).click();
      await expect(page.getByRole('dialog', { name: 'Add Source' })).toBeVisible();
    });

    test('shows all source type options', async ({ page }) => {
      await addSourceButton(page).click();

      await expect(page.locator('.source-type-name')).toHaveText([
        'GitHub',
        'GitLab',
        'Filesystem',
        'Web URL',
        'Text',
      ]);
    });

    test('shows the access-token GitHub fields behind the disclosure', async ({ page }) => {
      await addSourceButton(page).click();
      await page.getByRole('button', { name: 'Next' }).click();
      await openAccessTokenFields(page);

      await expect(page.locator('#ghOwner')).toBeVisible();
      await expect(page.locator('#ghRepo')).toBeVisible();
      await expect(page.locator('#ghBranch')).toBeVisible();
    });

    test('switches to GitLab form when selected', async ({ page }) => {
      await addSourceButton(page).click();

      await page.getByRole('button', { name: /GitLab/i }).click();
      await page.getByRole('button', { name: 'Next' }).click();

      await expect(page.locator('#glHost')).toBeVisible();
      await expect(page.locator('#glProjectId')).toBeVisible();
      await expect(page.locator('#glBranch')).toBeVisible();
    });

    test('switches to Filesystem form when selected', async ({ page }) => {
      await addSourceButton(page).click();

      await page.getByRole('button', { name: /Filesystem/i }).click();
      await page.getByRole('button', { name: 'Next' }).click();

      await expect(page.locator('#fsBasePath')).toBeVisible();
      await expect(page.locator('.toggle-title')).toContainText('Allow write operations');
    });

    test('switches to Web URL form when selected', async ({ page }) => {
      await addSourceButton(page).click();

      await page.getByRole('button', { name: /Web URL/i }).click();
      await page.getByRole('button', { name: 'Next' }).click();

      await expect(page.locator('#webUrl')).toBeVisible();
    });

    test('switches to Text form when selected', async ({ page }) => {
      await addSourceButton(page).click();

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

      await addSourceButton(page).click();
      await page.getByRole('button', { name: 'Next' }).click();
      await openAccessTokenFields(page);
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

      await addSourceButton(page).click();
      await page.getByRole('button', { name: 'Next' }).click();
      await openAccessTokenFields(page);
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

      await addSourceButton(page).click();
      await page.getByRole('button', { name: 'Next' }).click();
      await openAccessTokenFields(page);
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
      await addSourceButton(page).click();
      await page
        .getByRole('dialog', { name: 'Add Source' })
        .getByRole('button', { name: 'Cancel' })
        .click();

      await expect(page.getByRole('dialog', { name: 'Add Source' })).toHaveCount(0);
    });

    test('closes modal on backdrop click', async ({ page }) => {
      await addSourceButton(page).click();
      await expect(page.getByRole('dialog', { name: 'Add Source' })).toBeVisible();
      // The dismiss control fills the overlay; click a corner so the centered
      // dialog does not intercept the pointer.
      await page.locator('.ui-wizard-dismiss').click({ position: { x: 8, y: 8 } });

      await expect(page.getByRole('dialog', { name: 'Add Source' })).toHaveCount(0);
    });

    test('shows details fields on final step', async ({ page }) => {
      await addSourceButton(page).click();
      await page.getByRole('button', { name: 'Next' }).click();
      await openAccessTokenFields(page);
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

  test.describe('Source Editor', () => {
    test.beforeEach(async ({ page }) => {
      await page.emulateMedia({ reducedMotion: 'reduce' });
    });

    test('keeps Save in view when a tall editor first opens, before any scroll', async ({
      page,
    }) => {
      await routeApi(page, sourceDetailPattern, (route) => {
        if (route.request().method() === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ source: mockSources[0] }),
          });
        } else {
          route.continue();
        }
      });
      await page.setViewportSize(SHORT_VIEWPORT);

      await page.goto(`${SOURCES_PATH}?source=src-1`);

      const editor = page.locator('.source-details');
      const save = editor.getByRole('button', { name: 'Save' });
      await expect(save).toBeVisible();
      expect(await page.locator('.page-body').evaluate((body) => body.scrollTop)).toBe(0);
      const editorBox = await editor.boundingBox();
      const footerBox = await editor.locator('.source-details-actions').boundingBox();
      const saveBox = await save.boundingBox();
      expect(editorBox).not.toBeNull();
      expect(footerBox).not.toBeNull();
      expect(saveBox).not.toBeNull();
      expect(editorBox!.y + editorBox!.height).toBeGreaterThan(SHORT_VIEWPORT.height);
      expect(saveBox!.y + saveBox!.height).toBeLessThanOrEqual(SHORT_VIEWPORT.height);
      expect(footerBox!.y + footerBox!.height).toBeCloseTo(SHORT_VIEWPORT.height, 0);
    });

    test('scrolls a focused field clear of the sticky footer', async ({ page }) => {
      await routeApi(page, sourceDetailPattern, (route) => {
        if (route.request().method() === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ source: mockSources[0] }),
          });
        } else {
          route.continue();
        }
      });
      await page.setViewportSize(SHORT_VIEWPORT);

      await page.goto(`${SOURCES_PATH}?source=src-1`);

      const editor = page.locator('.source-details');
      const branch = editor.getByLabel('Branch');
      const token = editor.getByLabel('Access Token');
      const footer = editor.locator('.source-details-actions');
      await expect(token).toBeAttached();
      await page.locator('.page-body').evaluate((body) => {
        const field = body.querySelector('#credentials')!.getBoundingClientRect();
        const bar = body.querySelector('.source-details-actions')!.getBoundingClientRect();
        body.scrollTop += field.top - bar.top - (bar.height - field.height) / 2;
      });
      const hidden = await token.boundingBox();
      const bar = await footer.boundingBox();
      expect(hidden!.y).toBeGreaterThanOrEqual(bar!.y);
      expect(hidden!.y + hidden!.height).toBeLessThanOrEqual(bar!.y + bar!.height);
      await branch.focus();

      await page.keyboard.press('Tab');

      await expect(token).toBeFocused();
      const tokenBox = await token.boundingBox();
      const footerBox = await footer.boundingBox();
      expect(tokenBox).not.toBeNull();
      expect(footerBox).not.toBeNull();
      expect(tokenBox!.y + tokenBox!.height).toBeLessThanOrEqual(footerBox!.y);
    });
  });

  test.describe('Workspace Roles', () => {
    const routeRole = (page: Page, role: 'member' | 'viewer') =>
      routeApi(page, /\/api\/workspaces\/[^/]+\/members$/, (route) =>
        route.fulfill({
          json: {
            members: [
              {
                id: 'member-1',
                user_id: 'user-1',
                workspace_id: '00000000-0000-0000-0000-000000000001',
                role,
              },
            ],
          },
        })
      );

    test.beforeEach(async ({ page }) => {
      await page.unroute(sourcesRoutePattern);
      await routeApi(page, sourcesRoutePattern, (route) => {
        if (route.request().method() === 'GET' && isSourcesListRequest(route.request().url())) {
          route.fulfill({ json: { sources: mockSources } });
        } else {
          route.continue();
        }
      });
      await routeApi(page, sourceDetailPattern, (route) => {
        if (route.request().method() === 'GET') {
          route.fulfill({ json: { source: mockSources[0] } });
        } else {
          route.continue();
        }
      });
    });

    test('shows a viewer the sources without write controls and a read-only editor', async ({
      page,
    }) => {
      await routeRole(page, 'viewer');
      await page.reload();

      const table = page.getByRole('table', { name: 'Sources' });
      await expect(table.locator('.source-card')).toHaveCount(3);
      await expect(page.getByText("Viewers can't change sources.")).toBeVisible();
      await expect(table.getByRole('columnheader', { name: 'Actions' })).toHaveCount(0);
      for (const name of ['Add source', 'Verify', 'Disable', 'Enable', 'Delete']) {
        await expect(page.getByRole('button', { name, exact: true })).toHaveCount(0);
      }

      await table.getByRole('button', { name: 'acme/frontend' }).focus();
      await page.keyboard.press('Enter');

      const editor = page.locator('.source-details');
      await expect(editor.getByLabel('Name')).toBeDisabled();
      for (const name of ['Save', 'Verify', 'Cancel']) {
        await expect(editor.getByRole('button', { name, exact: true })).toHaveCount(0);
      }
    });

    test('opens a member the editor from the source name by keyboard', async ({ page }) => {
      await routeRole(page, 'member');
      await page.reload();

      const table = page.getByRole('table', { name: 'Sources' });
      await expect(table.getByRole('button', { name: 'Delete' })).toHaveCount(3);
      await expect(page.getByText("Viewers can't change sources.")).toHaveCount(0);

      await table.getByRole('button', { name: 'acme/frontend' }).focus();
      await page.keyboard.press('Space');

      const editor = page.locator('.source-details');
      await expect(editor.getByLabel('Name')).toBeEnabled();
      await expect(editor.getByRole('button', { name: 'Save' })).toBeVisible();
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

      await expect(page.getByText('Server error')).toBeVisible();
    });
  });

  test.describe('Loading State', () => {
    test('shows skeleton cards while loading', async ({ page }) => {
      let finishLoading: () => void = () => {};
      const sourcesCanLoad = new Promise<void>((resolve) => {
        finishLoading = resolve;
      });
      await page.unroute(sourcesRoutePattern);
      await routeApi(page, sourcesRoutePattern, async (route) => {
        if (isSourcesListRequest(route.request().url())) {
          await sourcesCanLoad;
          await route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ sources: mockSources }),
          });
        } else {
          route.continue();
        }
      });

      await page.reload();

      await expect(page.locator('.skeleton-card').first()).toBeVisible();
      finishLoading();
      await expect(page.locator('.source-card')).toHaveCount(3);
    });
  });

  test.describe('Legacy Route', () => {
    test('redirects /sources with a source to the open editor in workspace settings', async ({
      page,
    }) => {
      await routeApi(page, sourceDetailPattern, (route) => {
        if (route.request().method() === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ source: mockSources[0] }),
          });
        } else {
          route.continue();
        }
      });

      await page.goto('/sources?source=src-1');

      await expect(page).toHaveURL(`${SOURCES_PATH}?source=src-1`);
      await expect(
        page.locator('.source-details').getByRole('heading', { level: 3, name: 'acme/frontend' })
      ).toBeVisible();
    });

    test('keeps the hash when redirecting /sources', async ({ page }) => {
      await page.goto('/sources#details');

      await expect(page).toHaveURL(`${SOURCES_PATH}#details`);
      await expect(page.locator('.sources-section')).toBeVisible();
    });

    test('replaces the /sources entry in history', async ({ page }) => {
      await page.goto('/chats');
      await page.goto('/sources');
      await expect(page).toHaveURL(SOURCES_PATH);

      await page.goBack();

      await expect(page).toHaveURL('/chats');
    });
  });
});
