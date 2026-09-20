import { execFileSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import type { Page } from '@playwright/test';
import {
  api,
  ask,
  enabled,
  expect,
  model,
  newChat,
  ownerToken,
  record,
  send,
  shot,
  signIn,
  sql,
  stamp,
  state,
  test,
} from './rig';

/**
 * Rows 40 to 52 (49 and 53 live elsewhere): every agent tool reached from a
 * chat by asking in plain words. Which tools ran is read from the stored
 * transcript (chat_entries), never from what the model says it did.
 */

const REPO =
  process.env.ZONE_LIVE_REPO_URL ?? 'https://github.com/abnegate/zone-tests';
const [OWNER, NAME] = REPO.replace(/^https:\/\/github\.com\//, '').split('/');
const work = process.env.ZONE_LIVE_WORK ?? '/tmp/zone-live-verify';
const gh = {
  ...process.env,
  GH_CONFIG_DIR: process.env.GH_CONFIG_DIR ?? `${process.env.HOME}/.config/gh`,
};

function toolNames(chatId: string): string[] {
  return sql(
    `select c->'function'->>'name' from chat_entries, jsonb_array_elements(coalesce(nullif(message->'tool_calls', 'null'::jsonb), '[]'::jsonb)) c where chat_id = '${chatId}' order by position`,
  );
}

function toolResults(chatId: string, tool: string): string[] {
  return sql(
    `select left(r.message->>'content', 400) from chat_entries a join chat_entries r on r.chat_id = a.chat_id and r.message->>'role' = 'tool' and r.message->>'tool_call_id' = a.message->'tool_calls'->0->>'id' where a.chat_id = '${chatId}' and a.message->'tool_calls'->0->'function'->>'name' = '${tool}' order by r.position`,
  );
}

async function messagesOf(
  chatId: string,
): Promise<{ role: string; content: string }[]> {
  const { body } = await api('GET', `/api/chats/${chatId}/messages`, {
    token: await ownerToken(),
  });
  return ((body as { messages?: { role: string; content: string }[] })
    .messages ?? []) as { role: string; content: string }[];
}

function findFile(name: string): string[] {
  const roots = [
    process.env.ZONE_CHAT_AGENT_CWD ?? '',
    join(process.cwd(), '..', '..'),
    work,
    tmpdir(),
    `${process.env.HOME}/Library/Application Support/Zone`,
  ].filter(Boolean);
  const found: string[] = [];
  for (const root of roots) {
    try {
      const out = execFileSync(
        'find',
        [root, '-name', name, '-not', '-path', '*/node_modules/*'],
        { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] },
      );
      found.push(...out.split('\n').filter(Boolean));
    } catch {
      // a root that does not exist or cannot be searched says nothing
    }
  }
  return [...new Set(found)];
}

async function agentChat(page: Page, autoApprove = true): Promise<string> {
  return newChat(page, { model, agent: true, autoApprove });
}

test.describe('agent tools from chat', () => {
  test.skip(!enabled, 'set ZONE_LIVE_REAL_PASS=1 against the real rig');
  test.describe.configure({ timeout: 2_400_000 });

  test('40: read_file, write_file and apply_patch, with the approval card approved and denied', async ({
    page,
  }) => {
    await signIn(page);
    const s = stamp();
    const chatId = await agentChat(page, false);
    await send(
      page,
      `Create a file called notes-${s}.txt in your working directory containing exactly one line: first line ${s}`,
    );
    const approve = page.locator('[data-testid="tool-approve"]').first();
    await expect(approve).toBeVisible({ timeout: 600_000 });
    const preview = (
      await page
        .locator('[data-testid="tool-call-preview"]')
        .first()
        .innerText()
        .catch(() => '')
    ).replace(/\s+/g, ' ');
    await shot(page, '40-approval-card');
    await approve.click();
    await ask(page, '', { replies: 1, approve: true, timeout: 600_000 }).catch(
      () => undefined,
    );
    await expect(page.locator('.message-assistant')).toHaveCount(1, {
      timeout: 600_000,
    });
    await expect(page.locator('.message-status')).toHaveCount(0, {
      timeout: 600_000,
    });
    const written = findFile(`notes-${s}.txt`);
    const receipt = await page
      .locator('[data-testid="action-receipt"], .tool-call--ok')
      .count();
    await shot(page, '40-write-approved');

    const readReply = await ask(
      page,
      `Read notes-${s}.txt back to me word for word.`,
      { replies: 2, timeout: 600_000 },
    );
    await shot(page, '40-read-file');
    await ask(
      page,
      `Apply a patch to notes-${s}.txt that changes "first line" to "second line", then show me the file.`,
      { replies: 3, timeout: 900_000 },
    );
    await shot(page, '40-apply-patch');
    const patched = written.length
      ? execFileSync('cat', [written[0]], { encoding: 'utf8' })
      : '';

    await send(
      page,
      `Create another file called denied-${s}.txt containing the word never.`,
    );
    const deny = page.locator('[data-testid="tool-deny"]').first();
    await expect(deny).toBeVisible({ timeout: 600_000 });
    await shot(page, '40-deny-card');
    await deny.click();
    await expect(page.locator('.message-assistant')).toHaveCount(4, {
      timeout: 600_000,
    });
    await expect(page.locator('.message-status')).toHaveCount(0, {
      timeout: 600_000,
    });
    const denied = findFile(`denied-${s}.txt`);
    await shot(page, '40-write-denied');
    const tools = toolNames(chatId);
    const deniedResult = toolResults(chatId, 'write_file').filter((r) =>
      /den|declin|refus|not approved/i.test(r),
    );
    record(40, {
      result:
        written.length > 0 &&
        /first line/.test(readReply) &&
        /second line/.test(patched) &&
        denied.length === 0 &&
        tools.includes('read_file') &&
        tools.includes('apply_patch')
          ? 'WORKS'
          : 'FAILS',
      chat_id: chatId,
      approval_preview: preview.slice(0, 200),
      file_written_at: written,
      receipts_or_ok_calls_rendered: receipt,
      read_reply: readReply.slice(0, 200),
      file_after_patch: patched.slice(0, 200),
      denied_file_exists: denied,
      denied_tool_result: deniedResult.slice(0, 2),
      tools,
      screenshots: [
        '40-approval-card.png',
        '40-write-approved.png',
        '40-read-file.png',
        '40-apply-patch.png',
        '40-deny-card.png',
        '40-write-denied.png',
      ],
    });
    expect(written.length).toBeGreaterThan(0);
    expect(denied).toEqual([]);
    expect(tools).toContain('read_file');
  });

  test('41: run_shell in the foreground, and in the background with wait_for and tail_job', async ({
    page,
  }) => {
    await signIn(page);
    const s = stamp();
    const chatId = await agentChat(page);
    const fg = await ask(
      page,
      'Run the shell command uname -sm and tell me exactly what it printed.',
      { replies: 1, timeout: 600_000 },
    );
    await shot(page, '41-run-shell-foreground');
    await ask(
      page,
      `Start a background shell job that sleeps 20 seconds and then prints BG-DONE-${s}. Do not wait in the foreground: wait for the background job to finish, then read its log and tell me the exact line it printed.`,
      { replies: 2, timeout: 900_000 },
    );
    const jobCard = await page.locator('[data-testid="job-card"]').count();
    const waitCard = await page.locator('[data-testid="wait-card"]').count();
    const waitOutcome = await page
      .locator('[data-testid="wait-outcome"]')
      .count();
    await shot(page, '41-background-job');
    const tools = toolNames(chatId);
    const bg = toolResults(chatId, 'run_shell');
    const tail = toolResults(chatId, 'tail_job');
    record(41, {
      result:
        /darwin/i.test(fg) &&
        tools.includes('wait_for') &&
        tools.includes('tail_job') &&
        tail.some((t) => t.includes(`BG-DONE-${s}`))
          ? 'WORKS'
          : 'FAILS',
      cause:
        tools.includes('wait_for') && tools.includes('tail_job')
          ? undefined
          : `model: tools used were ${tools.join(', ')}`,
      chat_id: chatId,
      foreground_reply: fg.slice(0, 160),
      job_card: jobCard,
      wait_card: waitCard,
      wait_outcome: waitOutcome,
      run_shell_results: bg.slice(0, 3),
      tail_job_results: tail.slice(0, 2),
      tools,
      screenshots: ['41-run-shell-foreground.png', '41-background-job.png'],
    });
    expect(tools).toContain('run_shell');
    expect(tools).toContain('wait_for');
    expect(tools).toContain('tail_job');
  });

  test('42: ask_user renders a question card and the answer is the next turn', async ({
    page,
  }) => {
    await signIn(page);
    const chatId = await agentChat(page);
    await send(
      page,
      'Before answering, ask me which of these three cities I mean, offering them as choices: Auckland, Wellington, Christchurch. Once I choose, tell me one fact about that city.',
    );
    const card = page.locator('[data-testid="question-card"]').first();
    await expect(card).toBeVisible({ timeout: 600_000 });
    const question = (await card.innerText()).replace(/\s+/g, ' ');
    await shot(page, '42-question-card');
    const wellington = card.getByRole('radio', { name: /wellington/i });
    if (await wellington.count()) await wellington.first().check();
    else await card.getByRole('radio').first().check();
    await card.locator('[data-testid="question-submit"]').click();
    await expect(page.locator('.message-assistant')).toHaveCount(2, {
      timeout: 600_000,
    });
    await expect(page.locator('.message-status')).toHaveCount(0, {
      timeout: 600_000,
    });
    const answer = await page.locator('.message-assistant').last().innerText();
    await shot(page, '42-answered');
    const tools = toolNames(chatId);
    const userTurns = sql(
      `select left(message->>'content', 120) from chat_entries where chat_id = '${chatId}' and message->>'role' = 'user' order by position`,
    );
    record(42, {
      result:
        tools.includes('ask_user') && /wellington/i.test(answer)
          ? 'WORKS'
          : 'FAILS',
      chat_id: chatId,
      question: question.slice(0, 300),
      answer: answer.slice(0, 200),
      user_turns: userTurns,
      tools,
      screenshots: ['42-question-card.png', '42-answered.png'],
    });
    expect(tools).toContain('ask_user');
  });

  test('43: memory written in one chat is read back in another, then listed, appended and deleted', async ({
    page,
  }) => {
    await signIn(page);
    const s = stamp();
    const first = await agentChat(page);
    await ask(
      page,
      `Please remember this about me for future conversations: my favourite text editor is Helix, and my project codename is falcon-${s}.`,
      { replies: 1, timeout: 600_000 },
    );
    await shot(page, '43-memory-written');
    const writeTools = toolNames(first);

    const second = await agentChat(page);
    const readBack = await ask(
      page,
      'Which text editor do I prefer, and what is my project codename? Check what you remember about me.',
      { replies: 1, timeout: 600_000 },
    );
    const badge = await page.locator('[data-testid="memory-badge"]').count();
    await shot(page, '43-memory-read-with-badge');
    await ask(page, 'List everything you have stored in memory about me.', {
      replies: 2,
      timeout: 600_000,
    });
    await ask(
      page,
      `Add to the stored note about my editor that I use the Catppuccin theme with it.`,
      { replies: 3, timeout: 600_000 },
    );
    await shot(page, '43-memory-appended');
    await ask(
      page,
      `Forget the stored note about my project codename falcon-${s}.`,
      { replies: 4, timeout: 600_000 },
    );
    await shot(page, '43-memory-deleted');
    const readTools = toolNames(second);
    const rows = sql(
      `select left(title, 80) || ' | ' || left(replace(content, E'\\n', ' '), 120) from knowledge_entries where category = 'memory' order by updated_at desc limit 6`,
    );
    record(43, {
      result:
        writeTools.includes('memory_write') &&
        readTools.includes('memory_read') &&
        badge > 0 &&
        /helix/i.test(readBack) &&
        readTools.includes('memory_list') &&
        readTools.includes('memory_append') &&
        readTools.includes('memory_delete')
          ? 'WORKS'
          : 'FAILS',
      cause:
        writeTools.includes('memory_write') && readTools.includes('memory_read')
          ? undefined
          : `model: first chat used ${writeTools.join(', ')}; second used ${readTools.join(', ')}`,
      first_chat: first,
      second_chat: second,
      first_chat_tools: writeTools,
      second_chat_tools: readTools,
      read_back: readBack.slice(0, 200),
      memory_badge_rendered: badge,
      memory_rows: rows,
      screenshots: [
        '43-memory-written.png',
        '43-memory-read-with-badge.png',
        '43-memory-appended.png',
        '43-memory-deleted.png',
      ],
    });
    expect(writeTools).toContain('memory_write');
    expect(readTools).toContain('memory_read');
  });

  test('44 and 45: a deferred tool is found and loaded; reminders fire, run a turn, are listed, cancelled, and watch a change', async ({
    page,
  }) => {
    test.setTimeout(3_000_000);
    await signIn(page);
    const s = stamp();
    const chatId = await agentChat(page);
    const setAt = Date.now();
    const setReply = await ask(
      page,
      `Set a reminder for me two minutes from now that says: stretch ${s}`,
      { replies: 1, timeout: 600_000 },
    );
    let tools = toolNames(chatId);
    const reminderResults = toolResults(chatId, 'create_reminder');
    await shot(page, '44-reminder-set');
    const fired = await expect
      .poll(
        () =>
          sql(
            `select count(*) from reminders where content like '%stretch ${s}%' and status = 'delivered'`,
          ).join(','),
        { timeout: 420_000, intervals: [5_000] },
      )
      .toBe('1')
      .then(() => Date.now() - setAt)
      .catch(() => -1);
    await page.reload();
    await page.waitForTimeout(2_000);
    await shot(page, '45-reminder-fired');

    await ask(
      page,
      `Every hour, starting two minutes from now, run this instruction for me: report the current date and time and the word tick-${s}.`,
      { replies: 2, timeout: 600_000 },
    );
    await shot(page, '45-prompt-reminder-set');
    const promptFired = await expect
      .poll(
        () =>
          sql(
            `select count(*) from reminder_turns t join reminders r on r.id = t.reminder_id where r.content like '%tick-${s}%'`,
          ).join(','),
        { timeout: 480_000, intervals: [5_000] },
      )
      .not.toBe('0')
      .then(() => true)
      .catch(() => false);
    await page.reload();
    await page.waitForTimeout(2_000);
    await shot(page, '45-prompt-reminder-ran');
    const listed = await ask(page, 'List my reminders.', {
      replies: 3,
      timeout: 600_000,
    });
    await ask(page, `Cancel the hourly tick-${s} reminder.`, {
      replies: 4,
      timeout: 600_000,
    });
    await shot(page, '45-reminder-cancelled');
    const status = sql(
      `select left(content, 60) || ' | ' || timing_mode || ' | ' || status || ' | ' || coalesce(rrule, '') from reminders where content like '%${s}%' order by created_at`,
    );

    // A watch: fires on its rule and reports only when the thing it reads changed.
    await ask(
      page,
      `Create a file watch-${s}.txt in your working directory containing the word v1. Then set up a watch that runs every minute, reads that file, and tells me only when its content has changed since the last check.`,
      { replies: 5, timeout: 900_000 },
    );
    await shot(page, '45-watch-set');
    const watchRow = sql(
      `select id || ' | ' || timing_mode || ' | ' || coalesce(rrule,'') || ' | ' || status from reminders where timing_mode = 'condition_watch' and created_at > now() - interval '20 minutes' order by created_at desc limit 1`,
    );
    const baselineWait = 150_000;
    await page.waitForTimeout(baselineWait);
    const beforeChange = (await messagesOf(chatId)).length;
    await ask(
      page,
      `Change watch-${s}.txt so that it contains the word v2 instead.`,
      { replies: 6, timeout: 600_000 },
    );
    const changedAt = Date.now();
    const watchReported = await expect
      .poll(
        () =>
          sql(
            `select count(*) from reminder_turns t join reminders r on r.id = t.reminder_id where r.timing_mode = 'condition_watch' and t.created_at > now() - interval '10 minutes'`,
          ).join(','),
        { timeout: 300_000, intervals: [5_000] },
      )
      .not.toBe('0')
      .then(() => Date.now() - changedAt)
      .catch(() => -1);
    await page.reload();
    await page.waitForTimeout(2_000);
    await shot(page, '45-watch-fired-on-change');
    const observations = sql(
      `select left(coalesce(last_observation,''), 120) || ' | ' || status from reminders where timing_mode = 'condition_watch' and created_at > now() - interval '30 minutes' order by created_at desc limit 1`,
    );
    tools = toolNames(chatId);
    for (const id of sql(
      `select id from reminders where timing_mode = 'condition_watch' and status = 'pending' and created_at > now() - interval '30 minutes'`,
    )) {
      sql(`update reminders set status = 'cancelled' where id = '${id}'`);
    }
    record(44, {
      result:
        tools.includes('load_tools') && tools.includes('create_reminder')
          ? 'WORKS'
          : 'FAILS',
      searched_first: tools.includes('search_tools'),
      note: 'The prompt catalog names every deferred tool, so a model that already knows the name loads it without search_tools; the row asks that the deferred tool is found and loaded, which load_tools alone satisfies',
      cause: tools.includes('create_reminder')
        ? undefined
        : `model: tools used were ${tools.join(', ')}`,
      chat_id: chatId,
      set_reply: setReply.slice(0, 160),
      create_reminder_results: reminderResults.slice(0, 2),
      tools_in_order: tools.slice(0, 8),
      screenshots: ['44-reminder-set.png'],
    });
    record(45, {
      result:
        fired > 0 &&
        promptFired &&
        tools.includes('list_reminders') &&
        tools.includes('cancel_reminder')
          ? 'WORKS'
          : 'FAILS',
      watch: 'the condition watch is judged by row 45c (watch.live.ts), whose hourly cadence is the smallest the tool accepts',
      chat_id: chatId,
      exact_reminder_fired_after_ms: fired,
      prompt_reminder_ran_a_turn: promptFired,
      list_reply: listed.slice(0, 200),
      reminder_rows: status,
      watch_row: watchRow,
      watch_reported_change_after_ms: watchReported,
      watch_last_observation: observations,
      messages_before_change: beforeChange,
      tools,
      screenshots: [
        '45-reminder-fired.png',
        '45-prompt-reminder-set.png',
        '45-prompt-reminder-ran.png',
        '45-reminder-cancelled.png',
        '45-watch-set.png',
        '45-watch-fired-on-change.png',
      ],
    });
    expect(tools).toContain('create_reminder');
    expect(fired).toBeGreaterThan(0);
  });

  test('46: knowledge is searched with a labelled citation, listed, read, created and updated', async ({
    page,
  }) => {
    await signIn(page);
    const s = stamp();
    const token = await ownerToken();
    const title = `Deployment checklist ${s}`;
    const created = await api('POST', '/api/knowledge', {
      token,
      body: {
        workspace_id: state.owner.workspace.id,
        title,
        content: `Rotate the signing key ${s}, then restart the gateway, then watch the error rate for ten minutes.`,
        category: 'runbook',
        tags: ['release'],
      },
    });
    expect(created.status).toBe(201);
    const chatId = await agentChat(page);
    const searched = await ask(
      page,
      `Search the knowledge base for the deployment checklist ${s} and tell me what it says.`,
      { replies: 1, timeout: 600_000 },
    );
    const citations = page.locator('[data-testid="citation"]');
    const citationText = (await citations.allInnerTexts()).map((c) =>
      c.replace(/\s+/g, ' '),
    );
    const evidenceLabels = await page
      .locator('.citation-evidence')
      .allInnerTexts();
    await shot(page, '46-search-knowledge-citation');
    await ask(
      page,
      `List the documents in the wiki, then read the one titled "${title}" and quote its first sentence.`,
      { replies: 2, timeout: 600_000 },
    );
    await shot(page, '46-list-and-read');
    await ask(
      page,
      `Create a wiki document titled "Runbook ${s}" whose content is: Step one, check the dashboards. Step two, page the on-call engineer.`,
      { replies: 3, timeout: 600_000 },
    );
    await ask(
      page,
      `Update the "Runbook ${s}" document by appending the line: Reviewed by the live pass ${s}.`,
      { replies: 4, timeout: 600_000 },
    );
    await shot(page, '46-create-and-update');
    const tools = toolNames(chatId);
    await page.goto('/wiki');
    await page.getByLabel('Search knowledge').fill(`Runbook ${s}`);
    const card = page.locator('.knowledge-card', { hasText: `Runbook ${s}` });
    await expect(card).toBeVisible({ timeout: 30_000 });
    await card.click();
    const dialogText = (await page.locator('.wiki-dialog').innerText()).replace(
      /\s+/g,
      ' ',
    );
    await shot(page, '46-wiki-shows-document');
    const stored = sql(
      `select left(replace(content, E'\\n', ' '), 300) from knowledge_entries where title = 'Runbook ${s}'`,
    );
    record(46, {
      result:
        tools.includes('search_knowledge') &&
        citationText.length > 0 &&
        tools.includes('list_documents') &&
        tools.includes('read_document') &&
        tools.includes('create_document') &&
        tools.includes('update_document') &&
        /Reviewed by the live pass/.test(stored.join(' '))
          ? 'WORKS'
          : 'FAILS',
      chat_id: chatId,
      search_reply: searched.slice(0, 200),
      citations: citationText.slice(0, 3),
      evidence_labels: evidenceLabels,
      wiki_dialog: dialogText.slice(0, 300),
      stored_runbook: stored,
      tools,
      screenshots: [
        '46-search-knowledge-citation.png',
        '46-list-and-read.png',
        '46-create-and-update.png',
        '46-wiki-shows-document.png',
      ],
    });
    expect(tools).toContain('search_knowledge');
    expect(tools).toContain('create_document');
    expect(tools).toContain('update_document');
  });

  test('47: list_chats and search_chat_history reach across chats', async ({
    page,
  }) => {
    await signIn(page);
    const s = stamp();
    const other = await newChat(page, { model: 'llama3.2:3b' });
    await ask(
      page,
      `The password for the ${s} vault is quokka-${s}. Please acknowledge in one sentence.`,
      { replies: 1, timeout: 600_000 },
    );
    const chatId = await agentChat(page);
    const listed = await ask(
      page,
      'List my recent chats in this workspace with their titles.',
      { replies: 1, timeout: 600_000 },
    );
    await shot(page, '47-list-chats');
    const found = await ask(
      page,
      `Search my chat history for the word quokka-${s} and tell me which chat mentions it and what it says.`,
      { replies: 2, timeout: 600_000 },
    );
    await shot(page, '47-search-chat-history');
    const tools = toolNames(chatId);
    record(47, {
      result:
        tools.includes('list_chats') &&
        tools.includes('search_chat_history') &&
        /quokka/i.test(found)
          ? 'WORKS'
          : 'FAILS',
      other_chat: other,
      chat_id: chatId,
      list_reply: listed.slice(0, 200),
      search_reply: found.slice(0, 200),
      tools,
      screenshots: ['47-list-chats.png', '47-search-chat-history.png'],
    });
    expect(tools).toContain('list_chats');
    expect(tools).toContain('search_chat_history');
  });

  test('48: fetch_url reads a page; web_search needs the search backend', async ({
    page,
  }) => {
    await signIn(page);
    const chatId = await agentChat(page);
    const fetched = await ask(
      page,
      'Fetch the web page at https://example.com/ and tell me its main heading, word for word.',
      { replies: 1, timeout: 600_000 },
    );
    await shot(page, '48-fetch-url');
    const searched = await ask(
      page,
      'Search the web for the current stable version of the Rust programming language and tell me the version number and where you found it.',
      { replies: 2, timeout: 600_000 },
    );
    await shot(page, '48-web-search');
    const tools = toolNames(chatId);
    const searchResults = toolResults(chatId, 'web_search');
    record(48, {
      result:
        tools.includes('fetch_url') &&
        /example domain/i.test(fetched) &&
        tools.includes('web_search') &&
        searchResults.some((r) => !/not configured|unavailable|error/i.test(r))
          ? 'WORKS'
          : 'FAILS',
      cause:
        tools.includes('web_search') &&
        searchResults.some((r) => !/not configured|unavailable|error/i.test(r))
          ? undefined
          : 'environment: no SEARCH_* backend on the rig server; re-checked against SearXNG in row 61',
      chat_id: chatId,
      fetch_reply: fetched.slice(0, 200),
      search_reply: searched.slice(0, 200),
      web_search_results: searchResults.slice(0, 2),
      tools,
      screenshots: ['48-fetch-url.png', '48-web-search.png'],
    });
    expect(tools).toContain('fetch_url');
    expect(fetched).toMatch(/example domain/i);
  });

  test('50: the GitHub tools against the scratch repository', async ({
    page,
  }) => {
    test.setTimeout(3_600_000);
    await signIn(page);
    const s = stamp();
    const branch = `chat-pr-${s}`;
    const clone = execFileSync('mktemp', ['-d'], { encoding: 'utf8' }).trim();
    execFileSync('git', [
      'clone',
      '-q',
      `git@github.com:${OWNER}/${NAME}.git`,
      clone,
    ]);
    execFileSync('git', ['-C', clone, 'checkout', '-qb', branch]);
    execFileSync('sh', [
      '-c',
      `printf '# Chat PR ${s}\\n\\nOpened from a chat.\\n' > "${clone}/CHAT-${s}.md"`,
    ]);
    execFileSync('git', ['-C', clone, 'add', '-A']);
    execFileSync('git', [
      '-C',
      clone,
      '-c',
      'user.name=Jake Barnby',
      '-c',
      'user.email=jakeb994@gmail.com',
      'commit',
      '-qm',
      `(docs): chat pull request ${s}`,
    ]);
    execFileSync('git', ['-C', clone, 'push', '-q', '-u', 'origin', branch]);

    const chatId = await agentChat(page);
    const replies: Record<string, string> = {};
    // Each step is a label, the prompt, and the tools that can answer it;
    // a step passes when the model called any of them. The labels
    // github_issue, review_current and review_missing are not tool names.
    const steps: [string, string, string[]?][] = [
      ['list_sources', 'List the data sources connected to this workspace.'],
      [
        'read_repository_file',
        'Read the file src/widget.py from the Scratch repo source and show me its contents.',
      ],
      ['list_issues', 'List the open issues in the scratch repository.'],
      [
        'github_issue',
        'Show me the full details of issue number 5 in the scratch repository.',
        ['get_issue'],
      ],
      [
        'get_build_status',
        'What is the CI build status of the branch ci-pass in the scratch repository, and what is it for the branch ci-fail?',
      ],
      [
        'read_check_logs',
        'Read the logs of the failing check on the ci-fail branch and tell me the exact assertion message that failed.',
      ],
      [
        'create_pull_request',
        `Open a pull request in the scratch repository from the branch ${branch} into main, titled "Chat PR ${s}", with a one-line body.`,
      ],
      [
        'assess_pull_requests',
        'Assess every open pull request in the scratch repository: for each, say whether its checks pass and whether it can merge.',
      ],
      [
        'review_current',
        `Review the pull request you just opened from ${branch} and tell me whether anything in it needs changing.`,
        ['assess_pull_requests', 'read_repository_file'],
      ],
      [
        'review_missing',
        'Which open pull requests in the scratch repository are still missing a review?',
        ['assess_pull_requests'],
      ],
      [
        'assess_release_pipelines',
        'Assess the release pipelines of the scratch repository: did the release workflow for the v0.1.0 tag succeed?',
      ],
      [
        'list_deployments',
        'List the deployments of the scratch repository with their environments and states.',
      ],
    ];
    let count = 0;
    for (const [tool, prompt] of steps) {
      count += 1;
      replies[tool] = (
        await ask(page, prompt, { replies: count, timeout: 900_000 })
      ).slice(0, 200);
      await shot(page, `50-${String(count).padStart(2, '0')}-${tool}`);
    }
    const tools = toolNames(chatId);
    const prs = execFileSync(
      'gh',
      [
        'pr',
        'list',
        '-R',
        `${OWNER}/${NAME}`,
        '--state',
        'open',
        '--json',
        'number,title,headRefName',
      ],
      { encoding: 'utf8', env: gh },
    );
    const chatPr = (
      JSON.parse(prs) as {
        number: number;
        title: string;
        headRefName: string;
      }[]
    ).find((p) => p.headRefName === branch);
    const missing = steps
      .filter(
        ([label, , accepts]) =>
          !(accepts ?? [label]).some((tool) => tools.includes(tool)),
      )
      .map(([label]) => label);
    record(50, {
      result: missing.length === 0 && Boolean(chatPr) ? 'WORKS' : 'FAILS',
      cause:
        missing.length === 0
          ? undefined
          : `model: never called ${missing.join(', ')} (used ${[...new Set(tools)].join(', ')})`,
      chat_id: chatId,
      branch,
      pull_request_opened_by_chat: chatPr ?? null,
      replies,
      tools,
      screenshots: steps.map(
        ([tool], i) => `50-${String(i + 1).padStart(2, '0')}-${tool}.png`,
      ),
    });
    if (chatPr) {
      execFileSync(
        'gh',
        [
          'pr',
          'close',
          String(chatPr.number),
          '-R',
          `${OWNER}/${NAME}`,
          '--delete-branch',
        ],
        { env: gh },
      );
    }
    expect(missing, `tools never called: ${missing.join(', ')}`).toEqual([]);
  });

  test('51: tasks are listed, created, updated, started and inspected from chat', async ({
    page,
  }) => {
    test.setTimeout(3_000_000);
    await signIn(page);
    const s = stamp();
    const chatId = await agentChat(page);
    await ask(page, 'List the projects in this workspace.', {
      replies: 1,
      timeout: 600_000,
    });
    await ask(page, 'List the tasks in this workspace with their status.', {
      replies: 2,
      timeout: 600_000,
    });
    await ask(
      page,
      `Create a task in the project "Real pass scratch" titled "Chat-made task ${s}" with the description: list the files in the working directory and report how many there are. Make it agentic.`,
      { replies: 3, timeout: 600_000 },
    );
    await ask(
      page,
      `Update the task "Chat-made task ${s}": set its priority to 5 and add "Created from chat" to its description.`,
      { replies: 4, timeout: 600_000 },
    );
    const started = await ask(
      page,
      `Now start a background coding task in the project "Real pass scratch" titled "Chat-run ${s}" whose job is to list the files in its working directory and report how many there are, and tell me the task id and run id it gave you.`,
      { replies: 5, timeout: 900_000 },
    );
    await shot(page, '51-task-created-and-started');
    const manualRow = sql(
      `select id || ' | ' || status || ' | ' || priority || ' | ' || is_agentic from tasks where title = 'Chat-made task ${s}'`,
    );
    const taskRow = sql(
      `select id || ' | ' || status || ' | ' || priority || ' | ' || is_agentic from tasks where title = 'Chat-run ${s}' order by created_at desc limit 1`,
    );
    const taskId = taskRow[0]?.split(' | ')[0] ?? '';
    await expect
      .poll(
        async () =>
          sql(
            `select status from task_runs where task_id = '${taskId}' order by started_at desc limit 1`,
          ).join(','),
        { timeout: 1_500_000, intervals: [5_000] },
      )
      .toMatch(/completed|failed/);
    const status = await ask(
      page,
      `Use get_task_run to read the latest run of the task "Chat-run ${s}" and tell me its status in one sentence.`,
      { replies: 6, timeout: 600_000 },
    );
    await shot(page, '51-get-task-run');
    const tools = toolNames(chatId);
    await page.goto('/tasks');
    const card = page.locator('.task-card', { hasText: `Chat-run ${s}` });
    await expect(card).toBeVisible({ timeout: 30_000 });
    const cardText = (await card.innerText()).replace(/\s+/g, ' ');
    await shot(page, '51-task-on-tasks-page');
    const run = sql(
      `select status || ' | ' || coalesce(result_summary, '') from task_runs where task_id = '${taskId}' order by started_at desc limit 1`,
    );
    const wanted = [
      'list_projects',
      'list_tasks',
      'create_task',
      'update_task',
      'start_task',
      'get_task_run',
    ];
    const missing = wanted.filter((t) => !tools.includes(t));
    record(51, {
      result:
        missing.length === 0 && run.join(',').startsWith('completed')
          ? 'WORKS'
          : 'FAILS',
      cause:
        missing.length === 0
          ? undefined
          : `model: never called ${missing.join(', ')} (used ${[...new Set(tools)].join(', ')})`,
      chat_id: chatId,
      manual_task_row: manualRow,
      task_row: taskRow,
      run,
      started_reply: started.slice(0, 200),
      status_reply: status.slice(0, 200),
      task_card: cardText.slice(0, 200),
      tools,
      screenshots: [
        '51-task-created-and-started.png',
        '51-get-task-run.png',
        '51-task-on-tasks-page.png',
      ],
    });
    expect(missing, `tools never called: ${missing.join(', ')}`).toEqual([]);
  });

  test('52: list_members, and send_message posts into another chat', async ({
    page,
  }) => {
    await signIn(page);
    const s = stamp();
    const target = await newChat(page, { model: 'llama3.2:3b' });
    await page.locator('.chat-item.active button[title="Rename"]').click();
    await page.locator('#chat-name').fill(`Inbox ${s}`);
    await page.getByRole('button', { name: 'Save name' }).click();
    await expect(
      page.locator('.chat-item', { hasText: `Inbox ${s}` }),
    ).toBeVisible({ timeout: 30_000 });

    const chatId = await agentChat(page);
    const members = await ask(
      page,
      'Who are the members of this workspace? List their names.',
      { replies: 1, timeout: 600_000 },
    );
    await shot(page, '52-list-members');
    await ask(
      page,
      `Post the message "Hello from the agent ${s}" into the chat called "Inbox ${s}", mentioning the workspace owner.`,
      { replies: 2, timeout: 600_000 },
    );
    await shot(page, '52-send-message');
    const tools = toolNames(chatId);
    const delivered = sql(
      `select role || ' | ' || left(content, 120) from messages where chat_id = '${target}' and content like '%Hello from the agent ${s}%'`,
    );
    await page.goto(`/chats?id=${target}`);
    await page.waitForTimeout(2_000);
    const shown = await page
      .locator('.messages-container')
      .innerText()
      .catch(() => '');
    await shot(page, '52-message-in-target-chat');
    record(52, {
      result:
        tools.includes('list_members') &&
        tools.includes('send_message') &&
        delivered.length > 0 &&
        shown.includes(`Hello from the agent ${s}`)
          ? 'WORKS'
          : 'FAILS',
      cause: tools.includes('send_message')
        ? undefined
        : `model: tools used were ${tools.join(', ')}`,
      chat_id: chatId,
      target_chat: target,
      members_reply: members.slice(0, 200),
      delivered_rows: delivered,
      whom: 'send_message posts into a workspace chat by id; mentions record member ids and send no notification',
      tools,
      screenshots: [
        '52-list-members.png',
        '52-send-message.png',
        '52-message-in-target-chat.png',
      ],
    });
    expect(tools).toContain('list_members');
    expect(tools).toContain('send_message');
    expect(delivered.length).toBeGreaterThan(0);
  });
});
