import { mkdirSync } from 'node:fs';
import { test, expect } from '@playwright/test';
import type { Page } from '@playwright/test';

const screenshotsDir = 'screenshots';

const ensureScreenshotsDir = () => {
  mkdirSync(screenshotsDir, { recursive: true });
};

const goToStep = async (page: Page, step: number, heading: string) => {
  await page.click(`[data-step="${step}"]`);
  await expect(page.getByRole('heading', { name: heading })).toBeVisible();
};

test.beforeEach(async ({ page }) => {
  ensureScreenshotsDir();
  await page.goto('/');
  await page.waitForLoadState('networkidle');
});

test.describe('Screenshots - Installer Steps', () => {
  test('Domain step', async ({ page }) => {
    await expect(page.getByRole('heading', { name: 'Domain Configuration' })).toBeVisible();
    await page.screenshot({ path: `${screenshotsDir}/domain.png`, fullPage: true });
  });

  test('Security step', async ({ page }) => {
    await goToStep(page, 2, 'Security');
    await page.screenshot({ path: `${screenshotsDir}/security.png`, fullPage: true });
  });

  test('AI provider step', async ({ page }) => {
    await goToStep(page, 3, 'AI Provider Configuration');
    await page.screenshot({ path: `${screenshotsDir}/ai-provider.png`, fullPage: true });
  });

  test('Interface step', async ({ page }) => {
    await goToStep(page, 4, 'Interface Settings');
    await page.screenshot({ path: `${screenshotsDir}/interface.png`, fullPage: true });
  });

  test('Web search step', async ({ page }) => {
    await goToStep(page, 5, 'Web Search');
    await page.screenshot({ path: `${screenshotsDir}/web-search.png`, fullPage: true });
  });

  test('VPN step', async ({ page }) => {
    await goToStep(page, 6, 'VPN Configuration');
    await page.screenshot({ path: `${screenshotsDir}/vpn.png`, fullPage: true });
  });

  test('Advanced step', async ({ page }) => {
    await goToStep(page, 7, 'Advanced Settings');
    await page.screenshot({ path: `${screenshotsDir}/advanced.png`, fullPage: true });
  });
});

test.describe('Screenshots - Installation Modal', () => {
  test('Install complete modal', async ({ page }) => {
    await goToStep(page, 7, 'Advanced Settings');

    await page.route('**/api/install', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'text/plain',
        body: '{"progress": 50, "status": "Installing..."}\n{"progress": 100, "complete": true}\n',
      });
    });

    await page.click('button:has-text("Install")');
    await expect(page.getByRole('dialog')).toBeVisible({ timeout: 5000 });
    await expect(page.getByText('Installation Complete')).toBeVisible({ timeout: 10000 });

    await page.screenshot({ path: `${screenshotsDir}/install-complete.png`, fullPage: true });
  });
});
