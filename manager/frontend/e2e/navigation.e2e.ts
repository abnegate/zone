import { test, expect } from './fixtures';
import { setupAuth, mockCommonEndpoints } from './helpers/auth';
import { blockServiceWorker, routeApi } from './test-utils';

const tasksRoutePattern = /\/api\/workspaces\/[^/]+\/tasks/;
const tasksListPattern = /\/api\/workspaces\/[^/]+\/tasks\/?$/;
const sourcesRoutePattern = /\/api\/workspaces\/[^/]+\/sources/;
const sourcesListPattern = /\/api\/workspaces\/[^/]+\/sources\/?$/;

const isTasksListRequest = (requestUrl: string) =>
  tasksListPattern.test(new URL(requestUrl).pathname);
const isSourcesListRequest = (requestUrl: string) =>
  sourcesListPattern.test(new URL(requestUrl).pathname);

test.describe('Navigation', () => {
  test.beforeEach(async ({ context, page }) => {
    await blockServiceWorker(context);
    await mockCommonEndpoints(page);

    await routeApi(page, '**/api/chats*', (route) => {
      if (route.request().method() === 'GET') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ chats: [] }),
        });
      } else {
        route.continue();
      }
    });

    await routeApi(page, '**/api/projects*', (route) => {
      if (route.request().method() === 'GET') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ success: true, projects: [] }),
        });
      } else {
        route.continue();
      }
    });

    await routeApi(page, tasksRoutePattern, (route) => {
      if (
        route.request().method() === 'GET' &&
        isTasksListRequest(route.request().url())
      ) {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ success: true, tasks: [] }),
        });
      } else {
        route.continue();
      }
    });

    await routeApi(page, '**/api/sources/types', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ success: true, types: [] }),
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
          body: JSON.stringify({ success: true, sources: [] }),
        });
      } else {
        route.continue();
      }
    });

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

    await routeApi(page, '**/api/context/search*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ results: [], total: 0 }),
      });
    });

    // Mock organization members for organization settings
    await routeApi(page, '**/api/organizations/*/members*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ success: true, members: [] }),
      });
    });

    // Mock organization invitations
    await routeApi(page, '**/api/organizations/*/invitations*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ success: true, invitations: [] }),
      });
    });

    // Mock workspace members
    await routeApi(page, '**/api/workspaces/*/members*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ success: true, members: [] }),
      });
    });

    await routeApi(page, '**/api/workspaces/*/theme', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ theme: 'light' }),
      });
    });

    await page.goto('/');
    await setupAuth(page, { isAdmin: true });

    // Set organization and workspace context
    await page.evaluate(() => {
      localStorage.setItem('manager_current_org', '00000000-0000-0000-0000-000000000001');
      localStorage.setItem('manager_current_workspace', '00000000-0000-0000-0000-000000000001');
    });

    await page.reload();
    await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });
  });

  test('sidebar shows all navigation items', async ({ page }) => {
    const navItems = page.locator('.nav-item');
    await expect(navItems).toHaveCount(9);
    await expect(navItems.nth(0)).toContainText('Chats');
    await expect(navItems.nth(1)).toContainText('Projects');
    await expect(navItems.nth(2)).toContainText('Tasks');
    await expect(navItems.nth(3)).toContainText('Sources');
    await expect(navItems.nth(4)).toContainText('Search');
    await expect(navItems.nth(5)).toContainText('Models');
    await expect(navItems.nth(6)).toContainText('Wiki');
    await expect(navItems.nth(7)).toContainText('Organization');
    await expect(navItems.nth(8)).toContainText('Workspace');
  });

  test('Chats page is default route', async ({ page }) => {
    await expect(page.locator('.nav-item:has-text("Chats")')).toHaveClass(/active/);
    await expect(
      page.getByRole('heading', { name: 'Chats', exact: true })
    ).toBeVisible();
  });

  test('navigates to Models page', async ({ page }) => {
    await page.click('a[href="/models"]');
    await expect(page).toHaveURL('/models');
    await expect(
      page.getByRole('heading', { name: 'Models', exact: true })
    ).toBeVisible();
  });

  test('navigates to Chats page', async ({ page }) => {
    await page.click('a[href="/chats"]');
    await expect(page).toHaveURL('/chats');
    await expect(
      page.getByRole('heading', { name: 'Chats', exact: true })
    ).toBeVisible();
  });

  test('navigates to Projects page', async ({ page }) => {
    await page.click('a[href="/projects"]');
    await expect(page).toHaveURL('/projects');
    await expect(
      page.getByRole('heading', { name: 'Projects', exact: true })
    ).toBeVisible();
  });

  test('navigates to Tasks page', async ({ page }) => {
    await page.click('a[href="/tasks"]');
    await expect(page).toHaveURL('/tasks');
    await expect(
      page.getByRole('heading', { name: 'Tasks', exact: true })
    ).toBeVisible();
  });

  test('navigates to Sources page', async ({ page }) => {
    await page.click('a[href="/sources"]');
    await expect(page).toHaveURL('/sources');
    await expect(
      page.getByRole('heading', { name: 'Sources', exact: true })
    ).toBeVisible();
  });

  test('navigates to Search page', async ({ page }) => {
    await page.click('a[href="/search"]');
    await expect(page).toHaveURL('/search');
    await expect(page.getByRole('heading', { name: 'Context Search' })).toBeVisible();
  });

  test('navigates to Wiki page', async ({ page }) => {
    await page.click('a[href="/wiki"]');
    await expect(page).toHaveURL('/wiki');
    await expect(page.getByRole('heading', { name: 'Knowledge Base' })).toBeVisible();
  });

  test('navigates to Organization settings page', async ({ page }) => {
    // Wait for organizations to load before clicking
    await page.waitForSelector('.nav-item:has-text("Organization")');
    await page.click('a[href="/org-settings"]');
    await page.waitForURL('/org-settings', { timeout: 10000 });
    await expect(page.locator('.page-header .page-title')).toHaveText('Organization Settings');
  });

  test('navigates to Workspace settings page', async ({ page }) => {
    await page.click('a[href="/settings"]');
    await page.waitForURL('/settings', { timeout: 10000 });
    await expect(page.locator('.page-header .page-title')).toHaveText('Workspace Settings');
  });

  test('active nav item updates on navigation', async ({ page }) => {
    await expect(page.locator('.nav-item:has-text("Chats")')).toHaveClass(/active/);

    await page.click('a[href="/models"]');
    await expect(page.locator('.nav-item:has-text("Models")')).toHaveClass(/active/);
    await expect(page.locator('.nav-item:has-text("Chats")')).not.toHaveClass(/active/);
  });

  test('direct URL navigation works', async ({ page }) => {
    await page.goto('/projects');
    await expect(
      page.getByRole('heading', { name: 'Projects', exact: true })
    ).toBeVisible();
    await expect(page.locator('.nav-item:has-text("Projects")')).toHaveClass(/active/);
  });

  test('handles unknown routes gracefully', async ({ page }) => {
    // Click on any nav item first, then use history API to test unknown route
    await page.click('a[href="/chats"]');
    await expect(page).toHaveURL('/chats');

    // Navigate to unknown route via link or history
    await page.evaluate(() => window.history.pushState({}, '', '/unknown-route-12345'));
    await page.waitForURL(/unknown-route-12345/);

    // Sidebar should remain visible (app doesn't crash)
    await expect(page.locator('.sidebar')).toBeVisible();
  });

  test('browser back button works correctly', async ({ page }) => {
    await page.click('a[href="/chats"]');
    await expect(page).toHaveURL('/chats');

    await page.click('a[href="/projects"]');
    await expect(page).toHaveURL('/projects');

    await page.goBack();
    await expect(page).toHaveURL('/chats');
  });

  test('browser forward button works correctly', async ({ page }) => {
    await page.click('a[href="/chats"]');
    await expect(page).toHaveURL('/chats');

    await page.goBack();
    await expect(page).toHaveURL('/');

    await page.goForward();
    await expect(page).toHaveURL('/chats');
  });

  test('sidebar shows logout button', async ({ page }) => {
    await expect(page.locator('.logout-btn')).toBeVisible();
  });

  test('navigation preserves auth state', async ({ page }) => {
    await page.click('a[href="/chats"]');
    await expect(page).not.toHaveURL('/login');

    await page.click('a[href="/projects"]');
    await expect(page).not.toHaveURL('/login');

    await page.click('a[href="/tasks"]');
    await expect(page).not.toHaveURL('/login');
  });
});
