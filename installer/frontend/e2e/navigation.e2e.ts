import { test, expect } from '@playwright/test';

test.describe('Installer Navigation', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
  });

  test('displays installer form', async ({ page }) => {
    await expect(page.getByText('Zone', { exact: true })).toBeVisible();
  });

  test('shows step items', async ({ page }) => {
    await expect(page.locator('[data-step]')).toHaveCount(6);
  });

  test('keeps every step description inside the sidebar', async ({ page }) => {
    const sidebar = await page.getByTestId('installer-sidebar').boundingBox();
    expect(sidebar).not.toBeNull();
    let previousBottom = 0;
    for (const button of await page.locator('[data-step]').all()) {
      const description = await button.locator('span').last().boundingBox();
      expect(description).not.toBeNull();
      expect(description!.x + description!.width).toBeLessThanOrEqual(sidebar!.x + sidebar!.width);
      const row = await button.boundingBox();
      expect(row).not.toBeNull();
      expect(description!.y + description!.height).toBeLessThanOrEqual(row!.y + row!.height);
      expect(row!.y).toBeGreaterThanOrEqual(previousBottom);
      previousBottom = row!.y + row!.height;
    }
  });

  test('navigates forward through steps', async ({ page }) => {
    await expect(page.getByRole('heading', { name: 'Domain Configuration' })).toBeVisible();

    // Use step items to navigate (bypasses validation)
    await page.click('[data-step="2"]');
    await expect(page.getByRole('heading', { name: 'Security' })).toBeVisible();

    await page.click('[data-step="3"]');
    await expect(page.getByRole('heading', { name: 'AI Provider Configuration' })).toBeVisible();
  });

  test('navigates backward through steps', async ({ page }) => {
    // Navigate to step 3 via step items
    await page.click('[data-step="3"]');
    await expect(page.getByRole('heading', { name: 'AI Provider Configuration' })).toBeVisible();

    await page.click('text=Previous');
    await expect(page.getByRole('heading', { name: 'Security' })).toBeVisible();
  });

  test('Previous button disabled on first step', async ({ page }) => {
    await expect(page.locator('button:has-text("Previous")')).toBeDisabled();
  });

  test('can click step items to navigate', async ({ page }) => {
    await page.click('[data-step="3"]');
    await expect(page.getByRole('heading', { name: 'AI Provider Configuration' })).toBeVisible();
  });

  test('keyboard navigation with arrow keys', async ({ page }) => {
    // Start on step 1 (Domain) and verify
    await expect(page.getByRole('heading', { name: 'Domain Configuration' })).toBeVisible();

    // ArrowRight should not work on step 1 without valid security keys
    // Instead test ArrowLeft from step 2
    await page.click('[data-step="2"]');
    await expect(page.getByRole('heading', { name: 'Security' })).toBeVisible();

    // ArrowLeft should go back to Domain (no validation needed for back)
    await page.keyboard.press('ArrowLeft');
    await expect(page.getByRole('heading', { name: 'Domain Configuration' })).toBeVisible();
  });

  test('shows Install button on last step', async ({ page }) => {
    // Navigate to final step via step item
    await page.click('[data-step="6"]');

    await expect(page.locator('button:has-text("Install")')).toBeVisible();
  });
});
