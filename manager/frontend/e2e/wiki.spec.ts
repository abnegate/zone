import { test, expect } from '@playwright/test';
import { setupAuth, mockCommonEndpoints } from './helpers/auth';

test.describe('Wiki Page', () => {
  test.beforeEach(async ({ page }) => {
    // Set up API mocks
    await mockCommonEndpoints(page);

    // Navigate and set up auth
    await page.goto('/');
    await setupAuth(page);
    await page.reload();

    // Wait for app to load
    await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });

    // Navigate to wiki page
    await page.click('a[href="/wiki"]');
    await expect(page).toHaveURL('/wiki');
  });

  test.describe('Page Header', () => {
    test('displays page title', async ({ page }) => {
      await expect(page.locator('.page-header h1')).toContainText('Wiki');
    });

    test('displays subtitle', async ({ page }) => {
      await expect(page.locator('.page-header .subtitle')).toContainText('Knowledge base');
    });
  });

  test.describe('Stub Content', () => {
    test('displays knowledge base section', async ({ page }) => {
      await expect(page.locator('.stub-content h2')).toContainText('Knowledge Base');
    });

    test('displays description', async ({ page }) => {
      await expect(page.locator('.stub-description')).toContainText('growing knowledge base');
    });

    test('displays feature list', async ({ page }) => {
      await expect(page.locator('.stub-feature')).toHaveCount(3);
    });

    test('displays auto-populate feature', async ({ page }) => {
      await expect(page.locator('.stub-feature').first()).toContainText('Auto-populated from chat');
    });

    test('displays import feature', async ({ page }) => {
      await expect(page.locator('.stub-feature').nth(1)).toContainText('Import docs');
    });

    test('displays learning feature', async ({ page }) => {
      await expect(page.locator('.stub-feature').nth(2)).toContainText('Models learn');
    });

    test('displays stub icon', async ({ page }) => {
      await expect(page.locator('.stub-icon svg')).toBeVisible();
    });
  });
});
