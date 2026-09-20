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
 * Row 45c: a condition watch at the smallest cadence the reminder tool
 * accepts, which is hourly. The watch reads a file the chat wrote, the file
 * is changed after the first firing, and the next firing has to report the
 * change. Budget: the hour between two firings plus the model turns.
 */

test.describe('condition watch', () => {
  test.skip(!enabled, 'set ZONE_LIVE_REAL_PASS=1 against the real rig');
  test.describe.configure({ timeout: 2 * 3_600_000 });

  test('45c: an hourly watch reports the file change on its next firing', async ({
    page,
  }) => {
    await signIn(page);
    const s = stamp();
    const chatId = await newChat(page, {
      model,
      agent: true,
      autoApprove: true,
    });
    const first = new Date(Date.now() + 8 * 60_000);
    const clock = first.toLocaleTimeString('en-NZ', {
      hour: '2-digit',
      minute: '2-digit',
      hour12: false,
      timeZone: 'Pacific/Auckland',
    });
    await ask(
      page,
      `Create a file watch-${s}.txt in your working directory containing the single word v1. Then create a condition_watch reminder, with rrule FREQ=HOURLY (hourly is the smallest cadence the tool accepts) and due_at ${first.toISOString()} (that is ${clock} today in Auckland; use that exact UTC instant), whose prompt reads watch-${s}.txt with read_file and reports its contents only if they differ from the last observation. Name it "Watch ${s}".`,
      { replies: 1, timeout: 1_200_000 },
    );
    const rows = sql(
      `select id || ' | ' || timing_mode || ' | ' || coalesce(rrule, '') || ' | ' || status || ' | ' || to_char(due_at at time zone 'UTC', 'HH24:MI:SS') || 'Z' from reminders where chat_id = '${chatId}' and timing_mode = 'condition_watch' order by created_at desc limit 1`,
    );
    const id = rows[0]?.split(' | ')[0] ?? '';
    await shot(page, '45-watch-set');
    const fired = () =>
      id
        ? Number(
            sql(`select fired_count from reminders where id = '${id}'`)[0] ?? 0,
          )
        : 0;
    let firstFiringAfterMs = -1;
    if (id) {
      const t0 = Date.now();
      await expect
        .poll(fired, { timeout: 30 * 60_000, intervals: [10_000] })
        .toBeGreaterThan(0)
        .then(() => {
          firstFiringAfterMs = Date.now() - t0;
        })
        .catch(() => undefined);
    }
    const baseline = id
      ? sql(
          `select left(coalesce(last_observation, ''), 200) from reminders where id = '${id}'`,
        ).join(',')
      : '';
    await expect
      .poll(async () => page.locator('.message-status').count(), {
        timeout: 20 * 60_000,
        intervals: [5_000],
      })
      .toBe(0);
    const messagesAfterFirst =
      (
        (
          await api('GET', `/api/chats/${chatId}/messages`, {
            token: await ownerToken(),
          })
        ).body as { messages?: { role: string; content: string }[] }
      ).messages ?? [];
    await page.reload();
    await page.waitForTimeout(2_000);
    await shot(page, '45-watch-first-firing');

    await ask(
      page,
      `Change watch-${s}.txt so that it contains the single word v2 instead of v1. Do not touch the reminder.`,
      {
        replies:
          messagesAfterFirst.filter((m) => m.role === 'assistant').length + 1,
        timeout: 1_200_000,
      },
    );
    const changedAt = Date.now();
    let reportedAfterMs = -1;
    if (id) {
      await expect
        .poll(
          async () => {
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
                m.role === 'assistant' &&
                /\bv2\b/.test(m.content) &&
                !/instead of v1/.test(m.content) &&
                new Date(m.created_at ?? 0).getTime() > changedAt + 60_000,
            );
          },
          { timeout: 75 * 60_000, intervals: [15_000] },
        )
        .toBe(true)
        .then(() => {
          reportedAfterMs = Date.now() - changedAt;
        })
        .catch(() => undefined);
    }
    await page.reload();
    await page.waitForTimeout(2_000);
    await shot(page, '45-watch-fired-on-change');
    const after = id
      ? sql(
          `select fired_count || ' | ' || status || ' | ' || left(coalesce(last_observation, ''), 200) from reminders where id = '${id}'`,
        ).join(',')
      : '';
    const turns = id
      ? sql(
          `select to_char(created_at at time zone 'UTC', 'HH24:MI:SS') || 'Z attempts=' || attempts || ' claimed=' || coalesce(claimed_at::text, '') from reminder_turns where reminder_id = '${id}' order by created_at`,
        )
      : [];
    if (id)
      sql(
        `update reminders set status = 'cancelled' where id = '${id}' and status = 'pending'`,
      );
    record(45.7, {
      result:
        id && firstFiringAfterMs > 0 && reportedAfterMs > 0 ? 'WORKS' : 'FAILS',
      cause: !id
        ? 'model: no condition_watch reminder was created'
        : firstFiringAfterMs < 0
          ? 'the watch never fired for its first observation'
          : reportedAfterMs > 0
            ? undefined
            : 'the firing after the change did not report v2 within 75 minutes',
      chat_id: chatId,
      watch_row: rows,
      first_firing_after_ms: firstFiringAfterMs,
      baseline_observation: baseline,
      reported_change_after_ms: reportedAfterMs,
      reminder_after: after,
      turns,
      screenshots: [
        '45-watch-set.png',
        '45-watch-first-firing.png',
        '45-watch-fired-on-change.png',
      ],
    });
    expect(id, rows.join(' || ')).not.toBe('');
    expect(reportedAfterMs).toBeGreaterThan(0);
  });

  test('45c-change: after the first firing, the file changes and the next hourly firing reports it', async ({
    page,
  }) => {
    const id = process.env.ZONE_WATCH_ID ?? '';
    const file = process.env.ZONE_WATCH_FILE ?? '';
    test.skip(
      !id || !file,
      'set ZONE_WATCH_ID and ZONE_WATCH_FILE from the watch the first test created',
    );
    await signIn(page);
    await page.goto('/chats');
    const item = page.locator('.chat-item', { hasText: file });
    await expect(item.first()).toBeVisible({ timeout: 60_000 });
    await item.first().click();
    const chatId =
      sql(`select chat_id from reminders where id = '${id}'`)[0] ?? '';
    await expect
      .poll(async () => page.locator('.message-status').count(), {
        timeout: 20 * 60_000,
        intervals: [5_000],
      })
      .toBe(0);
    const before =
      (
        (
          await api('GET', `/api/chats/${chatId}/messages`, {
            token: await ownerToken(),
          })
        ).body as { messages?: { role: string; content: string }[] }
      ).messages ?? [];
    const firstFiring = sql(
      `select fired_count || ' | ' || to_char(last_fired_at at time zone 'UTC', 'HH24:MI:SS') || 'Z | ' || left(coalesce(last_observation, ''), 200) from reminders where id = '${id}'`,
    ).join(',');
    await shot(page, '45-watch-first-firing');
    await ask(
      page,
      `Change ${file} so that it contains the single word v2 instead of v1. Do not touch the reminder.`,
      {
        replies: before.filter((m) => m.role === 'assistant').length + 1,
        timeout: 1_200_000,
      },
    );
    const changedAt = Date.now();
    await shot(page, '45-watch-file-changed');
    let reportedAfterMs = -1;
    await expect
      .poll(
        async () => {
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
              m.role === 'assistant' &&
              /\bv2\b/.test(m.content) &&
              !/instead of v1/.test(m.content) &&
              new Date(m.created_at ?? 0).getTime() > changedAt + 120_000,
          );
        },
        { timeout: 80 * 60_000, intervals: [15_000] },
      )
      .toBe(true)
      .then(() => {
        reportedAfterMs = Date.now() - changedAt;
      })
      .catch(() => undefined);
    await page.reload();
    await page.waitForTimeout(2_000);
    await item
      .first()
      .click()
      .catch(() => undefined);
    await page.waitForTimeout(1_500);
    await shot(page, '45-watch-fired-on-change');
    const after = sql(
      `select fired_count || ' | ' || status || ' | ' || to_char(last_fired_at at time zone 'UTC', 'HH24:MI:SS') || 'Z | ' || left(coalesce(last_observation, ''), 300) from reminders where id = '${id}'`,
    ).join(',');
    const turns = sql(
      `select to_char(created_at at time zone 'UTC', 'HH24:MI:SS') || 'Z attempts=' || attempts || ' claimed=' || coalesce(claimed_at::text, '') from reminder_turns where reminder_id = '${id}' order by created_at`,
    );
    const firings = (
      (
        (
          await api('GET', `/api/chats/${chatId}/messages`, {
            token: await ownerToken(),
          })
        ).body as {
          messages?: { role: string; content: string; created_at?: string }[];
        }
      ).messages ?? []
    )
      .filter((m) => m.role === 'assistant')
      .map(
        (m) =>
          `${m.created_at ?? ''} ${m.content.replace(/\s+/g, ' ').slice(0, 160)}`,
      );
    sql(
      `update reminders set status = 'cancelled' where id = '${id}' and status = 'pending'`,
    );
    record(45.7, {
      result: reportedAfterMs > 0 ? 'WORKS' : 'FAILS',
      cause:
        reportedAfterMs > 0
          ? undefined
          : 'the hourly firing after the change did not report v2 within 80 minutes',
      chat_id: chatId,
      watch_id: id,
      first_firing: firstFiring,
      reported_change_after_ms: reportedAfterMs,
      reminder_after: after,
      turns,
      assistant_messages: firings,
      note: 'The first run of this row was cut short because the watch’s first firing was still generating when the change was sent; this test waits for the turn to end first',
      screenshots: [
        '45-watch-set.png',
        '45-watch-first-firing.png',
        '45-watch-file-changed.png',
        '45-watch-fired-on-change.png',
      ],
    });
    expect(reportedAfterMs).toBeGreaterThan(0);
  });
});
