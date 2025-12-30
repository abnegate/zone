import { test, expect } from '@playwright/test';

test.describe('Authentication', () => {
  test.beforeEach(async ({ page }) => {
    // Clear localStorage before each test
    await page.goto('/');
    await page.evaluate(() => localStorage.clear());
    await page.reload();
  });

  test('shows login overlay when not authenticated', async ({ page }) => {
    await page.goto('/');

    await expect(page.locator('.login-overlay')).toBeVisible();
    await expect(page.locator('.login-modal h2')).toHaveText('Zone');
    await expect(page.locator('input[type="password"]')).toBeVisible();
  });

  test('shows error for empty API key submission', async ({ page }) => {
    await page.goto('/');
    await page.click('button[type="submit"]');

    await expect(page.locator('.login-error')).toHaveText('Please enter an API key');
    await expect(page.locator('.login-overlay')).toBeVisible();
  });

  test('shows error for whitespace-only API key', async ({ page }) => {
    await page.goto('/');
    await page.fill('input[type="password"]', '   ');
    await page.click('button[type="submit"]');

    await expect(page.locator('.login-error')).toHaveText('Please enter an API key');
    await expect(page.locator('.login-overlay')).toBeVisible();
  });

  test('shows error for invalid API key', async ({ page }) => {
    // Mock failed auth response
    await page.route('/api/models', (route) => {
      route.fulfill({ status: 401, body: 'Unauthorized' });
    });

    await page.goto('/');
    await page.fill('input[type="password"]', 'invalid-key');
    await page.click('button[type="submit"]');

    await expect(page.locator('.login-error')).toHaveText('Invalid API key');
  });

  test('shows loading state during authentication', async ({ page }) => {
    // Mock slow auth response
    await page.route('/api/models', async (route) => {
      await new Promise(resolve => setTimeout(resolve, 500));
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [] }),
      });
    });

    await page.route('/api/browse*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [], has_more: false }),
      });
    });

    await page.goto('/');
    await page.fill('input[type="password"]', 'test-key');
    await page.click('.login-modal button[type="submit"]');

    // Should show loading state
    await expect(page.locator('.login-modal button[type="submit"]')).toContainText('Authenticating...');
    await expect(page.locator('.login-modal button[type="submit"]')).toBeDisabled();
    await expect(page.locator('input[type="password"]')).toBeDisabled();

    // Wait for login to complete
    await expect(page.locator('.login-overlay')).not.toBeVisible({ timeout: 2000 });
  });

  test('handles network error during authentication', async ({ page }) => {
    // Mock network failure
    await page.route('/api/models', (route) => {
      route.abort('failed');
    });

    await page.goto('/');
    await page.fill('input[type="password"]', 'test-key');
    await page.click('button[type="submit"]');

    await expect(page.locator('.login-error')).toHaveText('Invalid API key');
    await expect(page.locator('.login-overlay')).toBeVisible();
  });

  test('handles server error during authentication', async ({ page }) => {
    // Mock 500 server error
    await page.route('/api/models', (route) => {
      route.fulfill({ status: 500, body: 'Internal Server Error' });
    });

    await page.goto('/');
    await page.fill('input[type="password"]', 'test-key');
    await page.click('button[type="submit"]');

    await expect(page.locator('.login-error')).toHaveText('Invalid API key');
    await expect(page.locator('.login-overlay')).toBeVisible();
  });

  test('successful login hides overlay and shows main content', async ({ page }) => {
    // Mock successful auth response
    await page.route('/api/models', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [] }),
      });
    });

    await page.route('/api/browse*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [], has_more: false }),
      });
    });

    await page.goto('/');
    await page.fill('input[type="password"]', 'valid-api-key');
    await page.click('button[type="submit"]');

    await expect(page.locator('.login-overlay')).not.toBeVisible();
    await expect(page.locator('.sidebar')).toBeVisible();
    await expect(page.locator('.page-header h1')).toHaveText('Models');
  });

  test('stores API key in localStorage after successful login', async ({ page }) => {
    await page.route('/api/models', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [] }),
      });
    });

    await page.route('/api/browse*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [], has_more: false }),
      });
    });

    await page.goto('/');
    await page.fill('input[type="password"]', 'test-api-key');
    await page.click('button[type="submit"]');

    await expect(page.locator('.login-overlay')).not.toBeVisible();

    const storedKey = await page.evaluate(() => localStorage.getItem('manager_api_key'));
    expect(storedKey).toBe('test-api-key');
  });

  test('logout clears session and shows login overlay', async ({ page }) => {
    // Set up authenticated state
    await page.route('/api/models', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [] }),
      });
    });

    await page.route('/api/browse*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [], has_more: false }),
      });
    });

    await page.goto('/');
    await page.fill('input[type="password"]', 'test-api-key');
    await page.click('button[type="submit"]');
    await expect(page.locator('.login-overlay')).not.toBeVisible();

    // Click logout
    await page.click('.logout-btn');

    await expect(page.locator('.login-overlay')).toBeVisible();
    const storedKey = await page.evaluate(() => localStorage.getItem('manager_api_key'));
    expect(storedKey).toBeNull();
  });

  test('persists authentication across page reloads', async ({ page }) => {
    await page.route('/api/models', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [] }),
      });
    });

    await page.route('/api/browse*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [], has_more: false }),
      });
    });

    // Login first
    await page.goto('/');
    await page.fill('input[type="password"]', 'persistent-key');
    await page.click('button[type="submit"]');
    await expect(page.locator('.login-overlay')).not.toBeVisible();

    // Reload the page
    await page.reload();

    // Should still be authenticated
    await expect(page.locator('.login-overlay')).not.toBeVisible();
    await expect(page.locator('.sidebar')).toBeVisible();
  });

  test('auto-validates stored API key on page load', async ({ page }) => {
    // First set up a stored key
    await page.goto('/');
    await page.evaluate(() => localStorage.setItem('manager_api_key', 'stored-key'));

    // Mock validation request
    await page.route('/api/models', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [] }),
      });
    });

    await page.route('/api/browse*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [], has_more: false }),
      });
    });

    // Reload to trigger validation
    await page.reload();

    // Should skip login overlay
    await expect(page.locator('.login-overlay')).not.toBeVisible();
  });

  test('clears invalid stored API key on page load', async ({ page }) => {
    // First set up an invalid stored key
    await page.goto('/');
    await page.evaluate(() => localStorage.setItem('manager_api_key', 'invalid-stored-key'));

    // Mock failed validation
    await page.route('/api/models', (route) => {
      route.fulfill({ status: 401, body: 'Unauthorized' });
    });

    // Reload to trigger validation
    await page.reload();

    // Should show login overlay
    await expect(page.locator('.login-overlay')).toBeVisible();
  });

  test('input autofocuses on login overlay', async ({ page }) => {
    await page.goto('/');

    // Input should be focused
    await expect(page.locator('input[type="password"]')).toBeFocused();
  });

  test('can submit login form with Enter key', async ({ page }) => {
    await page.route('/api/models', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [] }),
      });
    });

    await page.route('/api/browse*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [], has_more: false }),
      });
    });

    await page.goto('/');
    await page.fill('input[type="password"]', 'test-key');
    await page.press('input[type="password"]', 'Enter');

    await expect(page.locator('.login-overlay')).not.toBeVisible();
  });
});
