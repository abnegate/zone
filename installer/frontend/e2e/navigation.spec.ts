import { test, expect } from '@playwright/test';

test.describe('Installer Navigation', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
  });

  test('displays installer form', async ({ page }) => {
    await expect(page.locator('h1')).toContainText('Zone Configuration');
  });

  test('shows step pills', async ({ page }) => {
    await expect(page.locator('.step-pill')).toHaveCount(7);
  });

  test('navigates forward through steps', async ({ page }) => {
    await expect(page.locator('h2')).toContainText('Domain Configuration');

    // Use step pills to navigate (bypasses validation)
    await page.click('.step-pill:nth-child(2)');
    await expect(page.locator('h2')).toContainText('Security');

    await page.click('.step-pill:nth-child(3)');
    await expect(page.locator('h2')).toContainText('Model Selection');
  });

  test('navigates backward through steps', async ({ page }) => {
    // Navigate to step 3 via step pills
    await page.click('.step-pill:nth-child(3)');
    await expect(page.locator('h2')).toContainText('Model Selection');

    await page.click('text=Previous');
    await expect(page.locator('h2')).toContainText('Security');
  });

  test('Previous button disabled on first step', async ({ page }) => {
    await expect(page.locator('button:has-text("Previous")')).toBeDisabled();
  });

  test('can click step pills to navigate', async ({ page }) => {
    await page.click('.step-pill:nth-child(3)');
    await expect(page.locator('h2')).toContainText('Model Selection');
  });

  test('keyboard navigation with arrow keys', async ({ page }) => {
    // Start on step 1 (Domain) and verify
    await expect(page.locator('h2')).toContainText('Domain Configuration');

    // ArrowRight should not work on step 1 without valid security keys
    // Instead test ArrowLeft from step 2
    await page.click('.step-pill:nth-child(2)');
    await expect(page.locator('h2')).toContainText('Security');

    // ArrowLeft should go back to Domain (no validation needed for back)
    await page.keyboard.press('ArrowLeft');
    await expect(page.locator('h2')).toContainText('Domain');
  });

  test('shows Install button on last step', async ({ page }) => {
    // Navigate to final step via step pill
    await page.click('.step-pill:nth-child(7)');

    await expect(page.locator('button:has-text("Install")')).toBeVisible();
  });
});
