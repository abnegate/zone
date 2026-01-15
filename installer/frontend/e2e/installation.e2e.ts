import { test, expect } from '@playwright/test';

test.describe('Installation Process', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
  });

  test('shows Install button on final step', async ({ page }) => {
    // Navigate to final step via step pill
    await page.click('[data-step="7"]');

    await expect(page.locator('button:has-text("Install")')).toBeVisible();
  });

  test('opens modal when Install clicked', async ({ page }) => {
    // Navigate to final step via step pill
    await page.click('[data-step="7"]');

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
    await page.click('[data-step="7"]');

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
    await page.click('[data-step="7"]');

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
    // Then check for completion
    await expect(page.locator('text=Installation Complete')).toBeVisible({ timeout: 10000 });
  });

  test('shows error message on failure', async ({ page }) => {
    // Navigate to final step via step pill
    await page.click('[data-step="7"]');

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

  test('can close modal after completion', async ({ page }) => {
    // Navigate to final step via step pill
    await page.click('[data-step="7"]');

    await page.route('**/api/install', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'text/plain',
        body: '{"progress": 100, "complete": true}\n',
      });
    });

    await page.click('button:has-text("Install")');
    await expect(page.getByRole('dialog')).toBeVisible({ timeout: 5000 });
    await expect(page.locator('text=Installation Complete')).toBeVisible({ timeout: 10000 });

    await page.click('button:has-text("Close")');
    await expect(page.getByRole('dialog')).toHaveCount(0);
  });
});
