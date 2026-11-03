import type { Locator, Page, Request, Route } from '@playwright/test';
import { test, expect } from './fixtures';
import { mockCommonEndpoints, setupAuth } from './helpers/auth';
import { MOCK_PNG_BYTES } from './helpers/chatSocket';
import { blockServiceWorker, routeApi } from './test-utils';

const TILE_SIZE = 72;
const TRAIN_URL = /\/api\/models\/train(\?.*)?$/;
const PREVIEW_URL = /\/api\/models\/train\/previews\//;
const LOADED_PREVIEW = 'step-250-0.png';
const SECOND_LOADED_PREVIEW = 'step-250-1.png';
const FAILED_PREVIEW = 'step-500-0.png';

const runningJob = {
  id: 'job-1',
  name: 'jerry',
  status: 'running',
  step: 500,
  total: 8000,
  loss: 0.0812,
  eta_seconds: 1800,
  previews: [
    `previews/${LOADED_PREVIEW}`,
    `previews/${SECOND_LOADED_PREVIEW}`,
    `previews/${FAILED_PREVIEW}`,
  ],
};

function previewName(request: Request): string {
  return new URL(request.url()).pathname.split('/').pop() ?? '';
}

function previewTiles(page: Page): Locator {
  return page.getByRole('complementary', { name: 'Training' }).locator('.train-meter-previews > *');
}

async function expectTiles(tiles: Locator, count: number): Promise<void> {
  await expect(tiles).toHaveCount(count);
  for (const tile of await tiles.all()) {
    const box = await tile.boundingBox();
    expect(box, 'every preview tile is laid out').not.toBeNull();
    expect(box?.width, 'preview tile width').toBe(TILE_SIZE);
    expect(box?.height, 'preview tile height').toBe(TILE_SIZE);
  }
}

test.describe('Training previews', () => {
  test('load with the session token in 72px tiles', async ({ context, page }) => {
    const held: Route[] = [];
    const previewRequests: Request[] = [];
    page.on('request', (request) => {
      if (PREVIEW_URL.test(request.url())) previewRequests.push(request);
    });

    await blockServiceWorker(context);
    await mockCommonEndpoints(page);
    await routeApi(page, TRAIN_URL, (route) =>
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(runningJob),
      })
    );
    await routeApi(page, PREVIEW_URL, (route) => {
      held.push(route);
    });

    await setupAuth(page);
    await page.goto('/');

    const dock = page.getByRole('complementary', { name: 'Training' });
    await expect(dock).toBeVisible({ timeout: 10000 });
    await expect
      .poll(() => new Set(held.map((route) => previewName(route.request()))).size)
      .toBe(runningJob.previews.length);

    const tiles = previewTiles(page);
    await expect(dock.getByRole('status', { name: 'Loading image' })).toHaveCount(
      runningJob.previews.length
    );
    await expect(dock.getByRole('img')).toHaveCount(0);
    await expectTiles(tiles, runningJob.previews.length);

    const accessToken = await page.evaluate(() => localStorage.getItem('manager_access_token'));
    expect(accessToken).toBeTruthy();
    for (const route of held) {
      expect(route.request().headers().authorization).toBe(`Bearer ${accessToken}`);
    }
    expect(
      previewRequests.filter((request) => request.resourceType() === 'image'),
      'no preview is requested by a plain <img> without the token'
    ).toHaveLength(0);

    for (const route of held) {
      if (previewName(route.request()) === FAILED_PREVIEW) {
        await route.fulfill({ status: 404, body: 'missing' });
      } else {
        await route.fulfill({ status: 200, contentType: 'image/png', body: MOCK_PNG_BYTES });
      }
    }

    await expect(dock.getByRole('img')).toHaveCount(2);
    await expect(dock.getByRole('img', { name: 'Training preview 1 of 3' })).toHaveAttribute(
      'src',
      /^blob:/
    );
    await expect(dock.getByRole('img', { name: 'Training preview 2 of 3' })).toHaveAttribute(
      'src',
      /^blob:/
    );
    await expect(dock.getByRole('alert', { name: 'Image unavailable' })).toHaveCount(1);
    await expect(dock.getByRole('status', { name: 'Loading image' })).toHaveCount(0);
    await expectTiles(tiles, runningJob.previews.length);
  });
});
