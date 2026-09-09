import { expect, signIn, test } from './harness';

/**
 * The whole LoRA chain, from the console, against real weights: a clip of a
 * real subject becomes frames, the frames are screened and cropped on the
 * subject, the trainer runs on FLUX Schnell, the winning checkpoint is probed
 * against its base, and the console reports what it measured.
 *
 * This is the check that says an adapter learned something. It needs the FLUX
 * weights and takes tens of minutes, so it runs only with
 * ZONE_LIVE_REAL_MODELS=1 and ZONE_TRAIN_CLIP pointing at a subject clip.
 */

const real = process.env.ZONE_LIVE_REAL_MODELS === '1';
const clip = process.env.ZONE_TRAIN_CLIP;

test.describe('real LoRA training', () => {
  test.skip(!real || !clip, 'set ZONE_LIVE_REAL_MODELS=1 and ZONE_TRAIN_CLIP');
  test.describe.configure({ timeout: 7_200_000 });

  test('a clip trains an adapter that beats its base', async ({ page, consoleErrors }) => {
    await signIn(page);
    await page.goto('/models');
    await page.getByRole('tab', { name: 'Train' }).click();
    await expect(page.getByRole('heading', { name: /train a lora/i })).toBeVisible();

    // The Train tab turns the clip into frames server-side: ffmpeg samples
    // above the kept rate, repeats are dropped, and each frame is cropped
    // square on whatever U2-Net finds.
    await page.getByLabel('Video', { exact: true }).setInputFiles(clip as string);
    const receipt = page.getByText(/\d+ frames read at [\d.]+\/s, \d+ kept/);
    await expect(receipt).toBeVisible({ timeout: 600_000 });
    const sampling = await receipt.innerText();

    const frames = page.locator('.train-pair');
    const kept = await frames.count();
    expect(kept, `no frames survived: ${sampling}`).toBeGreaterThan(0);

    const name = `live-zrkxyz-${Date.now()}`;
    await page.getByLabel('Name').fill(name);
    await page.getByLabel('Trigger word').fill('zrkxyz');

    // Caption them the way a user would, so the trigger carries the identity.
    for (let index = 0; index < kept; index += 1) {
      await frames
        .nth(index)
        .getByLabel(/^Caption for /)
        .fill('zrkxyz, in a bright white photo studio, full body, facing camera');
    }

    await page.getByRole('button', { name: 'Train', exact: true }).click();

    // A failed run answers with the error banner and never renders a result, so
    // waiting only on the result burns the whole timeout on a run that is
    // already over. `expect.poll` retries until it matches, so the poll ends on
    // *either* terminal state and the failure is reported after it, not from
    // inside it -- returning a failure string from the callback would just keep
    // retrying for the full timeout.
    const result = page.locator('.train-result');
    const failure = page.locator('.error-placeholder');
    let reason = '';
    await expect
      .poll(
        async () => {
          if (await result.count()) return 'finished';
          if (await failure.count()) {
            reason = await failure.first().innerText();
            return 'failed';
          }
          return 'running';
        },
        { timeout: 7_000_000, intervals: [5_000] }
      )
      .not.toBe('running');
    expect(reason, 'training reported an error instead of a result').toBe('');
    await expect(result).toContainText(/Training finished/);

    const quality = page.locator('.train-quality');
    const reported = await quality.innerText();

    // A run that could not be probed says so rather than showing a band, and
    // that is a real outcome — but it is not evidence the adapter learned.
    expect(
      reported,
      `probing did not run, so nothing was measured: ${reported}`
    ).not.toContain('Not measured');

    // 15% is what an adapter that learned nothing scores, so a healthy run has
    // to clear it. The band label is the console's own reading of the number.
    // The sign matters: `-5% better` would otherwise read as 5.
    const improvement = reported.match(/(-?\d+)% better/);
    expect(improvement, `no improvement reported: ${reported}`).not.toBeNull();
    expect(Number(improvement?.[1]), `reported: ${reported}`).toBeGreaterThan(15);
    expect(reported).not.toContain('No measurable learning');

    console.log(`sampling: ${sampling}`);
    console.log(`frames trained: ${kept}`);
    console.log(`quality: ${reported.replace(/\n+/g, ' | ')}`);
    expect(consoleErrors).toEqual([]);
  });
});
