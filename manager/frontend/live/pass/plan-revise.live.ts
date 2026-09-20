import type { Locator } from '@playwright/test';
import {
  api,
  enabled,
  expect,
  ownerToken,
  record,
  shot,
  signIn,
  sql,
  stamp,
  state,
  test,
} from './rig';

/**
 * Row 26, second task: a plan that is sent back instead of approved. The plan
 * card offers Approve and Revise (there is no Reject), so Revise with an
 * instruction to stop is what a person can do; the run must make no write.
 */

const REPO_PROJECT = 'Real pass repository';
const SOURCE = 'Scratch repo';

async function toggle(
  dialog: Locator,
  title: string,
  wanted: boolean,
): Promise<void> {
  const label = dialog.locator('label.toggle-label', { hasText: title });
  await expect(label).toBeVisible();
  const box = label.locator('input[type="checkbox"]');
  if ((await box.isChecked()) !== wanted)
    await label.locator('.toggle-slider').click();
  await expect(box).toBeChecked({ checked: wanted });
}

function calls(runId: string): string[] {
  return sql(
    `select case when message like 'Executing tool:%' then replace(message, 'Executing tool: ', '') || ' | call | ' || left(coalesce(metadata->>'args', ''), 160) else replace(replace(message, 'Tool ', ''), ' finished', '') || ' | ' || case when metadata->>'success' = 'true' then 'ok' else 'failed' end || ' | ' || left(coalesce(metadata->>'error', metadata->>'output', ''), 160) end from task_run_logs where task_run_id = '${runId}' and (message like 'Executing tool:%' or message like 'Tool % finished') order by created_at`,
  );
}

test.describe('plan revise', () => {
  test.skip(!enabled, 'set ZONE_LIVE_REAL_PASS=1 against the real rig');
  test.describe.configure({ timeout: 2_400_000 });

  test('26b: a plan sent back with Revise makes no write', async ({ page }) => {
    await signIn(page);
    const s = stamp();
    const title = `Rename the widget ${s}`;
    await page.goto('/tasks');
    await page.getByRole('button', { name: /new task/i }).click();
    const dialog = page.getByRole('dialog');
    await dialog
      .locator('.project-selection-option')
      .filter({ has: page.getByText(REPO_PROJECT, { exact: true }) })
      .first()
      .click();
    await dialog.getByRole('button', { name: 'Next' }).click();
    await dialog.locator('#task-title').fill(title);
    await dialog
      .locator('#task-description')
      .fill(
        `Rename the function area in src/widget.py to widget_area_${s} and update check.sh accordingly.`,
      );
    await dialog.getByRole('button', { name: 'Next' }).click();
    await toggle(dialog, 'Enable Agentic Mode', true);
    await toggle(dialog, 'Require plan approval', true);
    const select = dialog.locator('#task-source');
    if (await select.count()) {
      const labels = await select.locator('option').allInnerTexts();
      const match = labels.find((l) => l.includes(SOURCE));
      if (match) await select.selectOption({ label: match });
    }
    await dialog.getByRole('button', { name: 'Create Task' }).click();
    await expect(dialog).toBeHidden({ timeout: 30_000 });
    const { body } = await api(
      'GET',
      `/api/workspaces/${state.owner.workspace.id}/tasks`,
      { token: await ownerToken() },
    );
    const task = (
      (body as { tasks?: { id: string; title: string }[] }).tasks ?? []
    ).find((t) => t.title === title);
    expect(task).toBeTruthy();
    await page
      .locator('.task-card', { hasText: title })
      .getByRole('button', { name: 'Execute' })
      .click();
    await page.getByRole('button', { name: /start execution/i }).click();
    await expect
      .poll(
        () =>
          sql(
            `select status from task_runs where task_id = '${task!.id}' order by started_at desc limit 1`,
          ).join(','),
        { timeout: 1_500_000, intervals: [3_000] },
      )
      .toMatch(/waiting|completed|failed/);
    const parked = sql(
      `select id || '|' || status from task_runs where task_id = '${task!.id}' order by started_at desc limit 1`,
    )[0].split('|');
    const prompt = page.locator('.execution-question');
    let options: string[] = [];
    if (parked[1] === 'waiting') {
      await expect(prompt).toBeVisible({ timeout: 60_000 });
      options = await prompt
        .getByRole('radio')
        .evaluateAll((els) =>
          els.map((e) => (e as HTMLInputElement).labels?.[0]?.innerText ?? ''),
        );
      await prompt.getByRole('radio', { name: 'Revise' }).check();
      const other = prompt.locator('[data-testid="question-free-text"]');
      if (await other.isEnabled().catch(() => false))
        await other.fill(
          'Do not make this change. Stop here without editing any file.',
        );
      await shot(page, '26-plan-revise');
      await prompt.locator('[data-testid="question-submit"]').click();
    }
    await expect
      .poll(
        () =>
          sql(
            `select status from task_runs where task_id = '${task!.id}' order by started_at desc limit 1`,
          ).join(','),
        { timeout: 1_500_000, intervals: [3_000] },
      )
      .toMatch(/completed|failed|cancelled|waiting/);
    await page.waitForTimeout(3_000);
    const after = sql(
      `select id || '|' || status from task_runs where task_id = '${task!.id}' order by started_at desc limit 1`,
    )[0].split('|');
    const made = calls(after[0]);
    const writes = made.filter((c) =>
      /^(write_file|apply_patch|run_shell|run_command) \| ok/.test(c),
    );
    const summary = (
      await page
        .getByRole('dialog')
        .locator('.execution-summary')
        .innerText()
        .catch(() => '')
    ).replace(/\s+/g, ' ');
    await shot(page, '26-plan-revise-outcome');
    record(26.5, {
      result:
        parked[1] === 'waiting' && writes.length === 0 ? 'WORKS' : 'FAILS',
      note: 'The plan card offers Approve and Revise; there is no Reject. Revise with an instruction to stop was sent instead.',
      task_id: task!.id,
      run_id: after[0],
      parked_status: parked[1],
      options,
      status_after_revise: after[1],
      summary_after_revise: summary.slice(0, 200),
      writes,
      calls: made.slice(0, 12),
      screenshots: ['26-plan-revise.png', '26-plan-revise-outcome.png'],
    });
    expect(parked[1]).toBe('waiting');
    expect(writes).toEqual([]);
  });
});
