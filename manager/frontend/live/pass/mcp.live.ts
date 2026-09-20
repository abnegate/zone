import { execFileSync } from 'node:child_process';
import type { Locator } from '@playwright/test';
import {
  api,
  ask,
  enabled,
  expect,
  logLines,
  logMark,
  model,
  newChat,
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
 * Row 32: with magents on PATH and ZONE_MCP_ENABLED=true on the server, a task
 * that must hand work to another coding agent reaches the magents_* tools and
 * the console shows their calls. The rig hardcodes MCP off, so this lane
 * expects the server to have been restarted with it on (restart-server.sh).
 */

const SCRATCH_PROJECT = 'Real pass scratch';

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

test.describe('mcp', () => {
  test.skip(!enabled, 'set ZONE_LIVE_REAL_PASS=1 against the real rig');
  test.describe.configure({ timeout: 2_400_000 });

  test('32: a task that must use another coding agent reaches the magents tools', async ({
    page,
  }) => {
    const magents = execFileSync('sh', ['-c', 'command -v magents || true'], {
      encoding: 'utf8',
    }).trim();
    const serverEnv = execFileSync(
      'sh',
      [
        '-c',
        `ps -Eww -o command= -p $(lsof -nP -iTCP:${process.env.ZONE_LIVE_API_PORT ?? '8010'} -sTCP:LISTEN -t) | tr ' ' '\\n' | grep '^ZONE_MCP_ENABLED=' || echo ZONE_MCP_ENABLED=unset`,
      ],
      { encoding: 'utf8' },
    ).trim();
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
        `Hand this to another coding agent on this machine rather than doing it yourself: start a new independent agent session whose instruction is "print the words delegated-${s}", then message it asking for its result, and report back what the other agent said. Use the tools you have for talking to other agents.`,
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
      `Using your magents tools (they are MCP tools named magents_*): spawn a new agent session whose instruction is exactly "print the words delegated-${s} and nothing else", wait for its reply or read its transcript, tell me word for word what it printed, then stop that session.`,
      { replies: 1, timeout: 1_800_000 },
    );
    await shot(page, '32-magents-from-chat');
    const calls = sql(
      `select coalesce(nullif(message->'tool_calls', 'null'::jsonb), '[]'::jsonb) from chat_entries where chat_id = '${chatId}' and message->>'role' = 'assistant' order by created_at`,
    ).flatMap((line) =>
      (
        JSON.parse(line) as { function: { name: string; arguments: string } }[]
      ).map((c) => `${c.function.name} ${c.function.arguments.slice(0, 120)}`),
    );
    const results = sql(
      `select left(replace(message->>'content', E'\\n', ' '), 200) from chat_entries where chat_id = '${chatId}' and message->>'role' = 'tool' order by created_at`,
    );
    const magents = calls.filter((c) => c.startsWith('magents_'));
    const serverLog = logLines(mark, /mcp|magents/i)
      .slice(0, 12)
      .map((l) => l.slice(0, 200));
    const attached = logLines(mark, /Attached MCP tools/).length;
    record(32.5, {
      result:
        magents.length > 0 && /delegated-/.test(results.join(' '))
          ? 'WORKS'
          : 'FAILS',
      cause:
        magents.length > 0
          ? /delegated-/.test(results.join(' '))
            ? undefined
            : 'the magents tools were called but nothing read back the delegated words'
          : `model: no magents_* call from the chat (calls: ${[...new Set(calls.map((c) => c.split(' ')[0]))].join(', ')})`,
      chat_id: chatId,
      reply: reply.slice(0, 300),
      calls,
      tool_results: results.slice(0, 12),
      magents_calls: magents,
      mcp_attach_lines_since_mark: attached,
      server_log: serverLog,
      screenshots: ['32-magents-from-chat.png'],
    });
    expect(magents.length, calls.join(' | ')).toBeGreaterThan(0);
  });
});
