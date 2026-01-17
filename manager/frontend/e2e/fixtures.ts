import { test as base, expect } from '@playwright/test';

const defaultOrg = {
  id: '00000000-0000-0000-0000-000000000001',
  name: 'Default Org',
  slug: 'default',
  description: null,
  is_active: true,
  created_at: '2024-01-01T00:00:00Z',
  updated_at: '2024-01-01T00:00:00Z',
};

const defaultWorkspace = {
  id: '00000000-0000-0000-0000-000000000001',
  organization_id: '00000000-0000-0000-0000-000000000001',
  name: 'Default Workspace',
  slug: 'default',
  description: null,
  is_active: true,
  created_at: '2024-01-01T00:00:00Z',
  updated_at: '2024-01-01T00:00:00Z',
};

export const test = base.extend({
  context: async ({ context }, use) => {
    // Setup default API mocks at context level (applies to ALL pages)
    // NOTE: Playwright matches routes in REVERSE order - last registered is checked FIRST

    // Catch-all for any API requests that aren't mocked (checked LAST)
    await context.route('**/api/**', (route) => {
      const type = route.request().resourceType();
      if (type !== 'xhr' && type !== 'fetch') {
        return route.continue();
      }
      const url = route.request().url();
      console.warn(`[Fixture] Unmocked API call: ${url}`);
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({}),
      });
    });

    // Auth refresh mock
    await context.route('**/api/auth/refresh', (route) => {
      const type = route.request().resourceType();
      if (type !== 'xhr' && type !== 'fetch') {
        return route.continue();
      }
      route.fulfill({
        status: 401,
        contentType: 'application/json',
        body: JSON.stringify({ error: 'Invalid refresh token' }),
      });
    });

    // Handle all organization-related endpoints with a single regex
    await context.route(/\/api\/organizations/, (route) => {
      const type = route.request().resourceType();
      if (type !== 'xhr' && type !== 'fetch') {
        return route.continue();
      }
      const url = route.request().url();
      if (url.includes('/workspaces')) {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ workspaces: [defaultWorkspace] }),
        });
      } else {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ organizations: [defaultOrg] }),
        });
      }
    });

    await use(context);
  },
  page: async ({ page }, use) => {
    // Allow source file requests to continue (Vite dev server)
    await page.route('**/src/api/**', (route) => route.continue());
    await page.route('**/@fs/**/src/api/**', (route) => route.continue());
    await use(page);
  },
});

export { expect };
