import {
  ask,
  enabled,
  expect,
  model,
  newChat,
  record,
  shot,
  signIn,
  sql,
  stamp,
  test,
} from './rig';

/**
 * Row 51 follow-up: which tool schemas a chat turn actually carries, as the
 * model itself reads them. Decides whether start_task was in front of the
 * model when it said no such tool existed.
 */

test.describe('tool set seen by the model', () => {
  test.skip(!enabled, 'set ZONE_LIVE_REAL_PASS=1 against the real rig');
  test.describe.configure({ timeout: 1_800_000 });

  test('51c: the model lists the tool schemas it can see', async ({ page }) => {
    await signIn(page);
    const chatId = await newChat(page, {
      model,
      agent: true,
      autoApprove: true,
    });
    const reply = await ask(
      page,
      'Without calling any tool: list the exact names of every tool whose full schema (parameters) is currently in front of you, one per line, alphabetically. Then on a separate line list the names of tools you only know by name from the catalog, if any. Do not explain.',
      { replies: 1, timeout: 1_200_000 },
    );
    await shot(page, '51-tool-schemas-seen-by-model');
    const names = reply
      .split('\n')
      .map((l) => l.trim().replace(/^[-*\d.)\s`]+|`+$/g, ''))
      .filter((l) => /^[a-z][a-z0-9_]+$/.test(l));
    record(51.3, {
      result: names.includes('start_task') ? 'WORKS' : 'FAILS',
      chat_id: chatId,
      start_task_in_schemas: names.includes('start_task'),
      names,
      reply: reply.slice(0, 1500),
      screenshots: ['51-tool-schemas-seen-by-model.png'],
    });
  });

  test('51d: named outright, start_task creates and runs an agentic task and get_task_run reads its run', async ({
    page,
  }) => {
    await signIn(page);
    const s = stamp();
    const chatId = await newChat(page, {
      model,
      agent: true,
      autoApprove: true,
    });
    const started = await ask(
      page,
      `Call the start_task tool now (its schema is in front of you) with title "Named-run ${s}" and description "List the files in your working directory and report how many there are." Then tell me the task_id and run_id it returned.`,
      { replies: 1, timeout: 1_200_000 },
    );
    await shot(page, '51-start-task-named');
    const task =
      sql(
        `select id || '|' || status || '|' || is_agentic from tasks where title = 'Named-run ${s}' order by created_at desc limit 1`,
      )[0]?.split('|') ?? [];
    let run = '';
    if (task[0]) {
      await expect
        .poll(
          () =>
            sql(
              `select status from task_runs where task_id = '${task[0]}' order by started_at desc limit 1`,
            ).join(','),
          { timeout: 1_500_000, intervals: [5_000] },
        )
        .toMatch(/completed|failed/)
        .catch(() => undefined);
      run = sql(
        `select id || ' | ' || status || ' | ' || left(coalesce(result_summary, ''), 160) from task_runs where task_id = '${task[0]}' order by started_at desc limit 1`,
      ).join(',');
    }
    const status = await ask(
      page,
      `Use get_task_run to read the latest run of "Named-run ${s}" and tell me its status and result in one sentence.`,
      { replies: 2, timeout: 1_200_000 },
    );
    await shot(page, '51-get-task-run-named');
    const tools = sql(
      `select coalesce(nullif(message->'tool_calls', 'null'::jsonb), '[]'::jsonb) from chat_entries where chat_id = '${chatId}' and message->>'role' = 'assistant' order by created_at`,
    ).flatMap((line) =>
      (JSON.parse(line) as { function: { name: string } }[]).map(
        (c) => c.function.name,
      ),
    );
    await page.goto('/tasks');
    const card = page.locator('.task-card', { hasText: `Named-run ${s}` });
    const onPage = await card.count();
    if (onPage) await shot(page, '51-named-task-on-tasks-page');
    record(51.6, {
      result:
        tools.includes('start_task') &&
        tools.includes('get_task_run') &&
        /completed/.test(run)
          ? 'WORKS'
          : 'FAILS',
      cause: tools.includes('start_task')
        ? /completed/.test(run)
          ? undefined
          : `run: ${run || 'none'}`
        : `model: never called start_task (used ${[...new Set(tools)].join(', ')})`,
      chat_id: chatId,
      task_row: task,
      run,
      started_reply: started.slice(0, 300),
      status_reply: status.slice(0, 300),
      tools,
      task_card_on_tasks_page: onPage,
      screenshots: [
        '51-start-task-named.png',
        '51-get-task-run-named.png',
        '51-named-task-on-tasks-page.png',
      ],
    });
    expect(tools).toContain('start_task');
    expect(run).toMatch(/completed/);
  });
});
