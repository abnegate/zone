import { test, expect } from '@playwright/test';
import { fillRequiredSecrets } from './helpers';

test.describe('Installation Process', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    // Fill in required security secrets so Install validation passes
    await fillRequiredSecrets(page);
  });

  test('shows Install button on final step', async ({ page }) => {
    // Navigate to final step via step pill
    await page.click('[data-step="6"]');

    await expect(page.locator('button:has-text("Install")')).toBeVisible();
  });

  test('opens modal when Install clicked', async ({ page }) => {
    // Navigate to final step via step pill
    await page.click('[data-step="6"]');

    // Set up route before clicking Install
    await page.route('**/api/install', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'text/plain',
        body: '{"progress": 50, "status": "Installing..."}\n{"progress": 100, "complete": true}\n',
      });
    });

    // Click Install and wait for modal
    await page.click('button:has-text("Install")');

    // Modal should appear immediately (before API response)
    await expect(page.getByRole('dialog')).toBeVisible({ timeout: 5000 });
  });

  test('shows progress during installation', async ({ page }) => {
    // Navigate to final step via step pill
    await page.click('[data-step="6"]');

    await page.route('**/api/install', async (route) => {
      await new Promise((r) => setTimeout(r, 500));
      route.fulfill({
        status: 200,
        contentType: 'text/plain',
        body: '{"progress": 50, "status": "Installing..."}\n{"progress": 100, "complete": true}',
      });
    });

    await page.click('button:has-text("Install")');

    // Wait for modal to appear first
    await expect(page.getByRole('dialog')).toBeVisible({ timeout: 5000 });
    await expect(page.getByRole('progressbar')).toBeVisible();
  });

  test('shows success message on completion', async ({ page }) => {
    // Navigate to final step via step pill
    await page.click('[data-step="6"]');

    await page.route('**/api/install', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'text/plain',
        body: '{"progress": 100, "status": "Done!", "complete": true}\n',
      });
    });

    await page.click('button:has-text("Install")');

    // First verify modal opens
    await expect(page.getByRole('dialog')).toBeVisible({ timeout: 5000 });
    // Then check for completion (use exact match to avoid matching dialog title with different casing)
    await expect(page.getByRole('heading', { name: 'Installation Complete', exact: true })).toBeVisible({ timeout: 10000 });
  });

  test('shows error message on failure', async ({ page }) => {
    // Navigate to final step via step pill
    await page.click('[data-step="6"]');

    await page.route('**/api/install', (route) => {
      route.fulfill({
        status: 500,
        body: 'Internal Server Error',
      });
    });

    await page.click('button:has-text("Install")');

    // First verify modal opens
    await expect(page.getByRole('dialog')).toBeVisible({ timeout: 5000 });
    // Then check for error
    await expect(page.locator('text=Installation Failed')).toBeVisible({ timeout: 10000 });
  });

  test('opens Zone chat after completion without retired settings', async ({ page }) => {
    // Navigate to final step via step pill
    await page.click('[data-step="6"]');

    let config: Record<string, string> = {};
    await page.route('**/api/install', (route) => {
      config = route.request().postDataJSON();
      route.fulfill({
        status: 200,
        contentType: 'text/plain',
        body: '{"progress": 100, "complete": true}\n',
      });
    });

    await page.click('button:has-text("Install")');
    await expect(page.getByRole('dialog')).toBeVisible({ timeout: 5000 });
    await expect(page.getByRole('heading', { name: 'Installation Complete', exact: true })).toBeVisible({ timeout: 10000 });

    // Dialog chrome also exposes a Close control; target the footer action.
    await page.getByRole('dialog').locator('button.w-full', { hasText: 'Close' }).click();
    await expect(page.getByRole('dialog')).toHaveCount(0);
    await expect(page.getByRole('heading', { name: 'Installation complete' })).toBeVisible();
    await expect(page.getByRole('link', { name: 'http://manager.webui.localhost/chats' })).toHaveAttribute('href', 'http://manager.webui.localhost/chats');
    await expect(page.getByText('Web UI Auth', { exact: true })).toHaveCount(0);
    expect(config.DOMAIN_HOST_WEBUI).toBe('webui.localhost');
    expect(Object.keys(config).filter((key) => key.startsWith('WEBUI_'))).toEqual([]);
    expect(config).not.toHaveProperty('SEARCH_CONCURRENT_REQUESTS');
    await page.screenshot({ path: 'screenshots/zone-chat-completion.png', fullPage: true });
  });
});
