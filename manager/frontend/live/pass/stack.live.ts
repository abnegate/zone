import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import type { Page } from '@playwright/test';
import { expect, record, shot, test } from './rig';
import { setChecked } from '../stub';

/**
 * Rows 53, 57, 58, 61 and 62: the compose stack behind Traefik, driven at
 * http://manager.localhost with the tenants seeded through the stack's own
 * API. LiteLLM's and the manager's container logs are read with docker.
 *
 * Run with ZONE_LIVE_STATE pointing at the stack's state file; this lane uses
 * absolute URLs, so the rig's port in the Playwright config does not matter.
 */

const CONSOLE = process.env.ZONE_STACK_CONSOLE ?? 'http://manager.localhost';
const GRAFANA =
  process.env.ZONE_STACK_GRAFANA ?? 'http://grafana.webui.localhost';
const statePath = process.env.ZONE_LIVE_STATE ?? '';
const stack = JSON.parse(readFileSync(statePath, 'utf8')) as {
  api: string;
  owner: {
    email: string;
    password: string;
    organization: { id: string };
    workspace: { id: string };
  };
};
const model = process.env.ZONE_LIVE_AGENT_MODEL ?? 'qwen3.8:27b';

function dockerLogs(container: string, since: string): string {
  return execFileSync('docker', ['logs', '--since', since, container], {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  });
}

async function signInStack(page: Page): Promise<void> {
  await page.addInitScript(
    (context) => {
      localStorage.setItem('manager_current_org', context.organization);
      localStorage.setItem('manager_current_workspace', context.workspace);
    },
    {
      organization: stack.owner.organization.id,
      workspace: stack.owner.workspace.id,
    },
  );
  await page.goto(`${CONSOLE}/login`);
  await page.evaluate(() => {
    localStorage.removeItem('manager_access_token');
    localStorage.removeItem('manager_refresh_token');
    localStorage.removeItem('manager_user');
  });
  await page.goto(`${CONSOLE}/login`);
  await page.getByLabel('Email').fill(stack.owner.email);
  await page.getByLabel('Password').fill(stack.owner.password);
  await page.getByRole('button', { name: /sign in|log in/i }).click();
  await expect(page).not.toHaveURL(/\/login/, { timeout: 30_000 });
  await page.waitForFunction(
    () => Boolean(localStorage.getItem('manager_access_token')),
    { timeout: 30_000 },
  );
}

async function newStackChat(
  page: Page,
  options: { model: string; agent?: boolean },
): Promise<string> {
  await page.goto(`${CONSOLE}/chats`);
  await page
    .getByRole('button', { name: /new chat/i })
    .first()
    .click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  await dialog.getByRole('combobox', { name: /select model/i }).click();
  await page.getByRole('option', { name: options.model, exact: true }).click();
  if (options.agent) {
    await setChecked(
      dialog.getByRole('checkbox', { name: 'Agent mode' }),
      true,
    );
    await setChecked(
      dialog.getByRole('checkbox', { name: /auto-approve/i }),
      true,
    );
  }
  await dialog.getByRole('button', { name: 'Create Chat' }).click();
  await expect(page).toHaveURL(/\/chats\?id=/, { timeout: 30_000 });
  return new URL(page.url()).searchParams.get('id') ?? '';
}

async function turn(
  page: Page,
  message: string,
  replies: number,
  timeout = 900_000,
): Promise<string> {
  const box = page.getByPlaceholder(/type a message/i).first();
  await box.fill(message);
  await box.press('Enter');
  const assistant = page.locator('.message-assistant');
  const deadline = Date.now() + timeout;
  for (;;) {
    const approve = page.locator('[data-testid="tool-approve"]').first();
    if (await approve.isVisible().catch(() => false)) await approve.click();
    if (
      (await assistant.count()) >= replies &&
      (await page.locator('.message-status').count()) === 0
    )
      break;
    if (Date.now() > deadline)
      throw new Error(`turn did not finish: ${message}`);
    await page.waitForTimeout(1_000);
  }
  return assistant.last().innerText();
}

async function stackToken(): Promise<string> {
  const response = await fetch(`${stack.api}/api/auth/login`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({
      email: stack.owner.email,
      password: stack.owner.password,
    }),
  });
  return ((await response.json()) as { access_token: string }).access_token;
}

async function toolNames(chatId: string): Promise<string[]> {
  const token = await stackToken();
  const response = await fetch(`${stack.api}/api/chats/${chatId}/messages`, {
    headers: { authorization: `Bearer ${token}` },
  });
  const body = (await response.json()) as {
    messages?: {
      role: string;
      metadata?: { tool_calls?: { name: string }[] } | null;
    }[];
  };
  return (body.messages ?? []).flatMap((m) =>
    (m.metadata?.tool_calls ?? []).map((c) => c.name),
  );
}

test.describe('compose stack', () => {
  test.skip(
    !statePath.includes('stack'),
    'run with ZONE_LIVE_STATE pointing at the stack tenants',
  );
  test.describe.configure({ timeout: 2_400_000 });

  test('57: the console behind Traefik works as it does on the rig', async ({
    page,
  }) => {
    await signInStack(page);
    await shot(page, '57-traefik-signed-in');
    const visited: Record<string, string> = {};
    for (const path of [
      '/chats',
      '/projects',
      '/tasks',
      '/sources',
      '/search',
      '/models',
      '/wiki',
      '/org-settings',
      '/settings',
    ]) {
      await page.goto(`${CONSOLE}${path}`);
      await page.waitForTimeout(1_500);
      visited[path] = (
        await page
          .locator('h1')
          .first()
          .innerText()
          .catch(() => '')
      ).trim();
    }
    await page.goto(`${CONSOLE}/models`);
    await expect(page.locator('.model-item').first()).toBeVisible({
      timeout: 60_000,
    });
    const models = await page
      .locator('.model-item .model-name')
      .allInnerTexts();
    await shot(page, '57-traefik-models');
    const traefik = execFileSync(
      'docker',
      ['logs', '--since', '10m', 'traefik'],
      { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] },
    )
      .split('\n')
      .filter((l) => /manager.localhost/.test(l))
      .slice(-2);
    record(57, {
      result:
        Object.values(visited).filter(Boolean).length >= 8 && models.length > 0
          ? 'WORKS'
          : 'FAILS',
      console_url: CONSOLE,
      note: 'The stack routes the console at manager.localhost (the checklist names webui.localhost, which is the DOMAIN_HOST_WEBUI suffix the traefik, litellm, grafana and prometheus hosts hang off)',
      page_headings: visited,
      models_listed: models.slice(0, 8),
      traefik_log: traefik,
      screenshots: ['57-traefik-signed-in.png', '57-traefik-models.png'],
    });
    expect(models.length).toBeGreaterThan(0);
  });

  test('58: automatic routing sends a trivial question to the fast model and a hard one to the reasoning model', async ({
    page,
  }) => {
    await signInStack(page);
    const since = new Date().toISOString();
    const chatId = await newStackChat(page, { model: 'Automatic' });
    const trivial = await turn(
      page,
      'What is 2 plus 2? Answer with the number only.',
      1,
    );
    await shot(page, '58-trivial-question');
    const afterTrivial = dockerLogs('litellm', since);
    const hard = await turn(
      page,
      'Prove step by step that the sum of the first n odd numbers is n squared, and analyze the edge cases of the proof.',
      2,
      1_500_000,
    );
    await shot(page, '58-hard-question');
    const afterHard = dockerLogs('litellm', since);
    const modelsSeen = (text: string) => [
      ...new Set(
        [...text.matchAll(/model[=:]\s*'?([A-Za-z0-9_.:\-\/]+)/g)].map(
          (m) => m[1],
        ),
      ),
    ];
    const fastLog = modelsSeen(afterTrivial);
    const hardLog = modelsSeen(afterHard).filter((m) => !fastLog.includes(m));
    const ps = (await (
      await fetch('http://127.0.0.1:11434/api/ps')
    ).json()) as { models?: { name: string }[] };
    record(58, {
      result:
        fastLog.some((m) => /llama3\.2:1b/.test(m)) &&
        hardLog.some((m) => /qwen3\.8/.test(m))
          ? 'WORKS'
          : 'FAILS',
      cause:
        fastLog.some((m) => /llama3\.2:1b/.test(m)) &&
        hardLog.some((m) => /qwen3\.8/.test(m))
          ? undefined
          : `litellm log names: trivial ${fastLog.join(',')} then ${hardLog.join(',')}`,
      chat_id: chatId,
      trivial_reply: trivial.slice(0, 120),
      hard_reply: hard.slice(0, 160),
      litellm_models_after_trivial: fastLog,
      litellm_models_new_after_hard: hardLog,
      ollama_loaded_at_end: (ps.models ?? []).map((m) => m.name),
      litellm_log_tail: afterHard
        .split('\n')
        .filter((l) => /model/i.test(l))
        .slice(-6)
        .map((l) => l.slice(0, 200)),
      screenshots: ['58-trivial-question.png', '58-hard-question.png'],
    });
    expect(fastLog.length + hardLog.length).toBeGreaterThan(0);
  });

  test('61 and 53: web search through SearXNG, and the Prometheus and Grafana tools', async ({
    page,
  }) => {
    await signInStack(page);
    const chatId = await newStackChat(page, { model, agent: true });
    const searched = await turn(
      page,
      'Search the web for the current stable version of the Rust programming language and tell me the version and the page you found it on.',
      1,
    );
    await shot(page, '61-web-search');
    const searxng = execFileSync(
      'docker',
      ['logs', '--since', '10m', 'zone-pass-searxng'],
      { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] },
    )
      .split('\n')
      .filter((l) => /search|GET/.test(l))
      .slice(-3);
    const prom = await turn(
      page,
      'Ask Prometheus how many scrape targets are up right now, and tell me the number.',
      2,
    );
    await shot(page, '53-query-prometheus');
    const grafana = await turn(
      page,
      'List the Grafana dashboards that exist in this deployment.',
      3,
    );
    await shot(page, '53-list-grafana-dashboards');
    const tools = await toolNames(chatId);
    record(61, {
      result:
        tools.includes('web_search') &&
        /rust/i.test(searched) &&
        /1\.\d\d/.test(searched)
          ? 'WORKS'
          : 'FAILS',
      cause: tools.includes('web_search')
        ? undefined
        : `model: tools used were ${tools.join(', ')}`,
      chat_id: chatId,
      reply: searched.slice(0, 200),
      searxng_log: searxng.map((l) => l.slice(0, 160)),
      path: 'no VPN credentials on this machine: SearXNG runs alone as zone-pass-searxng on 127.0.0.1:8089 and SEARCH_SEARXNG_QUERY_URL points the manager at it, so the tool itself is what is judged',
      screenshots: ['61-web-search.png'],
    });
    record(53, {
      result:
        tools.includes('query_prometheus') &&
        tools.includes('list_grafana_dashboards')
          ? 'WORKS'
          : 'FAILS',
      cause:
        tools.includes('query_prometheus') &&
        tools.includes('list_grafana_dashboards')
          ? undefined
          : `model: tools used were ${tools.join(', ')}`,
      chat_id: chatId,
      prometheus_reply: prom.slice(0, 200),
      grafana_reply: grafana.slice(0, 200),
      tools,
      screenshots: [
        '53-query-prometheus.png',
        '53-list-grafana-dashboards.png',
      ],
    });
    expect(tools).toContain('web_search');
    expect(tools).toContain('query_prometheus');
    expect(tools).toContain('list_grafana_dashboards');
  });

  test('62: Grafana opens with dashboards that show data', async ({ page }) => {
    const password = execFileSync(
      'sh',
      [
        '-c',
        "grep -E '^MONITORING_GRAFANA_ADMIN_PASSWORD=' /Users/jakebarnby/Local/zone/.claude/worktrees/zone-e2e-verification-dba4d9/.env | cut -d= -f2-",
      ],
      { encoding: 'utf8' },
    ).trim();
    await page.goto(`${GRAFANA}/login`);
    await page.getByLabel(/email or username/i).fill('admin');
    await page.getByRole('textbox', { name: 'Password' }).fill(password);
    await page.getByRole('button', { name: /log in/i }).click();
    await page.waitForTimeout(3_000);
    await page.goto(`${GRAFANA}/dashboards`);
    await page.waitForTimeout(3_000);
    const list = (
      await page
        .locator('main')
        .innerText()
        .catch(() => '')
    ).replace(/\s+/g, ' ');
    await shot(page, '62-grafana-dashboards');
    const first = page.locator('a[href*="/d/"]').first();
    let panelText = '';
    if (await first.count()) {
      await first.click();
      await page.waitForTimeout(8_000);
      panelText = (
        await page
          .locator('main')
          .innerText()
          .catch(() => '')
      ).replace(/\s+/g, ' ');
      await shot(page, '62-grafana-dashboard-open');
    }
    const promTargets = (await (
      await fetch('http://prometheus.webui.localhost/api/v1/targets').catch(
        () => new Response('{}'),
      )
    )
      .json()
      .catch(() => ({}))) as {
      data?: { activeTargets?: { health: string; labels: { job: string } }[] };
    };
    const up = (promTargets.data?.activeTargets ?? [])
      .filter((t) => t.health === 'up')
      .map((t) => t.labels.job);
    const hasNumbers =
      /\d/.test(panelText) && !/No data/i.test(panelText.slice(0, 2000));
    record(62, {
      result:
        /dashboard/i.test(list) && (await first.count()) > 0 && hasNumbers
          ? 'WORKS'
          : 'FAILS',
      grafana: GRAFANA,
      dashboards_page: list.slice(0, 300),
      opened_dashboard: panelText.slice(0, 300),
      prometheus_targets_up: up,
      screenshots: [
        '62-grafana-dashboards.png',
        '62-grafana-dashboard-open.png',
      ],
    });
    expect(await first.count()).toBeGreaterThan(0);
  });
});
