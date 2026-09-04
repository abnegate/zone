import { test, expect } from './fixtures';
import { blockServiceWorker, routeApi, setupAuth } from './test-utils';

for (const width of [1280, 390]) {
  test(`model capabilities remain visible at ${width}px`, async ({ context, page }, testInfo) => {
    await page.setViewportSize({ width, height: 1000 });
    await blockServiceWorker(context);
    await routeApi(page, '**/api/models**', async (route) => {
      const browsing = new URL(route.request().url()).searchParams.has('source');
      await route.fulfill({ json: { models: browsing ? [
        { name: 'multimodal', description: 'Declared input, generation and tool capabilities.', downloads: 12000,
          capabilities: ['text', 'image_input', 'audio_input', 'video_input', 'reasoning', 'tools', 'image_generation', 'audio_generation', 'video_generation', 'embeddings'],
          details: { parameter_size: '7B', context_length: 128000 } },
        { name: 'text-model', capabilities: ['text'] },
        { name: 'unknown-model', description: 'No provider capability metadata.' },
      ] : [], next_cursor: null } });
    });
    await setupAuth(page);
    await page.goto('/models');
    await page.getByRole('tab', { name: 'Browse', exact: true }).click();
    await page.getByRole('tab', { name: 'Ollama', exact: true }).click();
    const row = page.locator('.browse-item').filter({ hasText: 'multimodal' });
    await expect(row).toBeVisible();
    for (const label of ['Text', 'Image input', 'Image generation', 'Tools', 'Audio input', 'Video input', 'Reasoning', 'Embeddings']) {
      await expect(row.getByText(label, { exact: true })).toBeVisible();
    }
    await expect(page.getByText('Capabilities unknown', { exact: true })).toBeVisible();
    const bounds = await row.boundingBox();
    const next = await page.locator('.browse-item').filter({ hasText: 'text-model' }).boundingBox();
    expect(bounds).not.toBeNull();
    expect(next).not.toBeNull();
    expect(next!.y).toBeGreaterThanOrEqual(bounds!.y + bounds!.height);
    await expect(row.getByRole('button', { name: 'Install', exact: true })).toBeVisible();
    await page.screenshot({ path: testInfo.outputPath(`capabilities-${width}.png`), fullPage: true, animations: 'disabled' });
    await row.click();
    const dialog = page.locator('.modal-details');
    await expect(dialog.getByText('Capabilities', { exact: true })).toBeVisible();
    await expect(dialog.getByText('Image generation', { exact: true })).toBeVisible();
    await expect(dialog.getByText('Tools', { exact: true })).toBeVisible();
    await expect(dialog.getByRole('button', { name: 'Install Model' })).toBeVisible();
    await page.screenshot({ path: testInfo.outputPath(`capabilities-detail-${width}.png`), fullPage: true, animations: 'disabled' });
  });
}
