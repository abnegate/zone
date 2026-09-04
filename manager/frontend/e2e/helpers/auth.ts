import type { Page } from '@playwright/test';
import { createMockJwt, routeApi } from '../test-utils';

function buildAuthState(isAdmin: boolean) {
  const permissions = isAdmin
    ? [
        'models:read',
        'models:create',
        'models:update',
        'models:delete',
        'chats:read',
        'chats:create',
        'chats:update',
        'chats:delete',
        'projects:read',
        'projects:create',
        'projects:update',
        'projects:delete',
        'tasks:read',
        'tasks:create',
        'tasks:update',
        'tasks:delete',
        'sources:read',
        'sources:create',
        'sources:update',
        'sources:delete',
        'wiki:read',
        'wiki:create',
        'wiki:update',
        'wiki:delete',
        'workspaces:read',
        'workspaces:update',
        'workspaces:delete',
        'organizations:read',
        'organizations:update',
        'organizations:delete',
        'users:read',
        'users:create',
        'users:update',
        'users:delete',
      ]
    : [
        'models:read',
        'models:create',
        'chats:read',
        'chats:create',
        'chats:update',
        'chats:delete',
        'projects:read',
        'projects:create',
        'projects:update',
        'projects:delete',
        'tasks:read',
        'tasks:create',
        'tasks:update',
        'tasks:delete',
        'sources:read',
        'sources:create',
        'sources:update',
        'sources:delete',
        'wiki:read',
        'wiki:create',
        'wiki:update',
        'workspaces:read',
        'workspaces:update',
      ];

  const now = Math.floor(Date.now() / 1000);
  const accessToken = createMockJwt({
    sub: 'user-1',
    email: 'test@example.com',
    roles: isAdmin ? ['admin', 'user'] : ['user'],
    permissions,
    exp: now + 86400,
  });

  const user = {
    id: 'user-1',
    email: 'test@example.com',
    display_name: 'Test User',
    is_active: true,
    is_admin: isAdmin,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
    last_login_at: new Date().toISOString(),
  };

  return { accessToken, user };
}

/**
 * Sets up authenticated state for e2e tests by storing mock tokens in localStorage.
 * Registers an init script so the next navigation (or reload) boots already signed in.
 * Prefer calling this before page.goto(target) so protected routes are not bounced to /login.
 */
export async function setupAuth(page: Page, options: { isAdmin?: boolean } = {}): Promise<void> {
  const authState = buildAuthState(options.isAdmin || false);

  await page.addInitScript((state) => {
    localStorage.setItem('manager_access_token', state.accessToken);
    localStorage.setItem('manager_refresh_token', 'mock-refresh-token');
    localStorage.setItem('manager_user', JSON.stringify(state.user));
  }, authState);

  const url = page.url();
  if (url && url !== 'about:blank') {
    await page.evaluate((state) => {
      localStorage.setItem('manager_access_token', state.accessToken);
      localStorage.setItem('manager_refresh_token', 'mock-refresh-token');
      localStorage.setItem('manager_user', JSON.stringify(state.user));
    }, authState);
  }
}

/**
 * Clears authentication state from localStorage.
 */
export async function clearAuth(page: Page): Promise<void> {
  await page.evaluate(() => {
    localStorage.removeItem('manager_access_token');
    localStorage.removeItem('manager_refresh_token');
    localStorage.removeItem('manager_user');
  });
}

/**
 * Common mock for models and browse endpoints that most pages need.
 */
export async function mockCommonEndpoints(page: Page): Promise<void> {
  const isApiEndpoint = (url: string) => {
    // Skip source files (Vite dev server)
    if (url.includes('/src/') || url.endsWith('.ts') || url.endsWith('.tsx')) {
      return false;
    }
    return true;
  };

  await routeApi(page, '**/api/auth/refresh', (route) => {
    if (!isApiEndpoint(route.request().url())) {
      route.continue();
      return;
    }
    route.fulfill({
      status: 401,
      contentType: 'application/json',
      body: JSON.stringify({ error: 'Invalid refresh token' }),
    });
  });

  await routeApi(page, '**/api/models*', (route) => {
    if (!isApiEndpoint(route.request().url())) {
      route.continue();
      return;
    }
    const url = new URL(route.request().url());
    const source = url.searchParams.get('source');

    if (source) {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ source, models: [], next_cursor: null }),
      });
      return;
    }

    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ models: [] }),
    });
  });

  // Mock organizations and workspaces for context switcher
  const orgResponse = {
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

  const workspaceResponse = {
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

  // Handle organizations with and without query params (e.g., ?active=true)
  await routeApi(page, '**/api/organizations?**', (route) => {
    if (!isApiEndpoint(route.request().url())) {
      route.continue();
      return;
    }
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify(orgResponse),
    });
  });
  await routeApi(page, '**/api/organizations', (route) => {
    if (!isApiEndpoint(route.request().url())) {
      route.continue();
      return;
    }
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify(orgResponse),
    });
  });

  // Handle workspaces with and without query params (e.g., ?active=true)
  await routeApi(page, '**/api/organizations/*/workspaces?**', (route) => {
    if (!isApiEndpoint(route.request().url())) {
      route.continue();
      return;
    }
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify(workspaceResponse),
    });
  });
  await routeApi(page, '**/api/organizations/*/workspaces', (route) => {
    if (!isApiEndpoint(route.request().url())) {
      route.continue();
      return;
    }
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify(workspaceResponse),
    });
  });
}
