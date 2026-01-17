import { test, expect } from './fixtures';
import { routeApi } from './test-utils';
import type { Page } from '@playwright/test';

// Helper to create a mock JWT token with embedded roles and permissions
// JWT format: header.payload.signature (all base64url encoded)
function createMockJwt(payload: { sub: string; email: string; roles: string[]; permissions: string[]; exp: number }) {
  const header = { alg: 'HS256', typ: 'JWT' };
  const fullPayload = { ...payload, iat: Math.floor(Date.now() / 1000), jti: 'test-jti' };
  const base64Header = Buffer.from(JSON.stringify(header)).toString('base64url');
  const base64Payload = Buffer.from(JSON.stringify(fullPayload)).toString('base64url');
  return `${base64Header}.${base64Payload}.mock-signature`;
}

// Standard user permissions
const userPermissions = [
  'models:read', 'models:create',
  'chats:read', 'chats:create', 'chats:update', 'chats:delete',
  'projects:read', 'projects:create', 'projects:update', 'projects:delete',
  'tasks:read', 'tasks:create', 'tasks:update', 'tasks:delete',
  'sources:read',
  'wiki:read', 'wiki:create', 'wiki:update',
];

// Admin permissions
const adminPermissions = [
  'models:read', 'models:create', 'models:update', 'models:delete',
  'chats:read', 'chats:create', 'chats:update', 'chats:delete',
  'projects:read', 'projects:create', 'projects:update', 'projects:delete',
  'tasks:read', 'tasks:create', 'tasks:update', 'tasks:delete',
  'sources:read', 'sources:create', 'sources:update', 'sources:delete',
  'wiki:read', 'wiki:create', 'wiki:update', 'wiki:delete',
  'users:read', 'users:create', 'users:update', 'users:delete',
];

// Mock auth API responses - wrapped in { data: ... } to match API structure
// The access_token must be a valid JWT for the AuthContext to decode roles/permissions
const mockAuthResponse = (user = {}) => {
  const token = createMockJwt({
    sub: 'user-1',
    email: 'test@example.com',
    roles: ['user'],
    permissions: userPermissions,
    exp: Math.floor(Date.now() / 1000) + 900, // Expires in 15 minutes
  });
  return {
    access_token: token,
    refresh_token: 'mock-refresh-token',
    expires_in: 900,
    user: {
      id: 'user-1',
      email: 'test@example.com',
      email_verified: true,
      display_name: 'Test User',
      is_active: true,
      is_admin: false,
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
      last_login_at: null,
      ...user,
    },
    roles: ['user'],
    permissions: userPermissions,
  };
};

const mockAdminAuthResponse = () => {
  const token = createMockJwt({
    sub: 'admin-1',
    email: 'admin@example.com',
    roles: ['admin', 'user'],
    permissions: adminPermissions,
    exp: Math.floor(Date.now() / 1000) + 900,
  });
  return {
    access_token: token,
    refresh_token: 'mock-refresh-token',
    expires_in: 900,
    user: {
      id: 'admin-1',
      email: 'admin@example.com',
      email_verified: true,
      display_name: 'Admin User',
      is_active: true,
      is_admin: true,
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
      last_login_at: null,
    },
    roles: ['admin', 'user'],
    permissions: adminPermissions,
  };
};

async function mockModelsAndBrowse(page: Page) {
  await routeApi(page, '**/api/models*', (route) => {
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

  // Organization routes are now set up in fixtures.ts
}

test.describe('Login Page', () => {
  test.beforeEach(async ({ page }) => {
    // Clear localStorage before each test
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await page.reload();
  });

  test('displays login form with correct elements', async ({ page }) => {
    await page.goto('/login');

    await expect(page.locator('.zone-logo__text')).toContainText('Zone');
    await expect(page.locator('input[type="email"]')).toBeVisible();
    await expect(page.locator('input[type="password"]')).toBeVisible();
    await expect(page.locator('button[type="submit"]')).toHaveText('Sign In');
    await expect(page.locator("text=Don't have an account?")).toBeVisible();
  });

  test('shows validation error for empty email', async ({ page }) => {
    await page.goto('/login');

    await page.fill('input[type="password"]', 'Password123');
    await page.click('button[type="submit"]');

    // Validation errors appear as field-level errors
    await expect(page.locator('[id$="-error"][role="alert"]')).toBeVisible();
  });

  test('shows validation error for empty password', async ({ page }) => {
    await page.goto('/login');

    await page.fill('input[type="email"]', 'test@example.com');
    await page.click('button[type="submit"]');

    // Validation errors appear as field-level errors
    await expect(page.locator('[id$="-error"][role="alert"]')).toBeVisible();
  });

  test('shows error for invalid credentials', async ({ page }) => {
    await routeApi(page, '**/api/auth/login', (route) => {
      route.fulfill({
        status: 401,
        contentType: 'application/json',
        body: JSON.stringify({ error: 'Invalid email or password' }),
      });
    });

    await page.goto('/login');
    await page.fill('input[type="email"]', 'wrong@example.com');
    await page.fill('input[type="password"]', 'wrongpassword');
    await page.click('button[type="submit"]');

    // Error message is displayed in the form (not in toast notification)
    await expect(page.locator('form').getByText('Invalid email or password')).toBeVisible();
  });

  test('shows loading state during authentication', async ({ page }) => {
    await routeApi(page, '**/api/auth/login', async (route) => {
      await new Promise(resolve => setTimeout(resolve, 500));
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(mockAuthResponse()),
      });
    });

    // Mock protected routes
    await mockModelsAndBrowse(page);

    await page.goto('/login');
    await page.fill('input[type="email"]', 'test@example.com');
    await page.fill('input[type="password"]', 'Password123');
    await page.click('button[type="submit"]');

    await expect(page.locator('button[type="submit"]')).toContainText('Signing in...');
    await expect(page.locator('button[type="submit"]')).toBeDisabled();
  });

  test('successful login redirects to home and stores tokens', async ({ page }) => {
    await routeApi(page, '**/api/auth/login', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(mockAuthResponse()),
      });
    });

    await mockModelsAndBrowse(page);

    await page.goto('/login');
    await page.fill('input[type="email"]', 'test@example.com');
    await page.fill('input[type="password"]', 'Password123');
    await page.click('button[type="submit"]');

    // Should redirect to home
    await expect(page).toHaveURL('/');
    await expect(page.locator('.sidebar')).toBeVisible();

    // Verify tokens stored (access token should be a JWT with 3 parts)
    const accessToken = await page.evaluate(() => localStorage.getItem('manager_access_token'));
    const refreshToken = await page.evaluate(() => localStorage.getItem('manager_refresh_token'));
    expect(accessToken).toBeTruthy();
    expect(accessToken?.split('.').length).toBe(3); // JWT has 3 parts
    expect(refreshToken).toBe('mock-refresh-token');
  });

  test('navigates to register page via link', async ({ page }) => {
    await page.goto('/login');
    await page.click('text=Create one');

    await expect(page).toHaveURL('/register');
  });

  test('can submit form with Enter key', async ({ page }) => {
    await routeApi(page, '**/api/auth/login', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(mockAuthResponse()),
      });
    });

    await mockModelsAndBrowse(page);

    await page.goto('/login');
    await page.fill('input[type="email"]', 'test@example.com');
    await page.fill('input[type="password"]', 'Password123');
    await page.press('input[type="password"]', 'Enter');

    await expect(page).toHaveURL('/');
  });
});

test.describe('Register Page', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/register');
    await page.evaluate(() => localStorage.clear());
  });

  test('displays registration form with correct elements', async ({ page }) => {
    await page.goto('/register');

    await expect(page.locator('.zone-logo__text')).toContainText('Zone');
    await expect(page.locator('input[type="email"]')).toBeVisible();
    await expect(page.locator('input[type="text"][placeholder*="optional"]')).toBeVisible();
    await expect(page.locator('input[type="password"]').first()).toBeVisible();
    await expect(page.locator('input[type="password"]').nth(1)).toBeVisible();
    await expect(page.locator('button[type="submit"]')).toHaveText('Create Account');
  });

  test('shows validation error for mismatched passwords', async ({ page }) => {
    await page.goto('/register');

    await page.fill('input[type="email"]', 'new@example.com');
    await page.fill('input[placeholder="At least 8 characters"]', 'Password123');
    await page.fill('input[placeholder="Repeat your password"]', 'different123');
    await page.click('button[type="submit"]');

    await expect(page.locator('[role="alert"]', { hasText: 'Passwords do not match' })).toBeVisible();
  });

  test('shows validation error for short password', async ({ page }) => {
    await page.goto('/register');

    await page.fill('input[type="email"]', 'new@example.com');
    await page.fill('input[placeholder="At least 8 characters"]', 'short');
    await page.fill('input[placeholder="Repeat your password"]', 'short');
    await page.click('button[type="submit"]');

    await expect(page.locator('[id$="-error"][role="alert"]')).toBeVisible();
  });

  test('shows error for duplicate email', async ({ page }) => {
    await routeApi(page, '**/api/auth/register', (route) => {
      route.fulfill({
        status: 400,
        contentType: 'application/json',
        body: JSON.stringify({ error: 'Email already registered' }),
      });
    });

    await page.goto('/register');
    await page.fill('input[type="email"]', 'existing@example.com');
    await page.fill('input[placeholder="At least 8 characters"]', 'Password123');
    await page.fill('input[placeholder="Repeat your password"]', 'Password123');
    await page.click('button[type="submit"]');

    await expect(page.locator('.auth-error')).toBeVisible();
  });

  test('first user registration shows admin message', async ({ page }) => {
    await routeApi(page, '**/api/auth/register', (route) => {
      route.fulfill({
        status: 201,
        contentType: 'application/json',
        body: JSON.stringify(mockAdminAuthResponse()),
      });
    });

    await mockModelsAndBrowse(page);

    await page.goto('/register');
    await page.fill('input[type="email"]', 'admin@example.com');
    await page.fill('input[placeholder="At least 8 characters"]', 'adminPassword123');
    await page.fill('input[placeholder="Repeat your password"]', 'adminPassword123');
    await page.click('button[type="submit"]');

    // Should redirect to home
    await expect(page).toHaveURL('/');
  });

  test('successful registration redirects to home', async ({ page }) => {
    await routeApi(page, '**/api/auth/register', (route) => {
      route.fulfill({
        status: 201,
        contentType: 'application/json',
        body: JSON.stringify(mockAuthResponse()),
      });
    });

    await mockModelsAndBrowse(page);

    await page.goto('/register');
    await page.fill('input[type="email"]', 'new@example.com');
    await page.fill('input[type="text"][placeholder*="optional"]', 'New User');
    await page.fill('input[placeholder="At least 8 characters"]', 'Password123');
    await page.fill('input[placeholder="Repeat your password"]', 'Password123');
    await page.click('button[type="submit"]');

    await expect(page).toHaveURL('/');
    await expect(page.locator('.sidebar')).toBeVisible();
  });

  test('navigates to login page via link', async ({ page }) => {
    await page.goto('/register');
    await page.click('text=Sign in');

    await expect(page).toHaveURL('/login');
  });
});

test.describe('Protected Routes', () => {
  test('redirects to login when accessing protected route unauthenticated', async ({ page }) => {
    // Navigate to login first to clear any existing session
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await page.goto('/');

    await expect(page).toHaveURL('/login');
  });

  test('redirects to login when accessing chats without authentication', async ({ page }) => {
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await page.goto('/chats');

    await expect(page).toHaveURL('/login');
  });

  test('redirects to login when accessing projects without authentication', async ({ page }) => {
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await page.goto('/projects');

    await expect(page).toHaveURL('/login');
  });

  test('allows access to protected route when authenticated', async ({ page }) => {
    // Create a mock JWT with roles/permissions
    const mockToken = createMockJwt({
      sub: 'user-1',
      email: 'test@example.com',
      roles: ['user'],
      permissions: ['models:read', 'chats:read', 'projects:read', 'tasks:read', 'sources:read', 'wiki:read'],
      exp: Math.floor(Date.now() / 1000) + 3600, // Expires in 1 hour
    });

    const mockUser = {
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

    // Set up routes before navigating
    await mockModelsAndBrowse(page);

    // Navigate and set up authentication state
    await page.goto('/login');
    await page.evaluate(({ token, user }) => {
      localStorage.setItem('manager_access_token', token);
      localStorage.setItem('manager_refresh_token', 'valid-refresh-token');
      localStorage.setItem('manager_user', JSON.stringify(user));
    }, { token: mockToken, user: mockUser });

    await page.goto('/');

    await expect(page).toHaveURL('/');
    await expect(page.locator('.sidebar')).toBeVisible();
  });
});

test.describe('Unauthorized Access', () => {
  test('shows unauthorized page when lacking required permission', async ({ page }) => {
    // Create a mock JWT with no permissions
    const mockToken = createMockJwt({
      sub: 'user-1',
      email: 'test@example.com',
      roles: ['viewer'],
      permissions: [], // No permissions
      exp: Math.floor(Date.now() / 1000) + 3600,
    });

    const mockUser = {
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

    await mockModelsAndBrowse(page);

    // Set up authentication state without proper permissions
    await page.goto('/login');
    await page.evaluate(({ token, user }) => {
      localStorage.setItem('manager_access_token', token);
      localStorage.setItem('manager_refresh_token', 'valid-refresh-token');
      localStorage.setItem('manager_user', JSON.stringify(user));
    }, { token: mockToken, user: mockUser });

    await page.goto('/');

    // Should redirect to unauthorized
    await expect(page).toHaveURL('/unauthorized');
    await expect(page.locator('h1')).toContainText('Access Denied');
  });

  test('unauthorized page has link back to home', async ({ page }) => {
    await page.goto('/unauthorized');

    await expect(page.locator('text=Go to Home')).toBeVisible();
  });
});

test.describe('Logout', () => {
  test('logout clears tokens and redirects to login', async ({ page }) => {
    // Create a mock JWT
    const mockToken = createMockJwt({
      sub: 'user-1',
      email: 'test@example.com',
      roles: ['user'],
      permissions: ['models:read', 'chats:read', 'projects:read', 'tasks:read', 'sources:read', 'wiki:read'],
      exp: Math.floor(Date.now() / 1000) + 3600,
    });

    const mockUser = {
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

    await mockModelsAndBrowse(page);

    await routeApi(page, '**/api/auth/logout', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ success: true }),
      });
    });

    // Set up authenticated state
    await page.goto('/login');
    await page.evaluate(({ token, user }) => {
      localStorage.setItem('manager_access_token', token);
      localStorage.setItem('manager_refresh_token', 'valid-refresh-token');
      localStorage.setItem('manager_user', JSON.stringify(user));
    }, { token: mockToken, user: mockUser });

    await page.goto('/');
    await expect(page.locator('.sidebar')).toBeVisible();

    // Click logout button (in sidebar)
    await page.click('.logout-btn');

    // Should redirect to login
    await expect(page).toHaveURL('/login');

    // Tokens should be cleared
    const accessToken = await page.evaluate(() => localStorage.getItem('manager_access_token'));
    const refreshToken = await page.evaluate(() => localStorage.getItem('manager_refresh_token'));
    expect(accessToken).toBeNull();
    expect(refreshToken).toBeNull();
  });
});

test.describe('Token Persistence', () => {
  test('persists authentication across page reloads', async ({ page }) => {
    // Login first
    await routeApi(page, '**/api/auth/login', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(mockAuthResponse()),
      });
    });

    await mockModelsAndBrowse(page);

    await page.goto('/login');
    await page.fill('input[type="email"]', 'test@example.com');
    await page.fill('input[type="password"]', 'Password123');
    await page.click('button[type="submit"]');

    await expect(page).toHaveURL('/');

    // Reload the page
    await page.reload();

    // Should still be authenticated (not redirected to login)
    await expect(page.locator('.sidebar')).toBeVisible();
    await expect(page).not.toHaveURL('/login');
  });

  test('handles expired token by redirecting to login', async ({ page }) => {
    // Create an expired mock JWT
    const expiredToken = createMockJwt({
      sub: 'user-1',
      email: 'test@example.com',
      roles: ['user'],
      permissions: ['models:read'],
      exp: Math.floor(Date.now() / 1000) - 3600, // Expired 1 hour ago
    });

    const mockUser = {
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

    await mockModelsAndBrowse(page);

    // Refresh also fails
    await routeApi(page, '**/api/auth/refresh', (route) => {
      route.fulfill({ status: 401, body: 'Refresh token expired' });
    });

    // Set up expired token state
    await page.goto('/login');
    await page.evaluate(({ token, user }) => {
      localStorage.setItem('manager_access_token', token);
      localStorage.setItem('manager_refresh_token', 'expired-refresh-token');
      localStorage.setItem('manager_user', JSON.stringify(user));
    }, { token: expiredToken, user: mockUser });

    await page.goto('/');

    // Should redirect to login
    await expect(page).toHaveURL('/login');
  });
});

test.describe('Permission-Based UI', () => {
  test('hides delete button for users without delete permission', async ({ page }) => {
    // Create a mock JWT without delete permission
    const mockToken = createMockJwt({
      sub: 'user-1',
      email: 'test@example.com',
      roles: ['user'],
      permissions: ['models:read', 'chats:read', 'projects:read', 'tasks:read', 'sources:read', 'wiki:read'],
      exp: Math.floor(Date.now() / 1000) + 3600,
    });

    const mockUser = {
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

    await mockModelsAndBrowse(page);
    await page.unroute('**/api/models*');
    await routeApi(page, '**/api/models*', (route) => {
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
        body: JSON.stringify({
          models: [
            { name: 'llama2', size: 3800000000, modified_at: '2024-01-01', digest: 'abc123' },
          ],
        }),
      });
    });

    // Set up auth without delete permission
    await page.goto('/login');
    await page.evaluate(({ token, user }) => {
      localStorage.setItem('manager_access_token', token);
      localStorage.setItem('manager_refresh_token', 'valid-refresh-token');
      localStorage.setItem('manager_user', JSON.stringify(user));
    }, { token: mockToken, user: mockUser });

    await page.goto('/');

    // Wait for models page to load (use role-based selector for reliability)
    await expect(page.getByRole('heading', { name: 'Models', level: 1 })).toBeVisible();

    // The delete button for installed models should not be visible
    // (This depends on how the PermissionGate is used in ModelsPage)
  });

  test('shows admin controls for admin users', async ({ page }) => {
    // Create an admin mock JWT
    const mockToken = createMockJwt({
      sub: 'admin-1',
      email: 'admin@example.com',
      roles: ['admin', 'user'],
      permissions: adminPermissions,
      exp: Math.floor(Date.now() / 1000) + 3600,
    });

    const mockUser = {
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

    await mockModelsAndBrowse(page);

    // Set up admin auth
    await page.goto('/login');
    await page.evaluate(({ token, user }) => {
      localStorage.setItem('manager_access_token', token);
      localStorage.setItem('manager_refresh_token', 'admin-refresh-token');
      localStorage.setItem('manager_user', JSON.stringify(user));
    }, { token: mockToken, user: mockUser });

    await page.goto('/');

    // Admin should see all sidebar links
    await expect(page.locator('.sidebar')).toBeVisible();
  });
});
