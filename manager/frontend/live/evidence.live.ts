import { api, createChat, expect, signIn, state, test, tokenFor } from './harness';

/**
 * PR #42's evidence discipline and PR #47's action receipts, rendered from what
 * the real server stored.
 *
 * The record is written through `POST /api/chats/{id}/messages` and read back
 * through the real thread load, so the server's own serialisation and the
 * console's schema both run. Which provenance the *server* assigns a tool's
 * citations is its own business and is covered by its tests; what has to hold
 * here is that a reader is never shown a claim as proof.
 */

const CITATIONS = [
  {
    kind: 'github_build',
    title: 'CI run 812',
    url: 'https://github.com/abnegate/zone/actions/runs/812',
    revision: '0123456789abcdef0123456789abcdef01234567',
    observed_at: '2026-09-09T08:00:00Z',
    provenance: 'server_execution',
    outcome: 'success',
    complete: true,
  },
  {
    kind: 'behavioral_verification',
    title: 'Nominated behavioural check',
    url: 'https://github.com/abnegate/zone/blob/main/tests/check.rs',
    observed_at: '2026-09-09T08:01:00Z',
    provenance: 'model_asserted',
    outcome: 'success',
    complete: true,
  },
  {
    kind: 'workspace_document',
    title: 'Release checklist',
    url: 'knowledge://5f2a2f16-0f4e-4a3f-93de-0e1a5f0f2a11',
    observed_at: '2026-09-09T08:02:00Z',
    provenance: 'server_execution',
    outcome: 'success',
    complete: false,
    note: 'Stored content was unavailable; this is not a complete document.',
  },
];

const RECEIPT = {
  id: 'call_live_1',
  action: 'create_document',
  target_type: 'document',
  target_id: '2b0f8a1e-6b3a-4f7f-9a44-30d2c2f77c01',
  target_label: 'Release checklist',
  actor_id: state.owner.user.id,
  actor_name: state.owner.user.display_name,
  occurred_at: '2026-09-09T08:03:00Z',
  success: true,
  outcome: 'Document created',
  href: '/wiki?id=2b0f8a1e-6b3a-4f7f-9a44-30d2c2f77c01',
};

async function threadWith(metadata: unknown, title: string): Promise<string> {
  const token = await tokenFor(state.owner);
  const chatId = await createChat(token, { title });
  const stored = await api('POST', `/api/chats/${chatId}/messages`, {
    token,
    body: { role: 'assistant', content: 'Here is what I checked.', metadata },
  });
  expect(stored.status).toBe(201);
  return chatId;
}

test('every citation is labelled by how good its evidence is', async ({
  page,
  consoleErrors,
}) => {
  const chatId = await threadWith({ citations: CITATIONS }, 'live citations');

  await signIn(page);
  await page.goto(`/chats?id=${chatId}`);

  const block = page.locator('[data-testid="citations"]');
  await expect(block).toBeVisible();
  await expect(block.getByRole('heading', { name: 'Sources' })).toBeVisible();

  const rows = block.locator('[data-testid="citation"]');
  await expect(rows).toHaveCount(3);

  // Complete, successful, and something other than the model saw it.
  const proven = rows.filter({ hasText: 'CI run 812' });
  await expect(proven).toContainText('Passing');
  await expect(proven).toContainText('GitHub build');
  await expect(proven).toContainText('0123456', { useInnerText: true });

  // The model asserted this one, so it is a claim however complete it looks.
  const claimed = rows.filter({ hasText: 'Nominated behavioural check' });
  await expect(claimed).toContainText('Claimed');
  await expect(claimed).toContainText('Claimed by the model, not verified');
  await expect(claimed).not.toContainText('Passing');

  // Server-observed but incomplete, which is not a pass either.
  const partial = rows.filter({ hasText: 'Release checklist' });
  await expect(partial).toContainText('Incomplete evidence');
  await expect(partial).toContainText('Stored content was unavailable');

  expect(consoleErrors).toEqual([]);
});

test('a workspace write is shown as a receipt, not as prose', async ({
  page,
  consoleErrors,
}) => {
  const chatId = await threadWith(
    { action_receipts: [RECEIPT] },
    'live receipts'
  );

  await signIn(page);
  await page.goto(`/chats?id=${chatId}`);

  const receipt = page.locator('[data-testid="action-receipt"]');
  await expect(receipt).toBeVisible();
  await expect(receipt).toContainText('Created document');
  await expect(receipt).toContainText('Release checklist');
  await expect(receipt).toContainText(state.owner.user.display_name);
  await expect(receipt).toContainText('Document created');
  await expect(receipt.locator('[data-testid="action-receipt-link"]')).toHaveAttribute(
    'href',
    RECEIPT.href
  );
  expect(consoleErrors).toEqual([]);
});

/**
 * The same rendering, reached the way a user reaches it: a real model calling a
 * real tool over real workspace data.
 *
 * Opt-in, because whether a model reaches for a tool is the model's decision and
 * not a property of this console. `llama3.2:3b` never did in five attempts;
 * `qwen3.8:27b` managed two in three. Set `ZONE_LIVE_AGENT_MODEL` to a model
 * that tool-calls to include it.
 */
test('a document the agent read is cited', async ({ page }) => {
  const model = process.env.ZONE_LIVE_AGENT_MODEL;
  test.skip(!model, 'set ZONE_LIVE_AGENT_MODEL to a tool-calling model');

  const token = await tokenFor(state.owner);
  const title = `Release checklist ${Date.now()}`;
  const created = await api('POST', '/api/knowledge', {
    token,
    body: {
      workspace_id: state.owner.workspace.id,
      title,
      content: 'Run the migrations, verify the console, then tag the release.',
      category: 'runbook',
      tags: ['release'],
    },
  });
  expect(created.status).toBe(201);

  const chatId = await createChat(token, { title: 'live agent citations', agent: true, model });
  await signIn(page);
  await page.goto(`/chats?id=${chatId}`);
  await expect(page.getByPlaceholder(/type a message/i)).toBeVisible();

  let cited = 0;
  for (let attempt = 1; attempt <= 4 && cited === 0; attempt += 1) {
    const box = page.getByPlaceholder(/type a message/i);
    await box.fill(
      `Call the list_documents tool with the query "${title}", then call read_document on the id it returns, and cite it.`
    );
    await box.press('Enter');
    // `.message-status` only exists once the turn has started, so waiting for
    // it to reach zero returns immediately and the next attempt would submit
    // over the top of a turn already running.
    await expect(page.locator('.message-assistant')).toHaveCount(attempt, {
      timeout: 280_000,
    });
    await expect(page.locator('.message-status')).toHaveCount(0, { timeout: 280_000 });
    const { body } = await api('GET', `/api/chats/${chatId}`, { token });
    const messages =
      (body as { chat?: { messages: { metadata?: { citations?: unknown[] } | null }[] } }).chat
        ?.messages ?? [];
    cited = messages.reduce(
      (total, message) => total + (message.metadata?.citations?.length ?? 0),
      0
    );
  }

  expect(cited, `${model} never called a document tool`).toBeGreaterThan(0);
  await expect(page.locator('[data-testid="citation"]').first()).toBeVisible();
  await expect(page.locator('[data-testid="citations"]').last()).toContainText(title);
});
