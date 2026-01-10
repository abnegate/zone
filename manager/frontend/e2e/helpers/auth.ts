import type { Page } from '@playwright/test';

/**
 * Sets up authenticated state for e2e tests by storing mock tokens in localStorage.
 * Call this after page.goto('/') and before any authenticated interactions.
 */
export async function setupAuth(page: Page, options: { isAdmin?: boolean } = {}): Promise<void> {
  const isAdmin = options.isAdmin || false;

  // Create JWT and set localStorage inside browser context where btoa is available
  await page.evaluate(
    ({ isAdmin }) => {
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

      // Create mock JWT
      const header = { alg: 'HS256', typ: 'JWT' };
      const now = Math.floor(Date.now() / 1000);
      const payload = {
        sub: 'user-1',
        email: 'test@example.com',
        roles: isAdmin ? ['admin', 'user'] : ['user'],
        permissions,
        iat: now,
        exp: now + 86400,
        jti: 'mock-jti',
      };

      const base64Header = btoa(JSON.stringify(header));
      const base64Payload = btoa(JSON.stringify(payload));
      const accessToken = `${base64Header}.${base64Payload}.mock-signature`;

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

      localStorage.setItem('manager_access_token', accessToken);
      localStorage.setItem('manager_refresh_token', 'mock-refresh-token');
      localStorage.setItem('manager_user', JSON.stringify(user));
    },
    { isAdmin }
  );
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
  await page.route('**/api/models*', (route) => {
    const url = new URL(route.request().url());
    const source = url.searchParams.get('source');

    if (source) {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ source, models: [], has_more: false }),
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
  await page.route('**/api/organizations', (route) => {
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

  await page.route('**/api/organizations/*/workspaces', (route) => {
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
}
