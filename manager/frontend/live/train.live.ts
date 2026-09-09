import { readdirSync } from 'node:fs';
import { join } from 'node:path';
import { expect, signIn, test } from './harness';

/**
 * Models → Train, against the real server: ffmpeg samples the clip, zone_vision
 * crops each frame on its subject, and the screening that PR #45 added decides
 * what the trainer is allowed to see.
 *
 * What a trained adapter learns is not checked here and is not claimed: that
 * needs the real weights. Everything up to and including the receipt is real.
 */

const clips = process.env.ZONE_COMFY_FIXTURES;
const trainingSet = process.env.ZONE_TRAIN_FIXTURES;

function requireFixtures(): { clip: string; images: string[] } {
  if (!clips || !trainingSet) {
    throw new Error('ZONE_COMFY_FIXTURES and ZONE_TRAIN_FIXTURES must be set');
  }
  return {
    clip: join(clips, 'training.mp4'),
    images: readdirSync(trainingSet)
      .filter((name) => name.endsWith('.png'))
      .sort()
      .map((name) => join(trainingSet, name)),
  };
}

async function openTrainTab(page: import('@playwright/test').Page) {
  await signIn(page);
  await page.goto('/models');
  await page.getByRole('tab', { name: 'Train' }).click();
  await expect(page.getByRole('heading', { name: /train a lora/i })).toBeVisible();
}

test('the installed base is offered, read from the models directory', async ({ page }) => {
  await openTrainTab(page);

  const base = page.getByLabel('Base');
  await expect(base).toBeVisible();
  await expect(base).not.toHaveText(/no trainable base installed/i);
  await expect(page.locator('body')).toContainText(/FLUX\.1 Schnell/);
});

test('a clip becomes reviewable frames with its sampling receipt', async ({
  page,
  consoleErrors,
}) => {
  const { clip } = requireFixtures();
  await openTrainTab(page);

  await page.getByLabel('Video', { exact: true }).setInputFiles(clip);

  // Sampling runs ffmpeg on the server, so the receipt is the first real signal.
  await expect(page.getByText(/training\.mp4: \d+ frames read at [\d.]+\/s, \d+ kept/)).toBeVisible(
    { timeout: 180_000 }
  );

  const pairs = page.locator('.train-pair');
  await expect(pairs.first()).toBeVisible();
  const kept = await pairs.count();
  expect(kept).toBeGreaterThan(0);

  // A frame is captioned by the clip it came from, and a mirrored one says so.
  await expect(page.locator('.train-pair-filename').first()).toContainText('training.mp4');
  await expect(page.locator('.train-pair-filename', { hasText: '(mirrored)' }).first()).toBeVisible();
  expect(consoleErrors).toEqual([]);
});

test('turning the mirror off keeps every frame the right way round', async ({ page }) => {
  const { clip } = requireFixtures();
  await openTrainTab(page);

  await page.getByLabel(/mirror half the frames/i).click();
  await expect(page.getByLabel(/mirror half the frames/i)).not.toBeChecked();

  await page.getByLabel('Video', { exact: true }).setInputFiles(clip);
  await expect(page.getByText(/frames read at/)).toBeVisible({ timeout: 180_000 });

  await expect(page.locator('.train-pair-filename', { hasText: '(mirrored)' })).toHaveCount(0);
});

test('a finished run reports what it screened out and that it was not scored', async ({
  page,
  consoleErrors,
}) => {
  const { images } = requireFixtures();
  await openTrainTab(page);

  await page.getByLabel('Name').fill(`live-${Date.now()}`);
  await page.getByLabel('Trigger word').fill('zrkxyz');
  await page.getByLabel('Target images').setInputFiles(images);
  await expect(page.locator('.train-pair')).toHaveCount(images.length);

  await page.getByRole('button', { name: 'Train', exact: true }).click();

  const result = page.locator('.train-result');
  await expect(result).toBeVisible({ timeout: 240_000 });
  await expect(result).toContainText(/Training finished/);

  // Screening dropped the blurred, undersized and repeated images, and says so.
  const screening = page.locator('.train-screening');
  await expect(screening).toContainText(/Trained on \d+ of \d+ images/);
  await expect(screening).toContainText(/near-duplicate/);
  await expect(screening).toContainText(/blurred frame/);
  await expect(screening).toContainText(/Image repairs/);
  await expect(screening).toContainText(/Your originals are untouched/);

  // Probing is best effort and cannot fail a run, so an unprobed run says so
  // rather than showing a band it has not measured.
  await expect(page.locator('.train-quality')).toContainText(/Not measured/);
  expect(consoleErrors).toEqual([]);
});
