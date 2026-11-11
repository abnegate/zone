import { execFileSync, spawn } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { once } from 'node:events';
import { join } from 'node:path';
import { createInterface } from 'node:readline';
import { setTimeout as after } from 'node:timers/promises';
import type { Locator, Page } from '@playwright/test';
import {
  api,
  ask,
  chooseOption,
  chooseTaskAction,
  enabled,
  expect,
  logLines,
  logMark,
  logSince,
  model,
  newChat,
  ownerToken,
  record,
  renderedToolCalls,
  shot,
  signIn,
  sql,
  stamp,
  state,
  test,
} from './rig';

/**
 * Row 32: with magents on PATH and ZONE_MCP_ENABLED=true on the server, a task
 * that must hand work to another coding agent reaches the magents_* tools and
 * the console shows their calls. Row 32c adds a streamable-HTTP server through
 * Settings and calls its one tool from a chat. The rig keeps MCP off unless it
 * is brought up with ZONE_LIVE_MCP_ENABLED=true, which this lane expects.
 */

const SCRATCH_PROJECT = 'Real pass scratch';

const SETTINGS = '/settings/organization/mcp';
const SERVER = 'stub_http';
const DELEGATE = 'delegate';
const TOOL = `${SERVER}__${DELEGATE}`;
const TOOL_LABEL = `Delegate (${SERVER})`;
const SERVER_LOG = new RegExp(`mcp|${SERVER}`, 'i');
const TRANSPORT = 'http';
const TRANSPORT_LABEL = 'Streamable HTTP';
const CONNECTED = 'Connected';
const OFF = /=(0|false|no|off)$/i;
const PORT_LINE = /^PORT=(\d+)$/;
const STUB_DEADLINE = 30_000;
const STUB = join(
  __dirname,
  '..',
  '..',
  '..',
  '..',
  'runner',
  'zone_server',
  'tests',
  'fixtures',
  'stub_mcp_http_server.py',
);

// Which agent magents should start, and where: a headless claude on a host
// whose Claude Code runs on the desktop app's sign-in answers "Not logged
// in", and codex refuses to start outside a git repository.
const AGENT = process.env.ZONE_MAGENTS_AGENT;
const AGENT_CWD = process.env.ZONE_MAGENTS_CWD;
const which = AGENT
  ? ` a new ${AGENT} session${AGENT_CWD ? ` in the directory ${AGENT_CWD}` : ''}`
  : ' a new agent session';

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

function serverSwitch(): string {
  return execFileSync(
    'sh',
    [
      '-c',
      `ps -Eww -o command= -p $(lsof -nP -iTCP:${process.env.ZONE_LIVE_API_PORT ?? '8010'} -sTCP:LISTEN -t) | tr ' ' '\\n' | grep '^ZONE_MCP_ENABLED=' || echo ZONE_MCP_ENABLED=unset`,
    ],
    { encoding: 'utf8' },
  ).trim();
}

function toolCalls(chatId: string): string[] {
  return sql(
    `select coalesce(nullif(message->'tool_calls', 'null'::jsonb), '[]'::jsonb) from chat_entries where chat_id = '${chatId}' and message->>'role' = 'assistant' order by created_at`,
  ).flatMap((line) =>
    (
      JSON.parse(line) as { function: { name: string; arguments: string } }[]
    ).map((c) => `${c.function.name} ${c.function.arguments.slice(0, 120)}`),
  );
}

function toolResults(chatId: string): string[] {
  return sql(
    `select replace(message->>'content', E'\\n', ' ') from chat_entries where chat_id = '${chatId}' and message->>'role' = 'tool' order by created_at`,
  );
}

interface Stub {
  port: number;
  stop(): void;
}

async function startStub(token: string): Promise<Stub> {
  const child = spawn('python3', ['-I', STUB, '--token', token], {
    stdio: ['ignore', 'pipe', 'inherit'],
  });
  const lines = createInterface({ input: child.stdout });
  const stop = () => {
    lines.close();
    child.kill();
  };
  const first = await Promise.race([
    once(lines, 'line').then(([line]) => String(line)),
    once(lines, 'close').then(() => 'its output closed'),
    once(child, 'error').then(([failure]) => String(failure)),
    after(STUB_DEADLINE, 'it printed nothing in time', { ref: false }),
  ]);
  const port = PORT_LINE.exec(first)?.[1];
  if (!port) {
    stop();
    throw new Error(`the MCP stub gave no port: ${first}`);
  }
  return { port: Number(port), stop };
}

function row(page: Page): Locator {
  return page
    .getByRole('list', { name: 'MCP servers' })
    .getByRole('listitem', { name: SERVER, exact: true });
}

async function remove(page: Page): Promise<void> {
  await page
    .getByRole('button', { name: `Actions for ${SERVER}`, exact: true })
    .click();
  await page.getByRole('menuitem', { name: 'Remove', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: `Remove ${SERVER}?` });
  await dialog
    .getByRole('button', { name: 'Remove server', exact: true })
    .click();
  await expect(dialog).toHaveCount(0);
}

/**
 * Open the organization's MCP servers with the stub's row gone. "Add server"
 * renders only once the list has loaded, so the row count after it is real.
 */
async function clear(page: Page): Promise<void> {
  await page.goto(SETTINGS);
  await expect(
    page.getByRole('heading', { name: 'MCP servers', level: 2 }),
  ).toBeVisible();
  await expect(
    page.getByRole('button', { name: 'Add server', exact: true }),
  ).toBeVisible();
  if (await row(page).count()) await remove(page);
  await expect(row(page)).toHaveCount(0);
}

test.describe('mcp', () => {
  test.skip(!enabled, 'set ZONE_LIVE_REAL_PASS=1 against the real rig');
  test.describe.configure({ timeout: 2_400_000 });

  test('32: a task that must use another coding agent reaches the magents tools', async ({
    page,
  }) => {
    const magents = execFileSync('sh', ['-c', 'command -v magents || true'], {
      encoding: 'utf8',
    }).trim();
    const serverEnv = serverSwitch();
    const attached = logLines(0, /Attached MCP tools/).slice(-2);
    await signIn(page);
    const s = stamp();
    const title = `Delegate to another agent ${s}`;
    await page.goto('/tasks');
    await page.getByRole('button', { name: /new task/i }).click();
    const dialog = page.getByRole('dialog');
    await dialog
      .locator('.project-selection-option')
      .filter({ has: page.getByText(SCRATCH_PROJECT, { exact: true }) })
      .first()
      .click();
    await dialog.getByRole('button', { name: 'Next' }).click();
    await dialog.locator('#task-title').fill(title);
    await dialog
      .locator('#task-description')
      .fill(
        `Hand this to another coding agent on this machine rather than doing it yourself: start${which.replace(' a new', ' a new independent')} whose instruction is "print the words delegated-${s}", then message it asking for its result, and report back what the other agent said. Use the tools you have for talking to other agents.`,
      );
    await dialog.getByRole('button', { name: 'Next' }).click();
    await toggle(dialog, 'Enable Agentic Mode', true);
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
    const mark = logMark();
    await chooseTaskAction(page, title, 'Run');
    await page.getByRole('button', { name: /start execution/i }).click();
    await expect
      .poll(
        () =>
          sql(
            `select status from task_runs where task_id = '${task!.id}' order by started_at desc limit 1`,
          ).join(','),
        { timeout: 1_800_000, intervals: [5_000] },
      )
      .toMatch(/completed|failed|cancelled/);
    await page.waitForTimeout(2_000);
    const calls = sql(
      `select case when message like 'Executing tool:%' then replace(message, 'Executing tool: ', '') || ' | call | ' || left(coalesce(metadata->>'args', ''), 160) else replace(replace(message, 'Tool ', ''), ' finished', '') || ' | ' || coalesce(metadata->>'success', '') || ' | ' || left(coalesce(metadata->>'error', metadata->>'output', ''), 160) end from task_run_logs where task_run_id = (select id from task_runs where task_id = '${task!.id}' order by started_at desc limit 1) and (message like 'Executing tool:%' or message like 'Tool % finished') order by created_at`,
    );
    const logs = (await page.locator('.log-entry').allInnerTexts()).map((l) =>
      l.replace(/\s+/g, ' '),
    );
    const magentsCalls = calls.filter((c) => c.startsWith('magents_'));
    const shownInConsole = logs.filter((l) => /magents/i.test(l));
    await shot(page, '32-mcp-task-run');
    record(32, {
      result:
        magentsCalls.length > 0 && shownInConsole.length > 0
          ? 'WORKS'
          : 'FAILS',
      cause:
        magentsCalls.length > 0
          ? undefined
          : magents
            ? `no magents_* call in the run (server ${serverEnv}); tools used: ${calls.map((c) => c.split(' | ')[0]).join(', ')}`
            : 'environment: magents is not on PATH',
      magents_on_path: magents,
      server_env: serverEnv,
      mcp_attach_log: attached,
      task_id: task!.id,
      magents_calls: magentsCalls,
      console_log_lines_with_magents: shownInConsole.slice(0, 5),
      all_calls: calls,
      server_log: logLines(mark, /mcp|magents/i).slice(0, 8),
      screenshots: ['32-mcp-task-run.png'],
    });
    expect(magentsCalls.length).toBeGreaterThan(0);
  });

  test('32b: from a chat, the magents tools spawn another agent session and read it back', async ({
    page,
  }) => {
    const mark = logMark();
    await signIn(page);
    const s = stamp();
    const chatId = await newChat(page, {
      model,
      agent: true,
      autoApprove: true,
    });
    const reply = await ask(
      page,
      `Using your magents tools (they are MCP tools named magents_*): spawn${which} whose instruction is exactly "print the words delegated-${s} and nothing else", wait for its reply or read its transcript, tell me word for word what it printed, then stop that session.`,
      { replies: 1, timeout: 1_800_000 },
    );
    await shot(page, '32-magents-from-chat');
    const calls = toolCalls(chatId);
    const results = toolResults(chatId);
    // The spawned session's title repeats the instruction, so what it
    // printed is read from the transcript's assistant turn, not from any
    // mention of the words.
    const printed = new RegExp(
      `"(last_assistant_action|text)":\\s*"delegated-${s}`,
    );
    const readBack =
      results.some((result) => printed.test(result)) &&
      reply.includes(`delegated-${s}`);
    const magents = calls.filter((c) => c.startsWith('magents_'));
    const serverLog = logLines(mark, /mcp|magents/i)
      .slice(0, 12)
      .map((l) => l.slice(0, 200));
    const attached = logLines(mark, /Attached MCP tools/).length;
    record(32.5, {
      result: magents.length > 0 && readBack ? 'WORKS' : 'FAILS',
      cause:
        magents.length > 0
          ? readBack
            ? undefined
            : 'the magents tools were called but nothing read back the delegated words'
          : `model: no magents_* call from the chat (calls: ${[...new Set(calls.map((c) => c.split(' ')[0]))].join(', ')})`,
      chat_id: chatId,
      reply: reply.slice(0, 300),
      calls,
      tool_results: results.slice(0, 12).map((result) => result.slice(0, 300)),
      magents_calls: magents,
      mcp_attach_lines_since_mark: attached,
      server_log: serverLog,
      screenshots: ['32-magents-from-chat.png'],
    });
    expect(magents.length, calls.join(' | ')).toBeGreaterThan(0);
  });

  test("32c: an HTTP server added in Settings reaches a chat's tool trace", async ({
    page,
  }) => {
    const serverEnv = serverSwitch();
    const token = randomBytes(24).toString('hex');
    const stub = await startStub(token);
    const scrub = (text: string) => text.split(token).join('<token>');
    const instruction = `delegated-${stamp()}`;
    const mark = logMark();
    const serverLog = () => ({
      server_log: logLines(mark, SERVER_LOG)
        .slice(0, 12)
        .map((line) => scrub(line).slice(0, 200)),
      token_in_server_log: logSince(mark).includes(token),
    });
    let submitted = false;
    try {
      await signIn(page);
      await clear(page);
      await page
        .getByRole('button', { name: 'Add server', exact: true })
        .click();
      const dialog = page.getByRole('dialog', { name: 'Add MCP server' });
      await expect(dialog).toBeVisible();
      const transport = dialog.getByRole('combobox', { name: 'Transport' });
      await chooseOption(transport, TRANSPORT);
      await expect(transport).toHaveText(TRANSPORT_LABEL);
      await dialog.getByLabel('Name', { exact: true }).fill(SERVER);
      await dialog
        .getByLabel('URL', { exact: true })
        .fill(`http://127.0.0.1:${stub.port}/mcp`);
      await dialog
        .getByRole('button', { name: 'Add header', exact: true })
        .click();
      await dialog
        .getByLabel('Header name 1', { exact: true })
        .fill('Authorization');
      await dialog
        .getByLabel('Value for Authorization', { exact: true })
        .fill(`Bearer ${token}`);
      submitted = true;
      await dialog
        .getByRole('button', { name: 'Add server', exact: true })
        .click();
      await expect(dialog).toHaveCount(0);

      const item = row(page);
      const status = item.getByRole('status');
      const connected = await expect(status)
        .toHaveText(CONNECTED, { timeout: 60_000 })
        .then(
          () => true,
          () => false,
        );
      const badge = (await status.allInnerTexts()).join(' ').trim();
      const refusal = scrub(
        (await item.locator('.mcp-server-error').allInnerTexts()).join(' '),
      );
      if (connected) {
        await item
          .getByRole('button', { name: `Tools for ${SERVER}`, exact: true })
          .click();
        await expect(
          item.getByRole('checkbox', { name: DELEGATE, exact: true }),
        ).toBeChecked();
      }
      await shot(page, '32c-http-server-in-settings');
      const settings = {
        server_env: serverEnv,
        server_status: badge,
        server_error: refusal,
      };
      if (!connected) {
        record(32.7, {
          result: 'FAILS',
          cause: OFF.test(serverEnv)
            ? `environment: the server runs with ${serverEnv}`
            : `product: ${SERVER} is ${badge || 'missing from the list'}${refusal ? `: ${refusal}` : ''}`,
          ...settings,
          ...serverLog(),
          screenshots: ['32c-http-server-in-settings.png'],
        });
      }
      expect(connected, `${SERVER} is ${badge}: ${refusal}`).toBe(true);

      const chatId = await newChat(page, {
        model,
        agent: true,
        autoApprove: true,
      });
      const reply = await ask(
        page,
        `You have an MCP tool named ${TOOL}. Call it once with the instruction "${instruction}", then tell me word for word what it returned.`,
        { replies: 1 },
      );
      await shot(page, '32c-http-tool-from-chat');
      const calls = toolCalls(chatId);
      const results = toolResults(chatId);
      const called = calls.filter((c) => c.startsWith(TOOL));
      const echoed = results.filter((result) =>
        result.includes(`delegated:${instruction}`),
      );
      const rendered = (await renderedToolCalls(page)).map((call) =>
        call.replace(/\s+/g, ' '),
      );
      const traced = rendered.filter((call) => call.includes(TOOL_LABEL));
      record(32.7, {
        result:
          called.length > 0 && echoed.length > 0 && traced.length > 0
            ? 'WORKS'
            : 'FAILS',
        cause:
          called.length === 0
            ? `model: no ${TOOL} call from the chat (calls: ${[...new Set(calls.map((c) => c.split(' ')[0]))].join(', ')})`
            : echoed.length === 0
              ? `product: ${TOOL} was called but no tool result carries delegated:${instruction}`
              : traced.length === 0
                ? `product: the tool trace shows no "${TOOL_LABEL}"`
                : undefined,
        ...settings,
        chat_id: chatId,
        reply: reply.slice(0, 300),
        calls,
        tool_results: results
          .slice(0, 12)
          .map((result) => result.slice(0, 300)),
        rendered_tool_calls: rendered,
        mcp_attach_lines_since_mark: logLines(mark, /Attached MCP tools/)
          .length,
        ...serverLog(),
        screenshots: [
          '32c-http-server-in-settings.png',
          '32c-http-tool-from-chat.png',
        ],
      });
      expect(called.length, calls.join(' | ')).toBeGreaterThan(0);
      expect(echoed.length, results.join(' | ')).toBeGreaterThan(0);
      expect(traced.length, rendered.join(' | ')).toBeGreaterThan(0);
    } finally {
      try {
        if (submitted) await clear(page);
      } finally {
        stub.stop();
      }
    }
  });
});
