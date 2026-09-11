import { createHash } from 'node:crypto';
import { expect, signIn, state, test, tokenFor } from './harness';
import { createChat } from './harness';

/**
 * The media lanes against real ComfyUI and real weights, driven from the
 * console. No stand-in: what is asserted here is the model's own output.
 *
 * Run with `ZONE_LIVE_REAL_MODELS=1` and a ComfyUI holding the weights the
 * lane needs; skipped otherwise, because a lane with no weights installed has
 * nothing to say.
 */

const real = process.env.ZONE_LIVE_REAL_MODELS === '1';

/** PNG dimensions straight out of the IHDR, so nothing decodes the image for us. */
function pngSize(bytes: Buffer): { width: number; height: number } {
  expect(bytes.subarray(1, 4).toString('ascii'), 'not a PNG').toBe('PNG');
  return { width: bytes.readUInt32BE(16), height: bytes.readUInt32BE(20) };
}

async function fetchAttachment(
  page: import('@playwright/test').Page,
  selector: string,
  index: number
): Promise<Buffer> {
  const source = await page.locator(selector).nth(index).getAttribute('src');
  expect(source, 'the media element resolved no source').toBeTruthy();
  // A typed array crosses the evaluate boundary as one buffer; a plain array
  // is serialised element by element, and a 4096-square upscale is 24 MB.
  const bytes = await page.evaluate(async (url) => {
    const response = await fetch(url as string);
    if (!response.ok) throw new Error(`artifact fetch failed: ${response.status}`);
    return new Uint8Array(await response.arrayBuffer());
  }, source);
  return Buffer.from(bytes);
}

async function openChat(page: import('@playwright/test').Page, title: string) {
  const token = await tokenFor(state.owner);
  const chatId = await createChat(token, { title });
  await signIn(page);
  await page.goto(`/chats?id=${chatId}`);
  await expect(page.getByPlaceholder(/type a message/i)).toBeVisible();
  return chatId;
}

/** A real diffusion pass takes minutes on Apple Silicon, so the wait is long. */
async function send(page: import('@playwright/test').Page, message: string) {
  const assistant = page.locator('.message-assistant');
  const before = await assistant.count();
  const box = page.getByPlaceholder(/type a message/i);
  await box.fill(message);
  await box.press('Enter');
  await expect(page.locator('.message-user').filter({ hasText: message })).toBeVisible();
  // A turn that fails renders an alert and no assistant message, so waiting on
  // the reply alone runs the whole media timeout before the alert is read.
  const failures = page.getByRole('alert');
  const failure = failures.nth(await failures.count());
  await expect(assistant.nth(before).or(failure).first()).toBeVisible({ timeout: 1_500_000 });
  if (await failure.count()) {
    expect(await failure.innerText(), 'the turn reported a failure').toBe('');
  }
  // `.message-status` is absent until `message_start`, so waiting for it to
  // reach zero returns before the turn begins and the long timeout below never
  // applies -- the media assertions would then run on the 30s default.
  await expect(page.locator('.message-status')).toHaveCount(0, { timeout: 1_500_000 });
}

test.describe('real models', () => {
  test.skip(!real, 'set ZONE_LIVE_REAL_MODELS=1 with the weights installed');
  test.describe.configure({ timeout: 3_600_000 });

  test('an image is generated, then genuinely upscaled four times', async ({
    page,
    consoleErrors,
  }) => {
    await openChat(page, 'real image and upscale');

    await send(page, 'draw a picture of a red bicycle leaning on a brick wall');
    await expect(page.locator('[data-testid="message-image"]')).toHaveCount(1);
    const generated = await fetchAttachment(page, '[data-testid="message-image"]', 0);
    const source = pngSize(generated);
    expect(source.width).toBeGreaterThanOrEqual(512);
    expect(source.height).toBeGreaterThanOrEqual(512);

    await send(page, 'upscale this');
    await expect(page.locator('[data-testid="message-image"]')).toHaveCount(2);
    const upscaled = await fetchAttachment(page, '[data-testid="message-image"]', 1);
    const result = pngSize(upscaled);

    // Real-ESRGAN x4plus, so four times the source on both axes.
    expect(result.width).toBe(source.width * 4);
    expect(result.height).toBe(source.height * 4);
    // And a different image, not the source re-served.
    expect(createHash('sha256').update(upscaled).digest('hex')).not.toBe(
      createHash('sha256').update(generated).digest('hex')
    );
    expect(upscaled.byteLength).toBeGreaterThan(generated.byteLength);
    await expect(page.locator('.message-assistant').last()).toContainText(/upscaled/i);
    expect(consoleErrors).toEqual([]);
  });

  test('an audio request comes back as a playable clip', async ({ page, consoleErrors }) => {
    await openChat(page, 'real audio');

    await send(page, 'make a background audio track that sounds like shuffling through a forest');

    const audio = page.locator('[data-testid="message-audio"]');
    await expect(audio).toHaveCount(1);
    const clip = await fetchAttachment(page, '[data-testid="message-audio"]', 0);
    // ACE-Step writes FLAC, and a real clip is not a handful of bytes.
    expect(clip.subarray(0, 4).toString('ascii')).toBe('fLaC');
    expect(clip.byteLength).toBeGreaterThan(100_000);
    expect(consoleErrors).toEqual([]);
  });

  test('a clip is generated and then genuinely upscaled', async ({ page, consoleErrors }) => {
    await openChat(page, 'real video');

    await send(page, 'make a video of a sunset over the ocean');
    const videos = page.locator('[data-testid="message-video"]');
    await expect(videos).toHaveCount(1);
    const generated = await fetchAttachment(page, '[data-testid="message-video"]', 0);
    expect(generated.byteLength).toBeGreaterThan(10_000);

    await send(page, 'upscale the video');
    await expect(videos).toHaveCount(2);
    const upscaled = await fetchAttachment(page, '[data-testid="message-video"]', 1);
    expect(upscaled.byteLength).toBeGreaterThan(generated.byteLength);
    expect(createHash('sha256').update(upscaled).digest('hex')).not.toBe(
      createHash('sha256').update(generated).digest('hex')
    );
    expect(consoleErrors).toEqual([]);
  });
});
