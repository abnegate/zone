import { execFileSync } from 'node:child_process';
import type { Page } from '@playwright/test';
import { enabled, expect, record, shot, signIn, test } from './rig';

/**
 * Rows 14 to 16: the Models page against the host Ollama. Installs are real
 * pulls (a small Ollama model and a small GGUF from HuggingFace), the delete is
 * checked against Ollama's own tag list, and the outage is a real one: the
 * Ollama app is quit and reopened.
 */

const ollama = process.env.OLLAMA_HOST ?? 'http://127.0.0.1:11434';
const SMALL = 'qwen2.5:0.5b';
const GGUF = 'hf.co/Qwen/Qwen2.5-0.5B-Instruct-GGUF:Q4_K_M';

async function installed(): Promise<string[]> {
  const response = await fetch(`${ollama}/api/tags`).catch(() => null);
  if (!response || !response.ok) return [];
  const body = (await response.json()) as { models?: { name: string }[] };
  return (body.models ?? []).map((m) => m.name);
}

async function installFromAddModel(
  page: Page,
  name: string,
  screenshot: string,
): Promise<string> {
  const panel = page.locator('.models-install-panel');
  await panel.locator('input[placeholder="Model name..."]').fill(name);
  await panel.getByRole('button', { name: 'Install' }).click();
  const job = page.locator('.pull-job', { hasText: name });
  await expect(job).toBeVisible({ timeout: 30_000 });
  await expect
    .poll(
      async () => (await job.innerText().catch(() => '')).replace(/\s+/g, ' '),
      {
        timeout: 1_500_000,
        intervals: [2_000],
      },
    )
    .toMatch(/Installation complete|Installation failed/);
  const text = (await job.innerText()).replace(/\s+/g, ' ');
  await shot(page, screenshot);
  return text;
}

test.describe('models page', () => {
  test.skip(!enabled, 'set ZONE_LIVE_REAL_PASS=1 against the real rig');
  test.describe.configure({ timeout: 3_600_000 });

  test('14 and 15: installed models are listed with their details, models install from the catalog and HuggingFace, and a delete reaches Ollama', async ({
    page,
  }) => {
    await signIn(page);
    await page.goto('/models');
    const rows = page.locator('.model-item');
    await expect(rows.first()).toBeVisible({ timeout: 60_000 });
    const before = await installed();
    const listed = await rows.count();
    const firstMeta = await rows.first().locator('.model-meta').innerText();
    const firstCapabilities = await rows
      .first()
      .locator('.model-capabilities')
      .innerText()
      .catch(() => '');
    const disk = page.locator(
      '[role="progressbar"][aria-label="Disk space used"]',
    );
    const diskShown = await disk.isVisible().catch(() => false);
    const diskValue = diskShown
      ? await disk.getAttribute('aria-valuenow')
      : null;
    const diskTitle = diskShown
      ? await page.locator('.models-disk').getAttribute('title')
      : null;
    await shot(page, '14-installed-models');

    const agentRow = rows
      .filter({ hasText: process.env.ZONE_LIVE_AGENT_MODEL ?? 'qwen3.8:27b' })
      .first();
    await agentRow.click();
    const details = page.locator('.modal-details');
    await expect(details).toBeVisible();
    const detailText = (await details.innerText()).replace(/\s+/g, ' ');
    await shot(page, '14-model-details');
    await details.locator('.modal-close').click();

    // Sort lives on the Browse tab; the Installed tab has no sort control.
    await page.getByRole('tab', { name: 'Browse' }).click();
    const sort = page.getByLabel('Sort models');
    await expect(sort).toBeVisible();
    await page
      .locator('[role="group"][aria-label="Filter by medium"]')
      .getByRole('button', { name: 'Text' })
      .click();
    await page
      .locator('[role="group"][aria-label="Filter by family"]')
      .getByRole('button', { name: 'Qwen' })
      .click();
    await page
      .locator('[role="group"][aria-label="Filter by size"]')
      .getByRole('button', { name: '≤3B' })
      .click();
    await sort.selectOption('size_asc');
    await page.locator('input[placeholder="Search models..."]').fill('qwen2.5');
    await page.getByRole('button', { name: 'Search', exact: true }).click();
    const browseRows = page.locator('.browse-item');
    await expect(browseRows.first()).toBeVisible({ timeout: 60_000 });
    const browseNames = await browseRows
      .locator('.browse-name')
      .allInnerTexts();
    await shot(page, '15-browse-filtered');

    // Install from the catalog: the qwen2.5 family opens its download options,
    // and the smallest is picked.
    const target = browseRows
      .filter({ has: page.locator('.browse-name', { hasText: /^qwen2\.5$/ }) })
      .first();
    let catalogInstall = '';
    if (await target.count()) {
      await target.getByRole('button', { name: 'Install' }).click();
      const chip = page
        .locator('.details-download-chip[title="qwen2.5:0.5b"]')
        .first();
      if (await chip.isVisible().catch(() => false)) {
        await shot(page, '15-download-options');
        await chip.click();
      } else if (
        await page
          .locator('.modal-details')
          .isVisible()
          .catch(() => false)
      ) {
        await page
          .locator('.modal-details')
          .getByRole('button', { name: 'Install Model' })
          .click();
      }
      const job = page.locator('.pull-job', { hasText: 'qwen2.5' }).first();
      await expect(job).toBeVisible({ timeout: 60_000 });
      await expect
        .poll(
          async () =>
            (await job.innerText().catch(() => '')).replace(/\s+/g, ' '),
          {
            timeout: 1_500_000,
            intervals: [2_000],
          },
        )
        .toMatch(/Installation complete|Installation failed/);
      catalogInstall = (await job.innerText()).replace(/\s+/g, ' ');
      await shot(page, '15-catalog-install-complete');
    } else {
      catalogInstall = `catalog rows: ${browseNames.join(', ')}`;
    }
    await page.getByRole('tab', { name: /^Installed/ }).click();
    if (!(await installed()).includes(SMALL)) {
      catalogInstall = `${catalogInstall} | fallback Add Model form: ${await installFromAddModel(page, SMALL, '15-add-model-install-complete')}`;
    }
    const afterSmall = await installed();

    const huggingFace = await installFromAddModel(
      page,
      GGUF,
      '15-huggingface-install-complete',
    );
    const afterGguf = await installed();

    // Delete the small catalog model from the Installed list.
    await page.getByRole('button', { name: 'Refresh' }).click();
    const smallRow = page.locator('.model-item', { hasText: SMALL }).first();
    await expect(smallRow).toBeVisible({ timeout: 60_000 });
    await smallRow.locator('button[title="Delete model"]').click();
    const confirm = page.getByRole('dialog');
    await expect(confirm).toContainText('Delete Model');
    await shot(page, '14-delete-confirm');
    await confirm.getByRole('button', { name: 'Delete', exact: true }).click();
    await expect(page.locator('.model-item', { hasText: SMALL })).toHaveCount(
      0,
      { timeout: 60_000 },
    );
    await shot(page, '14-after-delete');
    const afterDelete = await installed();

    record(14, {
      result:
        listed >= 1 &&
        /Size/.test(detailText) &&
        diskShown &&
        !afterDelete.includes(SMALL)
          ? 'WORKS'
          : 'FAILS',
      installed_rows: listed,
      first_row_meta: firstMeta,
      first_row_capabilities: firstCapabilities,
      disk_meter_percent: diskValue,
      disk_meter_title: diskTitle,
      details_text: detailText.slice(0, 300),
      sort_control: 'Browse tab only; the Installed tab has no sort control',
      deleted: SMALL,
      ollama_tags_after_delete_has_small: afterDelete.includes(SMALL),
      screenshots: [
        '14-installed-models.png',
        '14-model-details.png',
        '14-delete-confirm.png',
        '14-after-delete.png',
      ],
    });
    record(15, {
      result:
        afterSmall.includes(SMALL) &&
        afterGguf.some((n) =>
          n.startsWith('hf.co/Qwen/Qwen2.5-0.5B-Instruct-GGUF'),
        )
          ? 'WORKS'
          : 'FAILS',
      browse_rows_after_filters_and_search: browseNames.slice(0, 10),
      catalog_install: catalogInstall.slice(0, 300),
      huggingface_install: huggingFace.slice(0, 300),
      ollama_tags_before: before.length,
      ollama_tags_after_small: afterSmall.includes(SMALL),
      ollama_tags_after_gguf: afterGguf.filter((n) =>
        n.startsWith('hf.co/Qwen'),
      ),
      screenshots: [
        '15-browse-filtered.png',
        '15-download-options.png',
        '15-catalog-install-complete.png',
        '15-huggingface-install-complete.png',
      ],
    });
    expect(afterSmall).toContain(SMALL);
    expect(
      afterGguf.some((n) =>
        n.startsWith('hf.co/Qwen/Qwen2.5-0.5B-Instruct-GGUF'),
      ),
    ).toBe(true);
    expect(afterDelete).not.toContain(SMALL);
  });

  test('16: stopping Ollama shows the connection error, and the page recovers when it is back', async ({
    page,
  }) => {
    await signIn(page);
    await page.goto('/models');
    await expect(page.locator('.model-item').first()).toBeVisible({
      timeout: 60_000,
    });

    execFileSync('sh', [process.env.ZONE_PASS_OLLAMA_CTL ?? '', 'stop']);
    await expect
      .poll(async () => (await installed()).length, { timeout: 60_000 })
      .toBe(0);
    const stoppedResponse = page
      .waitForResponse(
        (r) =>
          r.url().includes('/api/models') && r.request().method() === 'GET',
        { timeout: 60_000 },
      )
      .then(
        async (r) =>
          `${r.status()} ${(await r.text().catch(() => '')).replace(/\s+/g, ' ').slice(0, 240)}`,
      )
      .catch(() => 'no request seen');
    await page.getByRole('button', { name: 'Refresh' }).click();
    const modelsApiWhileStopped = await stoppedResponse;
    await page.waitForTimeout(3_000);
    const errorShown = await page
      .getByText('Cannot connect to Ollama')
      .isVisible()
      .catch(() => false);
    const rowsWhileStopped = await page.locator('.model-item').count();
    const mainText = (await page.locator('main').innerText()).replace(
      /\s+/g,
      ' ',
    );
    await shot(page, '16-ollama-stopped');

    execFileSync('sh', [process.env.ZONE_PASS_OLLAMA_CTL ?? '', 'start']);
    await expect
      .poll(async () => (await installed()).length, { timeout: 120_000 })
      .toBeGreaterThan(0);
    const retry = page.getByRole('button', { name: 'Retry' });
    if (await retry.count()) await retry.click();
    else await page.getByRole('button', { name: 'Refresh' }).click();
    await expect(page.locator('.model-item').first()).toBeVisible({
      timeout: 60_000,
    });
    await shot(page, '16-ollama-recovered');
    record(16, {
      result: errorShown ? 'WORKS' : 'FAILS',
      cause: errorShown
        ? undefined
        : `product: with Ollama stopped (its /api/tags refused), Refresh on the Models page showed no error state; GET /api/models answered ${modelsApiWhileStopped.slice(0, 120)} and the page kept ${rowsWhileStopped} rows`,
      models_api_while_stopped: modelsApiWhileStopped,
      rows_while_stopped: rowsWhileStopped,
      page_text_while_stopped: mainText.slice(0, 300),
      recovery: errorShown
        ? 'Retry button after Ollama came back; the page does not poll on its own'
        : 'Refresh after Ollama came back',
      screenshots: ['16-ollama-stopped.png', '16-ollama-recovered.png'],
    });
    expect(errorShown, mainText.slice(0, 200)).toBe(true);
  });
});
