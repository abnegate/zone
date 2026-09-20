import type { Page } from '@playwright/test';
import {
  api,
  enabled,
  expect,
  logLines,
  logMark,
  newChat,
  ownerToken,
  record,
  send,
  settled,
  shot,
  signIn,
  sql,
  state,
  test,
} from './rig';

/**
 * Rows 8 to 13: organization settings (members, AI provider and models,
 * LiteLLM, billing) and workspace settings (theme, AI override).
 *
 * Which model answered an "Automatic" chat is read from Ollama's own list of
 * loaded models right after the turn, because the server writes no line that
 * names the model it chose; the org and workspace settings rows are read from
 * the database as well.
 */

const ollama = process.env.OLLAMA_HOST ?? 'http://127.0.0.1:11434';
const FAST = 'llama3.2:3b';
const REASON = process.env.ZONE_LIVE_AGENT_MODEL ?? 'qwen3.8:27b';
const EMBED = process.env.OLLAMA_MODEL_EMBED ?? 'qwen3-embedding:0.6b';
const VIDEO = 'wan2.2_ti2v_5B_fp16.safetensors';
const AUDIO = 'ace_step_v1_3.5b.safetensors';

async function loadedModels(): Promise<string[]> {
  const response = await fetch(`${ollama}/api/ps`);
  const body = (await response.json()) as { models?: { name: string }[] };
  return (body.models ?? []).map((m) => m.name);
}

async function unload(model: string): Promise<void> {
  await fetch(`${ollama}/api/generate`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ model, keep_alive: 0 }),
  }).catch(() => undefined);
}

async function successAlert(page: Page, text: RegExp): Promise<string> {
  const alert = page.locator('.alert-success');
  await expect(alert).toContainText(text, { timeout: 30_000 });
  return alert.innerText();
}

/** Whatever alert the page shows within a few seconds, success or error. */
async function anyAlert(page: Page): Promise<string> {
  const alert = page
    .locator('.alert-success, .alert-error, [data-sonner-toast]')
    .first();
  await alert
    .waitFor({ state: 'visible', timeout: 15_000 })
    .catch(() => undefined);
  return (await alert.innerText().catch(() => '')) || 'no alert shown';
}

async function accentVariable(page: Page): Promise<{
  theme: string | null;
  accent: string;
  fontSize: string;
  radius: string;
  font: string;
}> {
  return page.evaluate(() => {
    const root = document.documentElement;
    const style = getComputedStyle(root);
    return {
      theme: root.getAttribute('data-theme'),
      accent: style.getPropertyValue('--ui-accent').trim(),
      fontSize: root.style.fontSize,
      radius: style.getPropertyValue('--ui-radius-md').trim(),
      font: style.getPropertyValue('--ui-font-body').trim(),
    };
  });
}

test.describe('organization and workspace settings', () => {
  test.skip(!enabled, 'set ZONE_LIVE_REAL_PASS=1 against the real rig');
  test.describe.configure({ timeout: 600_000 });

  test('8: a member role is changed, and the audit log is checked for it', async ({
    page,
  }) => {
    await signIn(page);
    await page.goto('/org-settings');
    await page.getByRole('tab', { name: 'Members' }).click();
    const table = page.locator('table.members-table');
    await expect(table).toBeVisible({ timeout: 30_000 });
    const memberRows = await table.locator('tbody tr').count();
    const tableText = (await table.innerText()).replace(/\s+/g, ' ');
    // The members table carries no email (the API sends user ids only), so the
    // second tenant's row is the one that is not the owner's own.
    const row = table
      .locator('tbody tr')
      .filter({ hasNot: page.locator('select:disabled') })
      .first();
    await expect(
      row,
      `a row for the second tenant among ${memberRows}: ${tableText.slice(0, 200)}`,
    ).toBeVisible();
    const select = row.locator('select.role-select');
    await shot(page, '08-members-before');
    await select.selectOption('admin');
    const dialog = page.getByRole('dialog');
    await expect(dialog).toContainText('Confirm Role Change');
    await dialog.getByRole('button', { name: 'Confirm' }).click();
    const saved = await successAlert(page, /Role updated successfully/);
    await expect(row.locator('.role-badge')).toContainText(/admin/i, {
      timeout: 30_000,
    });
    await shot(page, '08-role-changed');
    const dbRole = sql(
      `select role from organization_members where organization_id = '${state.owner.organization.id}' and user_id = '${state.intruder.user.id}'`,
    ).join(',');

    await page.getByRole('tab', { name: 'Audit Logs' }).click();
    await page.waitForTimeout(2_000);
    const auditText = await page.locator('main, .page').first().innerText();
    const auditRows = await page
      .locator('table.audit-logs-table tbody tr')
      .count();
    await shot(page, '08-audit-logs');
    const auditDb = sql(`select count(*) from audit_logs`).join(',');

    // Put the role back so the tenancy rows keep their meaning.
    await page.getByRole('tab', { name: 'Members' }).click();
    await table
      .locator('tbody tr')
      .filter({ hasNot: page.locator('select:disabled') })
      .first()
      .locator('select.role-select')
      .selectOption('member');
    await successAlert(page, /Role updated successfully/);

    const auditShowsChange = auditRows > 0 && /role|member/i.test(auditText);
    record(8, {
      result: auditShowsChange ? 'WORKS' : 'FAILS',
      role_change_alert: saved,
      members_table_text: tableText.slice(0, 200),
      role_in_db_after_change: dbRole,
      audit_rows_rendered: auditRows,
      audit_rows_in_db: auditDb,
      audit_page_text: auditText.slice(0, 300),
      screenshots: [
        '08-members-before.png',
        '08-role-changed.png',
        '08-audit-logs.png',
      ],
    });
    expect(auditShowsChange, 'the role change shows in Audit Logs').toBe(true);
  });

  test('9 and 10: org AI settings drive an automatic chat, the LiteLLM section saves, and defaults come back', async ({
    page,
  }) => {
    await signIn(page);
    await page.goto('/org-settings');
    await expect(page.locator('#ai-provider')).toBeVisible({ timeout: 30_000 });
    await page.locator('#ai-provider').selectOption('self_hosted');
    await page.locator('#model-fast').selectOption(FAST);
    await page.locator('#model-reasoning').selectOption(REASON);
    const embedding = page.locator('#model-embedding');
    if ((await embedding.evaluate((el) => el.tagName)) === 'SELECT') {
      await embedding.selectOption(EMBED);
    } else {
      await embedding.fill(EMBED);
    }
    await page.locator('#model-video').selectOption(VIDEO);
    await page.locator('#model-audio').selectOption(AUDIO);
    await shot(page, '09-ai-settings-filled');
    await page.getByRole('button', { name: 'Save Changes' }).click();
    const saved = await successAlert(page, /Settings saved successfully/);
    const dbRow = sql(
      `select provider, model_fast, model_reasoning, model_embedding, model_video, model_audio from organization_ai_settings where organization_id = '${state.owner.organization.id}'`,
    ).join(' | ');

    // A chat on "Automatic" takes the fast model for a plain message and the
    // reasoning model for one that asks for step-by-step work.
    await unload(FAST);
    await unload(REASON);
    const mark = logMark();
    const chatId = await newChat(page, { model: 'Automatic' });
    await send(page, 'Say hello in five words.');
    await settled(page, 1, 300_000);
    const afterFast = await loadedModels();
    await shot(page, '09-auto-chat-fast-turn');
    await send(page, 'Explain step by step why 17 is a prime number.');
    await settled(page, 2, 600_000);
    const afterReason = await loadedModels();
    await shot(page, '09-auto-chat-reasoning-turn');
    const chatModel = sql(
      `select model_name from chats where id = '${chatId}'`,
    ).join(',');
    const logged = logLines(
      mark,
      new RegExp(`${FAST.replace('.', '\\.')}|${REASON.replace('.', '\\.')}`),
    ).slice(0, 3);

    // Row 10: the provider forms for OpenAI and Anthropic, and the LiteLLM section.
    await page.goto('/org-settings');
    await expect(page.locator('#ai-provider')).toBeVisible({ timeout: 30_000 });
    await page.locator('#ai-provider').selectOption('openai');
    await expect(page.locator('#openai-key')).toBeVisible();
    await shot(page, '10-provider-openai-form');
    await page.locator('#ai-provider').selectOption('anthropic');
    await expect(page.locator('#anthropic-key')).toBeVisible();
    await shot(page, '10-provider-anthropic-form');
    await page.locator('#ai-provider').selectOption('self_hosted');
    await page
      .locator('#litellm-host')
      .fill(process.env.LITELLM_HOST ?? 'http://127.0.0.1:11434/v1');
    await page.locator('#litellm-key').fill('live-verify');
    await page.getByRole('button', { name: 'Save Changes' }).click();
    const litellmSaved = await successAlert(
      page,
      /Settings saved successfully/,
    );
    const litellmRow = sql(
      `select litellm_host, litellm_key is not null from organization_ai_settings where organization_id = '${state.owner.organization.id}'`,
    ).join(' | ');
    await shot(page, '10-litellm-saved');

    await page.getByRole('button', { name: 'Reset to Defaults' }).click();
    const reset = await anyAlert(page);
    await shot(page, '09-reset-alert');
    await page.reload();
    await expect(page.locator('#ai-provider')).toBeVisible({ timeout: 30_000 });
    const fastAfterReset = await page.locator('#model-fast').inputValue();
    const rowsAfterReset = sql(
      `select count(*) from organization_ai_settings where organization_id = '${state.owner.organization.id}'`,
    ).join(',');
    await shot(page, '09-after-reset');

    const fastUsed = afterFast.includes(FAST);
    const reasonUsed = afterReason.includes(REASON);
    record(9, {
      result:
        fastUsed && reasonUsed && fastAfterReset === '' ? 'WORKS' : 'FAILS',
      saved_alert: saved,
      db_row_after_save: dbRow,
      chat_id: chatId,
      chat_model_name_column: chatModel,
      ollama_loaded_after_plain_turn: afterFast,
      ollama_loaded_after_step_by_step_turn: afterReason,
      server_log_lines_naming_a_model: logged,
      reset_alert: reset,
      fast_select_after_reset: fastAfterReset,
      org_settings_rows_after_reset: rowsAfterReset,
      screenshots: [
        '09-ai-settings-filled.png',
        '09-auto-chat-fast-turn.png',
        '09-auto-chat-reasoning-turn.png',
        '09-reset-alert.png',
        '09-after-reset.png',
      ],
    });
    record(10, {
      result: 'FAILS',
      cause:
        'environment: no OpenAI or Anthropic API key on this machine; the provider forms render and the LiteLLM section saves',
      litellm_saved_alert: litellmSaved,
      litellm_row: litellmRow,
      screenshots: [
        '10-provider-openai-form.png',
        '10-provider-anthropic-form.png',
        '10-litellm-saved.png',
      ],
    });
    expect(
      fastUsed,
      `the plain turn loaded ${FAST}: ${afterFast.join(',')}`,
    ).toBe(true);
    expect(
      reasonUsed,
      `the step-by-step turn loaded ${REASON}: ${afterReason.join(',')}`,
    ).toBe(true);
    expect(fastAfterReset).toBe('');
  });

  test('11: the billing page shows something true about the organization', async ({
    page,
  }) => {
    await signIn(page);
    await page.goto('/org-settings');
    await page.getByRole('tab', { name: 'Billing' }).click();
    await page.waitForTimeout(3_000);
    const panel = page.locator('main, .page').first();
    const text = await panel.innerText();
    await shot(page, '11-billing');
    const users = sql(
      `select count(*) from organization_members where organization_id = '${state.owner.organization.id}' and is_active`,
    ).join(',');
    const workspaces = sql(
      `select count(*) from workspaces where organization_id = '${state.owner.organization.id}'`,
    ).join(',');
    const projects = sql(
      `select count(*) from projects where workspace_id in (select id from workspaces where organization_id = '${state.owner.organization.id}')`,
    ).join(',');
    const userMetric = await page
      .locator('.metric-card', { hasText: /Members|Users/ })
      .locator('.current-value')
      .innerText()
      .catch(() => 'no metric');
    const workspaceMetric = await page
      .locator('.metric-card', { hasText: 'Workspaces' })
      .locator('.current-value')
      .innerText()
      .catch(() => 'no metric');
    const truthful =
      userMetric.trim() === users && workspaceMetric.trim() === workspaces;
    record(11, {
      result: truthful ? 'WORKS' : 'FAILS',
      page_text: text.slice(0, 400),
      users_metric: userMetric,
      users_in_db: users,
      workspaces_metric: workspaceMetric,
      workspaces_in_db: workspaces,
      projects_in_db: projects,
      screenshots: ['11-billing.png'],
    });
    expect(
      truthful,
      `billing counts users=${userMetric} (db ${users}) workspaces=${workspaceMetric} (db ${workspaces})`,
    ).toBe(true);
  });

  test('12: the workspace theme previews, saves, survives a reload and dresses every page', async ({
    page,
  }) => {
    await signIn(page);
    await page.goto('/settings');
    await page.getByRole('tab', { name: 'Theme' }).click();
    await expect(page.locator('#primary-light')).toBeVisible({
      timeout: 30_000,
    });
    const colours: Record<string, string> = {
      '#primary-light': '#d946ef',
      '#secondary-light': '#f59e0b',
      '#primary-dark': '#22c55e',
      '#secondary-dark': '#ef4444',
    };
    for (const [id, value] of Object.entries(colours)) {
      const wrapper = page.locator('.color-input-wrapper', {
        has: page.locator(id),
      });
      await wrapper.locator('.color-text-input').fill(value);
      await wrapper.locator('.color-text-input').dispatchEvent('change');
    }
    await page.locator('#font-family').selectOption('roboto');
    await page.locator('#font-size').fill('18');
    await page.getByLabel('Large').check();
    await page.waitForTimeout(500);
    const preview = await accentVariable(page);
    const previewButton = await page
      .locator('.preview-box')
      .getByRole('button', { name: 'Primary Button' })
      .evaluate((el) => getComputedStyle(el).backgroundColor);
    await shot(page, '12-theme-form-and-preview');
    await page
      .getByRole('tabpanel', { name: 'Theme' })
      .getByRole('button', { name: 'Save Changes' })
      .click();
    const saved = await successAlert(page, /Settings saved successfully/);
    const dbRow = sql(
      `select count(*) from workspace_themes where workspace_id = '${state.owner.workspace.id}'`,
    ).join(',');

    await page.reload();
    await expect(page.locator('#primary-light')).toBeVisible({
      timeout: 30_000,
    });
    const afterReload = await accentVariable(page);
    const pages: Record<
      string,
      ReturnType<typeof accentVariable> extends Promise<infer T> ? T : never
    > = {};
    for (const path of ['/chats', '/wiki', '/tasks']) {
      await page.goto(path);
      await page.waitForTimeout(1_500);
      pages[path] = await accentVariable(page);
      await shot(page, `12-theme-on-${path.slice(1)}`);
    }

    await page.goto('/settings');
    await page.getByRole('tab', { name: 'Theme' }).click();
    await page
      .getByRole('tabpanel', { name: 'Theme' })
      .getByRole('button', { name: 'Reset to Defaults' })
      .click();
    const reset = await anyAlert(page);
    const afterReset = await accentVariable(page);

    const expectedAccent =
      preview.theme === 'dark'
        ? colours['#primary-dark']
        : colours['#primary-light'];
    const worn = Object.values(pages).every(
      (p) => p.accent.toLowerCase() === expectedAccent && p.fontSize === '18px',
    );
    record(12, {
      result:
        preview.accent.toLowerCase() === expectedAccent &&
        afterReload.accent.toLowerCase() === expectedAccent &&
        worn
          ? 'WORKS'
          : 'FAILS',
      theme_mode: preview.theme,
      preview_variables: preview,
      preview_primary_button_background: previewButton,
      saved_alert: saved,
      workspace_theme_rows: dbRow,
      after_reload: afterReload,
      other_pages: pages,
      reset_alert: reset,
      after_reset: afterReset,
      screenshots: [
        '12-theme-form-and-preview.png',
        '12-theme-on-chats.png',
        '12-theme-on-wiki.png',
        '12-theme-on-tasks.png',
      ],
    });
    expect(preview.accent.toLowerCase()).toBe(expectedAccent);
    expect(afterReload.accent.toLowerCase()).toBe(expectedAccent);
    expect(worn, JSON.stringify(pages)).toBe(true);
  });

  test('13: a workspace AI override changes the fast model for this workspace only', async ({
    page,
  }) => {
    const token = await ownerToken();
    await signIn(page);
    await page.goto('/settings');
    await page.getByRole('tab', { name: 'AI Settings' }).click();
    const panel = page.getByRole('tabpanel', { name: 'AI Settings' });
    const override = page.getByLabel('Override organization AI settings');
    await override.check();
    await page.locator('#ai-provider').selectOption('self_hosted');
    await page
      .locator('#litellm-host')
      .fill(process.env.LITELLM_HOST ?? 'http://127.0.0.1:11434/v1');
    await page.locator('#litellm-key').fill('live-verify');
    await page.locator('#model-fast').selectOption(FAST);
    await shot(page, '13-workspace-ai-override-form');
    await panel.getByRole('button', { name: 'Save Changes' }).click();
    const saved = await successAlert(page, /Settings saved successfully/);
    const dbRow = sql(
      `select provider, model_fast, litellm_host, litellm_key is not null from workspace_ai_settings where workspace_id = '${state.owner.workspace.id}'`,
    ).join(' | ');
    const org = await api(
      'GET',
      `/api/organizations/${state.owner.organization.id}/settings/ai`,
      { token },
    );
    const effective = await api(
      'GET',
      `/api/organizations/${state.owner.organization.id}/workspaces/${state.owner.workspace.id}/settings/ai/effective`,
      { token },
    );

    await unload(FAST);
    const chatId = await newChat(page, { model: 'Automatic' });
    await send(page, 'Say hello in five words.');
    await settled(page, 1, 300_000);
    const loaded = await loadedModels();
    await shot(page, '13-workspace-chat-uses-override');

    await page.goto('/settings');
    await page.getByRole('tab', { name: 'AI Settings' }).click();
    await page
      .getByRole('tabpanel', { name: 'AI Settings' })
      .getByRole('button', { name: 'Reset to Defaults' })
      .click();
    const reset = await anyAlert(page);
    await shot(page, '13-reset-alert');
    const rowsAfter = sql(
      `select count(*) from workspace_ai_settings where workspace_id = '${state.owner.workspace.id}'`,
    ).join(',');

    const orgFast =
      (org.body as { model_fast?: string | null }).model_fast ?? null;
    const effectiveFast =
      (effective.body as { model_fast?: string | null }).model_fast ?? null;
    const ok =
      loaded.includes(FAST) && effectiveFast === FAST && orgFast !== FAST;
    record(13, {
      result: ok ? 'WORKS' : 'FAILS',
      saved_alert: saved,
      workspace_row: dbRow,
      org_model_fast: orgFast,
      effective_model_fast_for_workspace: effectiveFast,
      chat_id: chatId,
      ollama_loaded_after_turn: loaded,
      reset_alert: reset,
      workspace_rows_after_reset: rowsAfter,
      screenshots: [
        '13-workspace-ai-override-form.png',
        '13-workspace-chat-uses-override.png',
        '13-reset-alert.png',
      ],
    });
    expect(
      ok,
      `loaded=${loaded.join(',')} effective=${effectiveFast} org=${orgFast}`,
    ).toBe(true);
  });
});
