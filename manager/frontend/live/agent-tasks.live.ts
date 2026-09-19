import { type Locator, type Page } from '@playwright/test';
import { api, expect, signIn, state, test, tokenFor } from './harness';
import { latest, roundsFor, script, settledRounds, stamp } from './stub';

/**
 * Task runs driven from the console with a scripted model behind them: the
 * plan-approval hold, a run parked on a question, a run parked on a wait, a
 * refused command corrected in the same attempt, and (opt-in) a run that works
 * in a worktree and publishes a pull request.
 */

interface Run {
  id: string;
  status: string;
  pending_question?: unknown;
  waiting_on?: unknown;
  plan?: string;
}

async function owner(): Promise<string> {
  return tokenFor(state.owner);
}

interface Project {
  id: string;
  name: string;
}

function isProject(value: unknown): value is Project {
  const candidate = value as Partial<Project> | null;
  return typeof candidate?.id === 'string' && typeof candidate?.name === 'string';
}

function isRun(value: unknown): value is Run {
  const candidate = value as Partial<Run> | null;
  return typeof candidate?.id === 'string' && typeof candidate?.status === 'string';
}

/** A response the server answered with success and a body of the expected shape. */
async function fetched<T>(
  method: string,
  path: string,
  pick: (body: unknown) => T | undefined,
  options: { token: string; body?: unknown; ok?: number[] }
): Promise<T> {
  const { status, body } = await api(method, path, { token: options.token, body: options.body });
  if (!(options.ok ?? [200]).includes(status)) {
    throw new Error(`${method} ${path}: ${status} ${JSON.stringify(body).slice(0, 300)}`);
  }
  const picked = pick(body);
  if (picked === undefined) {
    throw new Error(`${method} ${path} answered an unexpected shape: ${JSON.stringify(body).slice(0, 300)}`);
  }
  return picked;
}

async function ensureProject(name: string): Promise<string> {
  const token = await owner();
  const projects = await fetched(
    'GET',
    `/api/projects?workspace_id=${state.owner.workspace.id}`,
    (body) => {
      const list = (body as { projects?: unknown }).projects;
      return Array.isArray(list) && list.every(isProject) ? (list as Project[]) : undefined;
    },
    { token }
  );
  const existing = projects.find((p) => p.name === name);
  if (existing) return existing.id;
  const created = await fetched(
    'POST',
    '/api/projects',
    (body) => {
      const project = (body as { project?: unknown }).project ?? body;
      return isProject(project) ? project : undefined;
    },
    { token, body: { name, description: 'live verification', workspace_id: state.owner.workspace.id }, ok: [200, 201] }
  );
  return created.id;
}

async function taskIdByTitle(title: string): Promise<string> {
  const token = await owner();
  const tasks = await fetched(
    'GET',
    `/api/workspaces/${state.owner.workspace.id}/tasks`,
    (body) => {
      const list = (body as { tasks?: unknown }).tasks;
      return Array.isArray(list) ? (list as { id: string; title: string }[]) : undefined;
    },
    { token }
  );
  const found = tasks.find((t) => t.title === title);
  if (!found) throw new Error(`no task titled ${title} among ${tasks.length} tasks`);
  return found.id;
}

async function runsOf(taskId: string): Promise<Run[]> {
  const token = await owner();
  return fetched(
    'GET',
    `/api/tasks/${taskId}/runs`,
    (body) => {
      const list = (body as { runs?: unknown }).runs;
      return Array.isArray(list) && list.every(isRun) ? (list as Run[]) : undefined;
    },
    { token }
  );
}

async function run(runId: string): Promise<Run> {
  const token = await owner();
  return fetched(
    'GET',
    `/api/tasks/runs/${runId}`,
    (body) => {
      const detail = (body as { run?: unknown }).run ?? body;
      return isRun(detail) ? detail : undefined;
    },
    { token }
  );
}

async function toggle(dialog: Locator, title: string, wanted: boolean): Promise<void> {
  const label = dialog.locator('label.toggle-label', { hasText: title });
  await expect(label).toBeVisible();
  const box = label.locator('input[type="checkbox"]');
  if ((await box.isChecked()) !== wanted) await label.locator('.toggle-slider').click();
  await expect(box).toBeChecked({ checked: wanted });
}

/** The New Task wizard, step by step, as a person fills it in. */
async function createTask(
  page: Page,
  options: { title: string; description: string; project: string; planApproval?: boolean }
): Promise<string> {
  await page.goto('/tasks');
  await page.getByRole('button', { name: /new task/i }).click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  // The whole name, not a prefix of it: a project named for the repository
  // lane shares its first words with the plain one, and a task filed under it
  // would work in a worktree and publish.
  await dialog
    .locator('.project-selection-option')
    .filter({ has: page.getByText(options.project, { exact: true }) })
    .first()
    .click();
  await dialog.getByRole('button', { name: 'Next' }).click();
  await dialog.locator('#task-title').fill(options.title);
  await dialog.locator('#task-description').fill(options.description);
  await dialog.getByRole('button', { name: 'Next' }).click();
  // The toggles hide their native checkbox behind a slider, so the label is what a person clicks.
  await toggle(dialog, 'Enable Agentic Mode', true);
  if (options.planApproval) await toggle(dialog, 'Require plan approval', true);
  await dialog.getByRole('button', { name: 'Create Task' }).click();
  await expect(dialog).toBeHidden({ timeout: 30_000 });
  await expect(page.locator('.task-card', { hasText: options.title })).toBeVisible();
  return taskIdByTitle(options.title);
}

async function startRun(page: Page, title: string): Promise<void> {
  await page.locator('.task-card', { hasText: title }).getByRole('button', { name: 'Execute' }).click();
  await page.getByRole('button', { name: /start execution/i }).click();
  await expect(page.locator('.execution-logs')).toContainText('Execution Logs');
}

async function waitingRun(taskId: string, timeout = 180_000): Promise<Run> {
  await expect
    .poll(async () => (await runsOf(taskId)).find((r) => r.status === 'waiting')?.id ?? null, {
      timeout,
      intervals: [1_000],
    })
    .not.toBeNull();
  const parked = (await runsOf(taskId)).find((r) => r.status === 'waiting');
  if (!parked) throw new Error('the run left waiting before it was read');
  return parked;
}

async function finished(taskId: string, timeout = 240_000): Promise<Run> {
  await expect
    .poll(async () => (await runsOf(taskId)).map((r) => r.status).join(','), { timeout, intervals: [1_000] })
    .toMatch(/completed|failed|cancelled/);
  const [latest] = await runsOf(taskId);
  return latest;
}

const PROJECT = 'Live verification';

test('a task that requires plan approval refuses a write, parks on its plan, and carries it out once approved', async ({
  page,
  consoleErrors,
}) => {
  test.setTimeout(420_000);
  const s = stamp();
  // The resumed turn's trigger is the server's own words, so it is read from here on.
  const since = await latest();
  await script(`PLANTASK ${s}`, [
    { calls: [{ name: 'write_file', arguments: { path: 'notes.txt', content: 'too early', reason: 'a write before the plan, which the hold refuses' } }] },
    { calls: [{ name: 'submit_plan', arguments: { plan: `1. Write notes-${s}.txt with the summary.\n2. Read it back to check.\nLeft out: nothing.` } }] },
  ]);
  await script('Plan approval', [
    { calls: [{ name: 'write_file', arguments: { path: `notes-${s}.txt`, content: `Approved and written ${s}`, reason: 'the approved first step' } }] },
    { calls: [{ name: 'read_file', arguments: { path: `notes-${s}.txt` } }] },
    { text: `Wrote and read notes-${s}.txt.` },
  ]);

  await signIn(page);
  await ensureProject(PROJECT);
  const title = `Plan me ${s}`;
  const taskId = await createTask(page, {
    title,
    description: `PLANTASK ${s}: write a note and read it back.`,
    project: PROJECT,
    planApproval: true,
  });
  await startRun(page, title);

  const parked = await waitingRun(taskId);
  expect(parked.pending_question).toBeTruthy();
  expect(parked.plan ?? '').toContain(`notes-${s}.txt`);

  const prompt = page.locator('.execution-question');
  await expect(prompt).toBeVisible({ timeout: 60_000 });
  await expect(prompt).toContainText('Plan approval');
  await expect(prompt.locator('[data-testid="question-preview"]')).toContainText(`notes-${s}.txt`);
  await prompt.getByRole('radio', { name: 'Approve' }).check();
  await prompt.locator('[data-testid="question-submit"]').click();

  const done = await finished(taskId);
  expect(done.status).toBe('completed');
  expect((await run(done.id)).plan ?? '').toContain(`notes-${s}.txt`);

  const held = await roundsFor(`PLANTASK ${s}`);
  expect(held).toHaveLength(2);
  expect(held[0].tools).toContain('submit_plan');
  expect(held[1].tool_results.map((r) => r.content).join('\n')).toMatch(/plan/i);
  const resumed = await settledRounds('Plan approval', 3, 60_000, since);
  expect(resumed[0].tools, 'submit_plan is withdrawn with the hold').not.toContain('submit_plan');
  expect(resumed[2].tool_results.map((r) => r.content).join('\n')).toContain(`Approved and written ${s}`);
  expect(consoleErrors).toEqual([]);
});

test('a task run parks on a question and resumes with the answer given in the console', async ({
  page,
  consoleErrors,
}) => {
  test.setTimeout(420_000);
  const s = stamp();
  await script(`ASKTASK ${s}`, [
    {
      calls: [
        {
          name: 'ask_user',
          arguments: {
            questions: [
              {
                header: 'Colour',
                question: 'Which colour should the banner be?',
                options: [
                  { label: `Blue-${s}`, description: 'The brand colour' },
                  { label: `Red-${s}`, description: 'The alert colour' },
                ],
                required: true,
              },
            ],
          },
        },
      ],
    },
  ]);
  await script(`Blue-${s}`, [{ text: `Blue-${s} it is; the banner is decided.` }]);

  await signIn(page);
  await ensureProject(PROJECT);
  const title = `Ask me ${s}`;
  const taskId = await createTask(page, { title, description: `ASKTASK ${s}: pick the banner colour.`, project: PROJECT });
  await startRun(page, title);

  const parked = await waitingRun(taskId);
  expect(parked.pending_question).toBeTruthy();
  const prompt = page.locator('.execution-question');
  await expect(prompt).toBeVisible({ timeout: 60_000 });
  await expect(prompt).toContainText('Which colour should the banner be?');
  await prompt.getByRole('radio', { name: `Blue-${s}` }).check();
  await prompt.locator('[data-testid="question-submit"]').click();

  const done = await finished(taskId);
  expect(done.status).toBe('completed');
  const answered = await settledRounds(`Blue-${s}`, 1, 60_000);
  expect(answered[0].last_user).toContain('Colour');
  expect(consoleErrors).toEqual([]);
});

test('a task run backgrounds a command, parks on the wait, and finishes with the job output', async ({
  page,
  consoleErrors,
}) => {
  test.setTimeout(420_000);
  const s = stamp();
  const jobId = '$re:^Started ([A-Za-z0-9_-]+) \\(pid';
  await script(`JOBTASK ${s}`, [
    // A task run has run_command, an allow-listed program with arguments, rather than a shell.
    { calls: [{ name: 'run_command', arguments: { command: 'python3', args: ['-c', `print(__import__('time').sleep(8) or 'job-${s}')`], background: true, reason: 'a build that outlives one round' } }] },
    { calls: [{ name: 'wait_for', arguments: { kind: 'job', id: jobId, timeout_secs: 90 } }] },
    { calls: [{ name: 'tail_job', arguments: { id: jobId } }] },
    { text: `The job printed job-${s}.` },
  ]);

  await signIn(page);
  await ensureProject(PROJECT);
  const title = `Job me ${s}`;
  const taskId = await createTask(page, { title, description: `JOBTASK ${s}: run the slow job and wait for it.`, project: PROJECT });
  await startRun(page, title);

  // The park is short-lived and `waiting_on` is on the run itself, so it is
  // read from the run's own endpoint rather than the screen or the list.
  let sawWaiting = false;
  await expect
    .poll(
      async () => {
        const [latest] = await runsOf(taskId);
        if (!latest) return 'none';
        const detail = await run(latest.id);
        if (detail.status === 'waiting' && detail.waiting_on) sawWaiting = true;
        return detail.status;
      },
      { timeout: 240_000, intervals: [250] }
    )
    .toMatch(/completed|failed/);
  const [latest] = await runsOf(taskId);
  expect(latest.status).toBe('completed');
  expect(sawWaiting, 'the run parked on the wait').toBe(true);

  const rounds = await roundsFor(`JOBTASK ${s}`);
  expect(rounds).toHaveLength(4);
  const log = rounds[3].tool_results.map((r) => r.content).join('\n');
  expect(log).toContain(`job-${s}`);
  expect(log).toMatch(/exited 0/);
  await expect(page.locator('.execution-logs')).toContainText(/job-|Waiting|exited/);
  expect(consoleErrors).toEqual([]);
});

test('a task run corrects a refused command in the same attempt rather than starting over', async ({
  page,
  consoleErrors,
}) => {
  test.setTimeout(300_000);
  const s = stamp();
  await script(`FIXTASK ${s}`, [
    // An argument carrying a semicolon is refused. The refusal is a tool result the model can act on.
    { calls: [{ name: 'run_command', arguments: { command: 'python3', args: ['-c', `print('first'); print('fix-${s}')`], reason: 'print the marker' } }] },
    { calls: [{ name: 'run_command', arguments: { command: 'python3', args: ['-c', `print('fix-${s}')`], reason: 'print the marker without the semicolon' } }] },
    { text: `The command printed fix-${s}.` },
  ]);

  await signIn(page);
  await ensureProject(PROJECT);
  const title = `Fix me ${s}`;
  const taskId = await createTask(page, { title, description: `FIXTASK ${s}: print the marker.`, project: PROJECT });
  await startRun(page, title);

  const done = await finished(taskId, 240_000);
  expect(done.status).toBe('completed');

  // One attempt: the refusal reached the model as a tool result, the round
  // after it still offered the tools, and the corrected command ran. A run
  // told to wrap up instead would have failed the attempt on the retry and
  // started over with a fresh context.
  const rounds = await roundsFor(`FIXTASK ${s}`);
  expect(rounds).toHaveLength(3);
  const refusal = rounds[1].tool_results.map((r) => r.content).join('\n');
  expect(refusal).toMatch(/;/);
  expect(rounds[1].tools.length).toBeGreaterThan(0);
  const output = rounds[2].tool_results.map((r) => r.content).join('\n');
  expect(output).toContain(`fix-${s}`);
  expect(consoleErrors).toEqual([]);
});

/**
 * The publication path, opt-in: a project linked to a repository the server
 * may push to. Set ZONE_LIVE_REPO_URL to that repository.
 */
test('a task with a repository works in a worktree, and its change is pushed and opened as a pull request', async ({
  page,
  consoleErrors,
}) => {
  const repo = process.env.ZONE_LIVE_REPO_URL;
  test.skip(!repo, 'set ZONE_LIVE_REPO_URL to a repository the server may push to');
  test.setTimeout(600_000);
  const s = stamp();
  await script(`REPOTASK ${s}`, [
    { calls: [{ name: 'write_file', arguments: { path: `live-verify/${s}.md`, content: `# Live verification ${s}\n\nWritten by a task run in a worktree.\n`, reason: 'the change to publish' } }] },
    { calls: [{ name: 'read_file', arguments: { path: `live-verify/${s}.md` } }] },
    { text: `Added live-verify/${s}.md.` },
  ]);

  await signIn(page);
  const projectName = 'Live verification repository';
  const projectId = await ensureProject(projectName);
  const token = await owner();
  const linked = await api('POST', `/api/projects/${projectId}/github`, {
    token,
    body: { repo_url: repo, access_token: process.env.ZONE_LIVE_REPO_TOKEN ?? 'proxy-authenticated' },
  });
  expect([200, 201], JSON.stringify(linked.body)).toContain(linked.status);

  const title = `Live verification ${s}`;
  const taskId = await createTask(page, {
    title,
    description: `REPOTASK ${s}: add a verification note under live-verify/.`,
    project: projectName,
  });
  await startRun(page, title);

  const done = await finished(taskId, 540_000);
  expect(done.status).toBe('completed');
  const task = await fetched(
    'GET',
    `/api/tasks/${taskId}`,
    (body) => {
      const detail = (body as { task?: unknown }).task ?? body;
      return typeof (detail as { id?: unknown })?.id === 'string' ? (detail as { pr_url?: string | null }) : undefined;
    },
    { token }
  );
  expect(task.pr_url ?? '').toMatch(/pull\/\d+/);
  const rounds = await roundsFor(`REPOTASK ${s}`);
  expect(rounds).toHaveLength(3);
  expect(rounds[2].tool_results.map((r) => r.content).join('\n')).toContain(`Live verification ${s}`);
  expect(consoleErrors).toEqual([]);
});
