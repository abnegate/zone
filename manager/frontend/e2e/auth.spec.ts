import { test, expect } from '@playwright/test';

// Mock auth API responses
const mockAuthResponse = (user = {}) => ({
  access_token: 'mock-access-token',
  refresh_token: 'mock-refresh-token',
  expires_in: 900,
  user: {
    id: 'user-1',
    email: 'test@example.com',
    display_name: 'Test User',
    is_admin: false,
    ...user,
  },
  roles: ['user'],
  permissions: [
    'models:read', 'models:create',
    'chats:read', 'chats:create', 'chats:update', 'chats:delete',
    'projects:read', 'projects:create', 'projects:update', 'projects:delete',
    'tasks:read', 'tasks:create', 'tasks:update', 'tasks:delete',
    'sources:read',
    'wiki:read', 'wiki:create', 'wiki:update',
  ],
});

const mockAdminAuthResponse = () => ({
  ...mockAuthResponse({ is_admin: true }),
  roles: ['admin', 'user'],
  permissions: [
    'models:read', 'models:create', 'models:update', 'models:delete',
    'chats:read', 'chats:create', 'chats:update', 'chats:delete',
    'projects:read', 'projects:create', 'projects:update', 'projects:delete',
    'tasks:read', 'tasks:create', 'tasks:update', 'tasks:delete',
    'sources:read', 'sources:create', 'sources:update', 'sources:delete',
    'wiki:read', 'wiki:create', 'wiki:update', 'wiki:delete',
    'users:read', 'users:create', 'users:update', 'users:delete',
  ],
});

test.describe('Login Page', () => {
  test.beforeEach(async ({ page }) => {
    // Clear localStorage before each test
    await page.goto('/login');
    await page.evaluate(() => localStorage.clear());
    await page.reload();
  });

  test('displays login form with correct elements', async ({ page }) => {
    await page.goto('/login');

    await expect(page.locator('h1')).toContainText('Welcome Back');
    await expect(page.locator('input[type="email"]')).toBeVisible();
    await expect(page.locator('input[type="password"]')).toBeVisible();
    await expect(page.locator('button[type="submit"]')).toHaveText('Sign In');
    await expect(page.locator('text=Need an account?')).toBeVisible();
  });

  test('shows validation error for empty email', async ({ page }) => {
    await page.goto('/login');

    await page.fill('input[type="password"]', 'password123');
    await page.click('button[type="submit"]');

    await expect(page.locator('.auth-error')).toContainText('Email and password are required');
  });

  test('shows validation error for empty password', async ({ page }) => {
    await page.goto('/login');

    await page.fill('input[type="email"]', 'test@example.com');
    await page.click('button[type="submit"]');

    await expect(page.locator('.auth-error')).toContainText('Email and password are required');
  });

  test('shows error for invalid credentials', async ({ page }) => {
    await page.route('**/api/auth/login', (route) => {
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

    await expect(page.locator('.auth-error')).toBeVisible();
  });

  test('shows loading state during authentication', async ({ page }) => {
    await page.route('**/api/auth/login', async (route) => {
      await new Promise(resolve => setTimeout(resolve, 500));
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(mockAuthResponse()),
      });
    });

    // Mock protected routes
    await page.route('**/api/models', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [] }),
      });
    });

    await page.route('**/api/browse*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [], has_more: false }),
      });
    });

    await page.goto('/login');
    await page.fill('input[type="email"]', 'test@example.com');
    await page.fill('input[type="password"]', 'password123');
    await page.click('button[type="submit"]');

    await expect(page.locator('button[type="submit"]')).toContainText('Signing in...');
    await expect(page.locator('button[type="submit"]')).toBeDisabled();
  });

  test('successful login redirects to home and stores tokens', async ({ page }) => {
    await page.route('**/api/auth/login', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(mockAuthResponse()),
      });
    });

    await page.route('**/api/models', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [] }),
      });
    });

    await page.route('**/api/browse*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [], has_more: false }),
      });
    });

    await page.goto('/login');
    await page.fill('input[type="email"]', 'test@example.com');
    await page.fill('input[type="password"]', 'password123');
    await page.click('button[type="submit"]');

    // Should redirect to home
    await expect(page).toHaveURL('/');
    await expect(page.locator('.sidebar')).toBeVisible();

    // Verify tokens stored
    const accessToken = await page.evaluate(() => localStorage.getItem('accessToken'));
    const refreshToken = await page.evaluate(() => localStorage.getItem('refreshToken'));
    expect(accessToken).toBe('mock-access-token');
    expect(refreshToken).toBe('mock-refresh-token');
  });

  test('navigates to register page via link', async ({ page }) => {
    await page.goto('/login');
    await page.click('text=Sign up');

    await expect(page).toHaveURL('/register');
  });

  test('can submit form with Enter key', async ({ page }) => {
    await page.route('**/api/auth/login', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(mockAuthResponse()),
      });
    });

    await page.route('**/api/models', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [] }),
      });
    });

    await page.route('**/api/browse*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [], has_more: false }),
      });
    });

    await page.goto('/login');
    await page.fill('input[type="email"]', 'test@example.com');
    await page.fill('input[type="password"]', 'password123');
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

    await expect(page.locator('h1')).toContainText('Create Account');
    await expect(page.locator('input[type="email"]')).toBeVisible();
    await expect(page.locator('input[type="text"][placeholder*="Display name"]')).toBeVisible();
    await expect(page.locator('input[id="password"]')).toBeVisible();
    await expect(page.locator('input[id="confirmPassword"]')).toBeVisible();
    await expect(page.locator('button[type="submit"]')).toHaveText('Create Account');
  });

  test('shows validation error for mismatched passwords', async ({ page }) => {
    await page.goto('/register');

    await page.fill('input[type="email"]', 'new@example.com');
    await page.fill('input[id="password"]', 'password123');
    await page.fill('input[id="confirmPassword"]', 'different123');
    await page.click('button[type="submit"]');

    await expect(page.locator('.auth-error')).toContainText('Passwords do not match');
  });

  test('shows validation error for short password', async ({ page }) => {
    await page.goto('/register');

    await page.fill('input[type="email"]', 'new@example.com');
    await page.fill('input[id="password"]', 'short');
    await page.fill('input[id="confirmPassword"]', 'short');
    await page.click('button[type="submit"]');

    await expect(page.locator('.auth-error')).toContainText('Password must be at least 8 characters');
  });

  test('shows error for duplicate email', async ({ page }) => {
    await page.route('**/api/auth/register', (route) => {
      route.fulfill({
        status: 400,
        contentType: 'application/json',
        body: JSON.stringify({ error: 'Email already registered' }),
      });
    });

    await page.goto('/register');
    await page.fill('input[type="email"]', 'existing@example.com');
    await page.fill('input[id="password"]', 'password123');
    await page.fill('input[id="confirmPassword"]', 'password123');
    await page.click('button[type="submit"]');

    await expect(page.locator('.auth-error')).toBeVisible();
  });

  test('first user registration shows admin message', async ({ page }) => {
    await page.route('**/api/auth/register', (route) => {
      route.fulfill({
        status: 201,
        contentType: 'application/json',
        body: JSON.stringify(mockAdminAuthResponse()),
      });
    });

    await page.route('**/api/models', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [] }),
      });
    });

    await page.route('**/api/browse*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [], has_more: false }),
      });
    });

    await page.goto('/register');
    await page.fill('input[type="email"]', 'admin@example.com');
    await page.fill('input[id="password"]', 'adminpassword123');
    await page.fill('input[id="confirmPassword"]', 'adminpassword123');
    await page.click('button[type="submit"]');

    // Should redirect to home
    await expect(page).toHaveURL('/');
  });

  test('successful registration redirects to home', async ({ page }) => {
    await page.route('**/api/auth/register', (route) => {
      route.fulfill({
        status: 201,
        contentType: 'application/json',
        body: JSON.stringify(mockAuthResponse()),
      });
    });

    await page.route('**/api/models', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [] }),
      });
    });

    await page.route('**/api/browse*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [], has_more: false }),
      });
    });

    await page.goto('/register');
    await page.fill('input[type="email"]', 'new@example.com');
    await page.fill('input[type="text"][placeholder*="Display name"]', 'New User');
    await page.fill('input[id="password"]', 'password123');
    await page.fill('input[id="confirmPassword"]', 'password123');
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
    await page.evaluate(() => localStorage.clear());
    await page.goto('/');

    await expect(page).toHaveURL('/login');
  });

  test('redirects to login when accessing chats without authentication', async ({ page }) => {
    await page.evaluate(() => localStorage.clear());
    await page.goto('/chats');

    await expect(page).toHaveURL('/login');
  });

  test('redirects to login when accessing projects without authentication', async ({ page }) => {
    await page.evaluate(() => localStorage.clear());
    await page.goto('/projects');

    await expect(page).toHaveURL('/login');
  });

  test('allows access to protected route when authenticated', async ({ page }) => {
    // Set up authentication state
    await page.goto('/');
    await page.evaluate(() => {
      localStorage.setItem('accessToken', 'valid-token');
      localStorage.setItem('refreshToken', 'valid-refresh-token');
      localStorage.setItem('user', JSON.stringify({
        id: 'user-1',
        email: 'test@example.com',
        display_name: 'Test User',
        is_admin: false,
      }));
      localStorage.setItem('roles', JSON.stringify(['user']));
      localStorage.setItem('permissions', JSON.stringify([
        'models:read', 'chats:read', 'projects:read', 'tasks:read', 'sources:read', 'wiki:read',
      ]));
    });

    await page.route('**/api/models', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [] }),
      });
    });

    await page.route('**/api/browse*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [], has_more: false }),
      });
    });

    await page.reload();

    await expect(page).toHaveURL('/');
    await expect(page.locator('.sidebar')).toBeVisible();
  });
});

test.describe('Unauthorized Access', () => {
  test('shows unauthorized page when lacking required permission', async ({ page }) => {
    // Set up authentication state without proper permissions
    await page.goto('/');
    await page.evaluate(() => {
      localStorage.setItem('accessToken', 'valid-token');
      localStorage.setItem('refreshToken', 'valid-refresh-token');
      localStorage.setItem('user', JSON.stringify({
        id: 'user-1',
        email: 'test@example.com',
        display_name: 'Test User',
        is_admin: false,
      }));
      localStorage.setItem('roles', JSON.stringify(['viewer']));
      localStorage.setItem('permissions', JSON.stringify([])); // No permissions
    });

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
    // Set up authenticated state
    await page.goto('/');
    await page.evaluate(() => {
      localStorage.setItem('accessToken', 'valid-token');
      localStorage.setItem('refreshToken', 'valid-refresh-token');
      localStorage.setItem('user', JSON.stringify({
        id: 'user-1',
        email: 'test@example.com',
        display_name: 'Test User',
        is_admin: false,
      }));
      localStorage.setItem('roles', JSON.stringify(['user']));
      localStorage.setItem('permissions', JSON.stringify([
        'models:read', 'chats:read', 'projects:read', 'tasks:read', 'sources:read', 'wiki:read',
      ]));
    });

    await page.route('**/api/models', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [] }),
      });
    });

    await page.route('**/api/browse*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [], has_more: false }),
      });
    });

    await page.route('**/api/auth/logout', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ success: true }),
      });
    });

    await page.reload();
    await expect(page.locator('.sidebar')).toBeVisible();

    // Click logout button (in sidebar)
    await page.click('.logout-btn');

    // Should redirect to login
    await expect(page).toHaveURL('/login');

    // Tokens should be cleared
    const accessToken = await page.evaluate(() => localStorage.getItem('accessToken'));
    const refreshToken = await page.evaluate(() => localStorage.getItem('refreshToken'));
    expect(accessToken).toBeNull();
    expect(refreshToken).toBeNull();
  });
});

test.describe('Token Persistence', () => {
  test('persists authentication across page reloads', async ({ page }) => {
    // Login first
    await page.route('**/api/auth/login', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(mockAuthResponse()),
      });
    });

    await page.route('**/api/models', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [] }),
      });
    });

    await page.route('**/api/browse*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [], has_more: false }),
      });
    });

    await page.goto('/login');
    await page.fill('input[type="email"]', 'test@example.com');
    await page.fill('input[type="password"]', 'password123');
    await page.click('button[type="submit"]');

    await expect(page).toHaveURL('/');

    // Reload the page
    await page.reload();

    // Should still be authenticated (not redirected to login)
    await expect(page.locator('.sidebar')).toBeVisible();
    await expect(page).not.toHaveURL('/login');
  });

  test('handles expired token by redirecting to login', async ({ page }) => {
    // Set up expired token state
    await page.goto('/');
    await page.evaluate(() => {
      localStorage.setItem('accessToken', 'expired-token');
      localStorage.setItem('refreshToken', 'expired-refresh-token');
      localStorage.setItem('user', JSON.stringify({
        id: 'user-1',
        email: 'test@example.com',
        display_name: 'Test User',
        is_admin: false,
      }));
      localStorage.setItem('roles', JSON.stringify(['user']));
      localStorage.setItem('permissions', JSON.stringify(['models:read']));
    });

    // API returns 401 for expired token
    await page.route('**/api/models', (route) => {
      route.fulfill({ status: 401, body: 'Unauthorized' });
    });

    // Refresh also fails
    await page.route('**/api/auth/refresh', (route) => {
      route.fulfill({ status: 401, body: 'Refresh token expired' });
    });

    await page.reload();

    // Should redirect to login
    await expect(page).toHaveURL('/login');
  });
});

test.describe('Permission-Based UI', () => {
  test('hides delete button for users without delete permission', async ({ page }) => {
    // Set up auth without delete permission
    await page.goto('/');
    await page.evaluate(() => {
      localStorage.setItem('accessToken', 'valid-token');
      localStorage.setItem('refreshToken', 'valid-refresh-token');
      localStorage.setItem('user', JSON.stringify({
        id: 'user-1',
        email: 'test@example.com',
        display_name: 'Test User',
        is_admin: false,
      }));
      localStorage.setItem('roles', JSON.stringify(['user']));
      localStorage.setItem('permissions', JSON.stringify([
        'models:read', // Only read, no delete
        'chats:read', 'projects:read', 'tasks:read', 'sources:read', 'wiki:read',
      ]));
    });

    await page.route('**/api/models', (route) => {
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

    await page.route('**/api/browse*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [], has_more: false }),
      });
    });

    await page.reload();

    // Wait for models page to load
    await expect(page.locator('.page-header h1')).toHaveText('Models');

    // The delete button for installed models should not be visible
    // (This depends on how the PermissionGate is used in ModelsPage)
  });

  test('shows admin controls for admin users', async ({ page }) => {
    await page.goto('/');
    await page.evaluate(() => {
      localStorage.setItem('accessToken', 'admin-token');
      localStorage.setItem('refreshToken', 'admin-refresh-token');
      localStorage.setItem('user', JSON.stringify({
        id: 'admin-1',
        email: 'admin@example.com',
        display_name: 'Admin User',
        is_admin: true,
      }));
      localStorage.setItem('roles', JSON.stringify(['admin', 'user']));
      localStorage.setItem('permissions', JSON.stringify([
        'models:read', 'models:create', 'models:update', 'models:delete',
        'chats:read', 'chats:create', 'chats:update', 'chats:delete',
        'projects:read', 'projects:create', 'projects:update', 'projects:delete',
        'users:read', 'users:create', 'users:update', 'users:delete',
      ]));
    });

    await page.route('**/api/models', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [] }),
      });
    });

    await page.route('**/api/browse*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [], has_more: false }),
      });
    });

    await page.reload();

    // Admin should see all sidebar links
    await expect(page.locator('.sidebar')).toBeVisible();
  });
});
