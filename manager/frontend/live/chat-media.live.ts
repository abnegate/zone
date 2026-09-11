import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import {
  createChat,
  expect,
  resetStub,
  sendAndSettle,
  signIn,
  state,
  stubState,
  test,
  tokenFor,
} from './harness';

/**
 * The chat media lanes, driven in the console against the real server.
 *
 * Each lane is checked against the exact bytes the ComfyUI stand-in holds for
 * it, because "an attachment arrived" is what passed all the way through the
 * defect PR #44 fixed: a video upscale that collected `LoadVideo`'s preview of
 * its own input would still have produced an attachment.
 */

const fixtures = process.env.ZONE_COMFY_FIXTURES;

function digestOf(name: string): string {
  if (!fixtures) throw new Error('ZONE_COMFY_FIXTURES must point at the stand-in fixtures');
  return createHash('sha256').update(readFileSync(join(fixtures, name))).digest('hex');
}

async function openChat(page: import('@playwright/test').Page, title: string) {
  const token = await tokenFor(state.owner);
  const chatId = await createChat(token, { title });
  await signIn(page);
  await page.goto(`/chats?id=${chatId}`);
  await expect(page.getByPlaceholder(/type a message/i)).toBeVisible();
  return { chatId, token };
}

/** Fetch an attachment the way the console does, and hash what came back. */
async function attachmentDigest(
  page: import('@playwright/test').Page,
  selector: string,
  index = 0
): Promise<string> {
  const source = await page.locator(selector).nth(index).getAttribute('src');
  expect(source, 'the media element must have resolved a source').toBeTruthy();
  // A typed array crosses the evaluate boundary as one buffer; a plain array
  // is serialised element by element.
  const bytes = await page.evaluate(async (url) => {
    const response = await fetch(url as string);
    if (!response.ok) throw new Error(`artifact fetch failed: ${response.status}`);
    return new Uint8Array(await response.arrayBuffer());
  }, source);
  return createHash('sha256').update(Buffer.from(bytes)).digest('hex');
}

test.beforeEach(async () => {
  await resetStub();
});

test('a soundscape request plays back in the thread', async ({ page, consoleErrors }) => {
  await openChat(page, 'live audio');

  await sendAndSettle(
    page,
    'make a background audio track that sounds like shuffling through a forest'
  );

  const audio = page.locator('[data-testid="message-audio"]');
  await expect(audio).toHaveCount(1);
  await expect(audio).toHaveAttribute('controls', '');
  expect(await attachmentDigest(page, '[data-testid="message-audio"]')).toBe(
    digestOf('audio.flac')
  );
  expect(consoleErrors).toEqual([]);
});

test('a drawing request renders in the thread', async ({ page, consoleErrors }) => {
  await openChat(page, 'live image');

  await sendAndSettle(page, 'draw a picture of a red bicycle in the rain');

  await expect(page.locator('[data-testid="message-image"]')).toHaveCount(1);
  expect(await attachmentDigest(page, '[data-testid="message-image"]')).toBe(
    digestOf('image.png')
  );
  expect(consoleErrors).toEqual([]);
});

test('upscaling points at the image already in the thread', async ({ page, consoleErrors }) => {
  await openChat(page, 'live upscale');

  await sendAndSettle(page, 'draw a picture of a red bicycle in the rain');
  await expect(page.locator('[data-testid="message-image"]')).toHaveCount(1);

  await sendAndSettle(page, 'upscale this');

  const images = page.locator('[data-testid="message-image"]');
  await expect(images).toHaveCount(2);
  expect(await attachmentDigest(page, '[data-testid="message-image"]', 1)).toBe(
    digestOf('upscaled.png')
  );
  await expect(page.locator('.message-assistant').last()).toContainText(/upscaled/i);

  // The source has to reach ComfyUI for an upscale to have anything to enlarge.
  const stub = await stubState();
  expect(stub.uploads.some((name) => name.endsWith('.png'))).toBe(true);
  expect(consoleErrors).toEqual([]);
});

test('upscaling with nothing to point at says so', async ({ page }) => {
  await openChat(page, 'live upscale refusal');

  const box = page.getByPlaceholder(/type a message/i);
  await box.fill('upscale this');
  await box.press('Enter');

  await expect(page.getByRole('alert')).toContainText(/needs an image or video/i);
  await expect(page.locator('[data-testid="message-image"]')).toHaveCount(0);
  expect((await stubState()).prompts).toHaveLength(0);
});

test('a clip generates and then upscales from its own output node', async ({
  page,
  consoleErrors,
}) => {
  await openChat(page, 'live video');

  await sendAndSettle(page, 'make a video of a sunset over the ocean');
  const videos = page.locator('[data-testid="message-video"]');
  await expect(videos).toHaveCount(1);
  expect(await attachmentDigest(page, '[data-testid="message-video"]')).toBe(
    digestOf('video.webm')
  );

  await sendAndSettle(page, 'upscale the video');

  await expect(videos).toHaveCount(2);
  // The upscaled clip, not the uploaded source LoadVideo previews.
  expect(await attachmentDigest(page, '[data-testid="message-video"]', 1)).toBe(
    digestOf('upscaled.webm')
  );
  expect((await stubState()).uploads.some((name) => name.endsWith('.webm'))).toBe(true);
  expect(consoleErrors).toEqual([]);
});
