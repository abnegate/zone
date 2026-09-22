import { execFileSync } from 'node:child_process';
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import type { Page } from '@playwright/test';
import {
  ask,
  enabled,
  evidenceDir,
  expect,
  model,
  newChat,
  record,
  shot,
  signIn,
  sql,
  test,
} from './rig';

/**
 * Rows 17 to 19: the Train tab driven by hand against the real FLUX weights
 * and the real vision model, then the adapter used from chat. The ComfyUI
 * history says which LoRA the server put in the prompt it submitted.
 */

const clip = process.env.ZONE_TRAIN_CLIP ?? '';
const comfy = process.env.ZONE_LIVE_COMFYUI_URL ?? 'http://127.0.0.1:8188';

async function openTrainTab(page: Page): Promise<void> {
  await page.goto('/models');
  await page.getByRole('tab', { name: 'Train' }).click();
  await expect(
    page.getByRole('heading', { name: /train a lora/i }),
  ).toBeVisible();
}

test.describe('training', () => {
  test.skip(!enabled || !clip, 'set ZONE_LIVE_REAL_PASS=1 and ZONE_TRAIN_CLIP');
  test.describe.configure({ timeout: 3 * 3_600_000 });

  test('17 and 18: a clip becomes frames, captions come from the vision model, and the adapter beats its base', async ({
    page,
  }) => {
    await signIn(page);
    await openTrainTab(page);
    const base = page.getByLabel('Base');
    await expect(base).toBeVisible();
    const baseText = await base.innerText();
    await shot(page, '17-train-tab');

    await page.getByLabel('Video', { exact: true }).setInputFiles(clip);
    const receipt = page.getByText(/\d+ frames read at [\d.]+\/s, \d+ kept/);
    await expect(receipt).toBeVisible({ timeout: 600_000 });
    const sampling = await receipt.innerText();
    const pairs = page.locator('.train-pair');
    const kept = await pairs.count();
    const mirrored = await page
      .locator('.train-pair-filename', { hasText: '(mirrored)' })
      .count();
    await shot(page, '17-frames-with-mirrors');

    // Mirror off, re-dropped on a fresh tab: no mirrored frames.
    await openTrainTab(page);
    const mirror = page.getByLabel(/mirror half the frames/i);
    await mirror.click();
    await expect(mirror).not.toBeChecked();
    await page.getByLabel('Video', { exact: true }).setInputFiles(clip);
    await expect(page.getByText(/frames read at/)).toBeVisible({
      timeout: 600_000,
    });
    const mirroredOff = await page
      .locator('.train-pair-filename', { hasText: '(mirrored)' })
      .count();
    const keptOff = await page.locator('.train-pair').count();
    await shot(page, '17-frames-mirror-off');

    // Back to the default (mirror on) for the run that trains.
    await openTrainTab(page);
    await page.getByLabel('Video', { exact: true }).setInputFiles(clip);
    await expect(page.getByText(/frames read at/)).toBeVisible({
      timeout: 600_000,
    });
    const framesToTrain = await page.locator('.train-pair').count();
    await page.getByRole('button', { name: 'Auto-caption images' }).click();
    const captions = page.locator('input[id^="train-caption-"]');
    await expect
      .poll(
        async () =>
          (
            await captions.evaluateAll((els) =>
              els.map((e) => (e as HTMLInputElement).value),
            )
          ).filter((v) => v.trim().length > 0).length,
        {
          timeout: 1_200_000,
          intervals: [5_000],
        },
      )
      .toBe(framesToTrain);
    const captionValues = await captions.evaluateAll((els) =>
      els.map((e) => (e as HTMLInputElement).value),
    );
    await shot(page, '17-captions-from-vision-model');

    const name = `live-pass-zrkxyz-${Date.now()}`;
    await page.getByLabel('Name').fill(name);
    await page.getByLabel('Trigger word').fill('zrkxyz');
    await shot(page, '17-ready-to-train');
    const startedAt = Date.now();
    await page.getByRole('button', { name: 'Train', exact: true }).click();
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
        { timeout: 2.5 * 3_600_000, intervals: [10_000] },
      )
      .not.toBe('running');
    const minutes = Math.round((Date.now() - startedAt) / 60_000);
    await shot(page, '18-training-result');
    const resultText = reason
      ? ''
      : (await result.innerText()).replace(/\s+/g, ' ');
    const screening = reason
      ? ''
      : (
          await page
            .locator('.train-screening')
            .innerText()
            .catch(() => '')
        ).replace(/\s+/g, ' ');
    const quality = reason
      ? ''
      : (
          await page
            .locator('.train-quality')
            .innerText()
            .catch(() => '')
        ).replace(/\s+/g, ' ');
    const improvement = Number(quality.match(/(-?\d+)% better/)?.[1] ?? NaN);
    const adapterFile = sql(`select 1`).length ? name : name;
    writeFileSync(join(evidenceDir, 'adapter-name.txt'), `${name}\n`);
    record(17, {
      result:
        /FLUX\.1 Schnell/.test(baseText) &&
        kept > 0 &&
        mirrored > 0 &&
        mirroredOff === 0 &&
        captionValues.every((c) => c.trim().length > 0)
          ? 'WORKS'
          : 'FAILS',
      base: baseText,
      sampling_receipt: sampling,
      frames_kept: kept,
      mirrored_frames: mirrored,
      frames_kept_mirror_off: keptOff,
      mirrored_frames_mirror_off: mirroredOff,
      captions: captionValues.slice(0, 4),
      clip,
      screenshots: [
        '17-train-tab.png',
        '17-frames-with-mirrors.png',
        '17-frames-mirror-off.png',
        '17-captions-from-vision-model.png',
        '17-ready-to-train.png',
      ],
    });
    record(18, {
      result:
        !reason &&
        /Training finished/.test(resultText) &&
        improvement > 15 &&
        !/No measurable learning/.test(quality) &&
        /Your originals are untouched/.test(screening)
          ? 'WORKS'
          : 'FAILS',
      cause: reason
        ? `training reported: ${reason.slice(0, 200)}`
        : improvement > 15
          ? undefined
          : `quality: ${quality.slice(0, 200)}`,
      adapter: adapterFile,
      training_minutes: minutes,
      result_text: resultText.slice(0, 200),
      screening: screening.slice(0, 400),
      quality,
      improvement_percent: improvement,
      screenshots: ['18-training-result.png'],
    });
    expect(reason, 'training reported an error').toBe('');
    expect(improvement).toBeGreaterThan(15);
  });

  test('19: the adapter is used from chat once it is the selected image model', async ({
    page,
  }) => {
    await signIn(page);
    const selected = execFileSync(
      'sh',
      [
        '-c',
        `ps -Eww -o command= -p $(lsof -nP -iTCP:${process.env.ZONE_LIVE_API_PORT ?? '8010'} -sTCP:LISTEN -t) | tr ' ' '\\n' | grep '^COMFYUI_CHECKPOINT=' || echo COMFYUI_CHECKPOINT=`,
      ],
      { encoding: 'utf8' },
    ).trim();
    const chatId = await newChat(page, { model });
    const seen: { id: string; loras: string[]; prompt: string }[] = [];
    let watching = true;
    const watcher = (async () => {
      while (watching) {
        const queue = (await fetch(`${comfy}/queue`)
          .then((r) => r.json())
          .catch(() => null)) as { queue_running?: unknown[][] } | null;
        for (const item of queue?.queue_running ?? []) {
          const id = String(item[1]);
          const graph = (item[2] ?? {}) as Record<
            string,
            { class_type: string; inputs: Record<string, unknown> }
          >;
          if (seen.some((s) => s.id === id)) continue;
          const loras = Object.values(graph)
            .filter((n) => /Lora/i.test(n.class_type))
            .map((n) => `${n.class_type}: ${String(n.inputs.lora_name ?? '')}`);
          const text = Object.values(graph).find(
            (n) =>
              n.class_type === 'CLIPTextEncode' &&
              typeof n.inputs.text === 'string',
          );
          seen.push({
            id,
            loras,
            prompt: String(text?.inputs.text ?? '').slice(0, 120),
          });
        }
        await new Promise((resolve) => setTimeout(resolve, 1_500));
      }
    })();
    const reply = await ask(
      page,
      'draw me zrkxyz standing in a snowy pine forest, full body, facing the camera',
      { replies: 1, timeout: 1_800_000 },
    );
    watching = false;
    await watcher;
    const images = page.locator('[data-testid="message-image"]');
    await expect(images).toHaveCount(1);
    const source = await images.first().getAttribute('src');
    const bytes = await page.evaluate(async (url) => {
      const response = await fetch(url as string);
      return new Uint8Array(await response.arrayBuffer());
    }, source);
    writeFileSync(
      join(evidenceDir, '19-adapter-image.png'),
      Buffer.from(bytes),
    );
    await shot(page, '19-adapter-image-in-chat');
    const render = seen.find((s) => /zrkxyz/.test(s.prompt)) ?? seen.at(-1);
    record(19, {
      result: render && render.loras.length > 0 ? 'WORKS' : 'FAILS',
      cause:
        render && render.loras.length > 0
          ? undefined
          : `the ComfyUI prompt the server submitted carried no LoRA loader (server ${selected})`,
      chat_id: chatId,
      reply: reply.slice(0, 120),
      server_selected_image_model: selected,
      comfy_prompts_seen_while_rendering: seen,
      note: 'Zone picks the image recipe from COMFYUI_CHECKPOINT; a trained adapter is used when that names the adapter file (its sidecar carries the flux-schnell-adapter recipe). There is no per-chat or trigger-word selection: with the base checkpoint selected the same prompt rendered a man in a red jacket (first run of this row)',
      resemblance: 'judged from 19-adapter-image.png in the report',
      screenshots: ['19-adapter-image-in-chat.png', '19-adapter-image.png'],
    });
    expect(render?.loras.length ?? 0).toBeGreaterThan(0);
  });
});
