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

    // Default chats list so Zod does not fail when a test lands on /
    await context.route(/\/api\/chats($|\?|\/)/i, (route) => {
      const type = route.request().resourceType();
      if (type !== 'xhr' && type !== 'fetch') {
        return route.continue();
      }
      if (route.request().method() === 'GET') {
        return route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ chats: [] }),
        });
      }
      return route.continue();
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

    // Endpoints the shell asks for on every page, so no test has to mock them
    // to reach the page it is about. A workspace with no theme override has no
    // stored row, which is the 404 the console is written against.
    await context.route(/\/api\/models\/setup(\?|$)/, (route) => {
      const type = route.request().resourceType();
      if (type !== 'xhr' && type !== 'fetch') {
        return route.continue();
      }
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          ram_bytes: 68719476736,
          ram_label: '64 GB',
          disk_free_bytes: 500000000000,
          disk_free_label: '500 GB',
          vision_min_ram_bytes: 17179869184,
          disk_margin_bytes: 10000000000,
          chat_preset: '32gb',
          recommended_preset: '32gb',
          chat_presets: [],
          features: [],
          wants_all: true,
          gate: null,
          totals: {
            size_bytes: 0,
            size_label: '0 B',
            present_bytes: 0,
            present_label: '0 B',
            needed_bytes: 0,
            needed_label: '0 B',
            working_space_bytes: 0,
            working_space_label: '0 B',
            required_free_bytes: 0,
            required_free_label: '0 B',
            free_now_bytes: 0,
            free_now_label: '0 B',
            short_by_bytes: 0,
            short_by_label: null,
          },
          licenses: [],
          artifacts: [],
          pulls: [],
        }),
      });
    });

    await context.route(/\/api\/models\/disk$/, (route) => {
      const type = route.request().resourceType();
      if (type !== 'xhr' && type !== 'fetch') {
        return route.continue();
      }
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          used_bytes: 120_000_000_000,
          total_bytes: 500_000_000_000,
          available_bytes: 380_000_000_000,
          percent: 24,
        }),
      });
    });

    await context.route(/\/api\/workspaces\/[^/]+\/theme$/, (route) => {
      const type = route.request().resourceType();
      if (type !== 'xhr' && type !== 'fetch') {
        return route.continue();
      }
      if (route.request().method() !== 'GET') {
        return route.continue();
      }
      route.fulfill({
        status: 404,
        contentType: 'application/json',
        body: JSON.stringify({ error: 'No theme override' }),
      });
    });

    await context.route(/\/api\/workspaces\/[^/]+\/sources(\?|$)/, (route) => {
      const type = route.request().resourceType();
      if (type !== 'xhr' && type !== 'fetch') {
        return route.continue();
      }
      if (route.request().method() !== 'GET') {
        return route.continue();
      }
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ sources: [] }),
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
