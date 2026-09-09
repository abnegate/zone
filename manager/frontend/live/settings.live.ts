import { api, expect, signIn, state, test, tokenFor } from './harness';

/**
 * The AI settings pages, against the real server: the audio checkpoint PR #39
 * added, stored through migration 016 and read back on a fresh load, and the
 * admin-only writes PR #46 introduced.
 */

const AUDIO_CHECKPOINT = 'ace_step_v1_3.5b.safetensors';

/** Clicking Save does not await the handler, so the reload can beat the write. */
async function save(page: import('@playwright/test').Page, pattern: RegExp) {
  const written = page.waitForResponse(
    (response) => pattern.test(new URL(response.url()).pathname) && response.request().method() === 'PUT'
  );
  await page.getByRole('button', { name: /save changes/i }).click();
  const response = await written;
  expect(response.status(), await response.text()).toBeLessThan(400);
}

test('the workspace settings page opens at all', async ({ page, consoleErrors }) => {
  await signIn(page);
  await page.goto('/settings');

  await expect(page.getByRole('tab', { name: /ai settings/i })).toBeVisible();
  await expect(page.getByRole('heading', { name: /access denied/i })).toHaveCount(0);
  expect(consoleErrors).toEqual([]);
});

test('the organization settings page opens at all', async ({ page, consoleErrors }) => {
  await signIn(page);
  await page.goto('/org-settings');

  await expect(page.getByRole('heading', { name: /access denied/i })).toHaveCount(0);
  await expect(page.getByRole('tab', { name: /ai settings/i })).toBeVisible();
  expect(consoleErrors).toEqual([]);
});

test('an audio checkpoint set on the organization survives a reload', async ({
  page,
  consoleErrors,
}) => {
  await signIn(page);
  await page.goto('/org-settings');
  await page.getByRole('tab', { name: /ai settings/i }).click();

  const audio = page.locator('#model-audio');
  await expect(audio).toBeVisible();
  await audio.selectOption(AUDIO_CHECKPOINT);
  await save(page, /\/settings\/ai$/);

  // Read it back from a fresh load, so what is asserted came from the database.
  await page.goto('/org-settings');
  await page.getByRole('tab', { name: /ai settings/i }).click();
  await expect(page.locator('#model-audio')).toHaveValue(AUDIO_CHECKPOINT);

  const token = await tokenFor(state.owner);
  const stored = await api(
    'GET',
    `/api/organizations/${state.owner.organization.id}/settings/ai`,
    { token }
  );
  expect(stored.status).toBe(200);
  expect(JSON.stringify(stored.body)).toContain(AUDIO_CHECKPOINT);
  expect(consoleErrors).toEqual([]);
});

test('the workspace audio checkpoint overrides the organization', async ({ page }) => {
  await signIn(page);
  await page.goto('/settings');
  await page.getByRole('tab', { name: /ai settings/i }).click();

  const override = page.getByRole('checkbox', { name: /override organization ai settings/i });
  await expect(override).toBeVisible();
  if (!(await override.isChecked())) await override.check();

  const audio = page.locator('#model-audio');
  await expect(audio).toBeVisible();
  await audio.selectOption(AUDIO_CHECKPOINT);
  await save(page, /\/settings\/ai$/);

  await page.goto('/settings');
  await page.getByRole('tab', { name: /ai settings/i }).click();
  await expect(page.locator('#model-audio')).toHaveValue(AUDIO_CHECKPOINT);

  // The organization test stored the same checkpoint and the suite runs
  // serially, so the effective value alone would be satisfied by the
  // organization's row. The workspace's own row can only hold this because the
  // override was written.
  const token = await tokenFor(state.owner);
  const workspace = await api(
    'GET',
    `/api/organizations/${state.owner.organization.id}/workspaces/${state.owner.workspace.id}/settings/ai`,
    { token }
  );
  expect(workspace.status).toBe(200);
  expect(JSON.stringify(workspace.body)).toContain(AUDIO_CHECKPOINT);

  const effective = await api(
    'GET',
    `/api/organizations/${state.owner.organization.id}/workspaces/${state.owner.workspace.id}/settings/ai/effective`,
    { token }
  );
  expect(effective.status).toBe(200);
  expect((effective.body as { model_audio?: string }).model_audio).toBe(AUDIO_CHECKPOINT);
});
