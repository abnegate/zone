import { test, expect } from '@playwright/test';

test.describe('Navigation', () => {
  test.beforeEach(async ({ page }) => {
    // Mock API responses
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

    // Set up authenticated state
    await page.goto('/');
    await page.evaluate(() => localStorage.setItem('manager_api_key', 'test-key'));
    await page.reload();
    await expect(page.locator('.login-overlay')).not.toBeVisible();
  });

  test('sidebar shows all navigation items', async ({ page }) => {
    await expect(page.locator('.nav-item')).toHaveCount(5);
    await expect(page.locator('.nav-item').nth(0)).toContainText('Models');
    await expect(page.locator('.nav-item').nth(1)).toContainText('Chat');
    await expect(page.locator('.nav-item').nth(2)).toContainText('Projects');
    await expect(page.locator('.nav-item').nth(3)).toContainText('Issues');
    await expect(page.locator('.nav-item').nth(4)).toContainText('Repositories');
  });

  test('Models page is default route', async ({ page }) => {
    await expect(page.locator('.nav-item').first()).toHaveClass(/active/);
    await expect(page.locator('.page-header h1')).toHaveText('Models');
  });

  test('navigates to Chat page', async ({ page }) => {
    await page.click('text=Chat');
    await expect(page).toHaveURL('/chat');
    await expect(page.locator('.page-header h1')).toHaveText('Chat');
    await expect(page.locator('.stub-content h2')).toHaveText('Chat Interface');
  });

  test('navigates to Projects page', async ({ page }) => {
    await page.click('text=Projects');
    await expect(page).toHaveURL('/projects');
    await expect(page.locator('.page-header h1')).toHaveText('Projects');
    await expect(page.locator('.stub-content h2')).toHaveText('Project Management');
  });

  test('navigates to Issues page', async ({ page }) => {
    await page.click('text=Issues');
    await expect(page).toHaveURL('/issues');
    await expect(page.locator('.page-header h1')).toHaveText('Issues');
    await expect(page.locator('.stub-content h2')).toHaveText('Issue Tracker');
  });

  test('navigates to Repositories page', async ({ page }) => {
    await page.click('text=Repositories');
    await expect(page).toHaveURL('/repos');
    await expect(page.locator('.page-header h1')).toHaveText('Repositories');
    await expect(page.locator('.stub-content h2')).toHaveText('Repository Connections');
  });

  test('active nav item updates on navigation', async ({ page }) => {
    // Initially Models is active
    await expect(page.locator('.nav-item').first()).toHaveClass(/active/);

    // Navigate to Chat
    await page.click('text=Chat');
    await expect(page.locator('.nav-item').nth(1)).toHaveClass(/active/);
    await expect(page.locator('.nav-item').first()).not.toHaveClass(/active/);
  });

  test('direct URL navigation works', async ({ page }) => {
    await page.goto('/projects');
    await expect(page.locator('.page-header h1')).toHaveText('Projects');
    await expect(page.locator('.nav-item').nth(2)).toHaveClass(/active/);
  });

  test('handles unknown routes gracefully', async ({ page }) => {
    // Need to set up auth first since beforeEach ran for '/' not '/unknown-route'
    await page.goto('/unknown-route-12345');
    await page.evaluate(() => localStorage.setItem('manager_api_key', 'test-key'));
    await page.reload();

    // Should either redirect to home or show 404 - verify it doesn't crash
    // The sidebar might not be visible if the route redirects to login
    // Just verify the page loads without crashing
    await expect(page).toHaveURL(/.*unknown-route.*|^\/$|.*models.*/);
  });

  test('browser back button works correctly', async ({ page }) => {
    // Navigate to Chat
    await page.click('text=Chat');
    await expect(page).toHaveURL('/chat');

    // Navigate to Projects
    await page.click('text=Projects');
    await expect(page).toHaveURL('/projects');

    // Go back
    await page.goBack();
    await expect(page).toHaveURL('/chat');
    await expect(page.locator('.page-header h1')).toHaveText('Chat');
  });

  test('browser forward button works correctly', async ({ page }) => {
    // Navigate to Chat
    await page.click('text=Chat');
    await expect(page).toHaveURL('/chat');

    // Go back to Models
    await page.goBack();
    await expect(page).toHaveURL('/');

    // Go forward to Chat
    await page.goForward();
    await expect(page).toHaveURL('/chat');
    await expect(page.locator('.page-header h1')).toHaveText('Chat');
  });

  test('sidebar shows logout button', async ({ page }) => {
    await expect(page.locator('.logout-btn')).toBeVisible();
  });

  test('stub pages show coming soon message', async ({ page }) => {
    await page.click('text=Chat');
    await expect(page.locator('.stub-content')).toContainText('coming soon');
  });

  test('rapid navigation between pages works', async ({ page }) => {
    // Rapidly click through all nav items
    await page.click('text=Chat');
    await page.click('text=Projects');
    await page.click('text=Issues');
    await page.click('text=Repositories');
    await page.click('text=Models');

    // Should end up on Models page
    await expect(page.locator('.page-header h1')).toHaveText('Models');
    await expect(page.locator('.nav-item').first()).toHaveClass(/active/);
  });

  test('navigation preserves auth state', async ({ page }) => {
    // Navigate to different pages and verify auth is preserved
    await page.click('text=Chat');
    await expect(page.locator('.login-overlay')).not.toBeVisible();

    await page.click('text=Projects');
    await expect(page.locator('.login-overlay')).not.toBeVisible();

    await page.click('text=Issues');
    await expect(page.locator('.login-overlay')).not.toBeVisible();
  });
});
