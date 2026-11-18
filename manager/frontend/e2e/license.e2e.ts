import path from 'node:path';
import type { Page } from '@playwright/test';
import { ACTIVE, COMMUNITY, LAPSED, UNVERIFIED } from '../src/features/enterprise/fixtures';
import { PRICING_URL } from '../src/features/enterprise/pricing';
import type { LicenseResponse } from '../src/features/enterprise/types';
import { expect, test } from './fixtures';
import { setupAdminAuth, setupCommonRoutes } from './layout-fixtures';
import { blockServiceWorker, routeApi } from './test-utils';

const LICENSE_PATH = '/settings/server/license';

const LICENSE_API = /\/api\/license$/;

const UNVERIFIED_WARNING = "A license key is stored, but this version of Zone can't verify it";

const FORBIDDEN = 'This needs a server admin.';

const BUY = 'Buy a license';

const DESKTOP = { width: 1440, height: 900 };
const PHONE = { width: 375, height: 812 };
const WIDTHS = [DESKTOP, PHONE];
const SCHEMES = ['light', 'dark'] as const;

const SCREENSHOTS = process.env.LICENSE_SCREENSHOTS;

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

async function expectNoSidewaysScroll(page: Page): Promise<void> {
  const overflow = await page.evaluate(() =>
    Math.max(
      document.documentElement.scrollWidth - document.documentElement.clientWidth,
      ...Array.from(
        document.querySelectorAll('.license-section'),
        (element) => element.scrollWidth - element.clientWidth
      )
    )
  );
  expect(overflow, 'the page never scrolls sideways').toBeLessThanOrEqual(0);
}

function contentHeight(page: Page): Promise<number> {
  return page.evaluate(() =>
    Math.ceil(
      Math.max(
        ...Array.from(
          document.querySelectorAll('*'),
          (element) => element.getBoundingClientRect().top + element.scrollHeight
        )
      )
    )
  );
}

async function shoot(page: Page, name: string): Promise<void> {
  await expectNoSidewaysScroll(page);
  const viewport = page.viewportSize() ?? DESKTOP;
  let height = viewport.height;
  for (let measured = await contentHeight(page); measured > height; ) {
    height = measured;
    await page.setViewportSize({ width: viewport.width, height });
    measured = await contentHeight(page);
  }
  await expectNoSidewaysScroll(page);
  await page.screenshot({
    path: path.join(SCREENSHOTS ?? '', `${name}.png`),
    animations: 'disabled',
  });
  await page.setViewportSize(viewport);
}

test.describe('License settings', () => {
  test('links a Community install to pricing in a new tab', async ({ page }) => {
    await openLicense(page);
    await serveLicense(page, COMMUNITY);
    await page.goto(LICENSE_PATH, { waitUntil: 'domcontentloaded' });

    const link = page.getByRole('link', { name: BUY });
    await expect(link).toBeVisible();
    await expect(link).toHaveAttribute('href', PRICING_URL);
    await expect(link).toHaveAttribute('target', '_blank');
    await expect(link).toHaveAttribute('rel', /\bnoopener\b/);
  });

  test('does not offer a purchase to an active install', async ({ page }) => {
    await openLicense(page);
    await serveLicense(page, ACTIVE);
    await page.goto(LICENSE_PATH, { waitUntil: 'domcontentloaded' });

    await expect(page.getByRole('button', { name: 'Remove license' })).toBeVisible();
    await expect(page.getByRole('link', { name: BUY })).toHaveCount(0);
  });

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
      await expect(page.getByRole('link', { name: BUY })).toBeInViewport();
      const overflow = await page.evaluate(
        () => document.documentElement.scrollWidth - document.documentElement.clientWidth
      );
      expect(overflow).toBeLessThanOrEqual(0);
    });
  });
});

test.describe('License layout', () => {
  test.skip(!SCREENSHOTS, 'set LICENSE_SCREENSHOTS to an output directory');

  const states: readonly (readonly [string, LicenseResponse])[] = [
    ['community', COMMUNITY],
    ['active', ACTIVE],
    ['lapsed', LAPSED],
  ];

  for (const viewport of WIDTHS) {
    for (const scheme of SCHEMES) {
      test(`lays the license states out at ${viewport.width}px in ${scheme}`, async ({ page }) => {
        await page.setViewportSize(viewport);
        await page.emulateMedia({ colorScheme: scheme });
        await openLicense(page);
        for (const [name, response] of states) {
          await page.unroute(LICENSE_API);
          await serveLicense(page, response);
          await page.goto(LICENSE_PATH, { waitUntil: 'domcontentloaded' });
          await expect(page.getByRole('heading', { name: 'License', level: 2 })).toBeVisible();
          await expect(page.locator('.license-summary')).toBeVisible();
          await shoot(page, `license-${name}-${scheme}-${viewport.width}`);
        }
      });
    }
  }
});
