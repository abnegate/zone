import type { Page } from '@playwright/test';
import { COMMUNITY, UNVERIFIED } from '../src/features/enterprise/fixtures';
import type { LicenseResponse } from '../src/features/enterprise/types';
import { expect, test } from './fixtures';
import { setupAdminAuth, setupCommonRoutes } from './layout-fixtures';
import { blockServiceWorker, routeApi } from './test-utils';

const LICENSE_PATH = '/settings/server/license';

const LICENSE_API = /\/api\/license$/;

const UNVERIFIED_WARNING = "A license key is stored, but this version of Zone can't verify it";

const FORBIDDEN = 'This needs a server admin.';

interface LicenseServer {
  reads: number;
  removals: number;
}

async function serveLicense(page: Page, initial: LicenseResponse): Promise<LicenseServer> {
  const server: LicenseServer = { reads: 0, removals: 0 };
  let current = initial;
  await routeApi(page, LICENSE_API, (route) => {
    if (route.request().method() === 'DELETE') {
      server.removals += 1;
      current = COMMUNITY;
    } else {
      server.reads += 1;
    }
    return route.fulfill({ status: 200, json: current });
  });
  return server;
}

async function openLicense(page: Page): Promise<void> {
  await blockServiceWorker(page.context());
  await setupCommonRoutes(page, false);
  await page.goto('/login', { waitUntil: 'domcontentloaded' });
  await setupAdminAuth(page);
}

test.describe('License settings', () => {
  test('warns about a stored key that no longer verifies and removes it', async ({ page }) => {
    await openLicense(page);
    const server = await serveLicense(page, UNVERIFIED);
    await page.goto(LICENSE_PATH, { waitUntil: 'domcontentloaded' });

    const warning = page.getByRole('status').filter({ hasText: UNVERIFIED_WARNING });
    await expect(warning).toBeVisible();
    await expect(warning).toContainText('Paste a renewed key.');
    await expect(page.getByText(/No license is installed/)).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Replace license' })).toBeVisible();

    await page.getByRole('button', { name: 'Remove license' }).click();
    const dialog = page.getByRole('dialog', { name: 'Remove the license?' });
    await expect(dialog).toContainText('stays on the Community edition');
    await dialog.getByRole('button', { name: 'Remove license' }).click();

    await expect(dialog).toBeHidden();
    await expect(page.getByText(/No license is installed/)).toBeVisible();
    await expect(page.getByRole('button', { name: 'Remove license' })).toHaveCount(0);
    expect(server.removals).toBe(1);
  });

  test('shows a refusal at once without asking the server again', async ({ page }) => {
    await openLicense(page);
    let reads = 0;
    await routeApi(page, LICENSE_API, (route) => {
      reads += 1;
      return route.fulfill({ status: 403, json: { error: FORBIDDEN } });
    });
    await page.goto(LICENSE_PATH, { waitUntil: 'domcontentloaded' });

    await expect(page.getByRole('alert').filter({ hasText: FORBIDDEN })).toBeVisible();
    expect(reads).toBe(1);
  });

  test.describe('on a phone', () => {
    test.use({ viewport: { width: 375, height: 812 } });

    test('keeps the unverified warning and its actions on screen without scrolling sideways', async ({
      page,
    }) => {
      await openLicense(page);
      await serveLicense(page, UNVERIFIED);
      await page.goto(LICENSE_PATH, { waitUntil: 'domcontentloaded' });

      await expect(page.getByRole('status').filter({ hasText: UNVERIFIED_WARNING })).toBeVisible();
      await expect(page.getByRole('button', { name: 'Remove license' })).toBeInViewport();
      const overflow = await page.evaluate(
        () => document.documentElement.scrollWidth - document.documentElement.clientWidth
      );
      expect(overflow).toBeLessThanOrEqual(0);
    });
  });
});
