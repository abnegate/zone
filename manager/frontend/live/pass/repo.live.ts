import { execFileSync } from 'node:child_process';
import type { Locator, Page } from '@playwright/test';
import {
  api,
  enabled,
  expect,
  logLines,
  logMark,
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
 * Rows 20 to 31: sources, projects and tasks against the scratch GitHub
 * repository, with real task runs on the real model. Tool calls a run made are
 * read back from task_tool_calls and the run logs, never from the model's prose.
 */

const REPO =
  process.env.ZONE_LIVE_REPO_URL ?? 'https://github.com/abnegate/zone-tests';
const TOKEN = process.env.ZONE_LIVE_REPO_TOKEN ?? '';
const [OWNER, NAME] = REPO.replace(/^https:\/\/github\.com\//, '').split('/');
const SOURCE = 'Scratch repo';
const REPO_PROJECT = 'Real pass repository';
const SCRATCH_PROJECT = 'Real pass scratch';
const BROKEN_PROJECT = 'Real pass broken';

interface Run {
  id: string;
  status: string;
  pending_question?: unknown;
  plan?: string | null;
  error_message?: string | null;
}

async function runsOf(taskId: string): Promise<Run[]> {
  const { body } = await api('GET', `/api/tasks/${taskId}/runs`, {
    token: await ownerToken(),
  });
  return ((body as { runs?: Run[] }).runs ?? []) as Run[];
}

async function taskById(taskId: string): Promise<Record<string, unknown>> {
  const { body } = await api('GET', `/api/tasks/${taskId}`, {
    token: await ownerToken(),
  });
  return ((body as { task?: Record<string, unknown> }).task ?? body) as Record<
    string,
    unknown
  >;
}

async function finished(
  taskId: string,
  timeout = 1_800_000,
  atLeast = 1,
): Promise<Run> {
  // The newest run is the one that matters: a task run again already has a
  // finished run in its list.
  await expect
    .poll(
      async () => {
        const runs = await runsOf(taskId);
        return runs.length >= atLeast ? runs[0].status : 'none';
      },
      { timeout, intervals: [3_000] },
    )
    .toMatch(/completed|failed|cancelled/);
  return (await runsOf(taskId))[0];
}

async function waiting(taskId: string, timeout = 1_500_000): Promise<Run> {
  await expect
    .poll(async () => (await runsOf(taskId)).map((r) => r.status).join(','), {
      timeout,
      intervals: [3_000],
    })
    .toMatch(/waiting|completed|failed/);
  return (await runsOf(taskId))[0];
}

function toolCalls(runId: string): string[] {
  // task_tool_calls stays empty for these runs; the run log carries each call
  // as "Executing tool: X" with its args and "Tool X finished" with its result.
  return sql(
    `select case when message like 'Executing tool:%' then replace(message, 'Executing tool: ', '') || ' | call | ' || left(replace(coalesce(metadata->>'args', ''), E'\n', ' '), 200) else replace(replace(message, 'Tool ', ''), ' finished', '') || ' | ' || case when metadata->>'success' = 'true' then 'ok' else 'failed' end || ' | ' || left(replace(coalesce(metadata->>'error', metadata->>'output', ''), E'\n', ' '), 200) end from task_run_logs where task_run_id = '${runId}' and (message like 'Executing tool:%' or message like 'Tool % finished') order by created_at`,
  );
}

function runLogs(runId: string): string[] {
  return sql(
    `select phase || ' | ' || log_level || ' | ' || left(message, 200) from task_run_logs where task_run_id = '${runId}' order by created_at`,
  );
}

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

async function createTask(
  page: Page,
  options: {
    title: string;
    description: string;
    project: string;
    planApproval?: boolean;
    source?: string;
  },
): Promise<string> {
  await page.goto('/tasks');
  await page.getByRole('button', { name: /new task/i }).click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  await dialog
    .locator('.project-selection-option')
    .filter({ has: page.getByText(options.project, { exact: true }) })
    .first()
    .click();
  await dialog.getByRole('button', { name: 'Next' }).click();
  await dialog.locator('#task-title').fill(options.title);
  await dialog.locator('#task-description').fill(options.description);
  await dialog.getByRole('button', { name: 'Next' }).click();
  await toggle(dialog, 'Enable Agentic Mode', true);
  if (options.planApproval) await toggle(dialog, 'Require plan approval', true);
  if (options.source) {
    const select = dialog.locator('#task-source');
    if (await select.count()) {
      const labels = await select.locator('option').allInnerTexts();
      const match = labels.find((l) => l.includes(options.source as string));
      if (match) await select.selectOption({ label: match });
    }
  }
  await dialog.getByRole('button', { name: 'Create Task' }).click();
  await expect(dialog).toBeHidden({ timeout: 30_000 });
  await expect(
    page.locator('.task-card', { hasText: options.title }),
  ).toBeVisible();
  const { body } = await api(
    'GET',
    `/api/workspaces/${state.owner.workspace.id}/tasks`,
    { token: await ownerToken() },
  );
  const found = (
    (body as { tasks?: { id: string; title: string }[] }).tasks ?? []
  ).find((t) => t.title === options.title);
  if (!found) throw new Error(`no task titled ${options.title}`);
  return found.id;
}

async function startRun(page: Page, title: string): Promise<void> {
  await page.goto('/tasks');
  await page
    .locator('.task-card', { hasText: title })
    .getByRole('button', { name: 'Execute' })
    .click();
  await page
    .getByRole('button', { name: /start execution|run again|try again/i })
    .click();
  await expect(page.locator('.execution-logs')).toContainText('Execution Logs');
}

async function projectId(name: string): Promise<string | null> {
  const { body } = await api(
    'GET',
    `/api/projects?workspace_id=${state.owner.workspace.id}`,
    { token: await ownerToken() },
  );
  const list =
    (body as { projects?: { id: string; name: string }[] }).projects ?? [];
  return list.find((p) => p.name === name)?.id ?? null;
}

async function sourceId(name: string): Promise<string | null> {
  const { body } = await api(
    'GET',
    `/api/workspaces/${state.owner.workspace.id}/sources`,
    { token: await ownerToken() },
  );
  const list = ((body as { sources?: { id: string; name: string }[] })
    .sources ?? (Array.isArray(body) ? body : [])) as {
    id: string;
    name: string;
  }[];
  return list.find((s) => s.name === name)?.id ?? null;
}

async function createProjectInConsole(
  page: Page,
  name: string,
  source: string | null,
  status: string,
): Promise<void> {
  await page.goto('/projects');
  await page.getByRole('button', { name: 'New project' }).click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  await dialog.locator('#project-name').fill(name);
  await dialog
    .locator('#project-description')
    .fill(`Created by the real pass for ${name}`);
  await dialog.getByRole('button', { name: 'Next' }).click();
  await page.waitForTimeout(400);
  const option = source
    ? dialog.locator('.source-selection-option', { hasText: source })
    : dialog.locator('.source-selection-option', { hasText: 'No Source' });
  await option.first().click();
  await dialog.getByRole('button', { name: 'Next' }).click();
  await page.waitForTimeout(400);
  await dialog
    .locator('.status-selection-option', { hasText: status })
    .first()
    .click();
  await dialog.getByRole('button', { name: 'Create Project' }).click();
  await expect(dialog).toBeHidden({ timeout: 30_000 });
  await expect(page.locator('.project-card', { hasText: name })).toBeVisible();
}

test.describe('sources, projects and tasks', () => {
  test.skip(!enabled, 'set ZONE_LIVE_REAL_PASS=1 against the real rig');
  test.describe.configure({ timeout: 2_400_000 });

  test('20: a GitHub source is added, verified, toggled', async ({ page }) => {
    await signIn(page);
    await page.goto('/sources');
    page.on('dialog', (d) => d.accept());
    const existing = page.locator('.source-card', { hasText: SOURCE });
    if (await existing.count()) {
      await existing.getByRole('button', { name: 'Delete' }).click();
      await expect(existing).toHaveCount(0, { timeout: 30_000 });
    }
    await page
      .locator('.sources-header')
      .getByRole('button', { name: 'Add source' })
      .click();
    const wizard = page.getByRole('dialog');
    await expect(wizard).toContainText('Add Source');
    const kinds = await wizard
      .locator('.source-type-option .source-type-name')
      .allInnerTexts();
    await wizard
      .locator('.source-type-option')
      .filter({ has: page.getByText('GitHub', { exact: true }) })
      .click();
    await shot(page, '20-source-kinds');
    await wizard.getByRole('button', { name: 'Next' }).click();
    await wizard.locator('#ghOwner').fill(OWNER);
    await wizard.locator('#ghRepo').fill(NAME);
    await wizard.locator('#ghBranch').fill('main');
    await wizard.locator('#credentials').fill(TOKEN);
    const writeToggle = await wizard
      .locator('label.toggle-label', { hasText: 'Allow write operations' })
      .count();
    const mainToggle = await wizard.getByText(/main repository/i).count();
    await shot(page, '20-source-github-form');
    await wizard.getByRole('button', { name: 'Next' }).click();
    await wizard.locator('#name').fill(SOURCE);
    await wizard
      .locator('#description')
      .fill('The live pass scratch repository');
    await wizard.getByRole('button', { name: 'Add Source' }).click();
    await expect(wizard).toBeHidden({ timeout: 30_000 });
    const card = page.locator('.source-card', { hasText: SOURCE });
    await expect(card).toBeVisible({ timeout: 30_000 });
    const statusAfterAdd = await card.locator('.source-status').innerText();

    await card.getByRole('button', { name: 'Verify' }).click();
    await expect(card.locator('.source-status')).toHaveText(
      /Verified|Error|Unverified/,
      { timeout: 60_000 },
    );
    await page.waitForTimeout(1_000);
    const statusAfterVerify = await card.locator('.source-status').innerText();
    const banner =
      (await page
        .locator('.sources-banner')
        .innerText()
        .catch(() => '')) || '';
    await shot(page, '20-source-verified');

    await card.getByRole('button', { name: 'Disable' }).click();
    await expect(card.locator('.source-status')).toHaveText(/Inactive/, {
      timeout: 30_000,
    });
    await shot(page, '20-source-disabled');
    await card.getByRole('button', { name: 'Enable' }).click();
    await expect(card.locator('.source-status')).not.toHaveText(/Inactive/, {
      timeout: 30_000,
    });
    const id = await sourceId(SOURCE);
    const db = sql(
      `select source_type, is_active, last_verified_at is not null from sources where id = '${id}'`,
    ).join(' | ');
    record(20, {
      result: /Verified/.test(statusAfterVerify) ? 'WORKS' : 'FAILS',
      cause: /Verified/.test(statusAfterVerify)
        ? undefined
        : `verify did not reach Verified: ${statusAfterVerify} ${banner}`,
      source_id: id,
      wizard_kinds: kinds,
      status_after_add: statusAfterAdd,
      status_after_verify: statusAfterVerify,
      banner_after_verify: banner,
      allow_write_toggle_for_github: writeToggle,
      main_repository_toggle: mainToggle,
      db,
      note: 'The wizard offers no Verify step, no "Allow write operations" for GitHub (Filesystem only) and no "Main repository" toggle; Verify and Enable/Disable live on the card. Delete is exercised in row 31.',
      screenshots: [
        '20-source-kinds.png',
        '20-source-github-form.png',
        '20-source-verified.png',
        '20-source-disabled.png',
      ],
    });
    expect(statusAfterVerify).toMatch(/Verified/);
  });

  test('21: the other source kinds, with the credentials this machine lacks', async ({
    page,
  }) => {
    await signIn(page);
    await page.goto('/sources');
    page.on('dialog', (d) => d.accept());
    const outcomes: Record<string, string> = {};

    async function add(
      kind: string,
      fill: (wizard: Locator) => Promise<void>,
      name: string,
    ): Promise<string> {
      await page
        .locator('.sources-header')
        .getByRole('button', { name: 'Add source' })
        .click();
      const wizard = page.getByRole('dialog');
      await wizard
        .locator('.source-type-option')
        .filter({ has: page.getByText(kind, { exact: true }) })
        .click();
      await wizard.getByRole('button', { name: 'Next' }).click();
      await fill(wizard);
      await wizard.getByRole('button', { name: 'Next' }).click();
      await wizard.locator('#name').fill(name);
      await wizard.getByRole('button', { name: 'Add Source' }).click();
      await page.waitForTimeout(3_000);
      const error =
        (await wizard
          .locator('.form-error')
          .innerText()
          .catch(() => '')) || '';
      if (error) {
        await shot(
          page,
          `21-${kind.toLowerCase().replace(/\W+/g, '-')}-refused`,
        );
        await wizard.getByRole('button', { name: 'Cancel' }).click();
        return `wizard refused: ${error}`;
      }
      await expect(wizard).toBeHidden({ timeout: 30_000 });
      const card = page.locator('.source-card', { hasText: name });
      await expect(card).toBeVisible({ timeout: 30_000 });
      await card.getByRole('button', { name: 'Verify' }).click();
      await page.waitForTimeout(8_000);
      const status = await card.locator('.source-status').innerText();
      const banner =
        (await page
          .locator('.sources-banner')
          .innerText()
          .catch(() => '')) || '';
      await shot(page, `21-${kind.toLowerCase().replace(/\W+/g, '-')}-verify`);
      await card.getByRole('button', { name: 'Delete' }).click();
      await expect(card).toHaveCount(0, { timeout: 30_000 });
      return `added; verify -> ${status}; ${banner}`.trim();
    }

    const kinds = await (async () => {
      await page
        .locator('.sources-header')
        .getByRole('button', { name: 'Add source' })
        .click();
      const wizard = page.getByRole('dialog');
      const names = await wizard
        .locator('.source-type-option .source-type-name')
        .allInnerTexts();
      await wizard.getByRole('button', { name: 'Cancel' }).click();
      return names;
    })();

    outcomes.gitlab = await add(
      'GitLab',
      async (w) => {
        await w.locator('#glProjectId').fill('gitlab-org/gitlab');
        await w.locator('#credentials').fill('glpat-not-a-real-token');
      },
      'GitLab bad token',
    );
    outcomes.web_url = await add(
      'Web URL',
      async (w) => w.locator('#webUrl').fill('https://example.com/'),
      'Example URL',
    );
    outcomes.text = await add(
      'Text',
      async (w) => {
        await w.locator('#textLabel').fill('Notes');
        await w
          .locator('#textContent')
          .fill('The live pass wrote this text source.');
      },
      'Text notes',
    );
    for (const kind of ['Notion', 'Slack', 'Email', 'Calendar']) {
      outcomes[kind.toLowerCase()] = kinds.includes(kind)
        ? 'offered although the server has no adapter for it'
        : 'not offered (no adapter on the server)';
    }
    const reachedVerify = (outcome: string) =>
      outcome.startsWith('added; verify ->');
    const unoffered = ['Notion', 'Slack', 'Email', 'Calendar'].filter(
      (kind) => !kinds.includes(kind),
    );
    const works =
      reachedVerify(outcomes.gitlab) &&
      reachedVerify(outcomes.web_url) &&
      reachedVerify(outcomes.text) &&
      unoffered.length === 4;
    const refused = Object.entries(outcomes)
      .filter(([, outcome]) => outcome.startsWith('wizard refused'))
      .map(([kind, outcome]) => `${kind}: ${outcome}`);
    record(21, {
      result: works ? 'WORKS' : 'FAILS',
      cause: works
        ? undefined
        : refused.length > 0
          ? `product: the wizard offers a kind the server refuses (${refused.join('; ')})`
          : unoffered.length < 4
            ? `product: the wizard offers a kind without an adapter (${['Notion', 'Slack', 'Email', 'Calendar'].filter((kind) => kinds.includes(kind)).join(', ')})`
            : `product: a kind did not reach Verify (${JSON.stringify(outcomes)})`,
      note: 'GitLab is verified with a deliberately bad token, so its verdict is the adapter refusing the credential, which is the point: the adapter answered',
      wizard_kinds: kinds,
      outcomes,
      screenshots: [
        '21-gitlab-verify.png',
        '21-web-url-refused.png',
        '21-text-verify.png',
      ],
    });
  });

  test('22: the GitHub source is indexed, found on Context Search, and cited in chat', async ({
    page,
  }) => {
    const id = await sourceId(SOURCE);
    expect(id, 'row 20 added the source').toBeTruthy();
    const token = await ownerToken();
    await expect
      .poll(
        async () => {
          const { body } = await api(
            'GET',
            `/api/workspaces/${state.owner.workspace.id}/sources/${id}`,
            { token },
          );
          return JSON.stringify(body);
        },
        { timeout: 600_000, intervals: [5_000] },
      )
      .toMatch(
        /"index_status":\s*"(completed|indexed|ready)"|"items_indexed":\s*[1-9]/,
      );
    const detail = await api(
      'GET',
      `/api/workspaces/${state.owner.workspace.id}/sources/${id}`,
      { token },
    );
    const items = sql(
      `select count(*) from content_items where source_id = '${id}'`,
    ).join(',');
    const sample = sql(
      `select uri from content_items where source_id = '${id}' order by uri limit 8`,
    );

    await signIn(page);
    await page.goto('/search');
    await page.getByRole('tab', { name: 'Hybrid' }).click();
    const pill = page.locator('.source-pill', { hasText: SOURCE });
    if (await pill.count()) await pill.click();
    await page
      .getByPlaceholder('Search your knowledge base...')
      .fill('widget area calculation');
    await page.getByRole('button', { name: 'Search', exact: true }).click();
    await page.waitForTimeout(6_000);
    const cards = page.locator('.result-card');
    const results = await cards.count();
    const first = results
      ? (await cards.first().innerText()).replace(/\s+/g, ' ')
      : '';
    await shot(page, '22-context-search-github');

    record(22, {
      result: results > 0 && Number(items) > 0 ? 'WORKS' : 'FAILS',
      cause:
        results > 0
          ? undefined
          : `no result for the repository on the Context Search page (content_items=${items})`,
      source_detail: JSON.stringify(detail.body).slice(0, 300),
      content_items: items,
      sample_uris: sample,
      results,
      first_result: first.slice(0, 300),
      note: 'Chat citation of repository content is checked in row 37',
      screenshots: ['22-context-search-github.png'],
    });
    expect(results).toBeGreaterThan(0);
  });

  test('23 and 24: projects are created, edited through the statuses, linked, synced and deleted', async ({
    page,
  }) => {
    await signIn(page);
    const token = await ownerToken();
    for (const name of [
      REPO_PROJECT,
      SCRATCH_PROJECT,
      BROKEN_PROJECT,
      'Real pass throwaway',
    ]) {
      const existing = await projectId(name);
      if (existing) await api('DELETE', `/api/projects/${existing}`, { token });
    }
    await createProjectInConsole(page, REPO_PROJECT, SOURCE, 'Active');
    await shot(page, '23-project-created');
    await createProjectInConsole(page, SCRATCH_PROJECT, null, 'Active');
    await createProjectInConsole(page, BROKEN_PROJECT, null, 'Active');
    await createProjectInConsole(page, 'Real pass throwaway', null, 'Active');

    await page.locator('.project-card', { hasText: REPO_PROJECT }).click();
    const details = page.locator('aside.project-details');
    await expect(details).toBeVisible();
    const statuses: string[] = [];
    const editResponses: string[] = [];
    for (const [value, label] of [
      ['on_hold', 'On Hold'],
      ['cancelled', 'Cancelled'],
      ['active', 'Active'],
    ]) {
      await details.getByRole('button', { name: 'Edit Project' }).click();
      const modal = page.getByRole('dialog', { name: 'Edit Project' });
      await modal.locator('#edit-status').selectOption(value);
      const saved = page
        .waitForResponse(
          (r) =>
            /\/api\/projects\/[^/]+$/.test(r.url()) &&
            ['PUT', 'PATCH'].includes(r.request().method()),
          { timeout: 30_000 },
        )
        .catch(() => null);
      await modal.getByRole('button', { name: 'Save Changes' }).click();
      const response = await saved;
      editResponses.push(
        `${label}: ${response ? response.request().method() : 'no'} -> ${response ? response.status() : 'no request'} ${response ? (await response.text()).slice(0, 120) : ''}`,
      );
      await page.waitForTimeout(1_500);
      if (await modal.isVisible().catch(() => false)) {
        editResponses.push(
          `${label}: modal stayed open: ${(await modal.innerText()).replace(/\s+/g, ' ').slice(0, 160)}`,
        );
        await modal.getByRole('button', { name: 'Cancel' }).click();
      }
      statuses.push(
        `${label}: ${await page.locator('.project-card', { hasText: REPO_PROJECT }).locator('.ui-badge').first().innerText()}`,
      );
      if (label === 'Cancelled') await shot(page, '23-project-cancelled');
    }
    const dbStatus = sql(
      `select status from projects where name = '${REPO_PROJECT}'`,
    ).join(',');

    const unlinkResponse = page
      .waitForResponse(
        (r) =>
          /\/api\/projects\/[^/]+\/source$/.test(r.url()) &&
          r.request().method() === 'DELETE',
      )
      .catch(() => null);
    const unlink = details.getByRole('button', { name: 'Unlink' });
    let unlinkStatus = 'no Unlink button';
    if (await unlink.count()) {
      await unlink.click();
      const response = await Promise.race([
        unlinkResponse,
        page.waitForTimeout(8_000).then(() => null),
      ]);
      unlinkStatus = response
        ? `DELETE /api/projects/{id}/source -> ${response.status()}`
        : 'no request observed';
    }
    await page.waitForTimeout(1_000);
    const linkedAfterUnlink = sql(
      `select source_id is not null from projects where name = '${REPO_PROJECT}'`,
    ).join(',');
    await shot(page, '23-project-after-unlink');

    // External sync (row 24).
    await details.getByRole('button', { name: '+ Add Sync' }).click();
    const sync = page.getByRole('dialog', { name: 'Add External Sync' });
    await expect(sync).toBeVisible();
    const providers = await sync
      .locator('#sync-provider option')
      .allInnerTexts();
    const directions = await sync
      .locator('#sync-direction option')
      .allInnerTexts();
    await sync.locator('#sync-provider').selectOption('github');
    await sync.locator('#sync-direction').selectOption('bidirectional');
    await sync.locator('#sync-repo-url').fill(REPO);
    await shot(page, '24-add-sync-form');
    const syncResponse = page.waitForResponse(
      (r) =>
        /\/api\/projects\/[^/]+\/sync$/.test(r.url()) &&
        r.request().method() === 'POST',
    );
    await sync.getByRole('button', { name: 'Add Sync Config' }).click();
    const syncStatus = (await syncResponse).status();
    await page.waitForTimeout(2_000);
    const syncText = (
      await details
        .locator('.sync-config-section')
        .innerText()
        .catch(() => '')
    ).replace(/\s+/g, ' ');
    const syncRows = sql(`select count(*) from sync_configs`).join(',');
    const syncedItems = sql(`select count(*) from synced_items`).join(',');
    await shot(page, '24-sync-after-add');
    if (await sync.isVisible().catch(() => false))
      await sync.getByRole('button', { name: 'Cancel' }).click();

    // Delete the throwaway project from its details pane.
    await page
      .locator('.project-card', { hasText: 'Real pass throwaway' })
      .click();
    await page
      .locator('aside.project-details')
      .getByRole('button', { name: 'Delete', exact: true })
      .click();
    const confirm = page.getByRole('dialog', { name: 'Delete Project' });
    await confirm.getByRole('button', { name: 'Delete Project' }).click();
    await expect(
      page.locator('.project-card', { hasText: 'Real pass throwaway' }),
    ).toHaveCount(0, { timeout: 30_000 });
    await shot(page, '23-project-deleted');

    const wizardHadGithubOption = false;
    record(23, {
      result:
        statuses[0].includes('On Hold') &&
        statuses[1].includes('Cancelled') &&
        unlinkStatus.includes('-> 2')
          ? 'WORKS'
          : 'FAILS',
      cause:
        statuses[0].includes('On Hold') && statuses[1].includes('Cancelled')
          ? unlinkStatus.includes('-> 2')
            ? undefined
            : `product: Unlink calls ${unlinkStatus}; the server registers no /api/projects/{id}/source route and the console shows no error`
          : `product: editing the status does not stick: ${editResponses.join(' || ').slice(0, 300)}`,
      statuses_after_edit: statuses,
      edit_responses: editResponses,
      status_in_db_at_end: dbStatus,
      unlink: unlinkStatus,
      project_still_linked_in_db: linkedAfterUnlink,
      wizard_source_choices:
        'No Source or an existing workspace source; no separate GitHub Repo choice',
      github_repo_choice_in_wizard: wizardHadGithubOption,
      screenshots: [
        '23-project-created.png',
        '23-project-cancelled.png',
        '23-project-after-unlink.png',
        '23-project-deleted.png',
      ],
    });
    record(24, {
      result: syncStatus < 300 && Number(syncRows) > 0 ? 'WORKS' : 'FAILS',
      cause:
        syncStatus < 300
          ? undefined
          : `product: + Add Sync posts to /api/projects/{id}/sync and the server answers ${syncStatus}; nothing is synced`,
      providers,
      directions,
      rollout_and_scope_controls:
        'none; the form offers Provider and Direction only',
      add_sync_status: syncStatus,
      sync_section_text: syncText.slice(0, 200),
      sync_configs_in_db: syncRows,
      synced_items_in_db: syncedItems,
      screenshots: ['24-add-sync-form.png', '24-sync-after-add.png'],
    });
    expect(syncStatus, 'Add Sync Config is accepted').toBeLessThan(300);
  });

  test('25: a read-only repository task runs from the wizard and completes with its activity', async ({
    page,
  }) => {
    const token = await ownerToken();
    const project = await projectId(REPO_PROJECT);
    expect(project, 'row 23 created the repository project').toBeTruthy();
    // The console has no control that links a project to a repository the
    // server may check out, so the API the publication lane uses is called here.
    const linked = await api('POST', `/api/projects/${project}/github`, {
      token,
      body: { repo_url: REPO, access_token: TOKEN },
    });
    expect([200, 201], JSON.stringify(linked.body)).toContain(linked.status);

    await signIn(page);
    const s = stamp();
    const title = `Read the widget checks ${s}`;
    const taskId = await createTask(page, {
      title,
      description:
        'Read README.md, check.sh and src/widget.py in this repository and summarise in three sentences what the CI check does and what makes it fail. Do not change any file.',
      project: REPO_PROJECT,
      source: SOURCE,
    });
    await shot(page, '25-task-created');
    const mark = logMark();
    await startRun(page, title);
    await expect(page.locator('.log-entry').first()).toBeVisible({
      timeout: 600_000,
    });
    await shot(page, '25-execution-logs-streaming');
    const run = await finished(taskId);
    await page.waitForTimeout(2_000);
    const dialogText = (
      await page
        .getByRole('dialog')
        .innerText({ timeout: 5_000 })
        .catch(() => 'execution dialog no longer open')
    ).replace(/\s+/g, ' ');
    await shot(page, '25-run-finished');
    const calls = toolCalls(run.id);
    const logs = runLogs(run.id);
    const reads = calls.filter((c) =>
      /^(read_file|read_repository_file|run_shell|run_command|list_files|search)\w* \| call/.test(
        c,
      ),
    );
    record(25, {
      result:
        run.status === 'completed' && reads.length > 0 ? 'WORKS' : 'FAILS',
      task_id: taskId,
      run_id: run.id,
      run_status: run.status,
      link_via_api: `POST /api/projects/{id}/github -> ${linked.status} (no console control)`,
      tool_calls: calls,
      logs_tail: logs.slice(-8),
      dialog_text: dialogText.slice(0, 500),
      server_log: logLines(mark, /tool=|Killed the background|checkout/i).slice(
        0,
        6,
      ),
      screenshots: [
        '25-task-created.png',
        '25-execution-logs-streaming.png',
        '25-run-finished.png',
      ],
    });
    expect(run.status).toBe('completed');
    expect(reads.length, `tool calls: ${calls.join(' || ')}`).toBeGreaterThan(
      0,
    );
  });

  test('26 and 30: plan approval holds a write, Approve carries it out and the change becomes a pull request; Revise sends a second one back', async ({
    page,
  }) => {
    await signIn(page);
    const s = stamp();
    const title = `Add the live note ${s}`;
    const taskId = await createTask(page, {
      title,
      description: `Create a new file live-verify/${s}.md at the repository root containing the heading "# Live verification ${s}" and one sentence saying this file was written by a Zone task run. Then read it back.`,
      project: REPO_PROJECT,
      planApproval: true,
      source: SOURCE,
    });
    await startRun(page, title);
    const parked = await waiting(taskId);
    const prompt = page.locator('.execution-question');
    let planText = '';
    let held = '';
    if (parked.status === 'waiting') {
      await expect(prompt).toBeVisible({ timeout: 60_000 });
      planText = (
        await prompt
          .locator('[data-testid="question-preview"]')
          .innerText()
          .catch(() => '')
      ).replace(/\s+/g, ' ');
      held = (
        await page.getByRole('dialog').locator('.execution-summary').innerText()
      ).replace(/\s+/g, ' ');
      await shot(page, '26-plan-approval-parked');
      await prompt.getByRole('radio', { name: 'Approve' }).check();
      await prompt.locator('[data-testid="question-submit"]').click();
    }
    const done = await finished(taskId);
    await shot(page, '26-approved-run-finished');
    const calls = toolCalls(done.id);
    const refusedWrite = calls.find(
      (c) =>
        /^(write_file|apply_patch) \| failed/.test(c) &&
        /plan|approv|refus|hold/i.test(c),
    );
    const writes = calls.filter((c) =>
      /^(write_file|apply_patch) \| ok/.test(c),
    );
    record(26, {
      result:
        parked.status === 'waiting' &&
        done.status === 'completed' &&
        writes.length > 0
          ? 'WORKS'
          : 'FAILS',
      task_id: taskId,
      run_id: done.id,
      parked_status: parked.status,
      summary_while_parked: held,
      plan_preview: planText.slice(0, 400),
      early_write_refused:
        refusedWrite ?? 'no refused write recorded before the plan',
      writes_after_approval: writes,
      tool_calls: calls,
      screenshots: [
        '26-plan-approval-parked.png',
        '26-approved-run-finished.png',
      ],
    });

    // Row 30: the change was pushed and opened as a pull request.
    const task = await expect
      .poll(async () => (await taskById(taskId)).pr_url ?? null, {
        timeout: 600_000,
        intervals: [5_000],
      })
      .not.toBeNull()
      .then(() => taskById(taskId));
    await page.goto('/tasks');
    const card = page.locator('.task-card', { hasText: title });
    await expect(card).toContainText(/PR: open/, { timeout: 60_000 });
    const link = card.locator('a.task-pr-link');
    await expect(link).toHaveText('View Pull Request');
    const prUrl = String(task.pr_url);
    await shot(page, '30-task-pr-open');
    const number = prUrl.match(/pull\/(\d+)/)?.[1] ?? '';
    const merge = execFileSync(
      'gh',
      [
        'pr',
        'merge',
        number,
        '-R',
        `${OWNER}/${NAME}`,
        '--squash',
        '--delete-branch',
      ],
      {
        encoding: 'utf8',
        env: {
          ...process.env,
          GH_CONFIG_DIR:
            process.env.GH_CONFIG_DIR ?? `${process.env.HOME}/.config/gh`,
        },
      },
    ).trim();
    await expect
      .poll(async () => String((await taskById(taskId)).pr_status ?? ''), {
        timeout: 900_000,
        intervals: [10_000],
      })
      .toBe('merged');
    await page.reload();
    await expect(page.locator('.task-card', { hasText: title })).toContainText(
      /PR: merged/,
      { timeout: 60_000 },
    );
    await shot(page, '30-task-pr-merged');
    const branchGone = execFileSync(
      'gh',
      ['api', `repos/${OWNER}/${NAME}/branches`, '--jq', '.[].name'],
      {
        encoding: 'utf8',
        env: {
          ...process.env,
          GH_CONFIG_DIR:
            process.env.GH_CONFIG_DIR ?? `${process.env.HOME}/.config/gh`,
        },
      },
    );
    record(30, {
      result: 'WORKS',
      task_id: taskId,
      pr_url: prUrl,
      branch: task.branch_name,
      merge_output: merge.slice(0, 200),
      branches_after_merge: branchGone.split('\n').filter(Boolean),
      screenshots: ['30-task-pr-open.png', '30-task-pr-merged.png'],
    });

    // A second task whose plan is sent back rather than approved.
    const s2 = stamp();
    const title2 = `Rename the widget ${s2}`;
    const task2 = await createTask(page, {
      title: title2,
      description: `Rename the function area in src/widget.py to widget_area-${s2} and update check.sh accordingly.`,
      project: REPO_PROJECT,
      planApproval: true,
      source: SOURCE,
    });
    await startRun(page, title2);
    const parked2 = await waiting(task2);
    const prompt2 = page.locator('.execution-question');
    const options = await prompt2
      .getByRole('radio')
      .evaluateAll((els) =>
        els.map((e) => (e as HTMLInputElement).labels?.[0]?.innerText ?? ''),
      );
    await prompt2.getByRole('radio', { name: 'Revise' }).check();
    const other = prompt2.locator('[data-testid="question-free-text"]');
    if (await other.isEnabled().catch(() => false))
      await other.fill(
        'Do not make this change. Stop here without editing any file.',
      );
    await shot(page, '26-plan-revise');
    await prompt2.locator('[data-testid="question-submit"]').click();
    const done2 = await finished(task2);
    const calls2 = toolCalls(done2.id);
    const writes2 = calls2.filter((c) =>
      /^(write_file|apply_patch) \| ok/.test(c),
    );
    await shot(page, '26-plan-revise-outcome');
    record(26.5, {
      result:
        parked2.status === 'waiting' && writes2.length === 0
          ? 'WORKS'
          : 'FAILS',
      note: 'The plan card offers Approve and Revise; there is no Reject. Revise with an instruction to stop was sent instead.',
      options,
      task_id: task2,
      run_id: done2.id,
      run_status: done2.status,
      writes: writes2,
      tool_calls: calls2.slice(0, 12),
      screenshots: ['26-plan-revise.png', '26-plan-revise-outcome.png'],
    });
    expect(parked.status).toBe('waiting');
    expect(done.status).toBe('completed');
    expect(writes.length).toBeGreaterThan(0);
  });

  test('27: a run that asks a question waits for the answer given in the execution view', async ({
    page,
  }) => {
    await signIn(page);
    const s = stamp();
    const title = `Colour file ${s}`;
    const taskId = await createTask(page, {
      title,
      description: `Before doing anything, ask me which colour I want (offer red, green and blue as choices) and wait for my answer. Then create a file named <colour>-${s}.txt in the working directory containing that colour name, and read it back.`,
      project: SCRATCH_PROJECT,
    });
    await startRun(page, title);
    const parked = await waiting(taskId);
    const summary = page.getByRole('dialog').locator('.execution-summary');
    let question = '';
    if (parked.status === 'waiting') {
      await expect(summary).toContainText(/Waiting for you/, {
        timeout: 60_000,
      });
      const prompt = page.locator('.execution-question');
      question = (await prompt.innerText()).replace(/\s+/g, ' ');
      await shot(page, '27-waiting-for-you');
      const green = prompt.getByRole('radio', { name: /green/i });
      if (await green.count()) {
        await green.first().check();
      } else {
        await prompt.getByRole('radio').first().check();
      }
      await prompt.locator('[data-testid="question-submit"]').click();
    }
    const done = await finished(taskId);
    await shot(page, '27-resumed-and-finished');
    const calls = toolCalls(done.id);
    const asked = calls.filter((c) => c.startsWith('ask_user | call'));
    const wrote = calls.filter(
      (c) => /^write_file \| call/.test(c) && /green/i.test(c),
    );
    record(27, {
      result:
        parked.status === 'waiting' &&
        done.status === 'completed' &&
        asked.length > 0
          ? 'WORKS'
          : 'FAILS',
      task_id: taskId,
      run_id: done.id,
      parked_status: parked.status,
      question: question.slice(0, 400),
      ask_user_calls: asked,
      write_with_answer: wrote,
      tool_calls: calls,
      screenshots: ['27-waiting-for-you.png', '27-resumed-and-finished.png'],
    });
    expect(parked.status).toBe('waiting');
    expect(done.status).toBe('completed');
    expect(asked.length).toBeGreaterThan(0);
  });

  test('28: a run backgrounds a long command, waits on it and tails its log', async ({
    page,
  }) => {
    await signIn(page);
    const s = stamp();
    const title = `Background job ${s}`;
    const taskId = await createTask(page, {
      title,
      description: `Start a shell command in the background that sleeps for 20 seconds and then prints JOB-DONE-${s}. Do not block on it in the foreground: wait for the background job to finish, then read its log and report the exact line it printed.`,
      project: SCRATCH_PROJECT,
    });
    await startRun(page, title);
    const done = await finished(taskId);
    await shot(page, '28-background-job-run');
    const calls = toolCalls(done.id);
    const background = calls.filter(
      (c) =>
        /^run_(shell|command) \| call/.test(c) &&
        /\\"background\\":\s*true|"background":\s*true/.test(c),
    );
    const waits = calls.filter((c) => c.startsWith('wait_for | call'));
    const tails = calls.filter((c) => c.startsWith('tail_job | call'));
    const logs = runLogs(done.id);
    record(28, {
      result:
        done.status === 'completed' &&
        background.length > 0 &&
        waits.length > 0 &&
        tails.length > 0
          ? 'WORKS'
          : 'FAILS',
      cause:
        background.length > 0 && waits.length > 0 && tails.length > 0
          ? undefined
          : `model: calls made were ${calls.map((c) => c.split(' | ')[0]).join(', ')}`,
      task_id: taskId,
      run_id: done.id,
      run_status: done.status,
      background_calls: background,
      wait_for_calls: waits,
      tail_job_calls: tails,
      logs_with_job_output: logs
        .filter((l) => l.includes(`JOB-DONE-${s}`))
        .slice(0, 3),
      tool_calls: calls,
      screenshots: ['28-background-job-run.png'],
    });
    expect(done.status).toBe('completed');
    expect(background.length).toBeGreaterThan(0);
    expect(waits.length).toBeGreaterThan(0);
    expect(tails.length).toBeGreaterThan(0);
  });

  test('29: a refused command is corrected in the same attempt', async ({
    page,
  }) => {
    await signIn(page);
    const s = stamp();
    const title = `Semicolon text ${s}`;
    const taskId = await createTask(page, {
      title,
      description: `Using a single echo command, print exactly this text to the terminal: alpha; beta; gamma-${s}. Report the command you ran and its output.`,
      project: SCRATCH_PROJECT,
    });
    await startRun(page, title);
    const done = await finished(taskId);
    await shot(page, '29-refused-then-corrected');
    const calls = toolCalls(done.id);
    const runs = await runsOf(taskId);
    const refused = calls.filter((c) =>
      /^run_(shell|command) \| failed/.test(c),
    );
    const corrected = calls.filter((c) => /^run_(shell|command) \| ok/.test(c));
    const logs = runLogs(done.id);
    record(29, {
      result:
        runs.length === 1 && refused.length > 0 && corrected.length > 0
          ? 'WORKS'
          : 'FAILS',
      cause:
        refused.length > 0
          ? undefined
          : `model: no call carried a ; argument; calls were ${calls.map((c) => c.split(' | ')[0]).join(', ')}`,
      task_id: taskId,
      runs_for_task: runs.length,
      run_status: done.status,
      refused_calls: refused,
      corrected_calls: corrected,
      logs_tail: logs.slice(-6),
      tool_calls: calls,
      screenshots: ['29-refused-then-corrected.png'],
    });
    expect(runs.length, 'the run did not restart').toBe(1);
    expect(refused.length).toBeGreaterThan(0);
    expect(corrected.length).toBeGreaterThan(0);
  });

  test('31: Run Again reruns a finished task, a failing run shows its error and Try Again, and a task is deleted', async ({
    page,
  }) => {
    await signIn(page);
    const token = await ownerToken();
    page.on('dialog', (d) => d.accept());
    const { body } = await api(
      'GET',
      `/api/workspaces/${state.owner.workspace.id}/tasks`,
      { token },
    );
    const readTask = (
      (body as { tasks?: { id: string; title: string }[] }).tasks ?? []
    ).find((t) => t.title.startsWith('Read the widget checks'));
    expect(readTask, 'row 25 left a finished task').toBeTruthy();
    const before = (await runsOf(readTask!.id)).length;
    await page.goto('/tasks');
    await page
      .locator('.task-card', { hasText: readTask!.title })
      .getByRole('button', { name: 'Execute' })
      .click();
    const again = page.getByRole('button', { name: 'Run Again' });
    await expect(again).toBeVisible({ timeout: 30_000 });
    await shot(page, '31-run-again-offered');
    await again.click();
    await expect(page.locator('.execution-logs')).toContainText(
      'Execution Logs',
    );
    const rerun = await finished(readTask!.id, 1_800_000, before + 1);
    const after = (await runsOf(readTask!.id)).length;
    await shot(page, '31-run-again-finished');

    const broken = await projectId(BROKEN_PROJECT);
    await api('POST', `/api/projects/${broken}/github`, {
      token,
      body: {
        repo_url: `https://github.com/${OWNER}/zone-tests-does-not-exist-${stamp()}`,
        access_token: TOKEN,
      },
    });
    const s = stamp();
    const title = `Doomed run ${s}`;
    const taskId = await createTask(page, {
      title,
      description: 'List the files at the repository root.',
      project: BROKEN_PROJECT,
    });
    await startRun(page, title);
    const failed = await finished(taskId, 600_000);
    await page.waitForTimeout(2_000);
    const dialog = page.getByRole('dialog');
    const notice = (await dialog.locator('.execution-notice').allInnerTexts())
      .join(' | ')
      .replace(/\s+/g, ' ');
    const tryAgain = await dialog
      .getByRole('button', { name: 'Try Again' })
      .isVisible()
      .catch(() => false);
    const runAgain = await dialog
      .getByRole('button', { name: 'Run Again' })
      .isVisible()
      .catch(() => false);
    await shot(page, '31-failed-run');
    await dialog
      .getByRole('button', { name: 'Close' })
      .first()
      .click()
      .catch(() => page.keyboard.press('Escape'));

    await page.goto('/tasks');
    await page
      .locator('.task-card', { hasText: title })
      .getByRole('button', { name: 'Delete' })
      .click();
    await expect(page.locator('.task-card', { hasText: title })).toHaveCount(
      0,
      { timeout: 30_000 },
    );
    const gone = sql(`select count(*) from tasks where id = '${taskId}'`).join(
      ',',
    );
    await shot(page, '31-task-deleted');
    record(31, {
      result:
        rerun.status === 'completed' &&
        after === before + 1 &&
        failed.status === 'failed' &&
        (tryAgain || runAgain) &&
        gone === '0'
          ? 'WORKS'
          : 'FAILS',
      note: 'After a failed run the dialog offers "Run Again"; "Try Again" is the label after a start error only',
      rerun_status: rerun.status,
      runs_before_and_after: [before, after],
      failed_run_status: failed.status,
      failed_run_error: failed.error_message,
      failure_notice: notice.slice(0, 300),
      try_again_button: tryAgain,
      run_again_button_after_failure: runAgain,
      task_rows_after_delete: gone,
      screenshots: [
        '31-run-again-offered.png',
        '31-run-again-finished.png',
        '31-failed-run.png',
        '31-task-deleted.png',
      ],
    });
    expect(rerun.status).toBe('completed');
    expect(failed.status).toBe('failed');
    expect(
      tryAgain || runAgain,
      `after a failure the dialog offers ${notice}`,
    ).toBe(true);
    expect(gone).toBe('0');
  });
});
