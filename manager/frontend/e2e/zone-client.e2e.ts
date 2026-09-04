import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import type { Page } from '@playwright/test';
import { expect, test } from './fixtures';
import { setupAuth, mockCommonEndpoints } from './helpers/auth';
import { routeApi } from './test-utils';

const setupHtml = readFileSync(
  path.resolve(
    path.dirname(fileURLToPath(import.meta.url)),
    '../../../runner/zone_installer/src/setup.html'
  ),
  'utf8'
);

async function mockZoneInfo(
  page: Page,
  body: { client?: boolean; host?: string } | null,
  status = 200
) {
  await page.route('**/__zone/info', async (route) => {
    if (body === null) {
      await route.fulfill({ status: 404, body: '' });
      return;
    }
    await route.fulfill({
      status,
      contentType: 'application/json',
      body: JSON.stringify(body),
    });
  });
}

async function mockChangeServerPage(page: Page) {
  await page.route('**/__zone/change-server', async (route) => {
    await route.fulfill({
      status: 200,
      contentType: 'text/html; charset=utf-8',
      body: setupHtml,
    });
  });
}

async function openAuthenticatedApp(page: Page) {
  await mockCommonEndpoints(page);
  await setupAuth(page);
  await page.goto('/');
  await expect(page.locator('.sidebar')).toBeAttached();
}

async function openSidebarIfNeeded(page: Page, mobile: boolean) {
  if (!mobile) {
    return;
  }
  await page.locator('.mobile-menu-btn').click();
  await expect(page.locator('.sidebar.open')).toBeVisible();
}

function clientSuite(name: string, options: { mobile: boolean; viewport?: { width: number; height: number } }) {
  test.describe(name, () => {
    if (options.mobile) {
      test.skip(({ browserName }) => browserName === 'firefox', 'Firefox does not support isMobile.');
      test.use({
        viewport: options.viewport ?? { width: 390, height: 844 },
        isMobile: true,
        hasTouch: true,
      });
    } else if (options.viewport) {
      test.use({ viewport: options.viewport });
    }

    test('hides Change Server unless the Zone client is serving the UI', async ({ page }) => {
      await mockZoneInfo(page, null);
      await openAuthenticatedApp(page);
      await openSidebarIfNeeded(page, options.mobile);
      await expect(page.getByRole('link', { name: 'Change Server' })).toHaveCount(0);
    });

    test('shows Change Server when the Zone client reports itself', async ({ page }) => {
      await mockZoneInfo(page, { client: true, host: 'https://zone.example.com' });
      await openAuthenticatedApp(page);
      await openSidebarIfNeeded(page, options.mobile);
      const link = page.getByRole('link', { name: 'Change Server' });
      await expect(link).toBeVisible();
      await expect(link).toHaveAttribute('href', '/__zone/change-server');
    });

    test('Change Server opens the first-launch setup page', async ({ page }) => {
      await mockZoneInfo(page, { client: true, host: 'https://zone.example.com' });
      await mockChangeServerPage(page);
      await openAuthenticatedApp(page);
      await openSidebarIfNeeded(page, options.mobile);
      await page.getByRole('link', { name: 'Change Server' }).click();
      await expect(page.getByRole('heading', { name: 'Zone' })).toBeVisible();
      await expect(page.getByLabel('Server URL')).toHaveValue('https://zone.example.com');
      await expect(page.getByRole('button', { name: 'Continue' })).toBeVisible();
    });

    test('setup form posts the server URL and returns home', async ({ page }) => {
      await mockZoneInfo(page, { client: true, host: 'https://old.example.com' });
      await mockChangeServerPage(page);
      await mockCommonEndpoints(page);
      await setupAuth(page);

      let savedHost = '';
      await routeApi(page, '**/api/setup', async (route) => {
        savedHost = route.request().postDataJSON().host;
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ ok: true }),
        });
      });

      await page.goto('/__zone/change-server');
      await expect(page).toHaveURL(/\/__zone\/change-server$/);
      await expect(page.getByLabel('Server URL')).toHaveValue('https://old.example.com');
      await page.getByLabel('Server URL').fill('https://zone.example.com');
      await page.getByRole('button', { name: 'Continue' }).click();
      await expect.poll(() => savedHost).toBe('https://zone.example.com');
      await expect(page).toHaveURL(/\/$/);
      await expect(page).not.toHaveURL(/__zone\/change-server/);
    });

    test('setup form shows an error when the server rejects the URL', async ({ page }) => {
      await mockZoneInfo(page, { client: true, host: 'https://zone.example.com' });
      await mockChangeServerPage(page);
      await mockCommonEndpoints(page);
      await setupAuth(page);

      await routeApi(page, '**/api/setup', async (route) => {
        await route.fulfill({
          status: 400,
          contentType: 'application/json',
          body: JSON.stringify({ error: 'Enter a valid Zone server URL' }),
        });
      });

      await page.goto('/__zone/change-server');
      await page.getByRole('button', { name: 'Continue' }).click();
      await expect(page.locator('.error')).toHaveText('Enter a valid Zone server URL');
      await expect(page.getByRole('button', { name: 'Continue' })).toBeEnabled();
      await expect(page).toHaveURL(/\/__zone\/change-server$/);
    });
  });
}

clientSuite('Desktop Zone client', {
  mobile: false,
  viewport: { width: 1280, height: 840 },
});

clientSuite('Android Zone client', {
  mobile: true,
  viewport: { width: 412, height: 915 },
});

clientSuite('iOS Zone client', {
  mobile: true,
  viewport: { width: 390, height: 844 },
});
