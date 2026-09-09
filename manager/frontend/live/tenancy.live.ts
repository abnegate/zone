import { api, expect, signIn, state, test, tokenFor } from './harness';

/**
 * PR #46, from the attacker's side. The second account is genuinely registered
 * and owns its own tenant; every id it names below belongs to the first one.
 *
 * The refusals are asserted as pairs where they can be: a foreign tenant and a
 * tenant that does not exist must answer alike, or the endpoint is an oracle
 * for which ids exist.
 */

const GHOST = '00000000-0000-4000-8000-0000000000ff';

async function tenantA(): Promise<{ token: string; taskId: string; runId: string }> {
  const token = await tokenFor(state.owner);
  const task = await api('POST', `/api/workspaces/${state.owner.workspace.id}/tasks`, {
    token,
    body: {
      title: 'Tenant A private task',
      description: 'Only tenant A may see this.',
      project_ids: [],
      priority: 2,
      is_agentic: false,
    },
  });
  expect(task.status).toBe(201);
  const taskId = (task.body as { task: { id: string } }).task.id;

  const run = await api('POST', `/api/tasks/${taskId}/runs`, { token });
  expect(run.status).toBe(201);
  return { token, taskId, runId: (run.body as { run: { id: string } }).run.id };
}

test("another tenant's task, theme and AI settings are all refused", async () => {
  const { taskId, runId } = await tenantA();
  const intruder = await tokenFor(state.intruder);
  const workspace = state.owner.workspace.id;
  const organization = state.owner.organization.id;

  // Reads answer 404 whether or not the id exists, so nothing is enumerable.
  for (const path of [
    `/api/tasks/${taskId}`,
    `/api/workspaces/${workspace}/tasks`,
    `/api/workspaces/${workspace}/theme`,
    `/api/organizations/${organization}/settings/ai`,
    `/api/tasks/runs/${runId}`,
  ]) {
    const refused = await api('GET', path, { token: intruder });
    expect(refused.status, `GET ${path}`).toBe(404);
  }

  // Writes refuse identically for a foreign tenant and one that does not exist.
  const writes: [string, string, unknown][] = [
    ['PUT', `/api/tasks/${taskId}`, { title: 'owned by the intruder' }],
    ['POST', `/api/tasks/${taskId}/runs`, undefined],
    ['POST', `/api/tasks/${taskId}/queue`, {}],
    ['PUT', `/api/workspaces/${workspace}/theme`, { primary_color_light: '#ff0000' }],
    [
      'PUT',
      `/api/organizations/${organization}/settings/ai`,
      { provider: 'self_hosted', litellm_host: 'http://attacker.example.com' },
    ],
  ];
  for (const [method, path, body] of writes) {
    const refused = await api(method, path, { token: intruder, body });
    expect(refused.status, `${method} ${path}`).toBeGreaterThanOrEqual(400);
    expect(refused.status, `${method} ${path}`).toBeLessThan(500);
  }

  const foreignTheme = await api('PUT', `/api/workspaces/${workspace}/theme`, {
    token: intruder,
    body: { primary_color_light: '#ff0000' },
  });
  const ghostTheme = await api('PUT', `/api/workspaces/${GHOST}/theme`, {
    token: intruder,
    body: { primary_color_light: '#ff0000' },
  });
  expect(ghostTheme.status).toBe(foreignTheme.status);
  expect(ghostTheme.body).toEqual(foreignTheme.body);

  const foreignAi = await api('PUT', `/api/organizations/${organization}/settings/ai`, {
    token: intruder,
    body: { provider: 'self_hosted' },
  });
  const ghostAi = await api('PUT', `/api/organizations/${GHOST}/settings/ai`, {
    token: intruder,
    body: { provider: 'self_hosted' },
  });
  expect(ghostAi.status).toBe(foreignAi.status);
  expect(ghostAi.body).toEqual(foreignAi.body);
});

test("the run stream is closed to a tenant that does not own it", async ({ page }) => {
  const { runId } = await tenantA();

  const watch = async (who: typeof state.owner) => {
    const token = await tokenFor(who);
    await signIn(page, who);
    return page.evaluate(
      async ({ run, bearer }) => {
        const socket = new WebSocket(
          `${location.origin.replace('http', 'ws')}/ws/tasks/runs/${run}`
        );
        return new Promise<{ frame: string | null; closed: boolean }>((resolve) => {
          let frame: string | null = null;
          const finish = (closed: boolean) => resolve({ frame, closed });
          socket.addEventListener('open', () =>
            socket.send(JSON.stringify({ type: 'auth', token: bearer }))
          );
          socket.addEventListener('message', (event) => {
            frame = String((event as MessageEvent).data);
            socket.close();
            finish(false);
          });
          socket.addEventListener('close', () => finish(true));
          setTimeout(() => finish(false), 15_000);
        });
      },
      { run: runId, bearer: token }
    );
  };

  const owner = await watch(state.owner);
  expect(owner.frame, 'the owner is told about their own run').toContain(runId);

  const intruder = await watch(state.intruder);
  expect(intruder.frame, 'a foreign tenant is told nothing about the run').toBeNull();
  expect(intruder.closed).toBe(true);
});

test("the intruder's own console shows nothing of the other tenant", async ({
  page,
  consoleErrors,
}) => {
  const { taskId } = await tenantA();

  // `WorkspaceProvider` restores only an organization the caller is a member of
  // and only a workspace from that organization, so seeding foreign ids into
  // local storage does not make the console request them -- it silently falls
  // back to the intruder's own tenant. Hostile ids are covered by the
  // request-level checks above, which is where that behaviour actually lives.
  // What this adds is that the rendered console leaks nothing across tenants.
  await signIn(page, state.intruder);
  await page.goto('/tasks');
  await expect(page.locator('main, .page').first()).toBeVisible();

  await expect(page.locator('body')).not.toContainText('Tenant A private task');
  await expect(page.locator('body')).not.toContainText(taskId);
  await expect(page.locator('body')).not.toContainText(state.owner.workspace.id);
  // Refusals the console expects are not app errors; a 500 would be.
  expect(consoleErrors.filter((entry) => entry.startsWith('5'))).toEqual([]);
});
