import { test, expect } from '@playwright/test';

test.describe('Installer Navigation', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
  });

  test('displays installer form', async ({ page }) => {
    await expect(page.locator('h1')).toContainText('Zone');
  });

  test('shows step items', async ({ page }) => {
    await expect(page.locator('.stepper-item')).toHaveCount(7);
  });

  test('navigates forward through steps', async ({ page }) => {
    await expect(page.locator('h2')).toContainText('Domain Configuration');

    // Use step items to navigate (bypasses validation)
    await page.click('.stepper-item:nth-child(2) .stepper-button');
    await expect(page.locator('h2')).toContainText('Security');

    await page.click('.stepper-item:nth-child(3) .stepper-button');
    await expect(page.locator('h2')).toContainText('AI Provider Configuration');
  });

  test('navigates backward through steps', async ({ page }) => {
    // Navigate to step 3 via step items
    await page.click('.stepper-item:nth-child(3) .stepper-button');
    await expect(page.locator('h2')).toContainText('AI Provider Configuration');

    await page.click('text=Previous');
    await expect(page.locator('h2')).toContainText('Security');
  });

  test('Previous button disabled on first step', async ({ page }) => {
    await expect(page.locator('button:has-text("Previous")')).toBeDisabled();
  });

  test('can click step items to navigate', async ({ page }) => {
    await page.click('.stepper-item:nth-child(3) .stepper-button');
    await expect(page.locator('h2')).toContainText('AI Provider Configuration');
  });

  test('keyboard navigation with arrow keys', async ({ page }) => {
    // Start on step 1 (Domain) and verify
    await expect(page.locator('h2')).toContainText('Domain Configuration');

    // ArrowRight should not work on step 1 without valid security keys
    // Instead test ArrowLeft from step 2
    await page.click('.stepper-item:nth-child(2) .stepper-button');
    await expect(page.locator('h2')).toContainText('Security');

    // ArrowLeft should go back to Domain (no validation needed for back)
    await page.keyboard.press('ArrowLeft');
    await expect(page.locator('h2')).toContainText('Domain');
  });

  test('shows Install button on last step', async ({ page }) => {
    // Navigate to final step via step item
    await page.click('.stepper-item:nth-child(7) .stepper-button');

    await expect(page.locator('button:has-text("Install")')).toBeVisible();
  });
});
