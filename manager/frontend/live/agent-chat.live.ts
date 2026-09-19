import { api, expect, signIn, state, test, tokenFor } from './harness';
import {
  approveIfAsked,
  newChat,
  roundsFor,
  script,
  send,
  settled,
  settledRounds,
  stamp,
} from './stub';

/**
 * The agent surface of a chat, driven as a person drives it, against the real
 * server with a scripted model behind it (see `stub.ts`). What each lane
 * proves is the machinery on the far side of the model: that a tool call is
 * executed, gated, recorded and rendered, and that the next round is handed
 * what the code says it is handed.
 */

async function messages(chatId: string): Promise<{ role: string; content: string }[]> {
  const token = await tokenFor(state.owner);
  const { body } = await api('GET', `/api/chats/${chatId}/messages`, { token });
  const found = (body as { messages?: { role: string; content: string }[] }).messages;
  return found ?? [];
}

async function knowledgeTitled(title: string): Promise<boolean> {
  const token = await tokenFor(state.owner);
  const { body } = await api(
    'GET',
    `/api/knowledge?workspace_id=${state.owner.workspace.id}&limit=100`,
    { token }
  );
  return JSON.stringify(body).includes(JSON.stringify(title));
}

async function createKnowledge(title: string, content: string, category?: string): Promise<string> {
  const token = await tokenFor(state.owner);
  const created = await api('POST', '/api/knowledge', {
    token,
    body: { workspace_id: state.owner.workspace.id, title, content, category },
  });
  expect(created.status, JSON.stringify(created.body)).toBe(201);
  const text = JSON.stringify(created.body);
  const id = /"id":"([0-9a-f-]{36})"/.exec(text)?.[1];
  if (!id) throw new Error(`no id in ${text}`);
  return id;
}

test('a new chat with the agent on streams a reply, shows its reasoning and offers the agent tools', async ({
  page,
  consoleErrors,
}) => {
  const s = stamp();
  await script(`hello ${s}`, [
    { reasoning: 'A greeting needs no tool.', text: `Hello ${s}, the stand-in is listening.` },
  ]);

  await signIn(page);
  await newChat(page, { agent: true });
  await send(page, `hello ${s}`);
  await settled(page, 1);

  const reply = page.locator('.message-assistant').last();
  await expect(reply).toContainText(`Hello ${s}`);
  await expect(reply.locator('[data-testid="reasoning"]')).toHaveCount(1);

  const rounds = await roundsFor(`hello ${s}`);
  expect(rounds).toHaveLength(1);
  for (const tool of ['ask_user', 'wait_for', 'search_tools', 'load_tools', 'memory_write', 'read_document', 'run_shell']) {
    expect(rounds[0].tools, `${tool} is offered in the core set`).toContain(tool);
  }
  expect(rounds[0].tools, 'create_reminder is deferred').not.toContain('create_reminder');
  expect(rounds[0].system).toContain('create_reminder');
  expect(consoleErrors).toEqual([]);
});

test('an outward write waits for confirmation, runs when approved and stops when denied', async ({
  page,
  consoleErrors,
}) => {
  const s = stamp();
  await script(`file the note ${s}`, [
    { calls: [{ name: 'create_document', arguments: { title: `Note ${s}`, content: `Filed by the stand-in ${s}.` } }] },
    { text: `Filed note ${s}.` },
  ]);
  await script(`file another note ${s}`, [
    { calls: [{ name: 'create_document', arguments: { title: `Denied ${s}`, content: 'never filed' } }] },
    { text: `Understood, not filed ${s}.` },
  ]);

  await signIn(page);
  await newChat(page, { agent: true });

  await send(page, `file the note ${s}`);
  await expect(page.locator('[data-testid="tool-approve"]').first()).toBeVisible({ timeout: 30_000 });
  await expect(page.locator('[data-testid="tool-call-preview"]').first()).toBeVisible();
  await page.locator('[data-testid="tool-approve"]').first().click();
  await settled(page, 1);
  await expect(page.locator('.message-assistant').last()).toContainText(`Filed note ${s}`);
  const receipt = page.locator('[data-testid="action-receipt"]').first();
  await expect(receipt).toBeVisible();
  await expect(receipt.locator('[data-testid="action-receipt-link"]')).toHaveAttribute('href', /\/wiki\?id=/);
  await expect.poll(() => knowledgeTitled(`Note ${s}`), { timeout: 20_000 }).toBe(true);

  await send(page, `file another note ${s}`);
  await expect(page.locator('[data-testid="tool-deny"]').first()).toBeVisible({ timeout: 30_000 });
  await page.locator('[data-testid="tool-deny"]').first().click();
  await settled(page, 2);
  await expect(page.locator('.message-assistant').last()).toContainText(`not filed ${s}`);
  const denied = await roundsFor(`file another note ${s}`);
  expect(denied).toHaveLength(2);
  expect(denied[1].tool_results.map((r) => r.content).join('\n')).toMatch(/den|declin|refus|not approved/i);
  expect(await knowledgeTitled(`Denied ${s}`)).toBe(false);
  expect(consoleErrors).toEqual([]);
});

test('a question the model asks becomes a card, and the answer is the next turn', async ({
  page,
  consoleErrors,
}) => {
  const s = stamp();
  await script(`which database ${s}`, [
    {
      calls: [
        {
          name: 'ask_user',
          arguments: {
            questions: [
              {
                header: 'Database',
                question: 'Which database should the service use?',
                options: [
                  { label: `Postgres-${s}`, description: 'The one already deployed' },
                  { label: `SQLite-${s}`, description: 'A file next to the binary' },
                ],
                required: true,
              },
            ],
          },
        },
      ],
    },
  ]);
  await script(`Postgres-${s}`, [{ text: `Postgres-${s} it is; nothing else to ask.` }]);

  await signIn(page);
  await newChat(page, { agent: true });
  await send(page, `which database ${s}`);

  const card = page.locator('[data-testid="question-card"]').first();
  await expect(card).toBeVisible({ timeout: 30_000 });
  await expect(card).toContainText('Which database should the service use?');
  await card.getByRole('radio', { name: `Postgres-${s}` }).check();
  await card.locator('[data-testid="question-submit"]').click();

  await settled(page, 2, 120_000);
  await expect(page.locator('.message-assistant').last()).toContainText(`Postgres-${s} it is`);
  const answered = await roundsFor(`Postgres-${s}`);
  expect(answered).toHaveLength(1);
  expect(answered[0].last_user).toContain('Database');
  expect(consoleErrors).toEqual([]);
});

test('a command runs in the background, the model waits for it, and reads its log', async ({
  page,
  consoleErrors,
}) => {
  const s = stamp();
  const jobId = '$re:^Started ([A-Za-z0-9_-]+) \\(pid';
  await script(`run the slow job ${s}`, [
    { calls: [{ name: 'run_shell', arguments: { command: `sleep 4; echo finished-${s}`, background: true, reason: 'A job that outlives one round' } }] },
    { calls: [{ name: 'wait_for', arguments: { kind: 'job', id: jobId, timeout_secs: 60 } }] },
    { calls: [{ name: 'tail_job', arguments: { id: jobId } }] },
    { text: `The job printed finished-${s}.` },
  ]);

  await signIn(page);
  await newChat(page, { agent: true, autoApprove: true });
  await send(page, `run the slow job ${s}`);
  await approveIfAsked(page, 5_000);
  await settled(page, 1, 180_000);

  await expect(page.locator('[data-testid="job-card"]').first()).toBeVisible();
  await expect(page.locator('[data-testid="wait-card"]').first()).toBeVisible();
  await expect(page.locator('[data-testid="wait-outcome"]').first()).toBeVisible();
  await expect(page.locator('.message-assistant').last()).toContainText(`finished-${s}`);

  const rounds = await roundsFor(`run the slow job ${s}`);
  expect(rounds).toHaveLength(4);
  const log = rounds[3].tool_results.map((r) => r.content).join('\n');
  expect(log).toContain(`finished-${s}`);
  expect(log).toMatch(/exited 0/);
  expect(consoleErrors).toEqual([]);
});

test('what is remembered in one chat is read back in the next, with the badge to say so', async ({
  page,
  consoleErrors,
}) => {
  const s = stamp();
  await script(`remember my editor ${s}`, [
    { calls: [{ name: 'memory_write', arguments: { category: 'fact', name: `editor-${s}`, description: 'Which editor they use', content: `Helix ${s}` } }] },
    { text: `Noted: Helix ${s}.` },
  ]);
  await script(`which editor do I use ${s}`, [
    { calls: [{ name: 'memory_read', arguments: { category: 'fact', name: `editor-${s}` } }] },
    { text: `You use Helix ${s}.` },
  ]);

  await signIn(page);
  await newChat(page, { agent: true, autoApprove: true });
  await send(page, `remember my editor ${s}`);
  await approveIfAsked(page, 5_000);
  await settled(page, 1);
  await expect(page.locator('.message-assistant').last()).toContainText(`Noted: Helix ${s}`);

  await newChat(page, { agent: true, autoApprove: true });
  await send(page, `which editor do I use ${s}`);
  await settled(page, 1);
  await expect(page.locator('.message-assistant').last()).toContainText(`You use Helix ${s}`);
  await expect(page.locator('[data-testid="memory-badge"]').first()).toBeVisible();

  const rounds = await roundsFor(`which editor do I use ${s}`);
  expect(rounds).toHaveLength(2);
  expect(rounds[0].system, 'the fact index names the entry').toContain(`editor-${s}`);
  expect(rounds[1].tool_results.map((r) => r.content).join('\n')).toContain(`Helix ${s}`);
  expect(consoleErrors).toEqual([]);
});

test('a deferred tool is found, loaded and used, and the reminder it sets fires into the chat', async ({
  page,
  consoleErrors,
}) => {
  test.setTimeout(300_000);
  const s = stamp();
  const due = new Date(Date.now() + 70_000).toISOString();
  await script(`remind me to stretch ${s}`, [
    { calls: [{ name: 'search_tools', arguments: { query: 'set a reminder' } }] },
    { calls: [{ name: 'load_tools', arguments: { names: ['create_reminder'] } }] },
    { calls: [{ name: 'create_reminder', arguments: { content: `Stretch ${s}`, due_at: due } }] },
    { text: `Reminder set for stretching ${s}.` },
  ]);

  await signIn(page);
  const chatId = await newChat(page, { agent: true, autoApprove: true });
  await send(page, `remind me to stretch ${s}`);
  await approveIfAsked(page, 5_000);
  await settled(page, 1);
  await expect(page.locator('.message-assistant').last()).toContainText(`Reminder set for stretching ${s}`);

  const rounds = await roundsFor(`remind me to stretch ${s}`);
  expect(rounds).toHaveLength(4);
  expect(rounds[0].tools).not.toContain('create_reminder');
  expect(rounds[1].tool_results.map((r) => r.content).join('\n')).toContain('create_reminder');
  expect(rounds[2].tools, 'the loaded tool is offered on the next round').toContain('create_reminder');
  expect(rounds[3].tool_results.map((r) => r.content).join('\n')).toMatch(/remind/i);

  await expect
    .poll(async () => (await messages(chatId)).some((m) => m.content.includes(`Stretch ${s}`)), {
      timeout: 200_000,
      intervals: [2_000],
    })
    .toBe(true);
  await page.reload();
  await expect(page.locator(`text=Stretch ${s}`).first()).toBeVisible({ timeout: 30_000 });
  expect(consoleErrors).toEqual([]);
});

test('a reminder that carries a prompt runs a turn when it fires, and can be listed and cancelled', async ({
  page,
  consoleErrors,
}) => {
  test.setTimeout(300_000);
  const s = stamp();
  const due = new Date(Date.now() + 70_000).toISOString();
  await script(`schedule the time report ${s}`, [
    { calls: [{ name: 'load_tools', arguments: { names: ['create_reminder', 'list_reminders', 'cancel_reminder'] } }] },
    { calls: [{ name: 'create_reminder', arguments: { content: `Time report ${s}`, due_at: due, rrule: 'FREQ=HOURLY', prompt: `Report the time ${s}` } }] },
    { calls: [{ name: 'list_reminders', arguments: {} }] },
    { text: `Scheduled the hourly time report ${s}.` },
  ]);
  await script(`Report the time ${s}`, [{ text: `Time report delivered ${s}.` }]);
  await script(`cancel the time report ${s}`, [
    { calls: [{ name: 'load_tools', arguments: { names: ['list_reminders', 'cancel_reminder'] } }] },
    { calls: [{ name: 'list_reminders', arguments: {} }] },
    // list_reminders answers JSON; inside one object, `content` precedes `id`.
    { calls: [{ name: 'cancel_reminder', arguments: { reminder_id: `$re:"content":"Time report ${s}"[^}]*?"id":"([0-9a-f-]{36})"` } }] },
    { calls: [{ name: 'list_reminders', arguments: {} }] },
    { text: `Cancelled the time report ${s}.` },
  ]);

  await signIn(page);
  const chatId = await newChat(page, { agent: true, autoApprove: true });
  await send(page, `schedule the time report ${s}`);
  await approveIfAsked(page, 5_000);
  await settled(page, 1);
  await expect(page.locator('.message-assistant').last()).toContainText(`Scheduled the hourly time report ${s}`);
  const scheduled = await roundsFor(`schedule the time report ${s}`);
  expect(scheduled[3].tool_results.map((r) => r.content).join('\n')).toContain(`Time report ${s}`);

  await settledRounds(`Report the time ${s}`, 1, 200_000);
  await expect
    .poll(async () => (await messages(chatId)).some((m) => m.content.includes(`Time report delivered ${s}`)), {
      timeout: 60_000,
      intervals: [2_000],
    })
    .toBe(true);
  await page.reload();
  await expect(page.locator(`text=Report the time ${s}`).first()).toBeVisible({ timeout: 30_000 });
  await expect(page.locator(`text=Time report delivered ${s}`).first()).toBeVisible();

  const before = (await messages(chatId)).length;
  await send(page, `cancel the time report ${s}`);
  await approveIfAsked(page, 5_000);
  await expect
    .poll(async () => (await roundsFor(`cancel the time report ${s}`)).length, { timeout: 120_000 })
    .toBe(5);
  const cancelled = await roundsFor(`cancel the time report ${s}`);
  const afterCancel = cancelled[3].tool_results.map((r) => r.content).join('\n');
  expect(afterCancel, 'cancel_reminder took the id list_reminders gave').not.toMatch(/not found|error/i);
  // The stand-in hands back every result of the turn; the second listing is the last of them.
  const listedAgain = cancelled[4].tool_results.at(-1)?.content ?? '';
  const entry = new RegExp(`"content":"Time report ${s}"[^}]*"status":"([a-z_]+)"`).exec(listedAgain);
  expect(entry?.[1], 'the hourly report is cancelled, not pending for its next firing').toBe('cancelled');
  expect((await messages(chatId)).length).toBeGreaterThan(before);
  expect(consoleErrors).toEqual([]);
});

test('a skill filed in the workspace is indexed in the prompt and read on demand', async ({
  page,
  consoleErrors,
}) => {
  const s = stamp();
  const title = `Release notes skill ${s}`;
  await createKnowledge(
    title,
    `---\nname: release-notes-${s}\ndescription: Use when asked for release notes ${s}.\n---\n# Release notes\n1. List the merged changes ${s}.\n2. Name what was deliberately left out.`,
    'skill'
  );
  await script(`draft the release notes ${s}`, [
    { calls: [{ name: 'read_document', arguments: { id: `$re:${title.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')} \\[([0-9a-f-]{36})\\]` } }] },
    { text: `Following the release notes skill ${s}.` },
  ]);

  await signIn(page);
  await newChat(page, { agent: true });
  await send(page, `draft the release notes ${s}`);
  await settled(page, 1);
  await expect(page.locator('.message-assistant').last()).toContainText(`Following the release notes skill ${s}`);

  const rounds = await roundsFor(`draft the release notes ${s}`);
  expect(rounds).toHaveLength(2);
  expect(rounds[0].system).toContain(title);
  expect(rounds[0].system).toContain(`Use when asked for release notes ${s}`);
  expect(rounds[1].tool_results.map((r) => r.content).join('\n')).toContain(`List the merged changes ${s}`);
  expect(consoleErrors).toEqual([]);
});

test('a knowledge search the model makes is cited on the reply', async ({ page, consoleErrors }) => {
  const s = stamp();
  const title = `Deployment checklist ${s}`;
  await createKnowledge(title, `Rotate the signing key ${s}, then restart the gateway, then watch the error rate for ten minutes.`);
  await script(`what does the deployment checklist ${s} say`, [
    { calls: [{ name: 'search_knowledge', arguments: { query: `deployment checklist ${s}` } }] },
    { text: `It says to rotate the signing key and restart the gateway ${s}.` },
  ]);

  await signIn(page);
  await newChat(page, { agent: true });
  await send(page, `what does the deployment checklist ${s} say`);
  await settled(page, 1);

  const rounds = await roundsFor(`what does the deployment checklist ${s} say`);
  expect(rounds).toHaveLength(2);
  expect(rounds[1].tool_results.map((r) => r.content).join('\n')).toContain(`Rotate the signing key ${s}`);
  const citations = page.locator('[data-testid="citations"]');
  await expect(citations).toBeVisible();
  await expect(citations).toContainText(title);
  expect(consoleErrors).toEqual([]);
});

test('a model that fails upstream is reported as a failure, not as silence', async ({
  page,
  consoleErrors,
}) => {
  const s = stamp();
  await script(`fall over ${s}`, [{ status: 503, body: 'upstream fell over' }]);

  await signIn(page);
  await newChat(page, { agent: true });
  await send(page, `fall over ${s}`);
  await expect(page.locator('.message-status')).toHaveCount(0, { timeout: 120_000 });
  const thread = page.locator('.chat-messages, .messages, main').first();
  await expect(thread).toContainText(/error|fail|unavailable|could not/i, { timeout: 30_000 });
  // The console records the failure it was told about; that report is not a broken render.
  expect(consoleErrors.filter((e) => !/\b(502|503|500)\b|upstream/i.test(e))).toEqual([]);
});
