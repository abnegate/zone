import { test, expect } from './fixtures';
import { mockCommonEndpoints, setupAuth } from './helpers/auth';
import { blockServiceWorker, remainingSetupPlan, routeApi } from './test-utils';

test.describe('First-run feature setup', () => {
  test.beforeEach(async ({ context, page }) => {
    await blockServiceWorker(context);
    await mockCommonEndpoints(page);
    await setupAuth(page, { setupComplete: false });
    await routeApi(page, '**/api/models/setup*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(remainingSetupPlan()),
      });
    });
    await routeApi(page, '**/api/chats*', (route) => {
      if (route.request().method() === 'GET') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ chats: [] }),
        });
        return;
      }
      route.continue();
    });
  });

  test('login lands on a setup step to choose features', async ({ page }) => {
    await page.goto('/');
    await expect(page).toHaveURL(/\/setup$/);
    await expect(page.getByRole('heading', { name: 'Set up Zone' })).toBeVisible();
    await expect(page.getByRole('checkbox', { name: 'Chat' })).toBeChecked();
    await expect(page.getByRole('checkbox', { name: 'Vision' })).toBeChecked();
    await expect(page.getByLabel('Chat: Installed')).toBeVisible();
    await expect(page.getByLabel('Vision: 4.7 GB')).toBeVisible();
    await expect(page.getByText('Free required')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Install selected' })).toBeEnabled();
    await expect(page.getByRole('button', { name: 'Skip for now' })).toBeVisible();
    await expect(page.getByText('1 Features')).toBeVisible();
    await expect(page.getByText('2 Folders')).toBeVisible();
    await expect(page.locator('.sidebar')).toHaveCount(0);
    await page.getByRole('combobox', { name: 'Chat models' }).click();
    await expect(page.getByRole('option', { name: '32 GB+ RAM' })).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(page.getByRole('option', { name: '32 GB+ RAM' })).toHaveCount(0);
  });

  test('skip for now enters the app and does not return to setup', async ({ page }) => {
    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'Set up Zone' })).toBeVisible();
    await page.getByRole('button', { name: 'Skip for now' }).click();
    await expect(page.getByRole('heading', { name: 'Host folders' })).toBeVisible();
    await expect(page).toHaveURL(/\/setup$/);
    await page.getByRole('button', { name: 'Skip for now' }).click();
    await expect(page).toHaveURL('/');
    await expect(page.locator('.sidebar')).toBeVisible();
    await page.reload();
    await expect(page).toHaveURL('/');
    await expect(page.locator('.sidebar')).toBeVisible();
    await expect(page.getByRole('heading', { name: 'Set up Zone' })).toHaveCount(0);
    await page.goto('/models');
    await expect(page.getByRole('tab', { name: 'Features' })).toBeVisible();
  });

  test('saving host folders finishes setup', async ({ page }) => {
    await page.goto('/');
    await page.getByRole('button', { name: 'Skip for now' }).click();
    await expect(page.getByRole('heading', { name: 'Host folders' })).toBeVisible();
    await page.getByLabel('Folder 1').fill('/Users/you/Local/jbs');
    await page.getByRole('button', { name: 'Save and continue' }).click();
    await expect(page).toHaveURL('/');
    await expect(page.locator('.sidebar')).toBeVisible();
  });
});
