import { api, expect, signIn, state, test, tokenFor } from './harness';

/**
 * PR #47's task execution, in the console: a run started from the task list
 * reaches a terminal state and its activity is shown.
 *
 * A run's *receipt* depends on the agent reaching a workspace write tool, which
 * is the model's decision — that check is opt-in below, and the rendering it
 * exercises is pinned deterministically in `TasksPage.test.tsx`. The
 * publication path (checkout, commit, PR) needs a repository and an agent CLI,
 * and is not covered here.
 */

async function seedTask(title: string): Promise<{ token: string; taskId: string }> {
  const token = await tokenFor(state.owner);
  const created = await api('POST', `/api/workspaces/${state.owner.workspace.id}/tasks`, {
    token,
    body: {
      title,
      description: 'Create a workspace document that records this run.',
      project_ids: [],
      priority: 1,
      is_agentic: false,
    },
  });
  expect(created.status).toBe(201);
  return { token, taskId: (created.body as { task: { id: string } }).task.id };
}

async function runStatuses(taskId: string, token: string): Promise<string[]> {
  const { body } = await api('GET', `/api/tasks/${taskId}/runs`, { token });
  return ((body as { runs?: { status: string }[] }).runs ?? []).map((run) => run.status);
}

test('a task appears in the list and its run view opens', async ({ page, consoleErrors }) => {
  const title = `Live task ${Date.now()}`;
  await seedTask(title);

  await signIn(page);
  await page.goto('/tasks');

  const card = page.locator('.task-card', { hasText: title });
  await expect(card).toBeVisible();
  await card.getByRole('button', { name: 'Execute' }).click();

  await expect(page.getByRole('dialog')).toContainText(title);
  await expect(page.getByRole('button', { name: /start execution/i })).toBeVisible();
  await expect(page.getByRole('dialog')).toContainText('Ready to run');
  expect(consoleErrors).toEqual([]);
});

test('a run started in the console finishes and shows its activity', async ({
  page,
  consoleErrors,
}) => {
  const title = `Live run ${Date.now()}`;
  const { token, taskId } = await seedTask(title);

  await signIn(page);
  await page.goto('/tasks');
  await page
    .locator('.task-card', { hasText: title })
    .getByRole('button', { name: 'Execute' })
    .click();
  await page.getByRole('button', { name: /start execution/i }).click();

  // Every run logs its phases whatever the agent decides to do.
  await expect(page.locator('.execution-logs')).toContainText('Execution Logs');
  await expect(page.locator('.log-entry').first()).toBeVisible({ timeout: 240_000 });
  await expect(page.locator('.execution-logs')).not.toContainText('No logs were recorded');

  await expect
    .poll(() => runStatuses(taskId, token), { timeout: 240_000, intervals: [1_000] })
    .not.toContain('running');
  await expect(page.getByRole('dialog')).toContainText(/Completed|Failed/);
  expect(consoleErrors).toEqual([]);
});

/**
 * The receipt path end to end. Opt-in, because whether the agent reaches a
 * write tool is the model's decision; set `ZONE_LIVE_AGENT_MODEL` to a model
 * that tool-calls.
 */
test('a run that changed something shows the receipt it recorded', async ({ page }) => {
  const model = process.env.ZONE_LIVE_AGENT_MODEL;
  test.skip(!model, 'set ZONE_LIVE_AGENT_MODEL to a tool-calling model');

  const title = `Live receipt run ${Date.now()}`;
  const { token, taskId } = await seedTask(title);

  await signIn(page);
  await page.goto('/tasks');
  await page
    .locator('.task-card', { hasText: title })
    .getByRole('button', { name: 'Execute' })
    .click();
  await page.getByRole('button', { name: /start execution/i }).click();

  await expect
    .poll(() => runStatuses(taskId, token), { timeout: 280_000, intervals: [1_000] })
    .not.toContain('running');

  const runs = await api('GET', `/api/tasks/${taskId}/runs`, { token });
  const runId = ((runs.body as { runs: { id: string }[] }).runs ?? [])[0]?.id;
  const logs = await api('GET', `/api/tasks/runs/${runId}/logs`, { token });
  const receipts = (
    logs.body as { logs: { metadata?: { action_receipt?: { action: string } } | null }[] }
  ).logs
    .map((log) => log.metadata?.action_receipt)
    .filter((receipt): receipt is { action: string } => Boolean(receipt));

  expect(receipts.length, `${model} made no workspace write to receipt`).toBeGreaterThan(0);

  const rendered = page.locator('[data-testid="action-receipt"]').first();
  await expect(rendered).toBeVisible({ timeout: 60_000 });
  // The record, not the bare "Workspace action receipt" line it arrived on.
  await expect(rendered).not.toContainText('Workspace action receipt');
  await expect(rendered.locator('[data-testid="action-receipt-link"]')).toHaveAttribute(
    'href',
    /\/(tasks|wiki)\?id=/
  );
});
