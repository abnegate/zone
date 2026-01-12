import type { Page, BrowserContext, Route } from '@playwright/test';

// Block service worker to allow Playwright route interception to work
export async function blockServiceWorker(context: BrowserContext) {
  await context.route('**/service-worker.js', (route) => {
    route.fulfill({ status: 404, body: '' });
  });
}

export async function routeApi(
  page: Page,
  url: string | RegExp,
  handler: (route: Route) => Promise<void> | void
) {
  await page.route(url, (route) => {
    const type = route.request().resourceType();
    if (type !== 'xhr' && type !== 'fetch') {
      return route.continue();
    }
    return handler(route);
  });
}

export async function routeApiContext(
  context: BrowserContext,
  url: string | RegExp,
  handler: (route: Route) => Promise<void> | void
) {
  await context.route(url, (route) => {
    const type = route.request().resourceType();
    if (type !== 'xhr' && type !== 'fetch') {
      return route.continue();
    }
    return handler(route);
  });
}

// Helper to create a mock JWT token with embedded roles and permissions
// JWT format: header.payload.signature (all base64url encoded)
export function createMockJwt(payload: {
  sub: string;
  email: string;
  roles: string[];
  permissions: string[];
  exp: number;
}) {
  const header = { alg: 'HS256', typ: 'JWT' };
  const fullPayload = { ...payload, iat: Math.floor(Date.now() / 1000), jti: 'test-jti' };
  const base64Header = Buffer.from(JSON.stringify(header)).toString('base64url');
  const base64Payload = Buffer.from(JSON.stringify(fullPayload)).toString('base64url');
  return `${base64Header}.${base64Payload}.mock-signature`;
}

// Standard user permissions
export const userPermissions = [
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
  'wiki:read',
  'wiki:create',
  'wiki:update',
];

// Admin permissions
export const adminPermissions = [
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
  'users:read',
  'users:create',
  'users:update',
  'users:delete',
];

// Default mock user
export const mockUser = {
  id: 'user-1',
  email: 'test@example.com',
  email_verified: true,
  display_name: 'Test User',
  is_active: true,
  is_admin: false,
  created_at: '2024-01-01T00:00:00Z',
  updated_at: '2024-01-01T00:00:00Z',
  last_login_at: null,
};

// Admin mock user
export const mockAdminUser = {
  id: 'admin-1',
  email: 'admin@example.com',
  email_verified: true,
  display_name: 'Admin User',
  is_active: true,
  is_admin: true,
  created_at: '2024-01-01T00:00:00Z',
  updated_at: '2024-01-01T00:00:00Z',
  last_login_at: null,
};

// Setup authentication for a page with regular user
export async function setupAuth(page: Page, options?: { admin?: boolean }) {
  const isAdmin = options?.admin ?? false;
  const user = isAdmin ? mockAdminUser : mockUser;
  const permissions = isAdmin ? adminPermissions : userPermissions;
  const roles = isAdmin ? ['admin', 'user'] : ['user'];

  const token = createMockJwt({
    sub: user.id,
    email: user.email,
    roles,
    permissions,
    exp: Math.floor(Date.now() / 1000) + 3600, // Expires in 1 hour
  });

  // Navigate to a page first to be able to set localStorage
  await page.goto('/login');

  // Unregister any service workers that might intercept requests
  await page.evaluate(async () => {
    if ('serviceWorker' in navigator) {
      const registrations = await navigator.serviceWorker.getRegistrations();
      await Promise.all(registrations.map((r) => r.unregister()));
    }
  });

  await page.evaluate(
    ({ token, user }) => {
      localStorage.setItem('manager_access_token', token);
      localStorage.setItem('manager_refresh_token', 'mock-refresh-token');
      localStorage.setItem('manager_user', JSON.stringify(user));
    },
    { token, user }
  );
}

// Setup common API route mocks for models page
export async function setupModelsMock(page: Page) {
  await routeApi(page, '**/api/models*', (route) => {
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
}
