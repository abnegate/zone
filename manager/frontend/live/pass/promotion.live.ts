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
  state,
  test,
} from './rig';

/**
 * Feature 133, first half: the same question is asked in four chats, three
 * of them separate, and answered alike, which is what the answer-promotion
 * job clusters. The answer comes from a knowledge entry the lane writes first:
 * asked to repeat a claim nothing in the workspace supports, the model refuses
 * in different words each time, which is no answer to promote.
 * The job first runs a whole six-hour period after the server starts (plus up
 * to a tenth of it), so the promotion itself is read from knowledge_entries
 * once the server has been up that long, not here.
 */

const FACT =
  'The Borealis build cache listens on port 7070 and keeps build artifacts for 14 days.';
const QUESTION =
  'Which port does the Borealis build cache listen on, and how long does it keep build artifacts?';

test.describe('answer promotion', () => {
  test.skip(!enabled, 'set ZONE_LIVE_REAL_PASS=1 against the real rig');
  test.describe.configure({ timeout: 1_800_000 });

  test('133: a question asked in several chats is answered alike', async ({
    page,
  }) => {
    const s = stamp();
    const entry = await api('POST', '/api/knowledge', {
      token: await ownerToken(),
      body: {
        workspace_id: state.owner.workspace.id,
        title: `Borealis build cache ${s}`,
        category: 'reference',
        tags: ['infrastructure'],
        content: FACT,
      },
    });
    expect(entry.status).toBe(201);
    await signIn(page);
    const chats: string[] = [];
    const replies: string[] = [];
    for (let turn = 0; turn < 3; turn += 1) {
      chats.push(await newChat(page, { model }));
      replies.push(await ask(page, QUESTION, { replies: 1, timeout: 600_000 }));
    }
    replies.push(await ask(page, QUESTION, { replies: 2, timeout: 600_000 }));
    await shot(page, '133-question-asked-again');
    const embedded = await expect
      .poll(
        () =>
          sql(
            `select count(*) from message_embeddings where chat_id in (${chats.map((c) => `'${c}'`).join(',')})`,
          )[0] ?? '0',
        { timeout: 120_000, intervals: [3_000] },
      )
      .not.toBe('0')
      .then(() => true)
      .catch(() => false);
    const embeddings = sql(
      `select count(*) from message_embeddings where chat_id in (${chats.map((c) => `'${c}'`).join(',')})`,
    );
    record(133.1, {
      list: 'features',
      feature: 'Answer promotion: exchanges seeded',
      result:
        embedded && replies.every((r) => r.includes('7070'))
          ? 'SEEDED'
          : 'FAILS',
      stamp: s,
      knowledge_entry: (entry.body as { id?: string }).id,
      workspace_id: state.owner.workspace.id,
      chats,
      replies: replies.map((r) => r.replace(/\s+/g, ' ').slice(0, 200)),
      message_embeddings: embeddings,
      screenshots: ['133-question-asked-again.png'],
    });
    expect(embedded).toBe(true);
  });
});
