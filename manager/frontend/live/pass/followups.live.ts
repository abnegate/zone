import {
  api,
  ask,
  enabled,
  expect,
  model,
  newChat,
  ownerToken,
  record,
  shot,
  signIn,
  sql,
  stamp,
  test,
} from './rig';

/**
 * Row 34, the context meter half: whether the meter's number moves across a
 * chat that grows, read after each of three turns.
 */

test.describe('follow-ups', () => {
  test.skip(!enabled, 'set ZONE_LIVE_REAL_PASS=1 against the real rig');
  test.describe.configure({ timeout: 1_800_000 });

  test('34b: the context meter moves as a chat grows', async ({ page }) => {
    await signIn(page);
    const chatId = await newChat(page, { model });
    const meter = page.getByRole('group', { name: 'Context usage' });
    const readings: string[] = [];
    const prompts = [
      'Reply with one short sentence: what is the capital of New Zealand?',
      'Now write eight sentences about the history of Wellington harbour.',
      'List twenty New Zealand native birds with one clause each about where they live.',
    ];
    for (const [index, prompt] of prompts.entries()) {
      await ask(page, prompt, { replies: index + 1, timeout: 900_000 });
      await page.waitForTimeout(2_000);
      readings.push(
        (await meter.count())
          ? (await meter.innerText()).replace(/\s+/g, ' ')
          : 'no meter rendered',
      );
      await shot(page, `34-context-meter-turn-${index + 1}`);
    }
    if (await meter.count()) {
      await meter
        .locator('.context-usage-toggle')
        .click()
        .catch(() => undefined);
      await page.waitForTimeout(500);
      await shot(page, '34-context-meter-details');
    }
    const numbers = readings.map((r) => {
      const short = r.match(/≈\s*([\d.]+)k/i);
      if (short) return Math.round(Number(short[1]) * 1000);
      return Number((r.match(/([\d,]+) tokens/)?.[1] ?? '0').replace(/,/g, ''));
    });
    const moved = numbers[2] > numbers[0];
    record(34.5, {
      result: moved ? 'WORKS' : 'FAILS',
      cause: moved ? undefined : `the meter read ${readings.join(' || ')}`,
      chat_id: chatId,
      readings,
      numbers,
      screenshots: [
        '34-context-meter-turn-1.png',
        '34-context-meter-turn-2.png',
        '34-context-meter-turn-3.png',
        '34-context-meter-details.png',
      ],
    });
    expect(moved, readings.join(' || ')).toBe(true);
  });

  test('45b: a reminder asked for at a clock time fires into the chat', async ({
    page,
  }) => {
    await signIn(page);
    const chatId = await newChat(page, {
      model,
      agent: true,
      autoApprove: true,
    });
    const due = new Date(Date.now() + 600_000);
    const clock = due.toLocaleTimeString('en-NZ', {
      hour: '2-digit',
      minute: '2-digit',
      hour12: false,
      timeZone: 'Pacific/Auckland',
    });
    const iso = due.toISOString();
    const setAt = Date.now();
    const reply = await ask(
      page,
      `Set a one-off reminder for me at ${clock} today (that is ${iso}) that says: hydrate now. Do not schedule it for any other time.`,
      { replies: 1, timeout: 600_000 },
    );
    const rows = sql(
      `select left(content, 40) || ' | ' || status || ' | due ' || to_char(due_at at time zone 'UTC', 'HH24:MI:SS') || 'Z' from reminders where content ilike '%hydrate now%' order by created_at desc limit 2`,
    );
    await shot(page, '45-clock-reminder-set');
    let firedAfter = -1;
    const delivered = () =>
      sql(
        `select status || ' | ' || fired_count || ' | ' || coalesce(to_char(last_fired_at at time zone 'UTC', 'HH24:MI:SS'), '') from reminders where content ilike '%hydrate now%' order by created_at desc limit 1`,
      ).join(',');
    await expect
      .poll(
        async () => {
          if (!/^delivered/.test(delivered())) return false;
          const { body } = await api('GET', `/api/chats/${chatId}/messages`, {
            token: await ownerToken(),
          });
          const messages =
            (
              body as {
                messages?: {
                  role: string;
                  content: string;
                  created_at?: string;
                }[];
              }
            ).messages ?? [];
          return messages.some(
            (m) =>
              m.role !== 'user' &&
              /hydrate now/i.test(m.content) &&
              new Date(m.created_at ?? 0).getTime() >= due.getTime() - 60_000,
          );
        },
        { timeout: 1_200_000, intervals: [5_000] },
      )
      .toBe(true)
      .then(() => {
        firedAfter = Date.now() - setAt;
      })
      .catch(() => undefined);
    const reminderAfter = delivered();
    await page.reload();
    await page.waitForTimeout(2_000);
    await shot(page, '45-clock-reminder-fired');
    record(45.5, {
      result: firedAfter > 0 ? 'WORKS' : 'FAILS',
      cause:
        firedAfter > 0
          ? undefined
          : `reminder rows: ${rows.join(' || ')}; reply: ${reply.slice(0, 160)}`,
      chat_id: chatId,
      asked_for: `${clock} (${iso})`,
      reminder_rows: rows,
      reminder_after: reminderAfter,
      reply: reply.slice(0, 200),
      fired_after_ms: firedAfter,
      screenshots: ['45-clock-reminder-set.png', '45-clock-reminder-fired.png'],
    });
    expect(firedAfter).toBeGreaterThan(0);
  });

  test('45c: a scheduled watch reports only when the file it reads changed', async ({
    page,
  }) => {
    await signIn(page);
    const s = stamp();
    const chatId = await newChat(page, {
      model,
      agent: true,
      autoApprove: true,
    });
    await ask(
      page,
      `Create a file watch-${s}.txt in your working directory containing the word v1. Then schedule a watch (a reminder in condition_watch mode, every minute) that reads that file and tells me only when its contents changed since the last check.`,
      { replies: 1, timeout: 900_000 },
    );
    const rows = sql(
      `select id || ' | ' || timing_mode || ' | ' || coalesce(rrule, '') || ' | ' || status from reminders where timing_mode = 'condition_watch' and created_at > now() - interval '15 minutes' order by created_at desc limit 1`,
    );
    await shot(page, '45-watch-set');
    const baselineTurns = () =>
      sql(
        `select count(*) from reminder_turns t join reminders r on r.id = t.reminder_id where r.timing_mode = 'condition_watch' and t.created_at > now() - interval '20 minutes'`,
      ).join(',');
    await page.waitForTimeout(150_000);
    const turnsBeforeChange = baselineTurns();
    const messagesBefore = (
      (
        (
          await api('GET', `/api/chats/${chatId}/messages`, {
            token: await ownerToken(),
          })
        ).body as { messages?: unknown[] }
      ).messages ?? []
    ).length;
    await ask(
      page,
      `Change watch-${s}.txt so that it contains the word v2 instead of v1.`,
      { replies: 2, timeout: 600_000 },
    );
    const changedAt = Date.now();
    let reported = -1;
    await expect
      .poll(
        async () => {
          const { body } = await api('GET', `/api/chats/${chatId}/messages`, {
            token: await ownerToken(),
          });
          const messages =
            (body as { messages?: { role: string; content: string }[] })
              .messages ?? [];
          return messages.filter(
            (m) =>
              m.role === 'assistant' &&
              /v2/.test(m.content) &&
              !/contains the word v2 instead/.test(m.content),
          ).length;
        },
        { timeout: 300_000, intervals: [5_000] },
      )
      .toBeGreaterThan(1)
      .then(() => {
        reported = Date.now() - changedAt;
      })
      .catch(() => undefined);
    await page.reload();
    await page.waitForTimeout(2_000);
    await shot(page, '45-watch-fired-on-change');
    const observation = sql(
      `select left(coalesce(last_observation, ''), 160) || ' | ' || status from reminders where timing_mode = 'condition_watch' and created_at > now() - interval '30 minutes' order by created_at desc limit 1`,
    );
    for (const id of sql(
      `select id from reminders where timing_mode = 'condition_watch' and status = 'pending'`,
    ))
      sql(`update reminders set status = 'cancelled' where id = '${id}'`);
    record(45.7, {
      result: rows.length > 0 && reported > 0 ? 'WORKS' : 'FAILS',
      cause:
        rows.length === 0
          ? 'model: no condition_watch reminder was created'
          : reported > 0
            ? undefined
            : 'the watch never reported the change within five minutes',
      chat_id: chatId,
      watch_row: rows,
      watch_turns_before_change: turnsBeforeChange,
      messages_before_change: messagesBefore,
      reported_after_ms: reported,
      last_observation: observation,
      screenshots: ['45-watch-set.png', '45-watch-fired-on-change.png'],
    });
    expect(rows.length).toBeGreaterThan(0);
    expect(reported).toBeGreaterThan(0);
  });

  test('51b: start_task creates and runs an agentic task, and get_task_run reads its run', async ({
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
      `Start a background coding task for me now in the project "Real pass scratch", titled "Chat-run ${s}", whose job is to list the files in its working directory and report how many there are. Then tell me the task id and run id it gave you.`,
      { replies: 1, timeout: 900_000 },
    );
    await shot(page, '51-start-task');
    const task =
      sql(
        `select id || '|' || status || '|' || is_agentic from tasks where title = 'Chat-run ${s}' order by created_at desc limit 1`,
      )[0]?.split('|') ?? [];
    let runStatus = 'no task';
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
      runStatus = sql(
        `select status from task_runs where task_id = '${task[0]}' order by started_at desc limit 1`,
      ).join(',');
    }
    const status = await ask(
      page,
      `What is the status of the run of the task "Chat-run ${s}"?`,
      { replies: 2, timeout: 600_000 },
    );
    await shot(page, '51-get-task-run');
    const tools = sql(
      `select c->'function'->>'name' from chat_entries, jsonb_array_elements(coalesce(nullif(message->'tool_calls', 'null'::jsonb), '[]'::jsonb)) c where chat_id = '${chatId}' order by position`,
    );
    await page.goto('/tasks');
    const card = page.locator('.task-card', { hasText: `Chat-run ${s}` });
    const onPage = await card.count();
    if (onPage) await shot(page, '51-task-on-tasks-page');
    record(51.5, {
      result:
        tools.includes('start_task') &&
        tools.includes('get_task_run') &&
        runStatus === 'completed' &&
        onPage > 0
          ? 'WORKS'
          : 'FAILS',
      cause: tools.includes('start_task')
        ? undefined
        : `model: tools used were ${tools.join(', ')}`,
      chat_id: chatId,
      task: task,
      run_status: runStatus,
      start_reply: started.slice(0, 200),
      status_reply: status.slice(0, 200),
      task_on_tasks_page: onPage,
      tools,
      note: 'start_task creates an agentic task and starts its run; it does not start an existing manual task, which is what the first row 51 chat asked for',
      screenshots: [
        '51-start-task.png',
        '51-get-task-run.png',
        '51-task-on-tasks-page.png',
      ],
    });
    expect(tools).toContain('start_task');
    expect(runStatus).toBe('completed');
  });
});
