import type { Locator, Page } from '@playwright/test';
import { api, chooseOption, expect, signIn, state, test, tokenFor } from './harness';

/**
 * The AI settings sections, against the real server: the audio checkpoint PR #39
 * added, stored through migration 016 and read back on a fresh load, and the
 * admin-only writes PR #46 introduced.
 */

const AUDIO_CHECKPOINT = 'ace_step_v1_3.5b.safetensors';
const WORKSPACE_GENERAL = '/settings/workspace/general';
const WORKSPACE_AI = '/settings/workspace/ai';
const ORGANIZATION_AI = '/settings/organization/ai';

/** Clicking Save does not await the handler, so the reload can beat the write. */
async function save(page: Page, button: Locator, pattern: RegExp) {
  const written = page.waitForResponse(
    (response) =>
      pattern.test(new URL(response.url()).pathname) && response.request().method() === 'PUT'
  );
  await button.click();
  const response = await written;
  expect(response.status(), await response.text()).toBeLessThan(400);
}

test('the workspace settings page opens at all', async ({ page, consoleErrors }) => {
  await signIn(page);
  await page.goto(WORKSPACE_GENERAL);

  await expect(page.getByRole('heading', { name: 'Theme Configuration' })).toBeVisible();
  await expect(page.getByRole('heading', { name: /access denied/i })).toHaveCount(0);
  expect(consoleErrors).toEqual([]);
});

test('the legacy organization settings path opens the organization AI defaults', async ({
  page,
  consoleErrors,
}) => {
  await signIn(page);
  await page.goto('/org-settings');

  await expect(page).toHaveURL(/\/settings\/organization\/ai$/);
  await expect(page.getByRole('heading', { name: /access denied/i })).toHaveCount(0);
  await expect(page.getByRole('heading', { name: 'AI Provider Configuration' })).toBeVisible();
  expect(consoleErrors).toEqual([]);
});

test('an audio checkpoint set on the organization survives a reload', async ({
  page,
  consoleErrors,
}) => {
  await signIn(page);
  await page.goto(ORGANIZATION_AI);

  const audio = page.locator('#model-audio');
  await expect(audio).toBeVisible();
  await chooseOption(audio, AUDIO_CHECKPOINT);
  await save(page, page.getByRole('button', { name: /save changes/i }), /\/settings\/ai$/);

  // Read it back from a fresh load, so what is asserted came from the database.
  await page.goto(ORGANIZATION_AI);
  await expect(page.locator('#model-audio')).toHaveText(AUDIO_CHECKPOINT);

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
  await page.goto(WORKSPACE_AI);

  const field = page.getByRole('group', { name: 'Audio Model', exact: true });
  const start = field.getByRole('button', { name: /^(Override|Edit) Audio Model$/ });
  await expect(start).toBeVisible();
  await start.click();

  const audio = page.locator('#override-audio');
  await expect(audio).toBeVisible();
  await chooseOption(audio, AUDIO_CHECKPOINT);
  await save(
    page,
    field.getByRole('button', { name: 'Save Audio Model' }),
    /\/workspaces\/[^/]+\/settings\/ai$/
  );

  await page.goto(WORKSPACE_AI);
  const reloaded = page.getByRole('group', { name: 'Audio Model', exact: true });
  await expect(reloaded).toContainText(AUDIO_CHECKPOINT);
  await expect(reloaded).toContainText('Set for this workspace');

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
