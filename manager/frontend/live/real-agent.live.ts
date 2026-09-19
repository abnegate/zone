import { api, expect, signIn, state, test, tokenFor } from './harness';
import {
  calledTools,
  forwardedFor,
  newChat,
  send,
  settled,
  stamp,
} from './stub';

/**
 * A real model behind the same rig. Nothing is scripted: each lane types what
 * a person would and reads back which tools the model reached for and what
 * the console showed. Opt-in, because whether a model uses a tool is the
 * model's decision and a small one on a CPU takes minutes per round: set
 * ZONE_LIVE_AGENT_MODEL to run these, with the stand-in forwarding unscripted
 * rounds upstream (MODEL_STUB_UPSTREAM).
 */

const model = process.env.ZONE_LIVE_AGENT_MODEL;
test.skip(!model, 'set ZONE_LIVE_AGENT_MODEL to a tool-calling model behind MODEL_STUB_UPSTREAM');
test.describe.configure({ timeout: 1_500_000 });

const ROUND = 900_000;

test('real model: asks with a question card when told not to decide alone', async ({ page }) => {
  const s = stamp();
  await signIn(page);
  await newChat(page, { agent: true, autoApprove: true });
  await send(
    page,
    `Marker ${s}. I need a database for a new service but you must not decide it yourself: ask me with your ask_user question card, offering Postgres and SQLite as the options, and wait for my answer.`
  );
  const card = page.locator('[data-testid="question-card"]').first();
  await expect(card).toBeVisible({ timeout: ROUND });
  const first = card.getByRole('radio').first();
  await first.check();
  await card.locator('[data-testid="question-submit"]').click();
  await settled(page, 2, ROUND);
  const rounds = await forwardedFor(`Marker ${s}`);
  expect(calledTools(rounds)).toContain('ask_user');
  test.info().annotations.push({ type: 'model', description: `${model}: ${calledTools(rounds).join(',') || 'no tools'}; ${rounds.map((r) => r.answer?.seconds).join('s,')}s` });
});

test('real model: stores a fact with the memory tool when asked to remember', async ({ page }) => {
  const s = stamp();
  await signIn(page);
  await newChat(page, { agent: true, autoApprove: true });
  await send(page, `Marker ${s}. Please remember for future conversations that my favourite editor is Helix. Store it with your memory tool, then confirm.`);
  await settled(page, 1, ROUND);
  const rounds = await forwardedFor(`Marker ${s}`);
  expect(calledTools(rounds)).toContain('memory_write');
  test.info().annotations.push({ type: 'model', description: `${model}: ${calledTools(rounds).join(',')}` });
});

test('real model: backgrounds a command, waits for it, and reports its output', async ({ page }) => {
  const s = stamp();
  await signIn(page);
  await newChat(page, { agent: true, autoApprove: true });
  await send(
    page,
    `Marker ${s}. Run the shell command \`sleep 20; echo real-model-done-${s}\` in the background, wait for it to finish with wait_for, then tell me exactly what it printed.`
  );
  await settled(page, 1, ROUND * 2);
  const rounds = await forwardedFor(`Marker ${s}`);
  const tools = calledTools(rounds);
  expect(tools).toContain('run_shell');
  expect(tools).toContain('wait_for');
  await expect(page.locator('.message-assistant').last()).toContainText(`real-model-done-${s}`);
  test.info().annotations.push({ type: 'model', description: `${model}: ${tools.join(',')}` });
});

test('real model: loads the reminder tool and sets a reminder that fires', async ({ page }) => {
  const s = stamp();
  await signIn(page);
  const chatId = await newChat(page, { agent: true, autoApprove: true });
  await send(
    page,
    `Marker ${s}. Set me a reminder for three minutes from now that says "stretch your legs ${s}". The reminder tool is not loaded yet; find and load it first.`
  );
  await settled(page, 1, ROUND * 2);
  const rounds = await forwardedFor(`Marker ${s}`);
  const tools = calledTools(rounds);
  expect(tools).toContain('load_tools');
  expect(tools).toContain('create_reminder');
  const token = await tokenFor(state.owner);
  await expect
    .poll(
      async () => {
        const { body } = await api('GET', `/api/chats/${chatId}/messages`, { token });
        return JSON.stringify(body).includes(`stretch your legs ${s}`);
      },
      { timeout: 420_000, intervals: [5_000] }
    )
    .toBe(true);
  test.info().annotations.push({ type: 'model', description: `${model}: ${tools.join(',')}` });
});

test('real model: reads a workspace skill before doing what it covers', async ({ page }) => {
  const s = stamp();
  const token = await tokenFor(state.owner);
  const created = await api('POST', '/api/knowledge', {
    token,
    body: {
      workspace_id: state.owner.workspace.id,
      title: `Greeting skill ${s}`,
      category: 'skill',
      content: `---\ndescription: Use whenever you greet someone or are asked to say hello.\n---\nEvery greeting ends with the single word "Cheers-${s}".`,
    },
  });
  expect(created.status).toBe(201);
  await signIn(page);
  await newChat(page, { agent: true, autoApprove: true });
  await send(page, `Marker ${s}. Say hello to me, following the workspace skill for greetings.`);
  await settled(page, 1, ROUND);
  const rounds = await forwardedFor(`Marker ${s}`);
  expect(calledTools(rounds)).toContain('read_document');
  await expect(page.locator('.message-assistant').last()).toContainText(`Cheers-${s}`);
  test.info().annotations.push({ type: 'model', description: `${model}: ${calledTools(rounds).join(',')}` });
});
